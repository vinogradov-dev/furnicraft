//! One open scene and everything bound to it.
//!
//! A document is the whole payload of one tab: the session (project, undo history, dirty marker),
//! the editor (selection, tools, cards), the evaluation cache and the kernel, the frame evaluated
//! from them, the split layout and its cameras, the renderer target of this tab and the gizmos that
//! act on it. Activating another tab swaps this object and nothing else happens — no file is read
//! or written, so a workspace holds several scenes at once (U45).
//!
//! The split is the one ADR 0029 draws: a document never stores a viewport, a hover highlight or a
//! window fold. What the *window* decides — the pointer, the armed navigation gesture, the
//! transform mode, the measured ribbon band, the viewport rectangle and the display defaults —
//! arrives as [`Shell`] and as the workspace [`Settings`], so two tabs share one window without
//! sharing one scene.
//!
//! Changes when: a scene gains a piece of derived state (a cache, a frame, a renderer target) or a
//! tool that acts on it; not when a panel or a card gains a field.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use furnicraft_engine::mesh_cmds::MESH_VERTICES_MOVE_ID;
use furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_MANY_ID;
use furnicraft_engine::{MeshVertexMove, MeshVerticesMove, NodePlacement, NodePlacementSetMany, Session};
use furnicraft_kernel_truck::TruckKernel;
use furnicraft_mesh::{EvaluationCache, object_mesh};
use furnicraft_model::{Catalog, CornerMove, Geometry, Id, NodeQuery, Placement, Project, Revision, StructuralKind};
use furnicraft_project::SceneFile;
use furnicraft_render::ViewportRenderer;
use furnicraft_scene::{Camera, ContactShadows, Polyline, Projection, SELECTION_LINE_COLOR, SceneColors, SceneFrame};
use furnicraft_text::Lang;
use furnicraft_ui::settings::{Settings, ViewportSettings};
use furnicraft_ui::{
    CommandBar, CommandBarAction, ContourGrip, DrawDraft, Editor, GripValue, MeshElement, Message, PaletteContext,
    ViewFlags, command_bar,
};
use furnicraft_units::Length;

use crate::contour_gizmo;
use crate::contour_gizmo::ContourGizmo;
use crate::draw_gizmo::{DrawAction, DrawGizmo, DrawModifiers};
use crate::extrude_gizmo::{ExtrudeEvent, ExtrudeGizmo};
use crate::file_menu::{FileAction, FileMenu, PreservedSections};
use crate::gizmo;
use crate::gizmo_math;
use crate::gizmo_math::{PlacementCommit, Preview, PreviewKey, WorkPlane};
use crate::mesh_pick;
use crate::rotate_gizmo::RotateGizmo;
use crate::scene_build::{Bounds, evaluate, home_camera};
use crate::scene_gizmo;
use crate::shortcuts::{DraftKey, Shortcuts, drawing_key, take_drawing_shortcut};
use crate::side_panels::Chrome;
use crate::strip::{strip_area, view_region};
use crate::translate_gizmo::TranslateGizmo;
use crate::vertex_gizmo;
use crate::vertex_gizmo::{MeshVertexCommit, VertexGizmo, VertexPreview};
use crate::view_cluster;
use crate::view_cluster::{ClusterAction, ClusterView, NavigationMode, ViewCluster};
use crate::view_layout::{Layout, LayoutKind};
use crate::{Shell, TransformMode, theme};

/// Orbit sensitivity in radians per point dragged.
const ORBIT_RADIANS_PER_POINT: f64 = 0.008;

/// Zoom sensitivity per scroll point.
const ZOOM_PER_SCROLL_POINT: f64 = 0.004;

/// Colour and radius of an unselected mesh vertex marker.
const MESH_VERTEX_COLOR: egui::Color32 = egui::Color32::from_rgb(226, 226, 226);
const MESH_VERTEX_RADIUS: f32 = 4.0;

/// Translucent fill of a selected face.
const MESH_FACE_FILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(70, 41, 11, 70);

/// Width of the bright stroke that repeats a selected line chain over the viewport image.
const SELECTION_OUTLINE_WIDTH: f32 = 2.25;

/// Width of the dark underlay below [`SELECTION_OUTLINE_WIDTH`], which keeps the stroke readable
/// over a light panel.
const SELECTION_UNDERLAY_WIDTH: f32 = 3.5;

/// The selected-chain stroke, converted from the scene's explicit line colour so the overlay and
/// the uploaded chain agree.
const SELECTION_OUTLINE: egui::Color32 = egui::Color32::from_rgb(
    (SELECTION_LINE_COLOR[0] * 255.0 + 0.5) as u8,
    (SELECTION_LINE_COLOR[1] * 255.0 + 0.5) as u8,
    (SELECTION_LINE_COLOR[2] * 255.0 + 0.5) as u8,
);

/// Dark underlay of the selected-chain stroke, premultiplied so the panel still shows through.
const SELECTION_UNDERLAY: egui::Color32 = egui::Color32::from_rgba_premultiplied(10, 16, 19, 220);

/// The display state the window owns and every frame of this tab is built with.
///
/// The floor grid, the edge list, the contact-shadow pass and the colours of the active theme
/// variant are the window's (ADR 0029), so the shell hands them to the active document; a frame
/// rebuilt by a command, a card or a shortcut then draws the same display state the viewport shows.
#[derive(Clone, Copy, Debug)]
struct Display {
    /// Whether the floor grid draws.
    grid: bool,
    /// The minor grid spacing, in millimetres.
    grid_step_mm: u32,
    /// Whether the edge pass draws the full triangulation instead of only the form edges.
    all_edges: bool,
    /// The switch and the strength of the contact-shadow pass.
    shadows: ContactShadows,
    /// The background, grid and line ink of the theme variant the window draws with.
    colors: SceneColors,
}

impl Display {
    /// Reads the display state the window currently shows.
    fn of(viewport: &ViewportSettings, cluster: &ViewCluster, colors: SceneColors) -> Self {
        Self {
            grid: viewport.grid,
            grid_step_mm: viewport.grid_step_mm,
            all_edges: viewport.all_edges,
            shadows: cluster.shadow_settings(viewport.shadow_strength),
            colors,
        }
    }

    /// Writes the display state into a frame.
    fn apply(self, scene: &mut SceneFrame) {
        scene.grid.visible = self.grid;
        scene.grid.spacing_mm = f64::from(self.grid_step_mm.max(1));
        scene.all_edges = self.all_edges;
        scene.contact_shadows = self.shadows;
        scene.apply_colors(self.colors);
    }
}

/// How long a rebuild may take before the shell announces the next one.
///
/// The evaluation is synchronous, so the only honest moment to paint «Перестроение…» is the frame
/// *before* it runs: a scene whose last rebuild overran this budget waits one frame, keeps the last
/// good geometry on screen and paints the busy state, and the work runs on the next frame, so the
/// window answers input in between. A cheap scene never shows the message at all.
pub(crate) const SLOW_REBUILD: Duration = Duration::from_millis(250);

/// What the last rebuild cost, and whether the pending one has announced itself yet.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Rebuild {
    /// How long the last evaluation took, if this scene has been evaluated since it opened.
    last: Option<Duration>,
    /// Whether the pending rebuild already painted its busy state.
    announced: bool,
}

impl Rebuild {
    /// Whether the last rebuild overran [`SLOW_REBUILD`].
    pub(crate) fn slow(&self) -> bool {
        self.last.is_some_and(|last| last >= SLOW_REBUILD)
    }

    /// Whether the status bar says «Перестроение…» while a rebuild is pending.
    pub(crate) fn busy(&self, pending: bool) -> bool {
        pending && (self.slow() || self.announced)
    }

    /// Whether the pending rebuild waits one frame so its busy state can be painted first.
    ///
    /// A cheap rebuild is never announced and never waits: the extra frame is paid only by the
    /// scenes that would have blocked the frame that asked for it.
    pub(crate) fn defer(&mut self, pending: bool) -> bool {
        if !pending {
            self.announced = false;
            return false;
        }
        if self.slow() && !self.announced {
            self.announced = true;
            return true;
        }
        false
    }

    /// Records how long a rebuild took.
    pub(crate) fn record(&mut self, elapsed: Duration) {
        self.last = Some(elapsed);
        self.announced = false;
    }
}

/// One open scene and everything derived from it.
pub(crate) struct Document {
    pub(crate) session: Session,
    pub(crate) editor: Editor,
    kernel: TruckKernel,
    /// Derived node geometry keyed by the inputs it was built from; a rebuild reuses every node
    /// the editing did not touch.
    cache: EvaluationCache,
    scene: SceneFrame,
    /// This scene's views: their cameras, presets and the tree that lays them out. View state in
    /// the tab; the document never sees it (ADR 0029).
    layout: Layout,
    /// World-space bounds of the latest evaluation, so *Frame all* fits the objects the scene has
    /// now instead of the ones it had when the file opened.
    scene_bounds: Bounds,
    scene_revision: Revision,
    /// The editor's visibility revision the frame was built from. A hidden subtree changes the
    /// frame without touching the document revision, so the rebuild watches both (ADR 0065).
    scene_visibility: u64,
    /// The preview overlay's version the frame was built from. A previewing operation shows a node
    /// the document does not hold yet, so the frame follows the overlay as it follows the revision.
    scene_preview: u64,
    /// How the last rebuild cost, so a scene that takes more than a frame announces the next one.
    rebuild: Rebuild,
    /// The rectangle the command bar drew last frame, so a press on it is chrome and never a pick.
    command_bar_rect: Option<egui::Rect>,
    renderer: ViewportRenderer,
    /// The wgpu handles this tab draws with, cloned from the window's render state.
    render_state: egui_wgpu::RenderState,
    /// The texture this tab draws into; a close frees it, so a workspace of tabs holds no stale
    /// render targets.
    texture_id: egui::TextureId,
    file: FileMenu,
    /// Top-level sections of the loaded file that this build does not understand; written back on
    /// save so an older build never silently destroys a newer build's data.
    extra: PreservedSections,
    translate_gizmo: TranslateGizmo,
    rotate_gizmo: RotateGizmo,
    vertex_gizmo: VertexGizmo,
    pub(crate) contour_gizmo: ContourGizmo,
    pub(crate) draw_gizmo: DrawGizmo,
    pub(crate) extrude_gizmo: ExtrudeGizmo,
    /// The corner orientation widget, which owns the roll-ring drag across frames.
    gizmo: gizmo::Gizmo,
    highlighted: Vec<Id>,
    /// The selected line chains of the frame last uploaded, already carrying any drag preview, so
    /// the screen-space outline follows a moved path without a frame of lag.
    selected_chains: Vec<Polyline>,
    uploaded_preview: Option<PreviewKey>,
    /// The failure this tab last reported, drawn by the status bar of the window.
    pub(crate) last_error: Option<String>,
    /// The window's display state, kept so a frame rebuilt outside the viewport draws what the
    /// viewport would.
    display: Display,
}

impl Document {
    /// Builds a document around a session: evaluates the scene, fits every view and registers its
    /// own render target.
    ///
    /// The workspace defaults come from `settings` and the display diagnostics, including the
    /// active theme's viewport colours, from the window's `shell`; none of it ever enters the scene
    /// file.
    pub(crate) fn new(
        render_state: &egui_wgpu::RenderState,
        settings: &Settings,
        shell: &Shell,
        session: Session,
        extra: PreservedSections,
    ) -> Result<Self, String> {
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default())?;
        let scene_bounds = evaluated.bounds;
        let fit = home_camera(scene_bounds)?;
        let layout = Layout::new(fit, settings.viewport.opening_view, settings.viewport.style);
        let display = Display::of(&settings.viewport, &shell.cluster, shell.colors);
        let mut scene = evaluated.frame;
        layout.apply_frame_style(&mut scene);
        display.apply(&mut scene);
        scene.camera = crate::scene_build::scene_camera(session.project(), layout.active_camera());

        let mut editor = Editor::new();
        // The fold and the pinned method of each tool are the ribbon state the workspace remembers; a
        // new tab opens the ribbon the way the user left the last one.
        editor.set_ribbon_open(settings.ribbon.open);
        editor.set_pinned_variants(settings.ribbon.pinned_variants.clone());
        let mut renderer = ViewportRenderer::new(&render_state.device, 1280, 800);
        let texture_id = {
            let mut egui_renderer = render_state.renderer.write();
            egui_renderer.register_native_texture(&render_state.device, renderer.color_view(), wgpu::FilterMode::Linear)
        };
        renderer.upload(&render_state.device, &scene).map_err(|error| error.to_string())?;

