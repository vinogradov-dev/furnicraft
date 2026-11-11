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
mod shortcuts;
mod side_panels;
mod status_bar;
mod strip;
#[cfg(test)]
mod test_support;
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
use furnicraft_model::{CornerMove, Id, NodeQuery, Placement, Project, Revision, StructuralKind};
use furnicraft_project::SceneFile;
use furnicraft_render::ViewportRenderer;
use furnicraft_scene::{Camera, Polyline, Projection, SELECTION_LINE_COLOR, SceneFrame};
use furnicraft_ui::settings::{Settings, SettingsSection, ViewportSettings};
use furnicraft_ui::ui_scale;
use furnicraft_ui::{ContourGrip, DrawDraft, Editor, GripValue, MeshElement, Palette, PaletteContext, icons, theme};
use furnicraft_units::Length;

use crate::contour_gizmo::ContourGizmo;
use crate::draw_gizmo::{DrawAction, DrawGizmo, DrawModifiers};
use crate::extrude_gizmo::{ExtrudeAction, ExtrudeEvent, ExtrudeGizmo};
use crate::file_menu::{FileAction, FileMenu, PreservedSections};
use crate::gizmo_math::{PlacementCommit, Preview, PreviewKey, WorkPlane};
use crate::header::{HeaderAction, HeaderState};
use crate::rotate_gizmo::RotateGizmo;
use crate::scene_build::{Bounds, empty_session, evaluate, home_camera, sample_session};
use crate::settings_store::SettingsStore;
use crate::shortcuts::{DraftKey, drawing_key, take_drawing_shortcut, take_shortcuts};
use crate::side_panels::{PanelHead, PanelSide, side_panel};
use crate::strip::{fit_uncovered, strip_area, strip_rect, uncovered, view_region};
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
    /// World-space bounds of the latest evaluation, so *Frame all* fits the objects the scene has
    /// now instead of the ones it had when the file opened.
    scene_bounds: Bounds,
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
    /// The corner orientation widget, which owns the roll-ring drag across frames.
    gizmo: gizmo::Gizmo,
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
    /// The pointer in viewport points from the previous frame, so a border hover can show its resize
    /// cursor and the wheel can tell whether it is over the active view.
    pointer_position: Option<egui::Pos2>,
    /// The band the floating ribbon covered on the last drawn frame. The side panels are inset by its
    /// height and the framing and the orientation widget stay below it (U51, ADR 0080). `None` until
    /// the first real frame, when the deterministic [`furnicraft_ui::toolbars::height`] stands in.
    ribbon_rect: Option<egui::Rect>,
    /// The central area the viewport drew into last frame, so a keyboard *Frame all* knows the view
    /// it fits into without waiting for the viewport to draw first.
    viewport_area: Option<egui::Rect>,
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
        let scene_bounds = evaluated.bounds;
        let fit = home_camera(scene_bounds)?;
        let layout = Layout::new(fit, settings.viewport.opening_view, settings.viewport.style);
        let mut scene = evaluated.frame;
        layout.apply_frame_style(&mut scene);
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

        let cluster = ViewCluster { all_edges: settings.viewport.all_edges, ..ViewCluster::default() };
        scene.contact_shadows = cluster.shadow_settings();

        Ok(Self {
            scene_revision: session.project().revision(),
            session,
            editor: Editor::new(),
            kernel,
            cache,
            scene,
            layout,
            scene_bounds,
            renderer,
            render_state,
            texture_id,
            translate_gizmo: TranslateGizmo::default(),
            rotate_gizmo: RotateGizmo::default(),
            vertex_gizmo: VertexGizmo::default(),
            contour_gizmo: ContourGizmo::default(),
            draw_gizmo: DrawGizmo::new(),
            extrude_gizmo: ExtrudeGizmo::default(),
            gizmo: gizmo::Gizmo::default(),
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
            ribbon_rect: None,
            viewport_area: None,
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
        let scene_bounds = evaluated.bounds;
        let fit = home_camera(scene_bounds)?;
        let mut scene = evaluated.frame;
        // Opening a file keeps every layout's display styles; only the cameras are refitted.
        self.layout.apply_frame_style(&mut scene);
        scene.all_edges = self.settings.viewport.all_edges;
        apply_viewport_defaults(&mut scene.grid, &self.settings.viewport);

        // A new document keeps the split the user arranged: every stored layout is refitted on the
        // new scene and returns to its standard presets, so a four-view workspace opens the new
        // model framed in every view and never points at where the old one was (ADR 0079).
        self.layout.reset_cameras(fit, self.settings.viewport.opening_view);
        scene.camera = scene_build::scene_camera(session.project(), self.layout.active_camera());

        self.session = session;
        self.node_count = self.session.project().query(&NodeQuery::any()).len();
        self.kernel = kernel;
        self.cache = cache;
        self.scene = scene;
        self.scene_bounds = scene_bounds;
        self.scene_revision = self.session.project().revision();
        self.editor = Editor::new();
        self.cluster = ViewCluster::default();
        self.cluster.all_edges = self.settings.viewport.all_edges;
        self.scene.contact_shadows = self.cluster.shadow_settings();
        self.translate_gizmo = TranslateGizmo::default();
        self.rotate_gizmo = RotateGizmo::default();
        self.vertex_gizmo = VertexGizmo::default();
        self.contour_gizmo = ContourGizmo::default();
        self.draw_gizmo = DrawGizmo::new();
        self.gizmo = gizmo::Gizmo::default();
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
        self.scene_bounds = evaluated.bounds;
        let mut scene = evaluated.frame;
        self.layout.apply_frame_style(&mut scene);
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
        let placement = self.drawing_placement()?;
        self.editor.commit_drawing(&mut self.session, placement)?;
        self.end_drawing()
    }

    /// Closes the running ring and commits it in one step.
    ///
    /// The only close path: a click on the first draft point, the ribbon's *Close* and the status
    /// bar's *Close* all end here, so no surface can close a ring and forget to commit it (ADR 0052).
    /// The ring is closed before the commit and reopened if the document refuses it, so a refusal
    /// leaves the shape the user drew editable instead of a finished draft that cannot be changed.
    fn close_draft(&mut self) -> Result<(), String> {
        let placement = self.drawing_placement()?;
        self.editor.close_and_commit_drawing(&mut self.session, placement)?;
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
    fn end_drawing(&mut self) -> Result<(), String> {
        self.draw_gizmo.reset();
        self.highlighted.clear();
        self.sync_scene()
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

    /// Fits the active view on the scene's current bounds.
    ///
    /// The fit camera is computed from the latest evaluation on every press, so *Frame all* follows
    /// objects created or moved since the file opened. Only the active view moves: its direction
    /// and its projection kind stay as the user left them. The fit targets the part of the view the
    /// floating ribbon leaves uncovered, so the scene is never framed under the strip (U51).
    fn frame_all(&mut self) {
        let fit = match home_camera(self.scene_bounds) {
            Ok(fit) => fit,
            Err(error) => {
                self.last_error = Some(error);
                return;
            }
        };
        let fit = match self.viewport_area {
            Some(area) => {
                let band = self.ribbon_band(area);
                let view = self
                    .layout
                    .regions(area)
                    .into_iter()
                    .find(|(leaf, _)| *leaf == self.layout.active())
                    .map(|(_, rect)| rect)
                    .unwrap_or(area);
                match fit_uncovered(fit, view.height(), uncovered(view, band).height()) {
                    Ok(fit) => fit,
                    Err(error) => {
                        self.last_error = Some(error);
                        return;
                    }
                }
            }
            None => fit,
        };
        self.layout.active_leaf_mut().frame(fit);
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// The band the floating ribbon covers at the top of the viewport `area`.
    ///
    /// The measured rectangle from this frame wins, because the ribbon's body can be taller than its
    /// deterministic height in a narrow window; the open state's height stands in on the first frame.
    fn ribbon_band(&self, area: egui::Rect) -> egui::Rect {
        let height = self
            .ribbon_rect
            .map_or_else(|| furnicraft_ui::toolbars::height(self.editor.ribbon_open()), |rect| rect.height());
        strip_rect(area, height.min(area.height()))
    }

    /// The height the side panels start below, so the floating ribbon never hides their header.
    fn ribbon_inset(&self) -> f32 {
        self.ribbon_rect
            .map_or_else(|| furnicraft_ui::toolbars::height(self.editor.ribbon_open()), |rect| rect.height())
    }

    /// Returns the active view to *Axonometric* without refitting the scene.
    ///
    /// The preset writes yaw and pitch only, so the model does not move and the view keeps its
    /// target, its distance and its projection (ADR 0049).
    fn home_view(&mut self) {
        self.layout.active_leaf_mut().home();
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.layout.active_camera());
    }

    /// Switches the active view's projection and refreshes the camera the viewport draws with.
    ///
    /// The ribbon, the Display popover and the corner widget all end here, so the three entry points
    /// cannot disagree. The projection is view state, so this is not a command and leaves no undo
    /// entry (ADR 0049).
    fn toggle_view_projection(&mut self) {
        self.layout.active_leaf_mut().toggle_projection();
        let camera = self.layout.active_camera();
        self.scene.camera = scene_build::scene_camera(self.session.project(), camera);
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
        // The render target covers the whole central area, including the band under the floating
        // ribbon, so the scene image runs under the strip (U51). Only the overlays that must stay
        // readable — the orientation widget — move into the uncovered part.
        self.viewport_area = Some(area);
        self.pointer_position = ui.input(|input| input.pointer.hover_pos());

        // The viewport is the whole central area; the layout divides it into views. Every view has
        // its own camera and every border is draggable. A press on a border resizes without moving
        // the target; a press anywhere else activates that view, whichever button it is, and only
        // the active view is ever manipulated (ADR 0079).
        let regions = self.layout.regions(area);
        let pointer = self.pointer_position;
        let pressed = ui.input(|input| input.pointer.any_pressed());
        if pressed && let Some(pointer) = response.interact_pointer_pos() {
            let primary = ui.input(|input| input.pointer.primary_pressed());
            if self.layout.press(area, pointer, primary) {
                // A border drag owns the primary button until it is released; nothing else in the
                // workspace reads the pointer meanwhile, so a resize cannot select an object or
                // move an axis under the user's hand.
                self.divider_dragging = true;
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

        // The active view is the only target: orbit, pan, zoom, the presets, selection, the gizmos
        // and the cluster all act on it. A press is how the user points them at a neighbour.
        let target_leaf = self.layout.active();
        let target_rect = regions.iter().find(|(leaf, _)| *leaf == target_leaf).map(|(_, rect)| *rect).unwrap_or(area);
        let target_camera = self.layout.leaf(target_leaf).map(|leaf| leaf.camera()).unwrap_or_default();
        let pointer_over_active = pointer.is_some_and(|pointer| target_rect.contains(pointer));
        // The scene's camera is the one the active view draws with; an authored camera wins over
        // both, exactly as it does for a single view (ADR 0049).
        self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);
        let rect = target_rect;
        // The orientation widget belongs to the part of the view the ribbon leaves uncovered, so the
        // strip never hides the cube (U51).
        let visible = uncovered(rect, self.ribbon_band(area));
        let gizmo_rect = gizmo::widget_rect(visible);

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
                        if let Err(error) = self.commit_drawing() {
                            self.last_error = Some(error);
                        }
                        true
                    } else {
                        false
                    }
                }
                DrawAction::Close => {
                    ui.ctx().request_repaint();
                    if let Err(error) = self.close_draft() {
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
        self.layout.apply_frame_style(&mut self.scene);
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
        let views: Vec<furnicraft_render::ViewSpec> = regions
            .iter()
            .filter_map(|(leaf, view_rect)| {
                let leaf = self.layout.leaf(*leaf)?;
                let camera = scene_build::scene_camera(self.session.project(), leaf.camera());
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
            && let Some(action) = self.gizmo.show(
                ui,
                visible,
                target_camera,
                self.cluster.angles_visible(),
                self.settings.viewport.roll_step_degrees,
            )
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
            self.scene.camera = scene_build::scene_camera(self.session.project(), target_camera);
        }

        // The cluster floats over the drawn viewport, so it is the last thing painted inside it.
        let view = ClusterView {
            owns_view: viewport_owns_view(self.session.project()),
            projection: self.scene.camera.projection(),
            work_plane: WorkPlane::from_camera(target_camera).label(),
            layout: self.layout.kind(),
            maximized: self.layout.maximized(),
            preset: self.layout.preset(target_leaf),
            style: self.layout.active_style(),
        };
        match view_cluster::show(ui, area, &mut self.cluster, view) {
            Some(ClusterAction::FrameAll) => self.frame_all(),
            Some(ClusterAction::Home) => self.home_view(),
            Some(ClusterAction::ToggleProjection) => self.toggle_view_projection(),
            Some(ClusterAction::SetLayout(kind)) => self.set_layout(kind),
            Some(ClusterAction::ToggleMaximize) => self.layout.toggle_maximized(),
            Some(ClusterAction::SetPreset(preset)) => {
                self.layout.active_leaf_mut().apply_preset(preset);
                let camera = self.layout.active_camera();
                self.scene.camera = scene_build::scene_camera(self.session.project(), camera);
            }
            Some(ClusterAction::SetStyle(style)) => {
                self.layout.set_active_style(style);
                // The style is frame data the renderer reads per view at draw time, so no upload.
                self.layout.apply_frame_style(&mut self.scene);
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
    fn scene_tree(&mut self, ui: &mut egui::Ui, inset: f32) {
        let head = PanelHead {
            title: "Scene",
            count: self.node_count,
            icon: icons::scene::ASSEMBLY,
            default_width: 240.0,
            min_width: 180.0,
        };
        side_panel(ui, PanelSide::Left, "scene_tree", &mut self.tree_open, head, inset, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.editor.show_scene_tree(ui, &mut self.session);
            });
        });
    }

    /// The ribbon over the design space: the tabs and the tool groups of the active tab.
    ///
    /// The strip floats on the middle layer instead of taking height from the viewport, so the scene
    /// image runs under it and a translucent ribbon blends with the furniture (U51, ADR 0080). It is
    /// anchored to `design` — the window below the header — so resizing a side panel never reflows
    /// it; the side panels are inset by [`Self::ribbon_inset`] instead.
    fn ribbon(&mut self, ui: &mut egui::Ui, design: egui::Rect) {
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
        // alpha, letting the scene it covers show through without changing any other surface.
        let fill = theme::surface(ui).gamma_multiply(self.settings.ribbon.opacity);
        // The band the strip occupies is also the band it owns the pointer over, so the tools behind
        // it never see a hover or a click there. The measured rectangle wins once egui has sized the
        // area, which is what keeps the blocked band and the drawn one the same.
        let band = self.ribbon_band(design);
        let rect = strip_area(ui.ctx(), "ribbon", band, fill, |ui| {
            match self.editor.show_ribbon(ui, &mut self.session, view) {
                Ok(result) => outcome = Some(result),
                Err(error) => self.last_error = Some(error),
            }
        });
        // The measured strip wins over the deterministic height: a narrow window grows the body with
        // its scroll bar, and the side panels must start below whatever the ribbon actually covers.
        if rect.is_some() {
            self.ribbon_rect = rect;
        }
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
            // The ribbon reports a *Close* instead of running it: the shell owns the work plane,
            // the gizmo state and the scene rebuild, and every close takes the same path (ADR 0052).
            if outcome.close_draft
                && let Err(error) = self.close_draft()
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

    /// The inspector, as a folding window over the same header row.
    fn panel_card(&mut self, ui: &mut egui::Ui, inset: f32) {
        let head = PanelHead {
            title: "Inspector",
            count: self.editor.selected_nodes().len(),
            icon: icons::actions::SETTINGS,
            default_width: 300.0,
            min_width: 260.0,
        };
        side_panel(ui, PanelSide::Right, "panel_card", &mut self.card_open, head, inset, |ui| {
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
            // The style default applies to the active view at once; the other views keep theirs.
            self.layout.set_active_style(self.settings.viewport.style);
            self.cluster.all_edges = self.settings.viewport.all_edges;
            self.layout.apply_frame_style(&mut self.scene);
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

/// Copies the workspace viewport defaults into a frame's floor grid.
///
/// The grid is view state, not document data, so the shell writes it here after every evaluation
/// instead of letting `furnicraft_scene` read the settings.
fn apply_viewport_defaults(grid: &mut furnicraft_scene::Grid, viewport: &ViewportSettings) {
    grid.visible = viewport.grid;
    grid.spacing_mm = f64::from(viewport.grid_step_mm.max(1));
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
            // Escape drops the draft and ends the tool; nothing reached the document and undo gains
            // no half figure. Enter and the right button commit a valid draft (ADR 0076), and a
            // refused commit keeps both the draft and the tool so the user can fix or cancel it.
            if !ui.ctx().text_edit_focused() {
                match drawing_key(escape, enter) {
                    Some(DraftKey::Cancel) => self.cancel_drawing(),
                    Some(DraftKey::Commit) => {
                        if let Err(error) = self.commit_drawing() {
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
        // A drawing shortcut starts its tool from anywhere outside a text field, including while
        // another draft runs: choosing a tool ends the running draft and starts the new one
        // (ADR 0076). The table is the one the toolbar hints show, so the key and the tooltip
        // cannot drift apart.
        if self.editor.contour_edit().is_none() && !ui.ctx().text_edit_focused() {
            let sides = self.editor.polygon_sides();
            if take_drawing_shortcut(ui.ctx(), &mut self.editor, sides) {
                self.draw_gizmo.reset();
                self.extrude_gizmo.cancel();
                self.cluster.navigation = None;
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
                self.frame_all();
            }
            if shortcuts.projection {
                self.layout.active_leaf_mut().toggle_projection();
                let camera = self.layout.active_camera();
                self.scene.camera = scene_build::scene_camera(self.session.project(), camera);
            }
            if shortcuts.maximize {
                self.layout.toggle_maximized();
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
        let design = ui.available_rect_before_wrap();
        self.ribbon(ui, design);
        let inset = self.ribbon_inset();
        self.scene_tree(ui, inset);
        self.panel_card(ui, inset);
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
}
