//! FurniCraft desktop application shell.
//!
//! This is the composition root: it creates the window, injects the truck kernel into the sample
//! session, evaluates panels into a scene, and wires the offscreen renderer into egui. The viewport
//! only changes view state here; document edits will be dispatched as engine commands.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod contour_gizmo;
mod draw_gizmo;
mod extrude_gizmo;
mod file_menu;
mod gizmo;
mod gizmo_math;
mod header;
mod mesh_pick;
mod rotate_gizmo;
mod scene_build;
mod scene_gizmo;
mod settings_store;
mod translate_gizmo;
mod vertex_gizmo;
mod view_cluster;
mod view_layout;

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use furnicraft_engine::mesh_cmds::MESH_VERTICES_MOVE_ID;
use furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_MANY_ID;
use furnicraft_engine::{MeshVertexMove, MeshVerticesMove, NodePlacement, NodePlacementSetMany, Session};
use furnicraft_kernel_truck::TruckKernel;
use furnicraft_mesh::{EvaluationCache, object_mesh};
use furnicraft_model::{Id, NodeQuery, Placement, Project, Revision, StructuralKind};
use furnicraft_project::SceneFile;
use furnicraft_render::ViewportRenderer;
use furnicraft_scene::{Camera, Polyline, Projection, SELECTION_LINE_COLOR, SceneFrame};
use furnicraft_ui::display::OpeningView;
use furnicraft_ui::settings::{Settings, SettingsSection, ViewportSettings};
use furnicraft_ui::ui_scale;
use furnicraft_ui::{DrawDraft, DrawTool, Editor, MeshElement, Palette, PaletteContext, icons, theme};
use furnicraft_units::Length;

use crate::contour_gizmo::ContourGizmo;
use crate::draw_gizmo::{DrawGizmo, DrawModifiers};
use crate::extrude_gizmo::{ExtrudeAction, ExtrudeEvent, ExtrudeGizmo};
use crate::file_menu::{FileAction, FileMenu, PreservedSections};
use crate::gizmo_math::{PlacementCommit, Preview, PreviewKey, WorkPlane};
use crate::header::{HeaderAction, HeaderState};
use crate::rotate_gizmo::RotateGizmo;
use crate::scene_build::{empty_session, evaluate, home_camera, sample_session};
use crate::settings_store::SettingsStore;
use crate::translate_gizmo::TranslateGizmo;
use crate::vertex_gizmo::{MeshVertexCommit, VertexGizmo, VertexPreview};
use crate::view_cluster::{ClusterAction, ClusterView, NavigationMode, ViewCluster};
use crate::view_layout::{Layout, LayoutKind};

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

/// Which transform gizmo the viewport shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransformMode {
    /// Three world-axis arrows move the selection.
    Move,
    /// Three local-axis rings turn the selection about its origin.
    Rotate,
}

struct FurniCraftApp {
    session: Session,
    editor: Editor,
    kernel: TruckKernel,
    /// Derived node geometry keyed by the inputs it was built from; a rebuild reuses every node
    /// the editing did not touch.
    cache: EvaluationCache,
    scene: SceneFrame,
    /// The workspace's views: their cameras, presets and the tree that lays them out. View state in
    /// the shell; the document never sees it (ADR 0029).
    layout: Layout,
    /// The camera a fresh view opens with, framed on the scene from the chosen direction. *Frame all*
    /// puts every view back here.
    home_camera: Camera,
    scene_revision: Revision,
    renderer: ViewportRenderer,
    render_state: egui_wgpu::RenderState,
    texture_id: egui::TextureId,
    translate_gizmo: TranslateGizmo,
    rotate_gizmo: RotateGizmo,
    vertex_gizmo: VertexGizmo,
    contour_gizmo: ContourGizmo,
    draw_gizmo: DrawGizmo,
    extrude_gizmo: ExtrudeGizmo,
    mode: TransformMode,
    highlighted: Vec<Id>,
    /// The selected line chains of the frame last uploaded, already carrying any drag preview, so
    /// the screen-space outline follows a moved path without a frame of lag.
    selected_chains: Vec<Polyline>,
    uploaded_preview: Option<PreviewKey>,
    files: FileMenu,
    /// Top-level sections of the loaded file that this build does not understand; written back on
    /// save so an older build never silently destroys a newer build's data.
    extra: PreservedSections,
    last_error: Option<String>,
    /// Whether the scene tree is unfolded.
    tree_open: bool,
    /// Whether the inspector is unfolded.
    card_open: bool,
    /// The floating viewport cluster: navigation, display style and settings.
    cluster: ViewCluster,
    /// Nodes below the root, cached with the evaluated scene so the tree's count chip costs no
    /// document scan on the frame path.
    node_count: usize,
    /// The workspace remembered between runs: chrome, viewport defaults and recent files.
    settings: Settings,
    /// Where [`Settings`] is read from and written to.
    settings_store: SettingsStore,
    /// The section the Settings window was last left on.
    settings_section: SettingsSection,
    /// Whether the Settings window is open.
    settings_open: bool,
    /// Whether the Home surface is open.
    home_open: bool,
    /// The command palette and its last-used order; session state, never document data.
    palette: Palette,
    /// The pointer in viewport points from the previous frame, so an action outside the viewport
    /// (the cluster, the ribbon) can still name the view it belongs to.
    pointer_position: Option<egui::Pos2>,
    /// Whether a view border owns the primary button until it is released.
    divider_dragging: bool,
    /// Why the defaults were used instead of the saved workspace, shown in the status bar.
    settings_notice: Option<String>,
}