        Ok(Self {
            scene_revision: session.project().revision(),
            scene_visibility: 0,
            scene_preview: session.preview_version(),
            rebuild: Rebuild::default(),
            command_bar_rect: None,
            session,
            editor,
            kernel,
            cache,
            scene,
            layout,
            scene_bounds,
            renderer,
            render_state: render_state.clone(),
            texture_id,
            file: FileMenu::new(),
            extra,
            translate_gizmo: TranslateGizmo::default(),
            rotate_gizmo: RotateGizmo::default(),
            vertex_gizmo: VertexGizmo::default(),
            contour_gizmo: ContourGizmo::default(),
            draw_gizmo: DrawGizmo::new(),
            extrude_gizmo: ExtrudeGizmo::default(),
            gizmo: gizmo::Gizmo::default(),
            highlighted: Vec::new(),
            selected_chains: Vec::new(),
            uploaded_preview: None,
            last_error: None,
            display,
        })
    }

    /// Reads a scene file into a new document.
    ///
    /// Opening is not an undoable document edit: it installs a session, so this tab starts with an
    /// empty history, an empty selection and a fresh evaluation. The catalogue is the workspace's —
    /// it is session state, not scene data, and the file never carries it.
    pub(crate) fn open(
        render_state: &egui_wgpu::RenderState,
        settings: &Settings,
        shell: &Shell,
        path: &Path,
        catalog: &Catalog,
    ) -> Result<Self, String> {
        let scene = SceneFile::read(path).map_err(|error| error.to_string())?;
        let extra = scene.extra().clone();
        let project = scene.into_document();
        let session = Session::with_catalog(project, catalog.clone());
        let mut document = Self::new(render_state, settings, shell, session, extra)?;
        document.file.set_path(Some(path.to_owned()));
        Ok(document)
    }

    /// Writes the scene and marks the document as saved.
    pub(crate) fn save(&mut self, path: &Path) -> Result<(), String> {
        SceneFile::new(self.session.project().clone())
            .with_extra(self.extra.clone())
            .write(path)
            .map_err(|error| error.to_string())?;
        self.session.mark_saved();
        self.file.set_path(Some(path.to_owned()));
        Ok(())
    }

    /// The file the scene came from, if it has one.
    pub(crate) fn path(&self) -> Option<&Path> {
        self.file.path()
    }

    /// The name the tab row draws for this scene.
    pub(crate) fn file_name(&self, lang: furnicraft_text::Lang) -> String {
        self.file.file_name(lang)
    }

    /// Whether the document differs from what was last saved.
    pub(crate) fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    /// Whether the header's undo button has an entry to take back.
    pub(crate) fn can_undo(&self) -> bool {
        self.session.can_undo()
    }

    /// Whether the header's redo button has an entry to reapply.
    pub(crate) fn can_redo(&self) -> bool {
        self.session.can_redo()
    }

    /// The render target this tab draws into, so a close can free it.
    pub(crate) fn texture_id(&self) -> egui::TextureId {
        self.texture_id
    }

    /// Whether the ribbon of this scene is unfolded.
    pub(crate) fn ribbon_open(&self) -> bool {
        self.editor.ribbon_open()
    }

    /// Folds or unfolds the ribbon of this scene, returning its new state.
    ///
    /// The fold is window chrome rather than scene data, so the shell writes it to the workspace
    /// settings and hands it to every open tab; the document only carries it to its editor.
    pub(crate) fn set_ribbon_open(&mut self, open: bool) -> bool {
        self.editor.set_ribbon_open(open)
    }

    /// The method each tool is pinned to, which is window chrome rather than scene data.
    pub(crate) fn pinned_variants(&self) -> &std::collections::BTreeMap<String, String> {
        self.editor.pinned_variants()
    }

    /// Replaces the pinned methods of this scene's ribbon from the workspace settings.
    pub(crate) fn set_pinned_variants(&mut self, pinned: std::collections::BTreeMap<String, String>) {
        self.editor.set_pinned_variants(pinned);
    }

    /// The selection and creation target the palette's parameter builders read.
    pub(crate) fn palette_context(&self) -> PaletteContext {
        PaletteContext::new(self.editor.selected_nodes().to_vec(), self.editor.creation_target(self.session.project()))
    }

    /// Draws the scene tree's body.
    ///
    /// The tree writes the session's hidden and locked flags directly, so the frame it just changed
    /// is rebuilt here instead of waiting a frame: `refresh` returns at once when neither the
    /// document nor the visible set moved.
    pub(crate) fn show_scene_tree(&mut self, ui: &mut egui::Ui) {
        self.editor.show_scene_tree(ui, &mut self.session);
        if let Err(error) = self.refresh() {
            self.last_error = Some(error);
        }
    }

    /// Draws the inspector's body.
    pub(crate) fn show_card(&mut self, ui: &mut egui::Ui) {
        self.editor.show_card(ui, &mut self.session);
    }

    /// Takes back one command and reports a refusal in the status bar.
    pub(crate) fn undo(&mut self) {
        if let Err(error) = self.session.undo() {
            self.last_error = Some(error.to_string());
        }
    }

    /// Reapplies one undone command and reports a refusal in the status bar.
    pub(crate) fn redo(&mut self) {
        if let Err(error) = self.session.redo() {
            self.last_error = Some(error.to_string());
        }
    }

    /// The refusal a destructive palette command gets while the primary selection is locked.
    pub(crate) fn locked_command_refusal(&self, id: &str, lang: Lang) -> Option<String> {
        locked_command_refusal(&self.editor, self.session.project(), id, lang)
    }

    /// The save the header's button and `Ctrl+S` ask for: the current path, or the path dialog.
    pub(crate) fn request_save(&mut self) -> Option<FileAction> {
        self.file.save()
    }

    /// Opens the Open dialog from the Home surface, which has no File menu of its own.
    pub(crate) fn request_open(&mut self) {
        self.file.request_open();
    }

    /// Draws the File menu button and its dialogs.
    pub(crate) fn show_file_menu(&mut self, ui: &mut egui::Ui) -> Option<FileAction> {
        self.file.show(ui)
    }

    /// Draws the dialogs the File menu opened and returns the operation one of them asked for.
    pub(crate) fn show_dialogs(&mut self, ctx: &egui::Context) -> Option<FileAction> {
        self.file.show_dialogs(ctx)
    }

    /// Re-uploads the frame when the selection moved.
    ///
    /// Selecting is view state, so the revision does not move; the uploaded highlight is the copy
    /// the last frame was built with, and a difference means the drawn outline is stale.
    pub(crate) fn sync_selection(&mut self) -> Result<(), String> {
        match self.editor.selected_nodes() == self.highlighted {
            true => Ok(()),
            false => self.upload_scene(),
        }
    }

    /// Applies the viewport section of the workspace settings to this scene.
    ///
    /// The style default applies to the active view at once and the other views keep theirs; a grid,
    /// edge-list or theme change rebuilds the uploaded frame, while an edge style is frame data the
    /// renderer reads at draw time.
    pub(crate) fn apply_viewport_settings(
        &mut self,
        before: &ViewportSettings,
        settings: &ViewportSettings,
        shell: &Shell,
    ) {
        let previous = self.display;
        self.display = Display::of(settings, &shell.cluster, shell.colors);
        self.layout.set_active_style(settings.style);
        self.layout.apply_frame_style(&mut self.scene);
        self.display.apply(&mut self.scene);
        let geometry_changed = settings.grid != before.grid
            || settings.grid_step_mm != before.grid_step_mm
            || settings.all_edges != before.all_edges
            || self.display.colors != previous.colors;
        if geometry_changed && let Err(error) = self.upload_scene() {
            self.last_error = Some(error);
        }
    }

    /// Runs the global shortcuts the shell read.
    ///
    /// The keys are read by the shell, because a modal or a focused text field takes them first, but
    /// everything they act on belongs to the active document: one tab's undo never touches another.
    /// The save is returned rather than performed: writing a file is the shell's job, because it
    /// also remembers the path in the workspace settings.
    pub(crate) fn handle_shortcuts(&mut self, shortcuts: Shortcuts) -> Option<FileAction> {
        let save = if shortcuts.save { self.file.save() } else { None };
        if shortcuts.undo {
            self.undo();
        }
        if shortcuts.redo {
            self.redo();
        }
        if shortcuts.frame {
            self.frame_all();
        }
        if shortcuts.projection {
            self.toggle_view_projection();
        }
        if shortcuts.maximize {
            self.layout.toggle_maximized();
        }
        save
    }

    /// The projection the active view is drawing with, for the status bar's readout.
    pub(crate) fn projection(&self) -> Projection {
        self.scene.camera.projection()
    }

    /// Whether the status bar should say «Перестроение…» on this frame.
    ///
    /// The shell asks before it syncs, so the frame that announces a heavy rebuild is the one that
    /// paints the message and the last good geometry, and the work runs on the frame after it. A
    /// cheap rebuild is never announced: it costs less than a frame and the message would be noise.
    pub(crate) fn busy(&self, viewport: &ViewportSettings, cluster: &ViewCluster, colors: SceneColors) -> bool {
        self.rebuild.busy(self.rebuild_pending(viewport, cluster, colors))
    }

    /// Whether the next [`Document::sync_scene`] will evaluate the scene.
    fn rebuild_pending(&self, viewport: &ViewportSettings, cluster: &ViewCluster, colors: SceneColors) -> bool {
        Display::of(viewport, cluster, colors).colors != self.display.colors
            || self.session.project().revision() != self.scene_revision
            || self.editor.visibility_revision() != self.scene_visibility
            || self.session.preview_version() != self.scene_preview
    }

    /// Takes the window's display state and rebuilds the frame when the document moved.
    ///
    /// The shell calls this once per frame for the active tab: the display switches belong to the
    /// window, the geometry to the tab, and a scene that did not move costs nothing. A scene whose
    /// last rebuild overran [`SLOW_REBUILD`] waits one frame here, so the busy state the shell just
    /// painted is on screen while the work runs; `ctx` asks for that frame.
    pub(crate) fn sync_scene(
        &mut self,
        ctx: &egui::Context,
        viewport: &ViewportSettings,
        cluster: &ViewCluster,
        colors: SceneColors,
    ) -> Result<(), String> {
        let previous = self.display;
        self.display = Display::of(viewport, cluster, colors);
        if self.display.colors != previous.colors {
            // A theme switch touches only the frame's own colours, so the uploaded frame is repainted
            // without evaluating a single node: the document revision does not move (X8).
            self.display.apply(&mut self.scene);
            return self.upload_scene();
        }
        let pending = self.rebuild_pending(viewport, cluster, colors);
        if self.rebuild.defer(pending) {
            ctx.request_repaint();
            return Ok(());
        }
        self.refresh()
    }

    /// Re-evaluates the scene when the document revision, the visibility or the preview moved.
    ///
    /// Every operation that changes the document ends here, so a new frame is uploaded before the
    /// viewport draws again: the evaluated geometry, this tab's per-view styles and the window's
    /// display state are composed into one frame. The evaluation is timed, so the next rebuild of a
    /// scene that takes longer than a frame is announced before it runs (M71).
    fn refresh(&mut self) -> Result<(), String> {
        let revision = self.session.project().revision();
        let visibility = self.editor.visibility_revision();
        let preview = self.session.preview_version();
        if revision == self.scene_revision && visibility == self.scene_visibility && preview == self.scene_preview {
            return Ok(());
        }
        // A delete or an undo can take a flag with it; pruning before the rebuild keeps a stale
        // hidden or locked id out of the frame the user is about to see.
        self.editor.retain_flags(self.session.project());
        let visibility = self.editor.visibility_revision();
        let started = Instant::now();
        let evaluated = evaluate(&mut self.cache, &mut self.kernel, &self.session, self.editor.flags())?;
        self.scene_bounds = evaluated.bounds;
        let mut scene = evaluated.frame;
        self.layout.apply_frame_style(&mut scene);
        self.display.apply(&mut scene);
        scene.camera = crate::scene_build::scene_camera(self.session.project(), self.layout.active_camera());
        self.scene = scene;
        self.scene_revision = revision;
        self.scene_visibility = visibility;
        self.scene_preview = preview;
        let uploaded = self.upload_scene();
        self.rebuild.record(started.elapsed());
        uploaded
    }

    pub(crate) fn upload_scene(&mut self) -> Result<(), String> {
        let selection = self.editor.selected_nodes();
        let mut geometry = Vec::new();
        for node in self.session.project().top_most_nodes(selection) {
            geometry.extend(self.session.project().geometry_ids_under(node).unwrap_or_default());
        }
        let frame = self.scene.with_selection_highlight(&geometry);
        let frame = match self.preview() {
            Some(Preview::Translation { drawables, delta }) => frame.with_world_translation(drawables, delta),
            Some(Preview::Rotation { drawables, pivot, rotation }) => {
                frame.with_world_rotation(drawables, pivot, rotation)
            }
            None => frame,
        };
        let frame = match self.vertex_gizmo.preview() {
            Some(preview) => preview_vertex_mesh(frame, self.session.project(), preview)?,
            None => frame,
        };
        // Keep the chains of the frame that is about to be uploaded, after every preview, so the
        // screen-space outline is drawn where the renderer draws the path.
        self.selected_chains = frame.polylines().iter().filter(|chain| geometry.contains(&chain.id)).cloned().collect();
        self.renderer.upload(&self.render_state.device, &frame).map_err(|error| error.to_string())?;
        self.highlighted.clear();
        self.highlighted.extend_from_slice(selection);
        self.uploaded_preview = self.preview_key();
        Ok(())
    }

    /// The scene change the active gizmo is previewing.
    fn preview(&self) -> Option<Preview<'_>> {
        if self.editor.contour_edit().is_some() || self.editor.mesh_edit().is_some() {
            return None;
        }
        // Only the armed gizmo is ever updated, so at most one of them holds a drag; asking both
        // means the frame follows the drag that is actually live even if the mode changed under it.
        self.translate_gizmo.preview().or_else(|| self.rotate_gizmo.preview())
    }

    /// The preview currently shown by the renderer.
    ///
    /// A contour edit paints its working copy as an overlay; the document solid is not rebuilt until
    /// the single command that applies the draft.
    fn preview_key(&self) -> Option<PreviewKey> {
        if self.editor.contour_edit().is_some() {
            return None;
        }
        if self.editor.mesh_edit().is_some() {
            return self.vertex_gizmo.preview_key();
        }
        self.translate_gizmo.preview_key().or_else(|| self.rotate_gizmo.preview_key())
    }

    /// Applies the live contour draft as one command and rebuilds the scene.
    pub(crate) fn apply_contour_edit(&mut self) -> Result<(), String> {
        self.editor.apply_contour_edit(&mut self.session)?;
        self.contour_gizmo.reset();
        self.refresh()
    }

    /// Leaves contour editing and throws the working copy away.
    pub(crate) fn cancel_contour_edit(&mut self) {
        self.editor.end_contour_edit();
        self.contour_gizmo.reset();
    }

    /// Leaves a drawing tool and throws the draft away.
    pub(crate) fn cancel_drawing(&mut self) {
        self.editor.cancel_drawing();
        self.draw_gizmo.reset();
    }

    /// Commits the live drawing on the work plane and ends the tool.
    ///
    /// The plane is the one the draft started on, so a commit does not depend on where the camera
    /// has orbited since. A refused path leaves the draft live and reports why.
    pub(crate) fn commit_drawing(&mut self, lang: Lang) -> Result<(), String> {
        let placement = self.drawing_placement()?;
        self.editor.commit_drawing(&mut self.session, placement, lang)?;
        self.end_drawing()
    }

    /// Closes the running ring and commits it in one step.
    ///
    /// The only close path: a click on the first draft point, the ribbon's *Close* and the status
    /// bar's *Close* all end here, so no surface can close a ring and forget to commit it (ADR 0052).
    /// The ring is closed before the commit and reopened if the document refuses it, so a refusal
    /// leaves the shape the user drew editable instead of a finished draft that cannot be changed.
    pub(crate) fn close_draft(&mut self, lang: Lang) -> Result<(), String> {
        let placement = self.drawing_placement()?;
        self.editor.close_and_commit_drawing(&mut self.session, placement, lang)?;
        self.end_drawing()
    }

    /// The placement of the work plane the live draft draws on.
    fn drawing_placement(&self) -> Result<Placement, String> {
        self.draw_gizmo
            .plane(self.scene.camera)
            .placement()
            .ok_or_else(|| "the work plane has no usable placement".to_owned())
    }

    /// Drops the pointer state a finished drawing leaves behind and rebuilds the scene.
    pub(crate) fn end_drawing(&mut self) -> Result<(), String> {
        self.draw_gizmo.reset();
        self.highlighted.clear();
        self.refresh()
    }

    /// Re-uploads the scene when the drag preview moved to a new position.
    ///
    /// A finished or cancelled drag also lands here with no preview, which restores the base
    /// upload. The key comparison keeps a still pointer from rebuilding the geometry every frame.
    /// A camera or light drag moves no drawable, so it only updates the lighting uniform and the
    /// glyphs; uploading the unchanged geometry again would cost a full vertex pass per frame.
    fn sync_preview(&mut self) -> Result<(), String> {
        if self.preview_key() == self.uploaded_preview {
            return Ok(());
        }
        if let Some(preview) = self.preview()
            && preview.drawables().is_empty()
        {
            self.uploaded_preview = self.preview_key();
            return Ok(());
        }
        self.upload_scene()
    }

    /// World placements the active drag previews for cameras and lights.
    ///
    /// A camera or a light has no drawable, so the scene frame cannot carry its preview. This list
    /// lets the icon layer and the lighting follow the pointer in the same frame as the axis
    /// handles instead of waiting for the command that lands on release.
    fn previewed_scene_placements(&self) -> Vec<(Id, Placement)> {
        preview_nodes(self.session.project(), self.editor.selected_nodes(), self.preview())
    }

    /// The vertices the active sub-object selection moves.
    ///
    /// Vertex mode is the stored list; face mode is the union of every selected polygon, so the
    /// gizmo, the preview and the dispatched command all see one deduplicated set.
    fn mesh_selection_vertices(&self, object: Id) -> Vec<u32> {
        let Some(edit) = self.editor.mesh_edit() else {
            return Vec::new();
        };
        let Some(mesh) = self.session.project().object(object).and_then(|object| object.geometry().as_mesh()) else {
            return Vec::new();
        };
        edit.selected_vertices(mesh)
    }

    /// Applies one event from the pull arrow.
    ///
    /// A drag only previews the height: the overlay shows the body the pull would produce and the
    /// document is not written. A click finishes the pull, and finishing is the one command.
    fn apply_extrude_event(&mut self, event: ExtrudeEvent) -> Result<(), String> {
        match event {
            ExtrudeEvent::Preview { object, height } => self.preview_extrude(object, height),
            ExtrudeEvent::Finish { object, height } => self.finish_extrude(object, height),
        }
    }

    /// Shows the pull's height as a session preview, without writing the document.
    ///
    /// The preview is the body itself at that height, evaluated from the same parameters the ✓
    /// would store, so the user sees the solid and not a stand-in for it. It moves no revision and
    /// leaves no undo entry (M71).
    fn preview_extrude(&mut self, object: Id, height: Length) -> Result<(), String> {
        let geometry = previewed_body(self.session.project(), object, height)?;
        self.session.set_preview_geometry(object, geometry);
        self.refresh()
    }

    /// Writes the pull's height once and drops the preview.
    ///
    /// Idempotent by construction: after the first call the pull is gone, so a second ✓ — a double
    /// click, a key and a button in one frame — has no object to write and dispatches nothing.
    pub(crate) fn finish_extrude(&mut self, object: Id, height: Length) -> Result<(), String> {
        if extrude_writes(self.extrude_gizmo.committed(), height) {
            self.editor.set_extrude_height(&mut self.session, object, height)?;
        }
        self.extrude_gizmo.cancel();
        self.session.clear_preview();
        self.refresh()
    }

    /// Drops the pull and its preview, leaving the body at the height the document holds.
    ///
    /// This is the ✕ and the Escape of the command bar: nothing was written, so restoring the
    /// pre-command state is dropping the overlay and evaluating the document again.
    pub(crate) fn cancel_extrude(&mut self) {
        if !self.extrude_gizmo.is_active() {
            return;
        }
        self.extrude_gizmo.cancel();
        self.session.clear_preview();
        if let Err(error) = self.refresh() {
            self.last_error = Some(error);
        }
    }

    /// Draws the command bar of the previewing operation and applies what it asked for.
    ///
    /// The bar is the one surface a previewing operation talks to: its field drives the preview
    /// live, the ✓ commits it with a single command and the ✕ drops it. A number that leaves the
    /// field's range or cannot be parsed is refused by the field itself, so the preview keeps the
    /// last height the document could hold.
    fn show_command_bar(&mut self, ui: &mut egui::Ui, viewport: egui::Rect, opacity: u32) {
        let Some(object) = self.extrude_gizmo.object() else {
            self.command_bar_rect = None;
            return;
        };
        let Some(height) = self.extrude_gizmo.height() else {
            self.command_bar_rect = None;
            return;
        };
        let focus = self.extrude_gizmo.take_focus();
        let mut millimetres = height.millimetres_f64();
        let maximum = (furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() / 1_000) as f64;
        let outcome = command_bar::show(
            ui,
            viewport,
            CommandBar {
                label: Message::CmdSetExtrusionHeight,
                value: &mut millimetres,
                unit: "mm",
                range: crate::extrude_gizmo::MIN_HEIGHT_MM..=maximum,
                decimals: 3,
                focus,
            },
            opacity,
        );
        // A press on the bar is chrome, exactly like a press on a panel: the bar floats over the
        // scene, so its rectangle is handed to the viewport's own hit test next frame.
        self.command_bar_rect = Some(outcome.rect);
        // The field's number is the preview: a drag or a typed value shows the body at that height.
        let whole = millimetres.round();
        if let Some(next) = Length::from_millimetres(whole as i64)
            && next != height
            && let Err(error) = self.preview_extrude(object, next)
        {
            self.last_error = Some(error);
        }
        match outcome.action {
            Some(CommandBarAction::Apply) => {
                let height = self.extrude_gizmo.height().unwrap_or(height);
                if let Err(error) = self.finish_extrude(object, height) {
                    self.last_error = Some(error);
                }
            }
            Some(CommandBarAction::Cancel) => self.cancel_extrude(),
            None => {}
        }
    }

    /// Dispatches one `node.placement.set_many` for a finished drag and rebuilds the scene.
    fn apply_gizmo_commit(&mut self, commit: PlacementCommit) -> Result<(), String> {
        // Defence in depth: the viewport refuses a locked selection before the drag starts, and the
        // commit refuses it again, so no other path can move a locked node either.
        if let Some(name) = locked_change(self.session.project(), &commit, &self.editor) {
            return Err(format!("{name} is locked. Unlock it in the scene tree to move it."));
        }
        let request = NodePlacementSetMany {
            placements: commit
                .changes
                .iter()
                .map(|change| NodePlacement { node: change.node, placement: change.placement })
                .collect(),
        };
        let params =
            serde_json::to_value(request).map_err(|error| format!("could not encode the placement batch: {error}"))?;
        self.session.dispatch(NODE_PLACEMENT_SET_MANY_ID, &params).map_err(|error| error.to_string())?;
        // The preview already showed the new position; rebuilding here keeps the document, the
        // scene and the inspector in agreement without waiting a frame.
        self.refresh()
    }

    /// Dispatches one `mesh.vertices.move` for a finished vertex drag and rebuilds the scene.
    fn apply_mesh_commit(&mut self, commit: MeshVertexCommit) -> Result<(), String> {
        let request = MeshVerticesMove {
            object: commit.object,
            moves: commit
                .moves
                .iter()
                .map(|(vertex, position)| MeshVertexMove { vertex: *vertex, position: *position })
                .collect(),
        };
        let params =
            serde_json::to_value(request).map_err(|error| format!("could not encode the vertex batch: {error}"))?;
        self.session.dispatch(MESH_VERTICES_MOVE_ID, &params).map_err(|error| error.to_string())?;
        self.refresh()
    }

    /// Fits the active view on the scene's current bounds.
    ///
    /// The fit camera is computed from the latest evaluation on every press, so *Frame all* follows
    /// objects created or moved since the file opened. Only the active view moves: its direction
    /// and its projection kind stay as the user left them. The fit targets the part of the view the
    /// scene rectangle covers — everything below the floating ribbon — so the scene is never framed
    /// under the strip (U51). The side panels float over that rectangle without narrowing it, so a
    /// fit is unchanged by folding or hiding one (U72).
    pub(crate) fn frame_all(&mut self) {
        let fit = match home_camera(self.scene_bounds) {
            Ok(fit) => fit,
            Err(error) => {
                self.last_error = Some(error);
                return;
            }
        };
        // The view rectangle already starts below the ribbon band and the panels never narrow it,
        // so the fit needs no further narrowing: the whole scene lands below the strip (U72).
        self.layout.active_leaf_mut().frame(fit);
        self.scene.camera = crate::scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// Returns the active view to *Axonometric* without refitting the scene.
    ///
    /// The preset writes yaw and pitch only, so the model does not move and the view keeps its
    /// target, its distance and its projection (ADR 0049).
    pub(crate) fn home_view(&mut self) {
        self.layout.active_leaf_mut().home();
        self.scene.camera = crate::scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// Switches the active view's projection and refreshes the camera the viewport draws with.
    ///
    /// The ribbon, the Display popover and the corner widget all end here, so the three entry points
    /// cannot disagree. The projection is view state, so this is not a command and leaves no undo
    /// entry (ADR 0049).
    pub(crate) fn toggle_view_projection(&mut self) {
        self.layout.active_leaf_mut().toggle_projection();
        let camera = self.layout.active_camera();
        self.scene.camera = crate::scene_build::scene_camera(self.session.project(), camera);
    }

    pub(crate) fn viewport(
        &mut self,
        ui: &mut egui::Ui,
        shell: &mut Shell,
        viewport: &ViewportSettings,
        chrome: &Chrome,
        opacity: u32,
    ) {
        // The toolbar can be wider than the panel in a narrow window; egui then reports the
        // overflowing width as available. Intersecting with the clip rect keeps the viewport and
        // its orientation widget inside the central panel.
        let area = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        if area.width() < 1.0 || area.height() < 1.0 {
            return;
        }
        let (_, response) = ui.allocate_exact_size(area.size(), egui::Sense::click_and_drag());
        if !ui.is_rect_visible(area) {
            return;
        }
        // The render target covers the whole central area, including the bands under the floating
        // ribbon and the translucent panels, so the scene image runs under the chrome (M2). The
        // split views tile the scene rectangle, which depends on the ribbon band only: the panels
        // float over it and folding one moves no view and no camera (U72). A pointer on the chrome
        // is never a viewport hit.
        shell.pointer_position = ui.input(|input| input.pointer.hover_pos());
        let pointer = shell.pointer_position;
        // A press on the command bar is chrome too: the bar floats over the scene, so a click on
        // its field or its buttons is never a pick. The rectangle is the one the bar drew last
        // frame, which is the one on screen while the bar stays open.
        let pointer_on_chrome = pointer.is_some_and(|pointer| {
            chrome.covers(pointer) || self.command_bar_rect.is_some_and(|bar| bar.contains(pointer))
        });
        let content = chrome.content(area);

        // The viewport is the whole central area; the layout divides the scene rectangle into
        // views. Every view has its own camera and every border is draggable. A press on a border
        // resizes without moving the target; a press anywhere else activates that view, whichever
        // button it is, and only the active view is ever manipulated (ADR 0079).
        let regions = self.layout.regions(content);
        let pressed = ui.input(|input| input.pointer.any_pressed());
        if pressed
            && !pointer_on_chrome
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let primary = ui.input(|input| input.pointer.primary_pressed());
            if self.layout.press(content, pointer, primary) {
                // A border drag owns the primary button until it is released; nothing else in the
                // workspace reads the pointer meanwhile, so a resize cannot select an object or
                // move an axis under the user's hand.
                shell.divider_dragging = true;
            }
        }
        if shell.divider_dragging {
            let delta = response.drag_delta();
            self.layout.drag_by(content, delta);
            if !ui.input(|input| input.pointer.primary_down()) {
                self.layout.end_drag();
                shell.divider_dragging = false;
            }
            if let Some(axis) = self.layout.dragging_axis() {
                ui.ctx().set_cursor_icon(axis.cursor());
            }
            ui.ctx().request_repaint();
        } else if !pointer_on_chrome
            && let Some(pointer) = pointer
            && let Some(axis) = self.layout.border_at(content, pointer)
        {
            ui.ctx().set_cursor_icon(axis.cursor());
        }

        // The active view is the only target: orbit, pan, zoom, the presets, selection, the gizmos
        // and the cluster all act on it. A press is how the user points them at a neighbour.
        let target_leaf = self.layout.active();
        let target_rect =
            regions.iter().find(|(leaf, _)| *leaf == target_leaf).map(|(_, rect)| *rect).unwrap_or(content);
        let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or_default();
        let pointer_over_active =
            pointer.is_some_and(|pointer| !chrome.covers(pointer) && target_rect.contains(pointer));
        // The scene's camera is the one the active view draws with; an authored camera wins over
        // both, exactly as it does for a single view (ADR 0049).
        self.scene.camera = crate::scene_build::scene_camera(self.session.project(), target_camera);
        let rect = target_rect;
        // The orientation widget belongs to the free corner of the view: below the ribbon band and
        // clear of both floating panels, mirrored to the left when the inspector leaves no room
        // (U73). The display-settings popover can turn it off, and a hidden widget draws nothing and
        // takes no click.
        let gizmo_rect = widget_placement(&shell.cluster, rect, chrome);

        let points_per_pixel = ui.ctx().pixels_per_point();
        let width = (area.width() * points_per_pixel).round().max(1.0) as u32;
        let height = (area.height() * points_per_pixel).round().max(1.0) as u32;
        let device = self.render_state.device.clone();
        if self.renderer.resize(&device, width, height) {
            self.render_state.renderer.write().update_egui_texture_from_wgpu_texture(
                &device,
                self.renderer.color_view(),
                wgpu::FilterMode::Linear,
                self.texture_id,
            );
        }

        let pointer_over_widget = !pointer_on_chrome
            && response.hover_pos().is_some_and(|pointer| gizmo_rect.is_some_and(|widget| widget.contains(pointer)));
        // A border drag owns the pointer: the tools, the gizmos and picking all stay out of its way
        // until it is released, so a resize can never select an object or move an axis.
        //
        // A pointer on the chrome owns the pointer only while no gesture is live: a panel or the
        // ribbon band is not the scene, so a press there never starts a gizmo, a draft, a selection
        // rectangle or a navigation drag. A gesture that already started keeps the pointer even if
        // the user drags it over a panel, so releasing there still commits the move.
        let capturing = self.translate_gizmo.is_dragging()
            || self.rotate_gizmo.is_dragging()
            || self.vertex_gizmo.is_dragging()
            || self.contour_gizmo.is_dragging()
            || self.extrude_gizmo.is_dragging()
            || self.gizmo.is_dragging();
        let interactive = !shell.divider_dragging && (!pointer_on_chrome || capturing);
        // A contour edit owns the pointer: the object gizmo and picking would fight the corner drag.
        let contour_editing = interactive && self.editor.contour_edit().is_some();
        // A drawing tool owns it too: the left button places points, not selections. A click on the
        // chrome is never a point, so the tool waits for a click on the scene.
        let drawing = interactive && !pointer_on_chrome && self.editor.drawing().is_some();
        // An armed primitive owns the next click: it places a cube, it does not select.
        let placing = interactive && !pointer_on_chrome && self.editor.pending_primitive().is_some();
        // The pull arrow is the live gesture after an extrusion. A contour edit, a draft, a
        // placement or a sub-object mode takes the pointer from it, so the arrow never competes
        // with a grip, a point or a cube for the same click.
        if self.extrude_gizmo.is_active()
            && (contour_editing || drawing || placing || self.editor.mesh_edit().is_some())
        {
            self.cancel_extrude();
        }
        let pulling = interactive && self.extrude_gizmo.is_active();
        // An armed navigation gesture owns a primary-button drag, but not the other tools: while a
        // contour edit, a draft or a sub-object mode is live, that mode keeps the pointer.
        let navigation = shell.cluster.navigation;
        let navigating = navigation.is_some() && !contour_editing && !drawing && !placing && !pulling;
        let mesh_object = if !interactive || contour_editing || drawing || placing || pulling {
            None
        } else {
            self.editor.mesh_edit().map(|edit| edit.object())
        };
        let navigating = navigating && mesh_object.is_none();
        // Face mode moves the union of the selected faces, so both sub-object modes feed the same
        // vertex gizmo and dispatch the same vertex command.
        let mesh_vertices = mesh_object.map(|object| self.mesh_selection_vertices(object)).unwrap_or_default();
        // Vertex editing owns the pointer: the object gizmo would move the whole mesh under the
        // user's hand, so it is neither updated nor painted until the mode ends.
        let vertex_update = if let Some(object) = mesh_object {
            let target = vertex_gizmo::MeshTarget {
                camera: self.scene.camera,
                project: self.session.project(),
                object,
                vertices: &mesh_vertices,
            };
            self.vertex_gizmo.update(ui, &response, rect, target, pointer_over_widget)
        } else {
            self.vertex_gizmo.reset();
            Ok(None)
        };
        match vertex_update {
            Ok(Some(commit)) => {
                if let Err(error) = self.apply_mesh_commit(commit) {
                    self.last_error = Some(error);
                }
            }
            Ok(None) => {}
            Err(error) => self.last_error = Some(error),
        }

        if contour_editing {
            match self.contour_gizmo.update(
                &response,
                rect,
                self.scene.camera,
                self.session.project(),
                &mut self.editor,
            ) {
                Ok(true) => ui.ctx().request_repaint(),
                Ok(false) => {}
                Err(error) => self.last_error = Some(error),
            }
        } else {
            self.contour_gizmo.reset();
        }

        if drawing {
            // Shift locks the next segment to an axis; Alt drops the end and grid snaps and takes
            // the pointer as it is. Tab, handled above, chooses straight or arc input.
            let modifiers = DrawModifiers {
                axis_lock: ui.input(|input| input.modifiers.shift),
                ignore_snaps: ui.input(|input| input.modifiers.alt),
            };
            let action = match self.draw_gizmo.update(
                &response,
                rect,
                self.scene.camera,
                &mut self.editor,
                pointer_over_widget,
                modifiers,
            ) {
                Ok(action) => action,
                Err(error) => {
                    self.last_error = Some(error);
                    DrawAction::Nothing
                }
            };
            // A fixed tool is done on its last point, and a polyline is closed through its first
            // point: both finish the drawing the way a CAD tool does, but only a close commits
            // through the shared close path.
            let ended = match action {
                DrawAction::Point => {
                    ui.ctx().request_repaint();
                    if self.editor.drawing().is_some_and(DrawDraft::is_finished) {
                        if let Err(error) = self.commit_drawing(furnicraft_ui::language(ui.ctx())) {
                            self.last_error = Some(error);
                        }
                        true
                    } else {
                        false
                    }
                }
                DrawAction::Close => {
                    ui.ctx().request_repaint();
                    if let Err(error) = self.close_draft(furnicraft_ui::language(ui.ctx())) {
                        self.last_error = Some(error);
                    }
                    true
                }
                DrawAction::Nothing => false,
            };
            // The right button commits the figure like Enter; it is the CAD habit and keeps the
            // left button free for placing points.
            if !ended
                && !pointer_over_widget
                && response.clicked_by(egui::PointerButton::Secondary)
                && let Err(error) = self.commit_drawing(furnicraft_ui::language(ui.ctx()))
            {
                self.last_error = Some(error);
            }
        } else {
            self.draw_gizmo.reset();
        }

        if pulling {
            // The pull reads the pointer itself: a press on the arrow starts a drag, the release
            // writes the height and keeps pulling, and a click anywhere else finishes. One drag is
            // one `object.extrude.height.set`, so the whole adjustment is a single undo entry.
            match self.extrude_gizmo.update(ui, &response, rect, self.scene.camera, self.session.project()) {
                Ok(Some(event)) => {
                    if let Err(error) = self.apply_extrude_event(event) {
                        self.last_error = Some(error);
                    }
                }
                Ok(None) => {}
                Err(error) => self.last_error = Some(error),
            }
        }

        if placing {
            // The armed cube lands where the user clicks: the plane point is its origin and the
            // plane's own rotation keeps its local axes on the plane.
            if response.clicked_by(egui::PointerButton::Primary)
                && !pointer_over_widget
                && !pointer_on_chrome
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let plane = WorkPlane::from_camera(self.scene.camera);
                let placement =
                    plane.pointer_point(self.scene.camera, rect, pointer).and_then(|local| plane.placement_at(local));
                match placement.map(|placement| self.editor.place_primitive(&mut self.session, placement)) {
                    Some(Ok(_)) => {
                        if let Err(error) = self.refresh() {
                            self.last_error = Some(error);
                        }
                    }
                    Some(Err(error)) => self.last_error = Some(error),
                    None => self.last_error = Some("The work plane has no point under the cursor.".to_owned()),
                }
            }
        }

        let target = gizmo_math::Target {
            camera: self.scene.camera,
            project: self.session.project(),
            selection: self.editor.selected_nodes(),
        };
        // A locked node keeps its transform: the guard is checked before the gizmo is updated, so a
        // drag never starts and no command is ever dispatched (ADR 0065). The status bar names the
        // node, so the missing handles read as a refusal instead of a bug.
        let locked_selection = self.editor.locked_selection(self.session.project());
        let update: Result<Option<PlacementCommit>, String> = if !interactive
            || contour_editing
            || drawing
            || placing
            || pulling
            || navigating
            || mesh_object.is_some()
            || locked_selection.is_some()
        {
            Ok(None)
        } else {
            match shell.mode {
                TransformMode::Move => self.translate_gizmo.update(ui, &response, rect, target, pointer_over_widget),
                TransformMode::Rotate => self.rotate_gizmo.update(ui, &response, rect, target, pointer_over_widget),
            }
        };
        match update {
            Ok(Some(commit)) => {
                if let Err(error) = self.apply_gizmo_commit(commit) {
                    self.last_error = Some(error);
                }
            }
            Ok(None) => {}
            Err(error) => self.last_error = Some(error),
        }
        if let Err(error) = self.sync_preview() {
            self.last_error = Some(error);
        }
        // A camera or a light has no drawable, so its glyph and its contribution to the frame are
        // driven by this list rather than by the uploaded geometry.
        let previewed = self.previewed_scene_placements();
        let dragging = if contour_editing {
            self.contour_gizmo.is_dragging()
        } else if pulling {
            self.extrude_gizmo.is_dragging()
        } else if mesh_object.is_some() {
            self.vertex_gizmo.is_dragging()
        } else {
            self.translate_gizmo.is_dragging() || self.rotate_gizmo.is_dragging()
        };
        let consumed_click = if contour_editing || drawing || placing || pulling {
            true
        } else if mesh_object.is_some() {
            self.vertex_gizmo.consumed_click()
        } else {
            match shell.mode {
                TransformMode::Move => self.translate_gizmo.consumed_click(),
                TransformMode::Rotate => self.rotate_gizmo.consumed_click(),
            }
        };

        // Looking through an authored camera pins the view: navigation would move a camera the
        // document owns, and a command is the only way to edit document data. The gesture acts on
        // the view under the pointer, so an orbit in one view leaves its neighbour alone.
        let navigable = viewport_owns_view(self.session.project());
        let navigation_rect = target_rect;
        let navigable = navigable && interactive;
        if navigable && response.hovered() && pointer_over_active && !dragging {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0
                && let Some(leaf) = self.layout.leaf_mut(target_leaf)
            {
                leaf.zoom((-f64::from(scroll) * ZOOM_PER_SCROLL_POINT).exp());
            }
        }

        let drag = response.drag_delta();
        // An armed navigation mode turns the primary button into the camera gesture; the gizmo is
        // neither updated nor painted then, so the drag cannot move an axis under the user's hand.
        if navigable
            && !dragging
            && !self.gizmo.is_dragging()
            && navigating
            && response.dragged_by(egui::PointerButton::Primary)
        {
            match navigation {
                Some(NavigationMode::Orbit) => {
                    if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                        leaf.orbit(
                            -f64::from(drag.x) * ORBIT_RADIANS_PER_POINT,
                            -f64::from(drag.y) * ORBIT_RADIANS_PER_POINT,
                        );
                    }
                }
                Some(NavigationMode::Pan) => {
                    if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                        leaf.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(navigation_rect.height()));
                    }
                }
                Some(NavigationMode::Zoom) => {
                    if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                        leaf.zoom((-f64::from(drag.y) * ZOOM_PER_SCROLL_POINT).exp());
                    }
                }
                None => {}
            }
        }
        // An axis drag owns the pointer; rotating the camera mid-drag would move the axis under the
        // pointer and change the drag parameter under the user's hand.
        if navigable && !dragging && !self.gizmo.is_dragging() && response.dragged_by(egui::PointerButton::Middle) {
            let modifiers = ui.input(|input| input.modifiers);
            if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                if modifiers.shift {
                    leaf.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(navigation_rect.height()));
                } else if modifiers.ctrl {
                    leaf.zoom((-f64::from(drag.y) * ZOOM_PER_SCROLL_POINT).exp());
                } else {
                    leaf.orbit(
                        -f64::from(drag.x) * ORBIT_RADIANS_PER_POINT,
                        -f64::from(drag.y) * ORBIT_RADIANS_PER_POINT,
                    );
                }
            }
        } else if navigable
            && !dragging
            && !self.gizmo.is_dragging()
            && response.dragged_by(egui::PointerButton::Secondary)
            && let Some(leaf) = self.layout.leaf_mut(target_leaf)
        {
            leaf.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(navigation_rect.height()));
        }
        // The composite camera follows the navigated view unless an authored camera wins.
        let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or(target_camera);
        self.scene.camera = crate::scene_build::scene_camera(self.session.project(), target_camera);

        if contour_editing {
            ui.ctx().set_cursor_icon(self.contour_gizmo.cursor());
        } else if drawing {
            ui.ctx().set_cursor_icon(self.draw_gizmo.cursor());
        } else if pulling {
            ui.ctx().set_cursor_icon(self.extrude_gizmo.cursor());
        } else if placing {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
        } else if navigating {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        } else if dragging || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }

        let pointer_over_gizmo = response
            .interact_pointer_pos()
            .is_some_and(|pointer| gizmo_rect.is_some_and(|widget| widget.contains(pointer)));
        let clicked = interactive
            && !pointer_on_chrome
            && if mesh_object.is_some() {
                // A double-click would otherwise toggle the same vertex twice; the first click of
                // the pair already selected it.
                response.clicked() && !response.double_clicked()
            } else {
                response.clicked() || response.double_clicked()
            };
        if !contour_editing
            && !drawing
            && !placing
            && !pulling
            && clicked
            && !pointer_over_gizmo
            && !consumed_click
            && let Some(pointer) = response.interact_pointer_pos()
            && rect.width() > 0.0
            && rect.height() > 0.0
        {
            if let Some(object) = mesh_object {
                let element = self.editor.mesh_edit().map(|edit| edit.element()).unwrap_or_default();
                let additive = ui.input(|input| input.modifiers.shift || input.modifiers.ctrl);
                match element {
                    MeshElement::Vertex => {
                        let hit =
                            mesh_pick::pick_vertex(self.scene.camera, rect, self.session.project(), object, pointer);
                        match hit {
                            Some(vertex) => {
                                self.editor.select_mesh_vertex(vertex, additive);
                            }
                            None if !additive => self.editor.clear_mesh_vertices(),
                            None => {}
                        }
                    }
                    MeshElement::Face => {
                        let hit =
                            mesh_pick::pick_face(self.scene.camera, rect, self.session.project(), object, pointer);
                        match hit {
                            Some(face) => {
                                self.editor.select_mesh_face(face, additive);
                            }
                            None if !additive => self.editor.clear_mesh_faces(),
                            None => {}
                        }
                    }
                }
                ui.ctx().request_repaint();
            } else if let Some(id) = scene_gizmo::pick(
                self.scene.camera,
                rect,
                self.session.project(),
                pointer,
                &previewed,
                self.editor.flags(),
            ) {
                // A camera or light icon has no mesh to ray-pick; it selects the node itself, not the
                // container above it.
                self.editor.select_node(Some(id));
                if let Err(error) = self.upload_scene() {
                    self.last_error = Some(error);
                }
            } else {
                let hit = mesh_pick::pick_scene(rect, &self.scene, pointer);
                if response.double_clicked() {
                    self.editor.enter_viewport_level(self.session.project(), hit);
                } else {
                    self.editor.select_viewport_hit(self.session.project(), hit);
                }
                if let Err(error) = self.upload_scene() {
                    self.last_error = Some(error);
                }
            }
        }

        // The style is frame data the renderer reads when it draws, so a button in the cluster
        // changes the next frame without re-uploading a buffer. The edge list is not: switching
        // between form edges and the full triangulation reads a different cached list, so the last
        // upload is replayed when the switch flips.
        self.layout.apply_frame_style(&mut self.scene);
        self.scene.contact_shadows = shell.cluster.shadow_settings(viewport.shadow_strength);
        if self.scene.all_edges != viewport.all_edges {
            self.scene.all_edges = viewport.all_edges;
            if let Err(error) = self.upload_scene() {
                self.last_error = Some(error);
            }
        }
        // A drag preview commits nothing, but the frame can still follow a moved light: the shader
        // reads one uniform, so a sun direction or a point-light position updates without a mesh
        // rebuild. With no moved light the base lighting is already correct.
        let lighting = if previewed.is_empty() {
            self.scene.lighting.clone()
        } else {
            crate::scene_build::authored_lighting_with(self.session.project(), &previewed, self.editor.flags())
                .unwrap_or_else(|| self.scene.lighting.clone())
        };
        // One pass per view into the same target: the camera comes from the view, the region from
        // its rectangle, and the aspect ratio from the region, so a narrow view is not stretched
        // by the workspace's shape.
        let views: Vec<furnicraft_render::ViewSpec> = regions
            .iter()
            .filter_map(|(leaf, view_rect)| {
                let leaf = self.layout.leaf(*leaf)?;
                let camera = crate::scene_build::scene_camera(self.session.project(), leaf.camera());
                Some(furnicraft_render::ViewSpec {
                    camera,
                    region: view_region(area, *view_rect, points_per_pixel),
                    shading: leaf.style(),
                })
            })
            .collect();
        self.renderer.render_views(&self.render_state.device, &self.render_state.queue, &self.scene, &lighting, &views);
        ui.painter().with_clip_rect(area).image(
            self.texture_id,
            area,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        // The overlaid feedback is drawn per view, with that view's camera and rectangle, so a
        // camera glyph or a selection outline lands where its own pass drew the model.
        for (leaf, view_rect) in &regions {
            let active = *leaf == self.layout.active();
            let camera = self
                .layout
                .leaf(*leaf)
                .map(|leaf| crate::scene_build::scene_camera(self.session.project(), leaf.camera()))
                .unwrap_or(self.scene.camera);
            scene_gizmo::paint(
                ui,
                *view_rect,
                camera,
                self.session.project(),
                self.editor.selected_nodes(),
                &previewed,
                self.editor.flags(),
            );
            if active {
                self.paint_mesh_selection(ui, *view_rect);
                self.paint_selection_outline(ui, *view_rect);
            }
        }
        let paint_target = gizmo_math::Target {
            camera: self.scene.camera,
            project: self.session.project(),
            selection: self.editor.selected_nodes(),
        };
        // The object gizmo is hidden in mesh mode: it acts on the whole selection, not on vertices.
        if let Some(edit) = self.editor.contour_edit() {
            let action = self.contour_gizmo.paint(ui, rect, self.scene.camera, self.session.project(), edit);
            if let Some(action) = action {
                let result = match action {
                    contour_gizmo::NumericAction::Apply(grip, value)
                    | contour_gizmo::NumericAction::Restore(grip, value) => {
                        self.editor.contour_edit_set_grip_value(grip, value)
                    }
                    contour_gizmo::NumericAction::ApplyAlong(grip, value, motion) => {
                        if let (ContourGrip::Corner(_), CornerMove::Along(segment), GripValue::Length(length)) =
                            (grip, motion, value)
                        {
                            self.editor.contour_edit_along_length(grip, segment, length)
                        } else {
                            self.editor.contour_edit_set_grip_value(grip, value)
                        }
                    }
                };
                match result {
                    Ok(()) => ui.ctx().request_repaint(),
                    Err(error) => self.last_error = Some(error.to_string()),
                }
            }
        } else if let Some(draft) = self.editor.drawing() {
            self.draw_gizmo.paint(ui, rect, self.scene.camera, draft);
        } else if let Some(object) = mesh_object {
            let target = vertex_gizmo::MeshTarget {
                camera: self.scene.camera,
                project: self.session.project(),
                object,
                vertices: &mesh_vertices,
            };
            self.vertex_gizmo.paint(ui, rect, target);
        } else if pulling {
            self.extrude_gizmo.paint(ui, rect, self.scene.camera, self.session.project());
        } else if navigating || locked_selection.is_some() {
            // The navigation gesture owns the viewport, and a locked selection owns nothing: an
            // axis under the pointer would compete with the camera drag, and a locked node draws no
            // handles because the drag it would start is refused.
        } else {
            match shell.mode {
                TransformMode::Move => self.translate_gizmo.paint(ui, rect, paint_target),
                TransformMode::Rotate => self.rotate_gizmo.paint(ui, rect, paint_target),
            }
        }
        // The widget belongs to the view under the pointer, so clicking an axis snaps that view and
        // the readout names the camera it draws with.
        if self.session.project().active_camera().is_none()
            && let Some(widget) = gizmo_rect
            && let Some(action) =
                self.gizmo.show(ui, widget, target_camera, shell.cluster.angles_visible(), viewport.roll_step_degrees)
        {
            match action {
                gizmo::GizmoAction::LookFrom(direction) => {
                    self.layout.active_leaf_mut().look_from(direction);
                }
                gizmo::GizmoAction::Home => self.home_view(),
                gizmo::GizmoAction::ToggleProjection => self.toggle_view_projection(),
                gizmo::GizmoAction::RollTo(degrees) => {
                    self.layout.active_leaf_mut().roll_to(degrees.radians_f64());
                }
                gizmo::GizmoAction::RollBy(degrees) => {
                    self.layout.active_leaf_mut().roll_by(degrees.radians_f64());
                }
            }
            let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or(target_camera);
            self.scene.camera = crate::scene_build::scene_camera(self.session.project(), target_camera);
        }

        // The cluster floats over the drawn viewport, so it is the last thing painted inside it. A
        // command bar takes the bottom-centre slot while a previewing operation runs, so the
        // cluster moves up by the bar's height and the two never overlap.
        let bar_offset = if self.extrude_gizmo.is_active() { command_bar::BAR_HEIGHT + 6.0 } else { 0.0 };
        let view = ClusterView {
            owns_view: viewport_owns_view(self.session.project()),
            projection: self.scene.camera.projection(),
            work_plane: WorkPlane::from_camera(target_camera).label(),
            layout: self.layout.kind(),
            maximized: self.layout.maximized(),
            preset: self.layout.preset(target_leaf),
            style: self.layout.active_style(),
        };
        match view_cluster::show(ui, content, &mut shell.cluster, view, opacity, bar_offset) {
            Some(ClusterAction::FrameAll) => self.frame_all(),
            Some(ClusterAction::Home) => self.home_view(),
            Some(ClusterAction::ToggleProjection) => self.toggle_view_projection(),
            Some(ClusterAction::SetLayout(kind)) => self.set_layout(kind),
            Some(ClusterAction::ToggleMaximize) => self.layout.toggle_maximized(),
            Some(ClusterAction::SetPreset(preset)) => {
                self.layout.active_leaf_mut().apply_preset(preset);
                let camera = self.layout.active_camera();
                self.scene.camera = crate::scene_build::scene_camera(self.session.project(), camera);
            }
            Some(ClusterAction::SetStyle(style)) => {
                self.layout.set_active_style(style);
                // The style is frame data the renderer reads per view at draw time, so no upload.
                self.layout.apply_frame_style(&mut self.scene);
            }
            Some(ClusterAction::OpenSettings) => shell.open_settings = true,
            None => {}
        }
        self.show_command_bar(ui, content, opacity);
        self.paint_view_frames(ui, content, &regions);
    }

    /// Divides the workspace into `kind`, keeping the cameras the views already have.
    fn set_layout(&mut self, kind: LayoutKind) {
        self.layout.set_kind(kind);
        self.scene.camera = crate::scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// Paints the accent frame of the active view and the borders between views.
    ///
    /// The frame is a thin stroke on the active leaf's rectangle, drawn after the cluster so it
    /// cannot be lost behind a widget; the borders are hairlines on the shared edges and are the
    /// only visible part of the layout, so a split reads as a surface rather than as clipped views.
    fn paint_view_frames(&self, ui: &egui::Ui, area: egui::Rect, regions: &[(crate::view_layout::LeafId, egui::Rect)]) {
        let painter = ui.painter().with_clip_rect(area);
        let line = theme::line(ui);
        for divider in self.layout.dividers(area) {
            let (start, end) = match divider.axis {
                crate::view_layout::SplitAxis::Columns => {
                    let x = divider.first.right();
                    (egui::pos2(x, divider.rect.top()), egui::pos2(x, divider.rect.bottom()))
                }
                crate::view_layout::SplitAxis::Rows => {
                    let y = divider.first.bottom();
                    (egui::pos2(divider.rect.left(), y), egui::pos2(divider.rect.right(), y))
                }
            };
            painter.line_segment([start, end], egui::Stroke::new(1.0, line));
        }
        if regions.len() > 1
            && let Some((_, active)) = regions.iter().find(|(leaf, _)| *leaf == self.layout.active())
        {
            painter.rect_stroke(
                active.shrink(0.5),
                egui::CornerRadius::ZERO,
                egui::Stroke::new(1.0, theme::accent(ui)),
                egui::StrokeKind::Inside,
            );
        }
    }

    /// Paints the selected sub-object elements over the viewport image.
    ///
    /// Markers and face fills are drawn by the egui painter, so sub-object mode needs no renderer
    /// change and leaves the uploaded scene untouched. In vertex mode every vertex is shown with
    /// the selected ones highlighted; in face mode only the selected faces are tinted.
    fn paint_mesh_selection(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(edit) = self.editor.mesh_edit() else {
            return;
        };
        let Some((placement, mesh)) = mesh_pick::vertex_frame(self.session.project(), edit.object()) else {
            return;
        };
        let preview = self.vertex_gizmo.preview();
        let painter = ui.painter().with_clip_rect(rect);
        match edit.element() {
            MeshElement::Vertex => {
                let selected: HashSet<u32> = edit.vertices().iter().copied().collect();
                for (index, local) in mesh.positions().iter().enumerate() {
                    let Ok(vertex) = u32::try_from(index) else {
                        continue;
                    };
                    // A marker follows the preview during a drag, so the dots stay inside the moved
                    // mesh instead of lagging at the old positions until the release.
                    let local = preview.as_ref().and_then(|preview| preview.moved(vertex, *local)).unwrap_or(*local);
                    let world = mesh_pick::to_world(placement, local);
                    let Some(position) = gizmo_math::screen_point(self.scene.camera, rect, world) else {
                        continue;
                    };
                    let is_selected = selected.contains(&vertex);
                    let radius = if is_selected { MESH_VERTEX_RADIUS + 1.0 } else { MESH_VERTEX_RADIUS };
                    let color = if is_selected { gizmo_math::ACTIVE_COLOR } else { MESH_VERTEX_COLOR };
                    painter.circle_filled(position, radius, color);
                    painter.circle_stroke(
                        position,
                        radius,
                        egui::Stroke::new(1.0, egui::Color32::from_black_alpha(160)),
                    );
                }
            }
            MeshElement::Face => {
                for face in edit.faces() {
                    let Ok(index) = usize::try_from(*face) else {
                        continue;
                    };
                    let Some(polygon) = mesh.faces().get(index) else {
                        continue;
                    };
                    let mut points = Vec::with_capacity(polygon.len());
                    let mut complete = true;
                    for vertex in polygon {
                        let Ok(vertex_index) = usize::try_from(*vertex) else {
                            complete = false;
                            break;
                        };
                        let Some(local) = mesh.positions().get(vertex_index) else {
                            complete = false;
                            break;
                        };
                        let local =
                            preview.as_ref().and_then(|preview| preview.moved(*vertex, *local)).unwrap_or(*local);
                        let world = mesh_pick::to_world(placement, local);
                        let Some(position) = gizmo_math::screen_point(self.scene.camera, rect, world) else {
                            complete = false;
                            break;
                        };
                        points.push(position);
                    }
                    if !complete || points.len() < 3 {
                        continue;
                    }
                    painter.add(egui::Shape::convex_polygon(
                        points,
                        MESH_FACE_FILL,
                        egui::Stroke::new(1.5, gizmo_math::ACTIVE_COLOR),
                    ));
                }
            }
        }
    }

    /// Paints the selected line chains over the viewport image as a screen-space outline.
    ///
    /// The renderer draws a chain as a one-pixel `LineList`, which wgpu cannot widen, so a selected
    /// chain is repeated here: a dark underlay first, then the bright stroke. The chains come from
    /// the frame last uploaded, so a drag preview and its outline agree.
    fn paint_selection_outline(&self, ui: &mut egui::Ui, rect: egui::Rect) {
        if self.selected_chains.is_empty() {
            return;
        }
        let painter = ui.painter().with_clip_rect(rect);
        for points in selection_outline(self.scene.camera, rect, &self.selected_chains) {
            painter.add(egui::Shape::line(
                points.clone(),
                egui::Stroke::new(SELECTION_UNDERLAY_WIDTH, SELECTION_UNDERLAY),
            ));
            painter.add(egui::Shape::line(points, egui::Stroke::new(SELECTION_OUTLINE_WIDTH, SELECTION_OUTLINE)));
        }
    }

    /// The ribbon over the design space: the tabs and the tool groups of the active tab.
    ///
    /// The strip floats on the middle layer instead of taking height from the viewport, so the scene
    /// image runs under it and a translucent ribbon blends with the furniture (U51, ADR 0080). It is
    /// anchored to `design` — the window below the header — so resizing a side panel never reflows
    /// it; the side panels are inset by [`Self::ribbon_inset`] instead.
    pub(crate) fn show_ribbon(
        &mut self,
        ui: &mut egui::Ui,
        design: egui::Rect,
        shell: &mut Shell,
        settings: &Settings,
    ) {
        let view = furnicraft_ui::RibbonView {
            perspective: matches!(
                crate::scene_build::scene_camera(self.session.project(), self.layout.active_camera()).projection(),
                Projection::Perspective { .. }
            ),
            owns_view: viewport_owns_view(self.session.project()),
            grid: self.draw_gizmo.grid(),
        };
        let mut outcome = None;
        // The ribbon is its own chrome surface — one step above the window's surface and distinct
        // from the panel fill the side panels use behind their content. The Appearance slider scales
        // that fill's alpha, letting the scene it covers show through without changing any other
        // surface.
        let fill = theme::at_opacity(theme::ribbon(ui), settings.appearance.panel_opacity);
        // The band the strip occupies is also the band it owns the pointer over, so the tools behind
        // it never see a hover or a click there. The measured rectangle wins once egui has sized the
        // area, which is what keeps the blocked band and the drawn one the same.
        let band = shell.ribbon_band(design, self.editor.ribbon_open());
        let rect = strip_area(ui.ctx(), "ribbon", band, fill, |ui| {
            match self.editor.show_ribbon(ui, &mut self.session, view) {
                Ok(result) => outcome = Some(result),
                Err(error) => self.last_error = Some(error),
            }
        });
        // The measured strip wins over the deterministic height: a narrow window grows the body with
        // its scroll bar, and the side panels must start below whatever the ribbon actually covers.
        if rect.is_some() {
            shell.ribbon_rect = rect;
        }
        if let Some(outcome) = outcome {
            if outcome.started_drawing {
                self.draw_gizmo.reset();
                self.cancel_extrude();
                // Starting a drawing tool is a deliberate change of gesture: an armed navigation
                // mode must not take the primary button back when the draft ends.
                shell.cluster.navigation = None;
            }
            if let Some((object, height)) = outcome.extruded {
                // The body exists at the height the field named; the arrow lets the user pull it
                // from there, and the ribbon keeps that height for the next extrusion.
                self.contour_gizmo.reset();
                self.draw_gizmo.reset();
                self.extrude_gizmo.start(object, height);
            }
            // The ribbon reports a *Close* instead of running it: the shell owns the work plane,
            // the gizmo state and the scene rebuild, and every close takes the same path (ADR 0052).
            if outcome.close_draft
                && let Err(error) = self.close_draft(furnicraft_ui::language(ui.ctx()))
            {
                self.last_error = Some(error);
            }
            match outcome.view {
                Some(furnicraft_ui::RibbonViewAction::FrameAll) => self.frame_all(),
                Some(furnicraft_ui::RibbonViewAction::ToggleProjection) => {
                    self.toggle_view_projection();
                }
                Some(furnicraft_ui::RibbonViewAction::ToggleGrid) => self.draw_gizmo.toggle_grid(),
                None => {}
            }
        }
    }

    /// Reads the keys a running tool owns and leaves the rest to the shell.
    ///
    /// `modals` is the window's answer to "does a dialog or the palette own Escape this frame": a
    /// modal that is up keeps both keys for itself, exactly as a focused text field does. The armed
    /// navigation mode and the display popover live on the [`Shell`], so the two that concern them
    /// are read and written there.
    pub(crate) fn handle_keys(&mut self, ui: &egui::Ui, modals: bool, shell: &mut Shell) {
        // An open variant list owns Escape and Enter: the popup closes itself and runs its
        // highlighted method when the ribbon draws later in this frame, and one key must not also
        // drop a draft or commit it (U67).
        let list = self.editor.variant_list_open();
        let escape = !list && ui.input(|input| input.key_pressed(egui::Key::Escape));
        let enter = !list && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if self.editor.contour_edit().is_some() {
            // The working copy is dropped by Escape and applied by Enter; typing in a field keeps
            // both keys for the field, and a corner drag keeps them for the drag.
            if !ui.ctx().text_edit_focused() && !self.contour_gizmo.is_dragging() {
                if escape {
                    self.cancel_contour_edit();
                } else if enter && let Err(error) = self.apply_contour_edit() {
                    self.last_error = Some(error);
                }
            }
        } else if self.editor.drawing().is_some() {
            // Escape drops the draft and ends the tool; nothing reached the document and undo gains
            // no half figure. Enter and the right button commit a valid draft (ADR 0076), and a
            // refused commit keeps both the draft and the tool so the user can fix or cancel it.
            if !ui.ctx().text_edit_focused() {
                match drawing_key(escape, enter) {
                    Some(DraftKey::Cancel) => self.cancel_drawing(),
                    Some(DraftKey::Commit) => {
                        if let Err(error) = self.commit_drawing(furnicraft_ui::language(ui.ctx())) {
                            self.last_error = Some(error);
                        }
                    }
                    None => {}
                }
                if ui.input(|input| input.key_pressed(egui::Key::G)) {
                    self.draw_gizmo.toggle_grid();
                }
                // Tab is the one key that switches the polyline between a straight segment and an
                // arc; Alt stays the snap override and never changes the gesture.
                if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                    self.editor.toggle_drawing_arc_mode();
                }
            }
        } else if self.editor.pending_primitive().is_some() {
            if escape && !ui.ctx().text_edit_focused() {
                self.editor.cancel_primitive();
            }
        } else if self.extrude_gizmo.is_active() {
            // Escape drops the pull and its preview whatever has the focus: the field's text is part
            // of the preview, so a cancel is never only a text cancel. Enter is the bar's when the
            // field has the focus, because the field commits the typed height first and the bar
            // then applies it; outside the field Enter finishes here, before the viewport draws.
            if escape {
                self.cancel_extrude();
            } else if enter
                && !ui.ctx().text_edit_focused()
                && let (Some(object), Some(height)) = (self.extrude_gizmo.object(), self.extrude_gizmo.height())
                && let Err(error) = self.finish_extrude(object, height)
            {
                self.last_error = Some(error);
            }
        } else if escape
            && !self.translate_gizmo.is_dragging()
            && !self.rotate_gizmo.is_dragging()
            && !self.vertex_gizmo.is_dragging()
            && !ui.ctx().text_edit_focused()
        {
            if modals {
                // The modal owns Escape for this frame and closes itself.
            } else if shell.cluster.settings_open {
                // The popover owns Escape for this frame and closes itself.
            } else if shell.cluster.navigation.is_some() {
                shell.cluster.navigation = None;
            } else if self.editor.mesh_edit().is_some() {
                self.editor.end_mesh_edit();
            } else if self.editor.creation_parent().is_some() {
                // The arm outranks leaving a container level: it is the mode the next creation uses,
                // and the chip names it until Escape or ✕ clears it (ADR 0056).
                self.editor.clear_creation_parent();
            } else {
                let before = self.editor.selected_nodes().to_vec();
                self.editor.exit_level(self.session.project());
                if self.editor.selected_nodes() != before.as_slice()
                    && let Err(error) = self.upload_scene()
                {
                    self.last_error = Some(error);
                }
            }
        }
        // A drawing shortcut starts its tool from anywhere outside a text field, including while
        // another draft runs: choosing a tool ends the running draft and starts the new one
        // (ADR 0076). The table is the one the toolbar hints show, so the key and the tooltip
        // cannot drift apart.
        if self.editor.contour_edit().is_none() && !ui.ctx().text_edit_focused() {
            let sides = self.editor.polygon_sides();
            if take_drawing_shortcut(ui.ctx(), &mut self.editor, sides) {
                self.draw_gizmo.reset();
                self.cancel_extrude();
                shell.cluster.navigation = None;
            }
        }
    }
}

/// The command ids that remove or restructure a node, and so refuse a locked one.
///
/// The palette builds every parameter from the selection, so these are the commands whose target is
/// a node the user may have locked. The guard runs in the shell, before the dispatch, because a lock
/// is a guard on a gesture and not a document rule (ADR 0065).
const DESTRUCTIVE_COMMANDS: [&str; 4] = [
    furnicraft_engine::node_cmds::NODE_DELETE_ID,
    furnicraft_engine::node_cmds::NODE_UNGROUP_ID,
    furnicraft_engine::node_cmds::NODE_COMPONENT_CONVERT_TO_GROUP_ID,
    furnicraft_engine::node_cmds::NODE_GROUP_ID,
];

/// The name of a locked node a finished gizmo drag would move, if there is one.
///
/// The viewport refuses the drag before it starts; this is the second gate, checked in
/// [`Document::apply_gizmo_commit`] so no other dispatch path can move a locked node either.
fn locked_change<'a>(project: &'a Project, commit: &PlacementCommit, editor: &Editor) -> Option<&'a str> {
    commit
        .changes
        .iter()
        .find(|change| editor.is_locked(change.node))
        .and_then(|change| project.node(change.node))
        .map(furnicraft_model::Node::name)
}

/// The refusal a destructive command gets while the primary selection is locked.
///
/// `id` is the stable command id the palette (or a future CLI) would dispatch; anything outside the
/// destructive set runs untouched.
fn locked_command_refusal(editor: &Editor, project: &Project, id: &str, lang: Lang) -> Option<String> {
    if !DESTRUCTIVE_COMMANDS.contains(&id) {
        return None;
    }
    let primary = editor.selected_node()?;
    editor.locked_refusal(project, primary, lang)
}