impl FurniCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let settings_store = SettingsStore::for_workspace();
        let loaded = settings_store.load();
        let settings = loaded.settings;
        theme::install_with(&cc.egui_ctx, settings.appearance.theme.preference(), settings.appearance.accent);
        cc.egui_ctx.set_zoom_factor(ui_scale::factor(settings.appearance.ui_scale));
        let session = sample_session()?;
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated = evaluate(&mut cache, &mut kernel, &session)?;
        let node_count = session.project().query(&NodeQuery::any()).len();
        let home_camera = opening_camera(evaluated.bounds, settings.viewport.opening_view)?;
        let mut layout = Layout::new(home_camera);
        if let Some(leaf) = layout.leaf_mut(layout.active()) {
            leaf.apply_preset(settings.viewport.opening_view);
        }
        let mut scene = evaluated.frame;
        scene.shading = settings.viewport.style;
        scene.all_edges = settings.viewport.all_edges;
        apply_viewport_defaults(&mut scene.grid, &settings.viewport);
        scene.camera = scene_build::scene_camera(session.project(), layout.active_camera());

        let render_state =
            cc.wgpu_render_state.clone().ok_or_else(|| "FurniCraft requires the wgpu renderer".to_owned())?;
        let mut renderer = ViewportRenderer::new(&render_state.device, 1280, 800);
        let texture_id = {
            let mut egui_renderer = render_state.renderer.write();
            egui_renderer.register_native_texture(&render_state.device, renderer.color_view(), wgpu::FilterMode::Linear)
        };
        renderer.upload(&render_state.device, &scene).map_err(|error| error.to_string())?;

        let cluster = ViewCluster {
            style: settings.viewport.style,
            all_edges: settings.viewport.all_edges,
            ..ViewCluster::default()
        };
        scene.contact_shadows = cluster.shadow_settings();

        Ok(Self {
            scene_revision: session.project().revision(),
            session,
            editor: Editor::new(),
            kernel,
            cache,
            scene,
            layout,
            home_camera,
            renderer,
            render_state,
            texture_id,
            translate_gizmo: TranslateGizmo::default(),
            rotate_gizmo: RotateGizmo::default(),
            vertex_gizmo: VertexGizmo::default(),
            contour_gizmo: ContourGizmo::default(),
            draw_gizmo: DrawGizmo::new(),
            extrude_gizmo: ExtrudeGizmo::default(),
            mode: TransformMode::Move,
            highlighted: Vec::new(),
            selected_chains: Vec::new(),
            uploaded_preview: None,
            files: FileMenu::new(),
            extra: PreservedSections::new(),
            last_error: None,
            tree_open: true,
            card_open: true,
            cluster,
            node_count,
            settings,
            settings_store,
            settings_section: SettingsSection::default(),
            settings_open: false,
            home_open: false,
            palette: Palette::new(),
            pointer_position: None,
            divider_dragging: false,
            settings_notice: loaded.notice,
        })
    }

    /// Replaces the whole scene with another session and its preserved sections.
    ///
    /// Opening a file is not an undoable document edit: it installs a new session, so the previous
    /// undo history, selection, gizmo drags and evaluated geometry are dropped together. The
    /// catalogue stays: it is session state, not scene data, and the file never carries it.
    fn adopt(&mut self, session: Session, extra: PreservedSections) -> Result<(), String> {
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated = evaluate(&mut cache, &mut kernel, &session)?;
        let home_camera = opening_camera(evaluated.bounds, self.settings.viewport.opening_view)?;
        let mut scene = evaluated.frame;
        scene.shading = self.settings.viewport.style;
        scene.all_edges = self.settings.viewport.all_edges;
        apply_viewport_defaults(&mut scene.grid, &self.settings.viewport);

        // A new document keeps the split the user arranged: only the cameras are refitted, so a
        // four-view workspace opens the new scene framed in every view. The active view takes the
        // workspace's opening preset, and the others show the same scene from the axonometric home,
        // so opening a file never leaves a view pointing at where the old model was.
        self.layout.reset_cameras(home_camera);
        if let Some(leaf) = self.layout.leaf_mut(self.layout.active()) {
            leaf.apply_preset(self.settings.viewport.opening_view);
        }
        scene.camera = scene_build::scene_camera(session.project(), self.layout.active_camera());

        self.session = session;
        self.node_count = self.session.project().query(&NodeQuery::any()).len();
        self.kernel = kernel;
        self.cache = cache;
        self.scene = scene;
        self.home_camera = home_camera;
        self.scene_revision = self.session.project().revision();
        self.editor = Editor::new();
        self.cluster = ViewCluster::default();
        self.cluster.style = self.settings.viewport.style;
        self.cluster.all_edges = self.settings.viewport.all_edges;
        self.scene.contact_shadows = self.cluster.shadow_settings();
        self.translate_gizmo = TranslateGizmo::default();
        self.rotate_gizmo = RotateGizmo::default();
        self.vertex_gizmo = VertexGizmo::default();
        self.contour_gizmo = ContourGizmo::default();
        self.draw_gizmo = DrawGizmo::new();
        self.highlighted.clear();
        self.selected_chains.clear();
        self.uploaded_preview = None;
        self.extra = extra;
        self.last_error = None;
        self.upload_scene()
    }

    /// Reads a scene file and makes it the current scene.
    fn open_scene(&mut self, path: &Path) -> Result<(), String> {
        let scene = SceneFile::read(path).map_err(|error| error.to_string())?;
        let extra = scene.extra().clone();
        let project = scene.into_document();
        let session = Session::with_catalog(project, self.session.catalog().clone());
        self.adopt(session, extra)?;
        self.files.set_path(Some(path.to_owned()));
        Ok(())
    }

    /// Writes the current scene and marks the document as saved.
    fn save_scene(&mut self, path: &Path) -> Result<(), String> {
        SceneFile::new(self.session.project().clone())
            .with_extra(self.extra.clone())
            .write(path)
            .map_err(|error| error.to_string())?;
        self.session.mark_saved();
        self.files.set_path(Some(path.to_owned()));
        Ok(())
    }

    /// Performs the header action the row returned.
    ///
    /// Undo, redo and save are the same operations the menu and the keyboard already take, so the
    /// header adds a place to click, not a second code path into the document.
    fn handle_header_action(&mut self, action: HeaderAction) {
        match action {
            HeaderAction::Home => {
                self.home_open = true;
                self.settings_notice = None;
            }
            HeaderAction::Undo => {
                if let Err(error) = self.session.undo() {
                    self.last_error = Some(error.to_string());
                }
            }
            HeaderAction::Redo => {
                if let Err(error) = self.session.redo() {
                    self.last_error = Some(error.to_string());
                }
            }
            HeaderAction::Save => {
                if let Some(action) = self.files.save(self.session.is_dirty()) {
                    self.handle_file_action(action);
                }
            }
            HeaderAction::Settings => {
                self.settings_open = true;
                self.settings_notice = None;
            }
            HeaderAction::CloseTab => {}
        }
    }

    /// Performs the operation the File menu returned and reports a failure in the toolbar.
    fn handle_file_action(&mut self, action: FileAction) {
        self.last_error = None;
        let recorded = match &action {
            FileAction::Open(path) | FileAction::Save(path) => Some(path.clone()),
            FileAction::New => None,
        };
        let result = match action {
            FileAction::New => {
                let adopted = empty_session().and_then(|session| self.adopt(session, PreservedSections::new()));
                if adopted.is_ok() {
                    // A new scene has no file yet; leaving the old path would make Save overwrite the
                    // scene the user just left.
                    self.files.set_path(None);
                }
                adopted
            }
            FileAction::Open(path) => self.open_scene(&path),
            FileAction::Save(path) => self.save_scene(&path),
        };
        match result {
            Ok(()) => {
                if let Some(path) = recorded {
                    self.settings.remember(&path);
                    self.persist_settings();
                }
            }
            Err(error) => self.last_error = Some(error),
        }
    }

    /// Writes the workspace settings and reports a failure in the status bar.
    ///
    /// The write is cheap and rare — a settings close, an opened or saved scene — so it happens at
    /// the moment the state changes instead of waiting for a shutdown hook a killed process can
    /// skip.
    fn persist_settings(&mut self) {
        match self.settings_store.save(&self.settings) {
            Ok(()) => self.settings_notice = None,
            Err(error) => self.last_error = Some(error),
        }
    }

    fn sync_scene(&mut self) -> Result<(), String> {
        let revision = self.session.project().revision();
        if revision == self.scene_revision {
            return Ok(());
        }
        self.node_count = self.session.project().query(&NodeQuery::any()).len();
        let evaluated = evaluate(&mut self.cache, &mut self.kernel, &self.session)?;
        let mut scene = evaluated.frame;
        scene.shading = self.cluster.style;
        scene.all_edges = self.cluster.all_edges;
        scene.contact_shadows = self.cluster.shadow_settings();
        apply_viewport_defaults(&mut scene.grid, &self.settings.viewport);
        scene.camera = scene_build::scene_camera(self.session.project(), self.layout.active_camera());
        self.scene = scene;
        self.scene_revision = revision;
        self.upload_scene()
    }

    fn upload_scene(&mut self) -> Result<(), String> {
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
        match self.mode {
            TransformMode::Move => self.translate_gizmo.preview(),
            TransformMode::Rotate => self.rotate_gizmo.preview(),
        }
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
        match self.mode {
            TransformMode::Move => self.translate_gizmo.preview_key(),
            TransformMode::Rotate => self.rotate_gizmo.preview_key(),
        }
    }

    /// Applies the live contour draft as one command and rebuilds the scene.
    fn apply_contour_edit(&mut self) -> Result<(), String> {
        self.editor.apply_contour_edit(&mut self.session)?;
        self.contour_gizmo.reset();
        self.sync_scene()
    }

    /// Leaves contour editing and throws the working copy away.
    fn cancel_contour_edit(&mut self) {
        self.editor.end_contour_edit();
        self.contour_gizmo.reset();
    }

    /// Leaves a drawing tool and throws the draft away.
    fn cancel_drawing(&mut self) {
        self.editor.cancel_drawing();
        self.draw_gizmo.reset();
    }

    /// Commits the live drawing on the work plane and ends the tool.
    ///
    /// The plane is the one the draft started on, so a commit does not depend on where the camera
    /// has orbited since. A refused path leaves the draft live and reports why.
    fn commit_drawing(&mut self) -> Result<(), String> {
        let placement = self
            .draw_gizmo
            .plane(self.scene.camera)
            .placement()
            .ok_or_else(|| "the work plane has no usable placement".to_owned())?;
        self.editor.commit_drawing(&mut self.session, placement)?;
        self.draw_gizmo.reset();
        self.highlighted.clear();
        self.sync_scene()?;
        Ok(())
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
    /// A released drag writes the height and keeps the pull open so the user can adjust it again;
    /// a click or Enter finishes the pull, writing the height only if the drag did not already.
    fn apply_extrude_event(&mut self, event: ExtrudeEvent) -> Result<(), String> {
        match event {
            ExtrudeEvent::Set { object, height } => {
                self.editor.set_extrude_height(&mut self.session, object, height)?;
                self.extrude_gizmo.committed_height(height);
                self.sync_scene()
            }
            ExtrudeEvent::Finish { object, height } => self.finish_extrude(object, height),
        }
    }

    /// Applies what the pull's numeric field asked for.
    ///
    /// Enter writes the typed height and closes the pull; Escape drops the pull without touching
    /// the document, so the body keeps the height its last committed drag gave it.
    fn apply_extrude_action(&mut self, action: ExtrudeAction) {
        match action {
            ExtrudeAction::Finish(height) => {
                let Some(object) = self.extrude_gizmo.object() else {
                    return;
                };
                if let Err(error) = self.finish_extrude(object, height) {
                    self.last_error = Some(error);
                }
            }
            ExtrudeAction::Cancel => self.extrude_gizmo.cancel(),
        }
    }

    /// Writes the final height if the document does not hold it yet, then closes the pull.
    fn finish_extrude(&mut self, object: Id, height: Length) -> Result<(), String> {
        if self.extrude_gizmo.committed() != Some(height) {
            self.editor.set_extrude_height(&mut self.session, object, height)?;
        }
        self.extrude_gizmo.cancel();
        self.sync_scene()
    }

    /// Dispatches one `node.placement.set_many` for a finished drag and rebuilds the scene.
    fn apply_gizmo_commit(&mut self, commit: PlacementCommit) -> Result<(), String> {
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
        self.sync_scene()
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
        self.sync_scene()
    }

    /// Refits every view on the scene, keeping the split and each view's direction.
    ///
    /// *Frame all* fits the workspace: with one view it is the only fit there is, and with several
    /// it is the one command that puts the whole scene in front of the user in every view.
    fn reset_view(&mut self) {
        let home = self.home_camera;
        for leaf in self.layout.leaves_mut() {
            leaf.frame(home);
        }
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// Returns one view to *Axonometric* without refitting the scene.
    ///
    /// The preset writes yaw and pitch only, so the model does not move and the view keeps its
    /// target, its distance and its projection (ADR 0049).
    fn home_view(&mut self, leaf: crate::view_layout::LeafId) {
        if let Some(leaf) = self.layout.leaf_mut(leaf) {
            leaf.home();
        }
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// The view the pointer is over, or the active one.
    ///
    /// *Home* and *Frame all* act on the view under the pointer, because the cluster floats over the
    /// workspace and the pointer names the view the user means; without a pointer over a view they
    /// act on the active one.
    fn pointer_leaf(&self, leaf_rects: &[(crate::view_layout::LeafId, egui::Rect)]) -> crate::view_layout::LeafId {
        let pointer = self.pointer_position;
        pointer
            .and_then(|pointer| leaf_rects.iter().find(|(_, rect)| rect.contains(pointer)).map(|(leaf, _)| *leaf))
            .unwrap_or(self.layout.active())
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        // The toolbar can be wider than the panel in a narrow window; egui then reports the
        // overflowing width as available. Intersecting with the clip rect keeps the viewport and
        // its orientation widget inside the central panel.
        let available = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        if available.width() < 1.0 || available.height() < 1.0 {
            return;
        }
        let (area, response) = ui.allocate_exact_size(available.size(), egui::Sense::click_and_drag());
        if !ui.is_rect_visible(area) {
            return;
        }
        self.pointer_position = ui.input(|input| input.pointer.hover_pos());

        // The viewport is the whole central area; the layout divides it into views. Every view has
        // its own camera and every border is draggable, so the pointer names the view an action
        // belongs to: a press activates it, and a press on a border starts a resize instead.
        let regions = self.layout.regions(area);
        let pointer = self.pointer_position;
        let hovered =
            pointer.and_then(|pointer| regions.iter().find(|(_, rect)| rect.contains(pointer)).map(|(leaf, _)| *leaf));
        let pressed = ui.input(|input| input.pointer.primary_pressed());
        if pressed && let Some(pointer) = response.interact_pointer_pos() {
            if self.layout.begin_drag(area, pointer) {
                // A border drag owns the primary button until it is released; nothing else in the
                // workspace reads the pointer meanwhile, so a resize cannot select an object or
                // move an axis under the user's hand.
                self.divider_dragging = true;
            } else if let Some(leaf) = hovered {
                self.layout.set_active(leaf);
            }
        }
        if self.divider_dragging {
            let delta = response.drag_delta();
            self.layout.drag_by(area, delta);
            if !ui.input(|input| input.pointer.primary_down()) {
                self.layout.end_drag();
                self.divider_dragging = false;
            }
            if let Some(axis) = self.layout.dragging_axis() {
                ui.ctx().set_cursor_icon(axis.cursor());
            }
            ui.ctx().request_repaint();
        } else if let Some(pointer) = self.pointer_position
            && let Some(axis) = self.layout.border_at(area, pointer)
        {
            ui.ctx().set_cursor_icon(axis.cursor());
        }

        // The view this frame's actions belong to: the one under the pointer, or the active one.
        let target_leaf = hovered.unwrap_or_else(|| self.layout.active());
        let target_rect = regions.iter().find(|(leaf, _)| *leaf == target_leaf).map(|(_, rect)| *rect).unwrap_or(area);
        let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or_default();
        // The scene's camera is the one the pointer view draws with; an authored camera wins over
        // both, exactly as it does for a single view (ADR 0049).
        self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);
        let rect = target_rect;
        let gizmo_rect = gizmo::widget_rect(rect);

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

        let pointer_over_widget =
            response.hover_pos().is_some_and(|pointer| gizmo_rect.is_some_and(|widget| widget.contains(pointer)));
        // A border drag owns the pointer: the tools, the gizmos and picking all stay out of its way
        // until it is released, so a resize can never select an object or move an axis.
        let interactive = !self.divider_dragging;
        // A contour edit owns the pointer: the object gizmo and picking would fight the corner drag.
        let contour_editing = interactive && self.editor.contour_edit().is_some();
        // A drawing tool owns it too: the left button places points, not selections.
        let drawing = interactive && self.editor.drawing().is_some();
        // An armed primitive owns the next click: it places a cube, it does not select.
        let placing = interactive && self.editor.pending_primitive().is_some();
        // The pull arrow is the live gesture after an extrusion. A contour edit, a draft, a
        // placement or a sub-object mode takes the pointer from it, so the arrow never competes
        // with a grip, a point or a cube for the same click.
        if self.extrude_gizmo.is_active()
            && (contour_editing || drawing || placing || self.editor.mesh_edit().is_some())
        {
            self.extrude_gizmo.cancel();
        }
        let pulling = interactive && self.extrude_gizmo.is_active();
        // An armed navigation gesture owns a primary-button drag, but not the other tools: while a
        // contour edit, a draft or a sub-object mode is live, that mode keeps the pointer.
        let navigation = self.cluster.navigation;
        let navigating = navigation.is_some() && !contour_editing && !drawing && !placing && !pulling;
        let mesh_object = if contour_editing || drawing || placing || pulling {
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
            let finished = match self.draw_gizmo.update(
                &response,
                rect,
                self.scene.camera,
                &mut self.editor,
                pointer_over_widget,
                modifiers,
            ) {
                Ok(true) => {
                    ui.ctx().request_repaint();
                    // A fixed tool is done on its last point and a closed polyline on its close
                    // click: the finished draft commits right away, as a CAD tool does.
                    self.editor.drawing().is_some_and(DrawDraft::is_finished)
                }
                Ok(false) => false,
                Err(error) => {
                    self.last_error = Some(error);
                    false
                }
            };
            if finished && let Err(error) = self.commit_drawing() {
                self.last_error = Some(error);
            }
            // The right button finishes the figure as Escape does; it is the CAD habit and keeps
            // the left button free for placing points.
            if !finished
                && !pointer_over_widget
                && response.clicked_by(egui::PointerButton::Secondary)
                && let Err(error) = self.commit_drawing()
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
                && let Some(pointer) = response.interact_pointer_pos()
            {
                let plane = WorkPlane::from_camera(self.scene.camera);
                let placement =
                    plane.pointer_point(self.scene.camera, rect, pointer).and_then(|local| plane.placement_at(local));
                match placement.map(|placement| self.editor.place_primitive(&mut self.session, placement)) {
                    Some(Ok(_)) => {
                        if let Err(error) = self.sync_scene() {
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
        let update: Result<Option<PlacementCommit>, String> =
            if contour_editing || drawing || placing || pulling || navigating || mesh_object.is_some() {
                Ok(None)
            } else {
                match self.mode {
                    TransformMode::Move => {
                        self.translate_gizmo.update(ui, &response, rect, target, pointer_over_widget)
                    }
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
            match self.mode {
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
        if navigable && response.hovered() && !dragging {
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
        if navigable && !dragging && navigating && response.dragged_by(egui::PointerButton::Primary) {
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
        if navigable && !dragging && response.dragged_by(egui::PointerButton::Middle) {
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
            && response.dragged_by(egui::PointerButton::Secondary)
            && let Some(leaf) = self.layout.leaf_mut(target_leaf)
        {
            leaf.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(navigation_rect.height()));
        }
        // The composite camera follows the navigated view unless an authored camera wins.
        let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or(target_camera);
        self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);

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
            } else if let Some(id) =
                scene_gizmo::pick(self.scene.camera, rect, self.session.project(), pointer, &previewed)
            {
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
        self.scene.shading = self.cluster.style;
        self.scene.contact_shadows = self.cluster.shadow_settings();
        if self.scene.all_edges != self.cluster.all_edges {
            self.scene.all_edges = self.cluster.all_edges;
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
            scene_build::authored_lighting_with(self.session.project(), &previewed)
                .unwrap_or_else(|| self.scene.lighting.clone())
        };
        // One pass per view into the same target: the camera comes from the view, the region from
        // its rectangle, and the aspect ratio from the region, so a narrow view is not stretched
        // by the workspace's shape.
        let views: Vec<(Camera, furnicraft_render::ViewRegion)> = regions
            .iter()
            .filter_map(|(leaf, view_rect)| {
                let leaf = self.layout.leaf(*leaf)?;
                let camera = scene_build::scene_camera(self.session.project(), leaf.camera());
                Some((camera, view_region(area, *view_rect, points_per_pixel)))
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
                .map(|leaf| scene_build::scene_camera(self.session.project(), leaf.camera()))
                .unwrap_or(self.scene.camera);
            scene_gizmo::paint(
                ui,
                *view_rect,
                camera,
                self.session.project(),
                self.editor.selected_nodes(),
                &previewed,
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
                let (grip, value) = match action {
                    contour_gizmo::NumericAction::Apply(grip, value)
                    | contour_gizmo::NumericAction::Restore(grip, value) => (grip, value),
                };
                match self.editor.contour_edit_set_grip_value(grip, value) {
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
            if let Some(action) = self.extrude_gizmo.paint(ui, rect, self.scene.camera, self.session.project()) {
                self.apply_extrude_action(action);
            }
        } else if navigating {
            // The navigation gesture owns the viewport; an axis under the pointer would compete
            // with the camera drag, so the transform gizmo stays hidden until the mode is disarmed.
        } else {
            match self.mode {
                TransformMode::Move => self.translate_gizmo.paint(ui, rect, paint_target),
                TransformMode::Rotate => self.rotate_gizmo.paint(ui, rect, paint_target),
            }
        }
        // The widget belongs to the view under the pointer, so clicking an axis snaps that view and
        // the readout names the camera it draws with.
        if self.session.project().active_camera().is_none()
            && let Some(action) = gizmo::show(ui, rect, target_camera, self.cluster.angles_visible())
        {
            match action {
                gizmo::GizmoAction::LookFrom(direction) => {
                    if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                        leaf.look_from(direction);
                    }
                }
                gizmo::GizmoAction::Home => self.home_view(target_leaf),
            }
            let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or(target_camera);
            self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);
        }

        // The cluster floats over the drawn viewport, so it is the last thing painted inside it.
        let view = ClusterView {
            owns_view: viewport_owns_view(self.session.project()),
            projection: self.scene.camera.projection(),
            work_plane: WorkPlane::from_camera(target_camera).label(),
            layout: self.layout.kind(),
            preset: self.layout.preset(target_leaf),
        };
        match view_cluster::show(ui, area, &mut self.cluster, view) {
            Some(ClusterAction::FrameAll) => self.reset_view(),
            // Home acts on the view the cluster hovers over, or the active one when the pointer is
            // not over a view.
            Some(ClusterAction::Home) => {
                let leaf = self.pointer_leaf(&regions);
                self.home_view(leaf);
            }
            Some(ClusterAction::ToggleProjection) => {
                if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                    leaf.toggle_projection();
                }
                let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or(target_camera);
                self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);
            }
            Some(ClusterAction::SetLayout(kind)) => self.set_layout(kind),
            Some(ClusterAction::SetPreset(preset)) => {
                if let Some(leaf) = self.layout.leaf_mut(target_leaf) {
                    leaf.apply_preset(preset);
                }
                let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or(target_camera);
                self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);
            }
            None => {}
        }
        self.paint_view_frames(ui, area, &regions);
    }

    /// Divides the workspace into `kind`, keeping the cameras the views already have.
    fn set_layout(&mut self, kind: LayoutKind) {
        self.layout.set_kind(kind);
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.layout.active_camera());
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

    /// The scene tree, as a folding window.
    ///
    /// The panel owns no heading and no tools of its own: the header row names it, counts its nodes
    /// and folds it, and the body is only the tree. Grouping survives in the row context menu.
    fn scene_tree(&mut self, ui: &mut egui::Ui) {
        let head = PanelHead {
            title: "Scene",
            count: self.node_count,
            icon: icons::scene::ASSEMBLY,
            default_width: 240.0,
            min_width: 180.0,
        };
        side_panel(ui, PanelSide::Left, "scene_tree", &mut self.tree_open, head, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.editor.show_scene_tree(ui, &mut self.session);
            });
        });
    }

    /// The ribbon under the header: the tabs and the tool groups of the active tab.
    ///
    /// The ribbon owns no document state and no viewport state: it returns whether a drawing tool
    /// started and at most one view action, which the shell performs with the same methods the
    /// status bar uses. It sits between the header and the side panels, so it spans the window.
    fn ribbon(&mut self, ui: &mut egui::Ui) {
        let view = furnicraft_ui::RibbonView {
            perspective: matches!(
                scene_build::scene_camera(self.session.project(), self.layout.active_camera()).projection(),
                Projection::Perspective { .. }
            ),
            owns_view: viewport_owns_view(self.session.project()),
            grid: self.draw_gizmo.grid(),
        };
        let mut outcome = None;
        // The ribbon is a raised chrome surface, so it takes the surface fill rather than the panel
        // one that the side panels use behind their content. The settings slider scales that fill's
        // alpha, letting the window chrome show through without changing any other surface.
        let frame = egui::Frame::new()
            .fill(theme::surface(ui).gamma_multiply(self.settings.ribbon.opacity))
            .inner_margin(egui::Margin::symmetric(6, 2));
        egui::Panel::top("ribbon").frame(frame).show(ui, |ui| {
            ui.add_space(theme::STEP);
            match self.editor.show_ribbon(ui, &mut self.session, view) {
                Ok(result) => outcome = Some(result),
                Err(error) => self.last_error = Some(error),
            }
            ui.add_space(theme::STEP);
        });
        if let Some(outcome) = outcome {
            if outcome.started_drawing {
                self.draw_gizmo.reset();
                self.extrude_gizmo.cancel();
                // Starting a drawing tool is a deliberate change of gesture: an armed navigation
                // mode must not take the primary button back when the draft ends.
                self.cluster.navigation = None;
            }
            if let Some((object, height)) = outcome.extruded {
                // The body exists at the height the field named; the arrow lets the user pull it
                // from there, and the ribbon keeps that height for the next extrusion.
                self.contour_gizmo.reset();
                self.draw_gizmo.reset();
                self.extrude_gizmo.start(object, height);
            }
            match outcome.view {
                Some(furnicraft_ui::RibbonViewAction::FrameAll) => self.reset_view(),
                Some(furnicraft_ui::RibbonViewAction::ToggleProjection) => {
                    if let Some(leaf) = self.layout.leaf_mut(self.layout.active()) {
                        leaf.toggle_projection();
                    }
                    let camera = self.layout.active_camera();
                    self.scene.camera = scene_build::scene_camera(self.session.project(), camera);
                }
                Some(furnicraft_ui::RibbonViewAction::ToggleGrid) => self.draw_gizmo.toggle_grid(),
                None => {}
            }
        }
    }

    /// The inspector, as a folding window over the same header row.
    fn panel_card(&mut self, ui: &mut egui::Ui) {
        let head = PanelHead {
            title: "Inspector",
            count: self.editor.selected_nodes().len(),
            icon: icons::actions::SETTINGS,
            default_width: 300.0,
            min_width: 260.0,
        };
        side_panel(ui, PanelSide::Right, "panel_card", &mut self.card_open, head, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.editor.show_card(ui, &mut self.session);
            });
        });
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            self.viewport(ui);
        });
    }

    /// The thin bar at the bottom of the window: the armed tool, the gesture it expects, the counts
    /// it would affect and the last error.
    ///
    /// View controls no longer live here: they float over the viewport in [`ViewCluster`], so the
    /// scene starts at the model instead of at a wall of labels. The bar is chrome and carries no
    /// document state; everything it shows is read from the session or the editor.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let info = self.status_info();
        let mut out = StatusOut::default();
        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.set_height(theme::STATUS_HEIGHT);
            // One thin row, left to right: the bar is chrome, not a second sidebar.
            ui.horizontal_centered(|ui| {
                (out.state, out.mode) = status_state_ui(ui, &info);
            });
        });
        self.apply_status(out);
    }

    /// Everything the bar reads from the session, the editor and the view camera.
    fn status_info(&self) -> StatusInfo {
        StatusInfo {
            error: self.last_error.clone(),
            notice: self.settings_notice.clone(),
            mode: self.mode,
            contour: self
                .editor
                .contour_edit()
                .map(|edit| (edit.draft().segment_count(), edit.is_dirty(), self.contour_gizmo.along())),
            drawing: self
                .editor
                .drawing()
                .map(|draft| (draft.tool(), draft.len(), draft.is_closed(), draft.arc_mode())),
            preview_error: self.draw_gizmo.preview_error().map(str::to_owned),
            extrude: self.extrude_gizmo.height(),
            primitive: self.editor.pending_primitive(),
            grid: self.draw_gizmo.grid(),
            mesh: self.editor.mesh_edit().map(|edit| {
                let count = match edit.element() {
                    MeshElement::Vertex => edit.vertices().len(),
                    MeshElement::Face => edit.faces().len(),
                };
                (edit.element(), count)
            }),
            navigation: self.cluster.navigation,
            creation_parent: self.editor.creation_parent_name(self.session.project()).map(str::to_owned),
        }
    }

    /// Runs the one action each side of the bar asked for.
    fn apply_status(&mut self, out: StatusOut) {
        if let Some(mode) = out.mode {
            self.mode = mode;
        }
        match out.state {
            Some(StatusAction::ApplyContour) => {
                if let Err(error) = self.apply_contour_edit() {
                    self.last_error = Some(error);
                }
            }
            Some(StatusAction::CancelContour) => self.cancel_contour_edit(),
            Some(StatusAction::ToggleContourAlong) => self.contour_gizmo.toggle_along(),
            Some(StatusAction::ToggleGrid) => self.draw_gizmo.toggle_grid(),
            Some(StatusAction::CloseDraft) => match self.editor.close_drawing() {
                Ok(()) => {
                    if let Err(error) = self.commit_drawing() {
                        self.last_error = Some(error);
                    }
                }
                Err(error) => self.last_error = Some(error.to_string()),
            },
            Some(StatusAction::FinishDraft) => {
                if let Err(error) = self.commit_drawing() {
                    self.last_error = Some(error);
                }
            }
            Some(StatusAction::CancelDraft) => self.cancel_drawing(),
            Some(StatusAction::FinishExtrude) => {
                if let (Some(object), Some(height)) = (self.extrude_gizmo.object(), self.extrude_gizmo.height())
                    && let Err(error) = self.finish_extrude(object, height)
                {
                    self.last_error = Some(error);
                }
            }
            Some(StatusAction::CancelExtrude) => self.extrude_gizmo.cancel(),
            Some(StatusAction::CancelPrimitive) => self.editor.cancel_primitive(),
            Some(StatusAction::ClearCreationParent) => self.editor.clear_creation_parent(),
            Some(StatusAction::EndNavigation) => self.cluster.navigation = None,
            None => {}
        }
    }

    /// Draws the command palette and runs the one command it returned.
    ///
    /// The palette lists engine specs and builds their parameters from the current selection; it
    /// never mutates the document itself. A successful dispatch selects a node the command created
    /// and lets the next frame rebuild the scene from the new revision.
    fn command_palette(&mut self, ui: &mut egui::Ui) {
        let context = self.palette_context();
        let Some(id) = self.palette.show(ui.ctx(), &self.session, &context) else {
            return;
        };
        match self.palette.activate(&mut self.session, &context, &id) {
            Ok(outcome) => {
                if let Some(created) = outcome.created_id() {
                    self.editor.select_node(Some(created));
                }
            }
            Err(error) => self.last_error = Some(error),
        }
    }

    /// The selection and creation target the palette's parameter builders read.
    fn palette_context(&self) -> PaletteContext {
        PaletteContext::new(self.editor.selected_nodes().to_vec(), self.editor.creation_target(self.session.project()))
    }

    /// Draws the Settings window and applies what it changed.
    ///
    /// The window edits a copy-free `&mut Settings`; a change is applied to the running application
    /// at once and written to the config file when the window closes. The document never sees any of
    /// it, which is the point of keeping the workspace in its own file.
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let before = self.settings.clone();
        let outcome = furnicraft_ui::settings::show(ctx, &mut self.settings, &mut self.settings_section);
        if outcome.changed {
            self.apply_settings(&before, ctx);
        }
        if outcome.close {
            self.settings_open = false;
            self.settings.sanitize();
            self.persist_settings();
        }
    }

    /// Applies the changed sections of the workspace settings.
    ///
    /// Appearance and viewport are compared section by section: changing the accent must not reset
    /// the display style to the stored default, and a change to the default opening view must not
    /// turn the camera the user is navigating.
    fn apply_settings(&mut self, before: &Settings, ctx: &egui::Context) {
        self.settings.sanitize();
        if self.settings.appearance != before.appearance {
            theme::install_with(ctx, self.settings.appearance.theme.preference(), self.settings.appearance.accent);
            ctx.set_zoom_factor(ui_scale::factor(self.settings.appearance.ui_scale));
        }
        if self.settings.viewport != before.viewport {
            self.cluster.style = self.settings.viewport.style;
            self.cluster.all_edges = self.settings.viewport.all_edges;
            self.scene.shading = self.cluster.style;
            self.scene.all_edges = self.cluster.all_edges;
            let geometry_changed = self.settings.viewport.grid != before.viewport.grid
                || self.settings.viewport.grid_step_mm != before.viewport.grid_step_mm
                || self.settings.viewport.all_edges != before.viewport.all_edges;
            apply_viewport_defaults(&mut self.scene.grid, &self.settings.viewport);
            // A style is frame data the renderer reads at draw time; only a grid or edge-list
            // change rebuilds an uploaded buffer.
            if geometry_changed && let Err(error) = self.upload_scene() {
                self.last_error = Some(error);
            }
        }
    }

    /// Draws the Home surface and performs the one action it asked for.
    ///
    /// Home goes through the same File menu path as the header, so a scene with unsaved changes still
    /// asks before New discards it and Open still warns in its dialog.
    fn home_surface(&mut self, ctx: &egui::Context) {
        if !self.home_open {
            return;
        }
        let Some(action) = furnicraft_ui::home::show(ctx, &self.settings.recent) else {
            return;
        };
        self.home_open = false;
        match action {
            furnicraft_ui::home::HomeAction::New | furnicraft_ui::home::HomeAction::EmptyTemplate => {
                let dirty = self.session.is_dirty();
                if let Some(action) = self.files.new_scene(dirty) {
                    self.handle_file_action(action);
                }
            }
            furnicraft_ui::home::HomeAction::Open => self.files.request_open(),
            furnicraft_ui::home::HomeAction::OpenRecent(path) => {
                self.handle_file_action(FileAction::Open(path));
            }
            furnicraft_ui::home::HomeAction::Close => {}
        }
    }
}

/// The status text and controls, read once per frame so both sides of the bar draw from data.
struct StatusInfo {
    error: Option<String>,
    /// Why the workspace defaults were used instead of the saved settings, if they were.
    notice: Option<String>,
    mode: TransformMode,
    contour: Option<(usize, bool, bool)>,
    drawing: Option<(DrawTool, usize, bool, bool)>,
    /// Why the pointer cannot extend the live draft, if it cannot.
    preview_error: Option<String>,
    /// The height a live pull arrow shows, if a fresh extrusion is waiting for it.
    extrude: Option<Length>,
    primitive: Option<(furnicraft_engine::PrimitiveKind, Length)>,
    grid: bool,
    mesh: Option<(MeshElement, usize)>,
    /// The navigation mode armed in the floating cluster, if any.
    navigation: Option<NavigationMode>,
    /// The container armed to receive the next creation, if any.
    creation_parent: Option<String>,
}

/// What the bar asked for, after the row has drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StatusOut {
    state: Option<StatusAction>,
    mode: Option<TransformMode>,
}

/// What the status text asked the shell to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StatusAction {
    ApplyContour,
    CancelContour,
    ToggleContourAlong,
    ToggleGrid,
    CloseDraft,
    FinishDraft,
    CancelDraft,
    FinishExtrude,
    CancelExtrude,
    CancelPrimitive,
    ClearCreationParent,
    EndNavigation,
}