/// Whether the viewport owns its view.
///
/// An authored camera stores its placement and projection in the document, so looking through one
/// pins both navigation and the projection toggle; a command is the only way to change either.
fn viewport_owns_view(project: &Project) -> bool {
    project.active_camera().is_none()
}

/// Where the corner orientation widget draws this frame, or `None` while it is hidden.
///
/// The placement is the free top corner of `view` below the ribbon band and beside both panels
/// ([`gizmo::widget_rect`]); the display switch in the cluster's settings popover turns the whole
/// widget off, and the shell then draws no cube, no home and no projection button and leaves the
/// click to the scene.
fn widget_placement(cluster: &ViewCluster, view: egui::Rect, chrome: &Chrome) -> Option<egui::Rect> {
    if cluster.widget_visible() { gizmo::widget_rect(view, chrome) } else { None }
}

/// Whether a finish still has to write the height.
///
/// The pull closes on the first finish, so a second check — a double click, or a key and a button
/// in one frame — finds no pull and dispatches nothing: the check applies once however often it is
/// clicked (M71).
fn extrude_writes(committed: Option<Length>, height: Length) -> bool {
    committed.is_some_and(|committed| committed != height)
}

/// The body `object` would have at `height`, for the session preview.
///
/// The preview is the stored body with one number changed, so the frame shows the exact solid the
/// ✓ would store: the same path, the same features and the same evaluator (M71).
fn previewed_body(project: &Project, object: Id, height: Length) -> Result<Geometry, String> {
    let node = project.object(object).ok_or_else(|| format!("{object:?} is not in the scene"))?;
    let mut body = node
        .geometry()
        .as_constructive()
        .cloned()
        .ok_or_else(|| format!("{} is not a constructive body", node.name()))?;
    body.set_height(height).map_err(|error| error.to_string())?;
    Ok(Geometry::Constructive(body))
}