/// A quiet hint that truncates instead of running under the right-hand view controls.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::text_dim(ui))).truncate());
}

/// The armed tool, the gesture it expects and the counts it would affect.
fn status_state_ui(ui: &mut egui::Ui, info: &StatusInfo) -> (Option<StatusAction>, Option<TransformMode>) {
    if let Some(error) = &info.error {
        ui.colored_label(theme::danger(ui), error);
        theme::divider(ui);
    } else if let Some(notice) = &info.notice {
        // A workspace that fell back to its defaults is a fact, not a failure: it uses the quiet
        // text colour and keeps the rest of the bar usable.
        hint(ui, notice);
        theme::divider(ui);
    }
    // The arm is sticky across creations, so the bar names the container the next node lands in;
    // without the chip a pressed tool would create in a container armed much earlier (ADR 0056).
    if let Some(name) = &info.creation_parent {
        ui.label(format!("Adding to: {name}"));
        if ui.button("✕").on_hover_text("Create at the scene root again").clicked() {
            return (Some(StatusAction::ClearCreationParent), None);
        }
        theme::divider(ui);
    }
    if let Some((segments, dirty, along)) = info.contour {
        ui.label(format!("Contour edit · {segments} segments{}", if dirty { " · modified" } else { "" }));
        let motion = ui.selectable_label(along, if along { "Along contour" } else { "Free move" }).clicked();
        let apply = ui.button("Apply").clicked();
        let cancel = ui.button("Cancel").clicked();
        hint(
            ui,
            if along {
                "Drag corners along their segment  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel"
            } else {
                "Drag corners freely  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel"
            },
        );
        let action = if motion {
            Some(StatusAction::ToggleContourAlong)
        } else if apply {
            Some(StatusAction::ApplyContour)
        } else if cancel {
            Some(StatusAction::CancelContour)
        } else {
            None
        };
        return (action, None);
    }
    if let Some((tool, points, closed, arc_mode)) = info.drawing {
        ui.label(format!(
            "Draw · {}{}{}",
            tool.label(),
            if points == 0 { String::new() } else { format!(" · {points} points") },
            if tool.closes() { if arc_mode { " · arc" } else { " · line" } } else { "" }
        ));
        let grid = ui.selectable_label(info.grid, "Grid 10 mm  (G)").clicked();
        // The ribbon's *Close* and this one read the same rule, so a closure one offers is never
        // greyed out by the other (ADR 0077).
        let close = ui
            .add_enabled(furnicraft_ui::toolbars::draft_can_close(tool, points, closed), egui::Button::new("Close"))
            .clicked();
        let finish = ui.add_enabled(points >= 2, egui::Button::new("Finish  (Esc)")).clicked();
        let cancel = ui.button("Cancel  (Ctrl+Esc)").clicked();
        hint(
            ui,
            "LMB add point  ·  Tab line/arc  ·  Shift axis lock  ·  Alt ignores snaps  ·  Esc/RMB finish  ·  Ctrl+Esc cancel",
        );
        if let Some(reason) = &info.preview_error {
            hint(ui, reason);
        }
        let action = if grid {
            Some(StatusAction::ToggleGrid)
        } else if close {
            Some(StatusAction::CloseDraft)
        } else if finish {
            Some(StatusAction::FinishDraft)
        } else if cancel {
            Some(StatusAction::CancelDraft)
        } else {
            None
        };
        return (action, None);
    }
    if let Some(height) = info.extrude {
        ui.label(format!("Extrude · {height}"));
        let finish = ui.button("Finish  (Enter)").clicked();
        let drop = ui.button("Drop  (Esc)").clicked();
        hint(ui, "Drag the arrow or type a height  ·  Enter finish  ·  Esc drop the pull");
        let action = if finish {
            Some(StatusAction::FinishExtrude)
        } else if drop {
            Some(StatusAction::CancelExtrude)
        } else {
            None
        };
        return (action, None);
    }
    if let Some((kind, size)) = info.primitive {
        let name = match kind {
            furnicraft_engine::PrimitiveKind::Cube => "Cube",
            furnicraft_engine::PrimitiveKind::Cylinder => "Cylinder",
            furnicraft_engine::PrimitiveKind::Sphere => "Sphere",
        };
        ui.label(format!("Primitive · {name} {size}"));
        let cancel = ui.button("Cancel  (Esc)").clicked();
        hint(ui, "LMB place on the work plane  ·  Esc cancel");
        return (cancel.then_some(StatusAction::CancelPrimitive), None);
    }
    if let Some(mode) = info.navigation {
        ui.label(format!("Navigation · {}", mode.name()));
        let done = ui.button("Done  (Esc)").clicked();
        hint(ui, mode.gesture());
        return (done.then_some(StatusAction::EndNavigation), None);
    }
    ui.label(egui::RichText::new("Gizmo").color(theme::text_dim(ui)));
    let mut mode = info.mode;
    let moved = ui.selectable_value(&mut mode, TransformMode::Move, "Move").clicked()
        | ui.selectable_value(&mut mode, TransformMode::Rotate, "Rotate").clicked();
    theme::divider(ui);
    if let Some((element, count)) = info.mesh {
        ui.label(format!("Mesh edit · {count} {} selected", element.noun()));
        ui.label(
            egui::RichText::new("LMB select  ·  Shift/Ctrl add  ·  Esc done  ·  MMB orbit").color(theme::text_dim(ui)),
        );
    } else {
        hint(ui, "LMB select  ·  double-click enter  ·  Esc leave  ·  MMB orbit  ·  Shift+MMB pan  ·  Ctrl+MMB zoom");
    }
    (None, moved.then_some(mode))
}

/// A hairline across the panel, quieter than `ui.separator()` and on the spacing grid.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, egui::Stroke::new(1.0, theme::line(ui)));
}

/// Which edge a folding side panel sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelSide {
    Left,
    Right,
}

/// What a folding side panel's header row draws: the title, the count chip and the icon the folded
/// strip keeps.
#[derive(Clone, Copy, Debug)]
struct PanelHead {
    title: &'static str,
    count: usize,
    icon: icons::Icon,
    default_width: f32,
    min_width: f32,
}

/// Draws a side panel that folds to [`theme::COLLAPSED_PANEL_WIDTH`] and keeps its width.
///
/// The expanded and collapsed views are two panels with distinct ids, switched by
/// [`egui::Panel::show_switched`], so the collapsed view's 26 pt state never overwrites the width the
/// user dragged the expanded view to. A click on the strip, a double-click on its edge or a drag
/// outward restores the panel; the chevron in the header folds it.
fn side_panel(
    ui: &mut egui::Ui,
    side: PanelSide,
    id: &str,
    open: &mut bool,
    head: PanelHead,
    body: impl FnOnce(&mut egui::Ui),
) {
    let mut expanded = *open;
    let mut fold_requested = false;
    let mut restore_requested = false;

    let panel = |id: String| match side {
        PanelSide::Left => egui::Panel::left(id),
        PanelSide::Right => egui::Panel::right(id),
    };
    let collapsed_fill = theme::surface(ui);
    let expanded_fill = theme::panel(ui);
    let collapsed = panel(format!("{id}_collapsed"))
        .resizable(true)
        .exact_size(theme::COLLAPSED_PANEL_WIDTH)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(collapsed_fill).inner_margin(egui::Margin::ZERO));
    let expanded_panel = panel(id.to_owned())
        .resizable(true)
        .default_size(head.default_width)
        .min_size(head.min_width)
        .frame(egui::Frame::new().fill(expanded_fill).inner_margin(egui::Margin::ZERO));

    egui::Panel::show_switched(ui, &mut expanded, collapsed, expanded_panel, |ui, unfolded| {
        if unfolded {
            if panel_header(ui, &head) {
                fold_requested = true;
            }
            rule(ui);
            egui::Frame::new().inner_margin(egui::Margin::symmetric(6, theme::STEP as i8)).show(ui, |ui| {
                body(ui);
            });
        } else {
            collapsed_strip(ui, &head, &mut restore_requested);
        }
    });

    if fold_requested {
        expanded = false;
    }
    if restore_requested {
        expanded = true;
    }
    *open = expanded;
}