/// World placements a drag previews for cameras and lights.
///
/// Only top-most moved nodes matter: a moved container carries its whole subtree. Cameras and
/// lights below any of those nodes get the preview's rigid transform applied to their world
/// placement; a preview with no moved camera or light returns an empty list.
fn preview_nodes(project: &Project, selection: &[Id], preview: Option<Preview<'_>>) -> Vec<(Id, Placement)> {
    let Some(preview) = preview else {
        return Vec::new();
    };
    let moved: Vec<Id> = gizmo_math::move_targets(project, selection).into_iter().map(|target| target.node).collect();
    if moved.is_empty() {
        return Vec::new();
    }
    let mut placements = Vec::new();
    for kind in [StructuralKind::Camera, StructuralKind::Light] {
        for id in project.query(&NodeQuery::any().with_kind(kind)) {
            if !is_under_any(project, id, &moved) {
                continue;
            }
            let Some(world) = project.world_placement(id) else {
                continue;
            };
            if let Some(previewed) = preview.moved_world(world) {
                placements.push((id, previewed));
            }
        }
    }
    placements
}

/// Whether `node` is one of `ancestors` or sits below one of them.
///
/// The drag preview applies one rigid transform to a top-most moved node and everything below it,
/// so a camera or a light inside a moved container has to follow the same transform.
fn is_under_any(project: &Project, mut node: Id, ancestors: &[Id]) -> bool {
    loop {
        if ancestors.contains(&node) {
            return true;
        }
        match project.parent_of(node) {
            Some(parent) => node = parent,
            None => return false,
        }
    }
}