/// The header of an unfolded side panel: the title, a count chip and the fold chevron.
///
/// Returns whether the chevron was clicked.
fn panel_header(ui: &mut egui::Ui, head: &PanelHead) -> bool {
    let mut fold = false;
    egui::Frame::new().fill(theme::surface(ui)).inner_margin(egui::Margin::symmetric(theme::STEP as i8, 0)).show(
        ui,
        |ui| {
            ui.set_height(theme::PANEL_HEADER_HEIGHT);
            ui.horizontal_centered(|ui| {
                ui.label(egui::RichText::new(head.title).strong());
                count_chip(ui, head.count);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_button_at(ui, icons::actions::COLLAPSE, 12.0, "Fold panel", true, false).clicked() {
                        fold = true;
                    }
                });
            });
        },
    );
    fold
}

/// A small count chip beside a panel title.
fn count_chip(ui: &mut egui::Ui, count: usize) {
    egui::Frame::new()
        .fill(theme::control(ui))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(count.to_string()).small().color(theme::text_dim(ui)));
        });
}

/// The folded panel: a 26 pt strip with the panel's icon and its title turned to read along the
/// strip. A click anywhere on the strip restores the panel.
fn collapsed_strip(ui: &mut egui::Ui, head: &PanelHead, restore: &mut bool) {
    let size = egui::vec2(ui.available_width(), ui.available_height());
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *restore = true;
    }
    let fill = if response.hovered() { theme::control_hover(ui) } else { theme::surface(ui) };
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, fill);

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, rect.top() + theme::PANEL_HEADER_HEIGHT / 2.0),
        egui::Vec2::splat(14.0),
    );
    icons::draw_icon(ui, head.icon, icon_rect, 14.0, theme::text_dim(ui));

    // The title is turned so it reads bottom to top. `with_angle` turns about the galley's top-left
    // corner, so the corner is offset by the rotated half-size to centre the turned text.
    let dim = theme::text_dim(ui);
    let galley = ui.painter().layout_no_wrap(head.title.to_owned(), egui::FontId::proportional(12.0), dim);
    let size = galley.size();
    let center = egui::pos2(rect.center().x, rect.center().y + theme::PANEL_HEADER_HEIGHT / 2.0);
    let pos = egui::pos2(center.x - size.y / 2.0, center.y + size.x / 2.0);
    ui.painter().add(egui::epaint::TextShape::new(pos, galley, dim).with_angle(-std::f32::consts::FRAC_PI_2));
    response.on_hover_text(format!("Open the {} panel", head.title));
}

/// Whether the viewport owns its view.
///
/// An authored camera stores its placement and projection in the document, so looking through one
/// pins both navigation and the projection toggle; a command is the only way to change either.
fn viewport_owns_view(project: &Project) -> bool {
    project.active_camera().is_none()
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

/// The camera a fresh viewport opens with: framed on the scene from the chosen direction.
///
/// The fit distance and the projection stay what [`home_camera`] computed, so choosing *Top* never
/// moves the model or changes its size; only the eye direction follows the setting.
fn opening_camera(bounds: crate::scene_build::Bounds, view: OpeningView) -> Result<Camera, String> {
    let mut camera = home_camera(bounds)?;
    let [x, y, z] = view.direction();
    camera.look_from(furnicraft_geom::DVec3::new(x, y, z));
    Ok(camera)
}

/// Copies the workspace viewport defaults into a frame's floor grid.
///
/// The grid is view state, not document data, so the shell writes it here after every evaluation
/// instead of letting `furnicraft_scene` read the settings.
fn apply_viewport_defaults(grid: &mut furnicraft_scene::Grid, viewport: &ViewportSettings) {
    grid.visible = viewport.grid;
    grid.spacing_mm = f64::from(viewport.grid_step_mm.max(1));
}

/// The physical pixels one view's rectangle covers inside the workspace.
///
/// The view rect is a point rectangle in the central panel; the target starts at the panel's
/// top-left. Both edges are rounded separately and the size is the difference, so two neighbours
/// that share an edge in points also share exactly one edge in pixels instead of overlapping or
/// leaving a seam. A view narrower than half a pixel still reaches one pixel.
fn view_region(area: egui::Rect, view: egui::Rect, points_per_pixel: f32) -> furnicraft_render::ViewRegion {
    let scale = |value: f32| (value * points_per_pixel).round().max(0.0) as u32;
    let left = scale(view.left() - area.left());
    let top = scale(view.top() - area.top());
    let right = scale(view.right() - area.left());
    let bottom = scale(view.bottom() - area.top());
    furnicraft_render::ViewRegion {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(1),
        height: bottom.saturating_sub(top).max(1),
    }
}

/// The global shortcuts, read in one pass and consumed so one key does one thing.
///
/// The list is the one the Settings window draws; a key added there has to be added here, and a key
/// removed here must leave the table. The drawing tools read their own keys, so only the modified
/// pair and the two view keys live at this level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Shortcuts {
    save: bool,
    undo: bool,
    redo: bool,
    frame: bool,
    projection: bool,
    palette: bool,
}

fn take_shortcuts(ctx: &egui::Context) -> Shortcuts {
    ctx.input_mut(|input| Shortcuts {
        save: input.consume_key(egui::Modifiers::CTRL, egui::Key::S),
        undo: input.consume_key(egui::Modifiers::CTRL, egui::Key::Z),
        redo: input.consume_key(egui::Modifiers::CTRL, egui::Key::Y),
        frame: input.consume_key(egui::Modifiers::NONE, egui::Key::F),
        projection: input.consume_key(egui::Modifiers::NONE, egui::Key::O),
        palette: input.consume_key(egui::Modifiers::COMMAND, egui::Key::K),
    })
}