/// Projects the selected line chains into viewport-space polylines.
///
/// A chain is cut where a point falls behind the camera, so the overlay never draws a segment
/// through the viewer; a chain with fewer than two projected points contributes nothing. The
/// function reads only the stored points and world transforms, so a test can call it without a GPU.
fn selection_outline(camera: Camera, viewport: egui::Rect, chains: &[Polyline]) -> Vec<Vec<egui::Pos2>> {
    let mut outlines = Vec::new();
    for chain in chains {
        let mut current = Vec::with_capacity(chain.points.len());
        for point in chain.points.iter() {
            let world = chain.world_transform.transform_point3(*point);
            match gizmo_math::screen_point(camera, viewport, world) {
                Some(position) => current.push(position),
                None => {
                    if current.len() >= 2 {
                        outlines.push(std::mem::take(&mut current));
                    } else {
                        current.clear();
                    }
                }
            }
        }
        if current.len() >= 2 {
            outlines.push(current);
        }
    }
    outlines
}

/// A copy of the frame with one object's dragged vertices at their previewed positions.
///
/// The mesh is cloned, the selected vertices are offset and the clone is tessellated for this frame
/// only; the document keeps the old positions until `mesh.vertices.move` commits them. An object
/// that is not a mesh in the current revision leaves the frame unchanged.
fn preview_vertex_mesh(frame: SceneFrame, project: &Project, preview: VertexPreview<'_>) -> Result<SceneFrame, String> {
    let Some(mesh) = project.object(preview.object).and_then(|object| object.geometry().as_mesh()) else {
        return Ok(frame);
    };
    let mut moved = mesh.clone();
    for vertex in preview.vertices {
        let Ok(index) = usize::try_from(*vertex) else {
            continue;
        };
        let Some(position) = moved.positions().get(index).copied() else {
            continue;
        };
        let Some(target) = preview.moved(*vertex, position) else {
            continue;
        };
        moved.set_position(index, target).map_err(|error| error.to_string())?;
    }
    let data = object_mesh(&moved).map_err(|error| error.to_string())?;
    Ok(frame.with_drawable_mesh(preview.object, Arc::new(data)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::{DMat4, DQuat, DVec3};
    use furnicraft_model::{CameraProjection, LightKind};
    use furnicraft_scene::Projection;
    use furnicraft_testkit::{millimetres, sample_project};

    use super::*;
    use crate::icons;
    use crate::view_cluster::projection_button;

    #[test]
    fn a_cheap_rebuild_is_never_announced() {
        let mut rebuild = Rebuild::default();
        assert!(!rebuild.busy(true), "nothing is known about a scene that has not rebuilt yet");
        rebuild.record(Duration::from_millis(8));
        assert!(!rebuild.slow(), "8 ms is inside the budget");
        assert!(!rebuild.busy(true), "a cheap rebuild shows no message");
        assert!(!rebuild.defer(true), "and it does not cost a frame");
        assert!(!rebuild.busy(false));
    }

    #[test]
    fn a_simulated_slow_rebuild_reports_busy_and_waits_one_frame() {
        // A rebuild that overran the budget is announced before the next one runs: the first frame
        // paints «Перестроение…» and keeps the last good geometry, and the work runs on the next.
        let mut rebuild = Rebuild::default();
        rebuild.record(SLOW_REBUILD);
        assert!(rebuild.slow());
        assert!(rebuild.busy(true), "the status bar says the scene is rebuilding");
        assert!(rebuild.defer(true), "the frame does no work, so it stays responsive");
        assert!(rebuild.busy(true), "the message is still up on the frame that works");
        assert!(!rebuild.defer(true), "the next frame pays for the rebuild");
        rebuild.record(Duration::from_millis(4));
        assert!(!rebuild.busy(true), "a fast rebuild ends the message");
    }

    #[test]
    fn the_rebuild_flag_clears_when_nothing_is_pending() {
        let mut rebuild = Rebuild::default();
        rebuild.record(SLOW_REBUILD);
        assert!(rebuild.defer(true));
        assert!(!rebuild.defer(false), "an idle frame runs no work and clears the flag");
        assert!(rebuild.defer(true), "so the next slow rebuild is announced again");
    }

    #[test]
    fn the_check_writes_once_and_a_second_one_writes_nothing() {
        // The pull is open and holds the document's height: the first check writes nothing either,
        // because the body already has that height.
        assert!(!extrude_writes(Some(millimetres(100)), millimetres(100)), "no write when the document agrees");
        assert!(extrude_writes(Some(millimetres(100)), millimetres(250)), "a new height is written");
        assert!(
            !extrude_writes(None, millimetres(250)),
            "a closed pull writes nothing, so a double check is one write"
        );
    }

    /// Root (1): shelf (2), drawer (3) with front (4), rig (5) carrying a camera and a light.
    struct Fixture {
        project: Project,
        rig: Id,
        camera: Id,
        light: Id,
    }

    fn fixture() -> Fixture {
        let mut project = sample_project();
        let root = project.root().id();
        let rig = project
            .add_component(
                root,
                "Rig",
                Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement"),
            )
            .expect("rig");
        let camera = project
            .add_camera(
                rig,
                "Camera",
                Placement::new(DVec3::new(0.0, 20.0, 0.0), DQuat::IDENTITY).expect("placement"),
                CameraProjection::DEFAULT,
            )
            .expect("camera");
        let light = project
            .add_light(
                rig,
                "Fill",
                Placement::new(DVec3::new(0.0, 0.0, 30.0), DQuat::IDENTITY).expect("placement"),
                LightKind::Point,
                [1.0; 3],
                1000.0,
            )
            .expect("light");
        Fixture { project, rig, camera, light }
    }

    #[test]
    fn the_display_switch_hides_the_corner_widget_without_touching_the_document() {
        // The widget is a view-state drawing, so its switch is one: a press on it draws nothing on
        // the next frame and cannot reach the document (U73).
        let session = crate::scene_build::sample_session().expect("the sample scene");
        let revision = session.project().revision();
        let view = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let chrome = Chrome {
            panels: crate::side_panels::PanelRects::default(),
            ribbon: egui::Rect::from_min_size(
                view.min,
                egui::vec2(view.width(), furnicraft_ui::toolbars::height(true)),
            ),
        };
        let mut cluster = ViewCluster::default();
        assert!(cluster.widget_visible(), "the corner widget is on until the switch turns it off");
        assert!(widget_placement(&cluster, view, &chrome).is_some(), "a plain view has a free corner");

        cluster.show_widget = false;
        assert!(
            widget_placement(&cluster, view, &chrome).is_none(),
            "a hidden widget draws no cube, no home and no projection button"
        );
        assert_eq!(session.project().revision(), revision, "a display switch is view state, never an edit");
    }

    #[test]
    fn a_theme_switch_repaints_the_frame_without_touching_the_document() {
        let session = crate::scene_build::sample_session().expect("the sample scene");
        let mut cache = EvaluationCache::new();
        let mut kernel = TruckKernel::new();
        let evaluated = evaluate(&mut cache, &mut kernel, &session, &ViewFlags::default()).expect("evaluated scene");
        let revision = session.project().revision();
        let settings = ViewportSettings::default();
        let cluster = ViewCluster::default();
        let mut dark = evaluated.frame.clone();
        let mut light = evaluated.frame;
        Display::of(&settings, &cluster, SceneColors::DARK).apply(&mut dark);
        Display::of(&settings, &cluster, SceneColors::LIGHT).apply(&mut light);

        assert_eq!(dark.background, SceneColors::DARK.background);
        assert_eq!(light.background, SceneColors::LIGHT.background, "the viewport clears to the light chrome");
        assert_eq!(light.grid.minor_color, SceneColors::LIGHT.grid.minor_color, "the floor grid follows the variant");
        assert_eq!(light.line_ink, SceneColors::LIGHT.line_ink, "the unlit chains follow it too");
        assert!(light.polylines().iter().all(|line| line.color.is_none()), "a drawn path takes the frame ink");
        let materials =
            |frame: &SceneFrame| frame.drawables().iter().map(|drawable| drawable.color).collect::<Vec<_>>();
        assert_eq!(materials(&light), materials(&dark), "the furniture keeps its material colours");
        assert_eq!(dark.grid.visible, light.grid.visible, "the viewport's grid switch is not the theme's");
        assert_eq!(session.project().revision(), revision, "a variant is view state, never an edit");
    }

    #[test]
    fn an_authored_camera_takes_the_view_out_of_the_viewport() {
        let mut fixture = fixture();
        assert!(viewport_owns_view(&fixture.project), "the built-in camera is view state");
        fixture.project.set_active_camera(Some(fixture.camera)).expect("activate the camera");
        assert!(!viewport_owns_view(&fixture.project), "an authored camera owns navigation and projection");
        fixture.project.set_active_camera(None).expect("clear the camera");
        assert!(viewport_owns_view(&fixture.project));
    }

    #[test]
    fn no_preview_moves_no_camera_or_light() {
        let fixture = fixture();
        assert!(
            preview_nodes(&fixture.project, &[fixture.rig], None).is_empty(),
            "without a drag the document is the only truth"
        );
    }

    #[test]
    fn moving_a_container_previews_the_cameras_and_lights_inside_it() {
        let fixture = fixture();
        let delta = DVec3::new(10.0, -5.0, 2.0);
        let preview = Preview::Translation { drawables: &[], delta };
        let moved = preview_nodes(&fixture.project, &[fixture.rig], Some(preview));
        assert_eq!(moved.len(), 2, "the camera and the light both follow the rig");
        for (node, placement) in moved {
            let world = fixture.project.world_placement(node).expect("world placement");
            assert_eq!(placement.translation(), world.translation() + delta, "node {node:?} keeps the rig's shift");
            assert_eq!(placement.rotation(), world.rotation(), "a translation keeps the rotation");
        }
    }

    #[test]
    fn turning_a_container_turns_the_nested_icon_about_the_preview_pivot() {
        let fixture = fixture();
        let pivot = DVec3::new(100.0, 0.0, 0.0);
        let rotation = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        let preview = Preview::Rotation { drawables: &[], pivot, rotation };
        let moved = preview_nodes(&fixture.project, &[fixture.rig], Some(preview));
        assert_eq!(moved.len(), 2);
        for (node, placement) in moved {
            let world = fixture.project.world_placement(node).expect("world placement");
            let expected = pivot + rotation * (world.translation() - pivot);
            assert!(
                (placement.translation() - expected).length() < 1.0e-9,
                "node {node:?} turns about the preview pivot"
            );
            assert!(
                placement.rotation().dot(rotation * world.rotation()).abs() > 1.0 - 1.0e-9,
                "the world turn is pre-multiplied onto the stored rotation"
            );
        }
    }

    #[test]
    fn a_selected_camera_is_previewed_without_a_container() {
        let fixture = fixture();
        let delta = DVec3::new(0.0, 0.0, -7.0);
        let preview = Preview::Translation { drawables: &[], delta };
        let moved = preview_nodes(&fixture.project, &[fixture.camera], Some(preview));
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].0, fixture.camera, "only the selected camera moves");
        let world = fixture.project.world_placement(fixture.camera).expect("world placement");
        assert_eq!(moved[0].1.translation(), world.translation() + delta);
    }

    #[test]
    fn a_preview_of_a_node_without_icons_lists_nothing() {
        let fixture = fixture();
        let preview = Preview::Translation { drawables: &[], delta: DVec3::X };
        assert!(
            preview_nodes(&fixture.project, &[Id::new(2).expect("id")], Some(preview)).is_empty(),
            "the shelf carries neither a camera nor a light"
        );
    }

    #[test]
    fn is_under_any_walks_up_to_the_moved_ancestor() {
        let fixture = fixture();
        assert!(is_under_any(&fixture.project, fixture.camera, &[fixture.rig]));
        assert!(is_under_any(&fixture.project, fixture.rig, &[fixture.rig]));
        assert!(is_under_any(&fixture.project, Id::new(4).expect("id"), &[Id::new(3).expect("id")]));
        assert!(!is_under_any(&fixture.project, Id::new(2).expect("id"), &[Id::new(3).expect("id")]));
        assert!(!is_under_any(&fixture.project, fixture.camera, &[fixture.light]));
    }

    #[test]
    fn the_projection_button_follows_the_camera_the_viewport_draws_with() {
        // The strip reads the frame camera, which is the authored camera when one is active and the
        // viewport's own camera otherwise. Both paths must lead to the icon of the mode that is
        // actually drawn, which is what ADR 0049 promises an authored camera.
        let mut fixture = fixture();
        let view = Camera::default();
        assert_eq!(
            projection_button(crate::scene_build::scene_camera(&fixture.project, view).projection()).0,
            icons::view::PERSPECTIVE,
            "with no authored camera the viewport draws its own perspective view"
        );

        fixture.project.set_active_camera(Some(fixture.camera)).expect("activate the camera");
        fixture
            .project
            .set_camera_projection(
                fixture.camera,
                CameraProjection::Orthographic { height: Length::from_millimetres(500).expect("height") },
            )
            .expect("set the projection");

        let drawn = crate::scene_build::scene_camera(&fixture.project, view);
        assert!(matches!(drawn.projection(), Projection::Orthographic { .. }));
        assert_eq!(
            projection_button(drawn.projection()).0,
            icons::view::ORTHOGRAPHIC,
            "an authored orthographic camera draws its own mode, not the viewport's"
        );
        assert!(!viewport_owns_view(&fixture.project), "and the toggle stays disabled");
    }

    fn outline_viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(800.0, 600.0))
    }

    fn selection_chain(points: Vec<DVec3>) -> Polyline {
        Polyline::new(Id::new(7).expect("id"), Arc::from(points), DMat4::IDENTITY, SELECTION_LINE_COLOR)
    }

    #[test]
    fn selection_outline_projects_every_stored_point_at_the_stored_transform() {
        let camera = Camera::default();
        let viewport = outline_viewport();
        let points = vec![DVec3::new(-100.0, 0.0, 0.0), DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0)];
        let outlines = selection_outline(camera, viewport, &[selection_chain(points.clone())]);
        assert_eq!(outlines.len(), 1, "an open chain is one outline");
        assert_eq!(outlines[0].len(), 3, "every stored point is projected, including the last");
        assert!(outlines[0].iter().all(|point| viewport.contains(*point)));

        let mut moved = selection_chain(points);
        moved.world_transform = DMat4::from_translation(DVec3::new(0.0, 0.0, 200.0));
        let moved_outlines = selection_outline(camera, viewport, &[moved]);
        assert_ne!(moved_outlines[0], outlines[0], "the outline reads the chain's world transform");
    }

    #[test]
    fn selection_outline_cuts_a_chain_where_it_falls_behind_the_camera() {
        let camera = Camera::default();
        let behind = camera.eye() - camera.forward() * 100.0;
        let points = vec![
            DVec3::new(-100.0, 0.0, 0.0),
            DVec3::ZERO,
            behind,
            DVec3::new(0.0, 100.0, 0.0),
            DVec3::new(100.0, 100.0, 0.0),
        ];
        let outlines = selection_outline(camera, outline_viewport(), &[selection_chain(points)]);
        assert_eq!(outlines.len(), 2, "the segment through the viewer is dropped");
        assert_eq!(outlines[0].len(), 2);
        assert_eq!(outlines[1].len(), 2);
    }
    /// A drag that started somewhere else and then met a lock is refused at the commit, with the
    /// node named, so no dispatch path can move a locked node.
    #[test]
    fn a_locked_node_refuses_a_gizmo_commit() {
        let project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let mut editor = Editor::new();
        let placement = Placement::IDENTITY;
        let commit = PlacementCommit { changes: vec![gizmo_math::PlacementChange { node: shelf, placement }] };

        assert_eq!(locked_change(&project, &commit, &editor), None, "an unlocked node moves");
        editor.set_locked(shelf, true);
        assert_eq!(locked_change(&project, &commit, &editor), Some("Shelf"), "the refusal names the node");
    }

    #[test]
    fn a_locked_primary_selection_refuses_a_destructive_palette_command() {
        let project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let mut editor = Editor::new();
        let delete = furnicraft_engine::node_cmds::NODE_DELETE_ID;

        assert_eq!(
            locked_command_refusal(&editor, &project, delete, furnicraft_text::Lang::En),
            None,
            "nothing is selected"
        );
        editor.select_node(Some(shelf));
        assert_eq!(
            locked_command_refusal(&editor, &project, delete, furnicraft_text::Lang::En),
            None,
            "an unlocked node deletes"
        );
        editor.set_locked(shelf, true);
        let refusal = locked_command_refusal(&editor, &project, delete, furnicraft_text::Lang::En)
            .expect("a locked node refuses");
        assert!(refusal.contains("Shelf") && refusal.contains("locked"), "the sentence names it: {refusal}");
        assert_eq!(
            locked_command_refusal(
                &editor,
                &project,
                furnicraft_engine::node_cmds::NODE_RENAME_ID,
                furnicraft_text::Lang::En
            ),
            None,
            "a command that removes nothing runs on a locked node"
        );

        editor.set_locked(shelf, false);
        assert_eq!(
            locked_command_refusal(&editor, &project, delete, furnicraft_text::Lang::En),
            None,
            "unlocking lets it through"
        );
    }
}