impl eframe::App for FurniCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Err(error) = self.sync_scene() {
            self.last_error = Some(error);
        }
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
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
            // Escape or the right button finishes the figure; Ctrl+Esc (or the Cancel button)
            // throws the draft away. Enter stays as the keyboard finish the status bar names.
            if !ui.ctx().text_edit_focused() {
                let ctrl = ui.input(|input| input.modifiers.ctrl);
                if escape && ctrl {
                    self.cancel_drawing();
                } else if (escape || enter)
                    && let Err(error) = self.commit_drawing()
                {
                    self.last_error = Some(error);
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
            // The field parses a typed height itself and keeps both keys while it has focus.
            // Outside it, Enter closes the pull and Escape drops it, leaving the height the
            // document already holds.
            if !ui.ctx().text_edit_focused() {
                if escape {
                    self.extrude_gizmo.cancel();
                } else if enter
                    && let (Some(object), Some(height)) = (self.extrude_gizmo.object(), self.extrude_gizmo.height())
                    && let Err(error) = self.finish_extrude(object, height)
                {
                    self.last_error = Some(error);
                }
            }
        } else if escape
            && !self.translate_gizmo.is_dragging()
            && !self.rotate_gizmo.is_dragging()
            && !self.vertex_gizmo.is_dragging()
            && !ui.ctx().text_edit_focused()
        {
            if self.settings_open || self.home_open || self.palette.is_open() {
                // The modal owns Escape for this frame and closes itself.
            } else if self.cluster.settings_open {
                // The popover owns Escape for this frame and closes itself.
            } else if self.cluster.navigation.is_some() {
                self.cluster.navigation = None;
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
        // A drawing shortcut starts its tool from anywhere outside a text field. The table is the
        // one the toolbar hints show, so the key and the tooltip cannot drift apart.
        if self.editor.contour_edit().is_none() && self.editor.drawing().is_none() && !ui.ctx().text_edit_focused() {
            for tool in furnicraft_ui::toolbars::CONTOUR_TOOLS {
                let Some(shortcut) = furnicraft_ui::toolbars::shortcut(tool) else {
                    continue;
                };
                let modifiers = if shortcut.shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
                if ui.input_mut(|input| input.consume_key(modifiers, shortcut.key)) {
                    let sides = self.editor.polygon_sides();
                    if !self.editor.start_drawing_with(tool, sides) {
                        self.last_error = Some("A drawing is already in progress.".to_owned());
                    }
                    self.draw_gizmo.reset();
                    break;
                }
            }
        }
        if self.editor.selected_nodes() != self.highlighted
            && let Err(error) = self.upload_scene()
        {
            self.last_error = Some(error);
        }
        // The keys the header and the Settings page advertise. They are read only outside a text
        // field and a modal, so typing a file path or a name never saves, undoes or reframes.
        if !ui.ctx().text_edit_focused() && !self.settings_open && !self.home_open && !self.palette.is_open() {
            let shortcuts = take_shortcuts(ui.ctx());
            if shortcuts.palette {
                self.palette.open();
            }
            if shortcuts.save
                && let Some(action) = self.files.save(self.session.is_dirty())
            {
                self.handle_file_action(action);
            }
            if shortcuts.undo
                && let Err(error) = self.session.undo()
            {
                self.last_error = Some(error.to_string());
            }
            if shortcuts.redo
                && let Err(error) = self.session.redo()
            {
                self.last_error = Some(error.to_string());
            }
            if shortcuts.frame {
                self.reset_view();
            }
            if shortcuts.projection {
                if let Some(leaf) = self.layout.leaf_mut(self.layout.active()) {
                    leaf.toggle_projection();
                }
                let camera = self.layout.active_camera();
                self.scene.camera = scene_build::scene_camera(self.session.project(), camera);
            }
        }
        let dirty = self.session.is_dirty();
        let header = HeaderState {
            title: self.files.file_name(),
            dirty,
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
        };
        let mut header_action = None;
        let mut file_action = None;
        egui::Panel::top("header").show(ui, |ui| {
            header_action = header::show(ui, &header, |ui| {
                file_action = self.files.show(ui, dirty);
            });
        });
        if let Some(action) = file_action {
            self.handle_file_action(action);
        }
        if let Some(action) = self.files.show_dialogs(ui.ctx(), dirty) {
            self.handle_file_action(action);
        }
        if let Some(action) = header_action {
            self.handle_header_action(action);
        }
        self.ribbon(ui);
        self.scene_tree(ui);
        self.panel_card(ui);
        self.status_bar(ui);
        self.workspace(ui);
        self.command_palette(ui);
        self.settings_window(ui.ctx());
        self.home_surface(ui.ctx());
    }

    fn on_exit(&mut self) {
        self.persist_settings();
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FurniCraft")
            .with_inner_size([1280.0, 840.0])
            .with_min_inner_size([640.0, 480.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "FurniCraft",
        options,
        Box::new(|cc| {
            let app = FurniCraftApp::new(cc)?;
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_geom::{DMat4, DQuat, DVec3};
    use furnicraft_model::{CameraProjection, LightKind};
    use furnicraft_testkit::sample_project;
    use furnicraft_units::Length;

    use super::*;
    use crate::view_cluster::{cluster_anchor, projection_button};

    /// Collects the text of every text shape, including nested ones.
    fn drawn_text(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
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
    fn the_projection_button_draws_the_mode_the_view_is_in() {
        let cases = [
            (Projection::Perspective { fov_y: 0.9 }, icons::view::PERSPECTIVE, "orthographic"),
            (Projection::Orthographic { height: 600.0 }, icons::view::ORTHOGRAPHIC, "perspective"),
        ];
        for (projection, expected_icon, action) in cases {
            let (icon, hint) = projection_button(projection);
            assert_eq!(icon, expected_icon, "{projection:?} draws itself, not the mode a click would reach");
            assert!(hint.contains(action), "the hint names the switch to `{action}`: {hint}");
            assert!(hint.contains("(O)"), "the hint keeps the shortcut: {hint}");
        }
    }

    #[test]
    fn the_projection_button_follows_the_camera_the_viewport_draws_with() {
        // The strip reads `self.scene.camera`, which is the authored camera when one is active and
        // the viewport's own camera otherwise. Both paths must lead to the icon of the mode that is
        // actually drawn, which is what ADR 0049 promises an authored camera.
        let mut fixture = fixture();
        let view = Camera::default();
        assert_eq!(
            projection_button(scene_build::scene_camera(&fixture.project, view).projection()).0,
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

        let drawn = scene_build::scene_camera(&fixture.project, view);
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
    fn one_view_region_covers_the_whole_target_and_four_tile_it() {
        let area = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1000.0, 600.0));
        let whole = view_region(area, area, 1.0);
        assert_eq!(whole, furnicraft_render::ViewRegion { x: 0, y: 0, width: 1000, height: 600 });

        // Two columns and two rows at the default ratio, converted at 2 physical pixels per point.
        let half = egui::vec2(500.0, 300.0);
        let quadrants = [
            egui::Rect::from_min_size(area.min, half),
            egui::Rect::from_min_size(area.min + egui::vec2(0.0, 300.0), half),
            egui::Rect::from_min_size(area.min + egui::vec2(500.0, 0.0), half),
            egui::Rect::from_min_size(area.min + egui::vec2(500.0, 300.0), half),
        ];
        let regions: Vec<_> = quadrants.iter().map(|quadrant| view_region(area, *quadrant, 2.0)).collect();
        assert_eq!(
            regions,
            vec![
                furnicraft_render::ViewRegion { x: 0, y: 0, width: 1000, height: 600 },
                furnicraft_render::ViewRegion { x: 0, y: 600, width: 1000, height: 600 },
                furnicraft_render::ViewRegion { x: 1000, y: 0, width: 1000, height: 600 },
                furnicraft_render::ViewRegion { x: 1000, y: 600, width: 1000, height: 600 },
            ]
        );
        let covered: u32 = regions.iter().map(|region| region.width * region.height).sum();
        assert_eq!(covered as f32, 1000.0 * 600.0 * 4.0, "four views cover the target exactly");
    }

    #[test]
    fn a_view_is_never_a_zero_pixel_region() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 10.0));
        let sliver = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(0.1, 0.1));
        let region = view_region(area, sliver, 0.5);
        assert_eq!((region.width, region.height), (1, 1), "a rounding edge still reaches one pixel");
    }

    #[test]
    fn adjacent_view_regions_share_one_edge_without_overlapping() {
        // The point rectangles tile the area, but each region is rounded from its own rect. Both
        // sides compute the same corner from the same number, so they meet exactly; the widths are
        // rounded separately, so the pair can leave at most one pixel for the background clear.
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(801.0, 601.0));
        let x = area.left() + area.width() * 0.37;
        let left = egui::Rect::from_min_max(area.min, egui::pos2(x, area.bottom()));
        let right = egui::Rect::from_min_max(egui::pos2(x, area.top()), area.max);
        for scale in [1.0, 1.25, 2.0] {
            let left = view_region(area, left, scale);
            let right = view_region(area, right, scale);
            let whole = view_region(area, area, scale);
            assert_eq!(left.x, 0);
            assert_eq!(left.y, 0);
            assert_eq!(right.x, left.x + left.width, "the right view starts where the left one ends");
            assert_eq!(right.x + right.width, whole.width, "the pair covers the target exactly");
            assert!(left.width >= 1 && right.width >= 1);
        }
    }

    #[test]
    fn a_pointer_in_a_view_reaches_the_camera_drawn_under_it() {
        // The shell tests the pointer against top-left rectangles and the renderer scissors each
        // view from the same rectangle unchanged. A flip on one side alone would draw one camera
        // under another view's pointer, so this pins the two together.
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut layout = crate::view_layout::Layout::new(Camera::default());
        layout.set_kind(crate::view_layout::LayoutKind::Quad);
        let regions = layout.regions(area);
        assert_eq!(regions.len(), 4);

        for (leaf, view) in &regions {
            let region = view_region(area, *view, 1.0);
            // The pointer at the centre of this view resolves to this leaf.
            let pointer = view.center();
            let hits = regions.iter().find(|(_, rect)| rect.contains(pointer)).map(|(leaf, _)| *leaf);
            assert_eq!(hits, Some(*leaf), "the pointer over {view:?} names that view");

            // And the scissor of this leaf covers the pixel under that pointer.
            let pixel = (pointer.x as u32, pointer.y as u32);
            let (x, y, width, height) = (region.x, region.y, region.width, region.height);
            assert!(
                (x..x + width).contains(&pixel.0) && (y..y + height).contains(&pixel.1),
                "the camera under {pointer:?} is drawn inside {x},{y} {width}×{height}"
            );
        }

        // The top half of the workspace holds the top row of views, in both coordinate systems.
        let top_left = egui::pos2(200.0, 100.0);
        let (leaf, view) = regions
            .iter()
            .find(|(_, rect)| rect.contains(top_left))
            .map(|(leaf, rect)| (*leaf, *rect))
            .expect("a view under the pointer");
        let region = view_region(area, view, 1.0);
        assert_eq!((region.x, region.y), (0, 0), "the top-left view is drawn in the target's top-left");
        assert_eq!(layout.leaf(leaf).map(|leaf| leaf.camera()), Some(Camera::default()));
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

    fn idle_status() -> StatusInfo {
        StatusInfo {
            error: None,
            notice: None,
            mode: TransformMode::Move,
            contour: None,
            drawing: None,
            preview_error: None,
            extrude: None,
            primitive: None,
            grid: true,
            mesh: None,
            navigation: None,
            creation_parent: None,
        }
    }

    #[test]
    fn the_status_bar_draws_one_thin_row() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = idle_status();
        let mut out = StatusOut::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 400.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            ui.set_height(theme::STATUS_HEIGHT);
            ui.horizontal_centered(|ui| {
                (out.state, out.mode) = status_state_ui(ui, &info);
            });
        });
        output.drop_without_applying_deltas();
        assert_eq!(out.state, None, "drawing the idle bar asks for nothing");
        assert_eq!(out.mode, None);
    }

    #[test]
    fn the_status_bar_names_the_armed_drawing_tool() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { drawing: Some((DrawTool::Rectangle, 3, false, false)), ..idle_status() };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            out.state = status_state_ui(ui, &info).0;
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Draw")), "the armed tool is named: {texts:?}");
        assert!(texts.iter().any(|text| text.contains('3')), "the draft's point count is shown: {texts:?}");
        assert_eq!(out.state, None, "drawing alone runs no tool");
    }

    #[test]
    fn the_status_bar_offers_close_from_two_segments_and_through_a_pending_arc() {
        // The status bar is the second *Close* button. It enables from two segments — the closing
        // one makes three — and a pending arc is drawn as a segment the draft already has, so the
        // count it reports is the one the command needs (ADR 0077). The rule is the ribbon's, so a
        // closure one surface offers is never greyed out by the other.
        assert!(furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 2, false));
        assert!(furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 3, false));
        assert!(!furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 1, false));
        assert!(!furnicraft_ui::toolbars::draft_can_close(DrawTool::Polyline, 2, true));

        // The bar still draws the button, so the pair never vanishes between clicks.
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { drawing: Some((DrawTool::Polyline, 2, false, true)), ..idle_status() };
        let output = ctx.run_ui(Default::default(), |ui| {
            status_state_ui(ui, &info);
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Close")), "the bar names its own Close: {texts:?}");
        assert!(
            texts.iter().any(|text| text.contains("arc")),
            "the bar names the pending arc's segment kind: {texts:?}"
        );
    }

    #[test]
    fn the_status_bar_names_the_armed_container_and_offers_to_clear_it() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { creation_parent: Some("Carcass".to_owned()), ..idle_status() };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            out.state = status_state_ui(ui, &info).0;
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(
            texts.iter().any(|text| text.contains("Adding to: Carcass")),
            "the armed container names itself: {texts:?}"
        );
        assert!(texts.iter().any(|text| text.contains('✕')), "the bar offers to clear the arm: {texts:?}");
    }

    #[test]
    fn the_status_bar_names_the_armed_navigation_gesture() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        let info = StatusInfo { navigation: Some(NavigationMode::Orbit), ..idle_status() };
        let mut out = StatusOut::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            out.state = status_state_ui(ui, &info).0;
        });
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text.contains("Navigation · Orbit")), "the armed mode is named: {texts:?}");
        assert!(
            texts.iter().any(|text| text.contains("Drag in the viewport to orbit")),
            "the bar names the gesture the mode expects: {texts:?}"
        );
        assert_eq!(out.state, None, "an idle armed mode runs nothing until its button is clicked");
    }

    #[test]
    fn the_cluster_is_anchored_to_the_bottom_centre_of_the_viewport() {
        let wide = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0));
        let narrow = egui::Rect::from_min_size(egui::pos2(240.0, 0.0), egui::vec2(460.0, 600.0));
        let offset = theme::FLOATING_OFFSET;
        for viewport in [wide, narrow] {
            let anchor = cluster_anchor(viewport, offset);
            assert!((anchor.x - viewport.center().x).abs() < f32::EPSILON, "the cluster is centred on x");
            assert!((anchor.y - (viewport.bottom() - offset)).abs() < f32::EPSILON, "and lifted off the bottom");
        }
        assert_ne!(
            cluster_anchor(wide, offset).x,
            cluster_anchor(narrow, offset).x,
            "folding a side panel moves the cluster with the surface it floats over"
        );
    }

    #[test]
    fn the_angle_switch_controls_the_readout() {
        let mut cluster = ViewCluster::default();
        assert!(!cluster.angles_visible(), "the readout is off until the switch is turned on (U42)");
        cluster.show_angles = true;
        assert!(cluster.angles_visible(), "the switch controls the orientation readout");
    }

    #[test]
    fn the_display_settings_switch_controls_the_contact_shadow() {
        let mut cluster = ViewCluster::default();
        assert!(cluster.shadow_settings().draws(), "the soft ground shadow is on by default");
        cluster.contact_shadows = false;
        assert!(!cluster.shadow_settings().visible, "the switch reaches the frame");
        assert!(!cluster.shadow_settings().draws());
        cluster.contact_shadows = true;
        cluster.shadow_strength = 50.0;
        assert!((cluster.shadow_settings().strength - 0.5).abs() < 1.0e-6);
        cluster.shadow_strength = 0.0;
        assert!(!cluster.shadow_settings().draws(), "a zero strength draws nothing either");
    }

    #[test]
    fn a_folded_panel_keeps_its_expanded_width_and_draws_no_body() {
        let ctx = egui::Context::default();
        furnicraft_ui::theme::install(&ctx);
        ctx.global_style_mut(|style| style.animation_time = 0.0);
        let head = PanelHead {
            title: "Scene",
            count: 4,
            icon: icons::scene::ASSEMBLY,
            default_width: 240.0,
            min_width: 180.0,
        };
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0))),
            ..Default::default()
        };
        let mut open = true;
        let width_open = std::cell::Cell::new(0.0);
        let output = ctx.run_ui(input(), |ui| {
            side_panel(ui, PanelSide::Left, "fold_test", &mut open, head, |ui| {
                width_open.set(ui.max_rect().width());
                ui.label("BODY-CONTENT");
            });
        });
        output.drop_without_applying_deltas();
        assert!(open, "the panel starts unfolded");
        assert!(width_open.get() > theme::COLLAPSED_PANEL_WIDTH, "an unfolded panel has its content width");

        open = false;
        let folded = ctx.run_ui(input(), |ui| {
            side_panel(ui, PanelSide::Left, "fold_test", &mut open, head, |ui| {
                ui.label("BODY-CONTENT");
            });
        });
        let folded_text = drawn_text(&folded.shapes);
        folded.drop_without_applying_deltas();
        assert!(!open, "the panel stays folded");
        assert!(
            !folded_text.iter().any(|text| text.contains("BODY-CONTENT")),
            "a folded panel draws no body: {folded_text:?}"
        );
        assert!(folded_text.iter().any(|text| text.contains("Scene")), "the strip keeps the turned title");

        open = true;
        let width_again = std::cell::Cell::new(0.0);
        let output = ctx.run_ui(input(), |ui| {
            side_panel(ui, PanelSide::Left, "fold_test", &mut open, head, |ui| {
                width_again.set(ui.max_rect().width());
                ui.label("BODY-CONTENT");
            });
        });
        output.drop_without_applying_deltas();
        assert!(open, "the panel is unfolded again");
        assert!(
            (width_again.get() - width_open.get()).abs() < 1.0,
            "the expanded width survives the fold: {} vs {}",
            width_again.get(),
            width_open.get()
        );
    }

    /// One key press, as the platform would hand it to `egui`.
    fn key_input(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }],
            ..Default::default()
        }
    }

    #[test]
    fn the_advertised_shortcuts_are_consumed() {
        let ctx = egui::Context::default();
        let mut read = Shortcuts::default();
        let output = ctx.run_ui(key_input(egui::Key::Z, egui::Modifiers::CTRL), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { undo: true, ..Shortcuts::default() });

        let output = ctx.run_ui(key_input(egui::Key::F, egui::Modifiers::NONE), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { frame: true, ..Shortcuts::default() });

        // `F` with a modifier is a different shortcut and must not reframe.
        let output = ctx.run_ui(key_input(egui::Key::F, egui::Modifiers::CTRL), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts::default());

        let output = ctx.run_ui(key_input(egui::Key::K, egui::Modifiers::COMMAND), |ui| {
            read = take_shortcuts(ui.ctx());
        });
        output.drop_without_applying_deltas();
        assert_eq!(read, Shortcuts { palette: true, ..Shortcuts::default() });
    }
}
