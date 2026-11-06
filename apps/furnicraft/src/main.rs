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
mod file_menu;
mod gizmo;
mod gizmo_math;
mod mesh_pick;
mod rotate_gizmo;
mod scene_build;
mod scene_gizmo;
mod translate_gizmo;
mod vertex_gizmo;

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
use furnicraft_ui::{DrawDraft, Editor, MeshElement, icons, ui_scale};

use crate::contour_gizmo::ContourGizmo;
use crate::draw_gizmo::DrawGizmo;
use crate::file_menu::{FileAction, FileMenu, PreservedSections};
use crate::gizmo_math::{PlacementCommit, Preview, PreviewKey, WorkPlane};
use crate::rotate_gizmo::RotateGizmo;
use crate::scene_build::{empty_session, evaluate, home_camera, sample_session};
use crate::translate_gizmo::TranslateGizmo;
use crate::vertex_gizmo::{MeshVertexCommit, VertexGizmo, VertexPreview};

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
    /// The navigable view camera. The rendered camera in `scene` is this one unless an authored
    /// camera is active, so orbit, pan and zoom never overwrite authored camera data.
    view_camera: Camera,
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
}

impl FurniCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let session = sample_session()?;
        let mut kernel = TruckKernel::new();
        let mut cache = EvaluationCache::new();
        let evaluated = evaluate(&mut cache, &mut kernel, &session)?;
        let home_camera = home_camera(evaluated.bounds)?;
        let view_camera = home_camera;
        let mut scene = evaluated.frame;
        scene.camera = scene_build::scene_camera(session.project(), view_camera);

        let render_state =
            cc.wgpu_render_state.clone().ok_or_else(|| "FurniCraft requires the wgpu renderer".to_owned())?;
        let mut renderer = ViewportRenderer::new(&render_state.device, 1280, 800);
        let texture_id = {
            let mut egui_renderer = render_state.renderer.write();
            egui_renderer.register_native_texture(&render_state.device, renderer.color_view(), wgpu::FilterMode::Linear)
        };
        renderer.upload(&render_state.device, &scene).map_err(|error| error.to_string())?;

        Ok(Self {
            scene_revision: session.project().revision(),
            session,
            editor: Editor::new(),
            kernel,
            cache,
            scene,
            view_camera,
            home_camera,
            renderer,
            render_state,
            texture_id,
            translate_gizmo: TranslateGizmo::default(),
            rotate_gizmo: RotateGizmo::default(),
            vertex_gizmo: VertexGizmo::default(),
            contour_gizmo: ContourGizmo::default(),
            draw_gizmo: DrawGizmo::new(),
            mode: TransformMode::Move,
            highlighted: Vec::new(),
            selected_chains: Vec::new(),
            uploaded_preview: None,
            files: FileMenu::new(),
            extra: PreservedSections::new(),
            last_error: None,
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
        let home_camera = home_camera(evaluated.bounds)?;
        let view_camera = home_camera;
        let mut scene = evaluated.frame;
        scene.camera = scene_build::scene_camera(session.project(), view_camera);

        self.session = session;
        self.kernel = kernel;
        self.cache = cache;
        self.scene = scene;
        self.view_camera = view_camera;
        self.home_camera = home_camera;
        self.scene_revision = self.session.project().revision();
        self.editor = Editor::new();
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

    /// Performs the operation the File menu returned and reports a failure in the toolbar.
    fn handle_file_action(&mut self, action: FileAction) {
        self.last_error = None;
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
        if let Err(error) = result {
            self.last_error = Some(error);
        }
    }

    fn sync_scene(&mut self) -> Result<(), String> {
        let revision = self.session.project().revision();
        if revision == self.scene_revision {
            return Ok(());
        }
        let evaluated = evaluate(&mut self.cache, &mut self.kernel, &self.session)?;
        let mut scene = evaluated.frame;
        scene.camera = scene_build::scene_camera(self.session.project(), self.view_camera);
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

    fn reset_view(&mut self) {
        self.view_camera = self.home_camera;
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.view_camera);
    }

    fn viewport(&mut self, ui: &mut egui::Ui) {
        // The toolbar can be wider than the panel in a narrow window; egui then reports the
        // overflowing width as available. Intersecting with the clip rect keeps the viewport and
        // its orientation widget inside the central panel.
        let available = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        if available.width() < 1.0 || available.height() < 1.0 {
            return;
        }
        let (rect, response) = ui.allocate_exact_size(available.size(), egui::Sense::click_and_drag());
        if !ui.is_rect_visible(rect) {
            return;
        }
        let gizmo_rect = gizmo::widget_rect(rect);

        let points_per_pixel = ui.ctx().pixels_per_point();
        let width = (rect.width() * points_per_pixel).round().max(1.0) as u32;
        let height = (rect.height() * points_per_pixel).round().max(1.0) as u32;
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
        // A contour edit owns the pointer: the object gizmo and picking would fight the corner drag.
        let contour_editing = self.editor.contour_edit().is_some();
        // A drawing tool owns it too: the left button places points, not selections.
        let drawing = self.editor.drawing().is_some();
        // An armed primitive owns the next click: it places a cube, it does not select.
        let placing = self.editor.pending_primitive().is_some();
        let mesh_object = if contour_editing || drawing || placing {
            None
        } else {
            self.editor.mesh_edit().map(|edit| edit.object())
        };
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
            let axis_lock = ui.input(|input| input.modifiers.shift);
            let finished = match self.draw_gizmo.update(
                &response,
                rect,
                self.scene.camera,
                &mut self.editor,
                pointer_over_widget,
                axis_lock,
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
        } else {
            self.draw_gizmo.reset();
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
            if contour_editing || drawing || placing || mesh_object.is_some() {
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
        } else if mesh_object.is_some() {
            self.vertex_gizmo.is_dragging()
        } else {
            self.translate_gizmo.is_dragging() || self.rotate_gizmo.is_dragging()
        };
        let consumed_click = if contour_editing || drawing || placing {
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
        // document owns, and a command is the only way to edit document data.
        let navigable = viewport_owns_view(self.session.project());
        if navigable && response.hovered() && !dragging {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.view_camera.zoom((-f64::from(scroll) * ZOOM_PER_SCROLL_POINT).exp());
            }
        }

        let drag = response.drag_delta();
        // An axis drag owns the pointer; rotating the camera mid-drag would move the axis under the
        // pointer and change the drag parameter under the user's hand.
        if navigable && !dragging && response.dragged_by(egui::PointerButton::Middle) {
            let modifiers = ui.input(|input| input.modifiers);
            if modifiers.shift {
                self.view_camera.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(rect.height()));
            } else if modifiers.ctrl {
                let factor = (-f64::from(drag.y) * ZOOM_PER_SCROLL_POINT).exp();
                self.view_camera.zoom(factor);
            } else {
                self.view_camera
                    .orbit(-f64::from(drag.x) * ORBIT_RADIANS_PER_POINT, -f64::from(drag.y) * ORBIT_RADIANS_PER_POINT);
            }
        } else if navigable && !dragging && response.dragged_by(egui::PointerButton::Secondary) {
            self.view_camera.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(rect.height()));
        }
        // The composite camera follows the navigable view unless an authored camera wins.
        self.scene.camera = scene_build::scene_camera(self.session.project(), self.view_camera);

        if contour_editing {
            ui.ctx().set_cursor_icon(self.contour_gizmo.cursor());
        } else if drawing {
            ui.ctx().set_cursor_icon(self.draw_gizmo.cursor());
        } else if placing {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Copy);
        } else if dragging || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }

        let pointer_over_gizmo = response
            .interact_pointer_pos()
            .is_some_and(|pointer| gizmo_rect.is_some_and(|widget| widget.contains(pointer)));
        let clicked = if mesh_object.is_some() {
            // A double-click would otherwise toggle the same vertex twice; the first click of the
            // pair already selected it.
            response.clicked() && !response.double_clicked()
        } else {
            response.clicked() || response.double_clicked()
        };
        if !contour_editing
            && !drawing
            && !placing
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

        // A drag preview commits nothing, but the frame can still follow a moved light: the shader
        // reads one uniform, so a sun direction or a point-light position updates without a mesh
        // rebuild. With no moved light the base lighting is already correct.
        let lighting = if previewed.is_empty() {
            self.scene.lighting.clone()
        } else {
            scene_build::authored_lighting_with(self.session.project(), &previewed)
                .unwrap_or_else(|| self.scene.lighting.clone())
        };
        self.renderer.render_with_lighting(&self.render_state.device, &self.render_state.queue, &self.scene, &lighting);
        ui.painter().with_clip_rect(rect).image(
            self.texture_id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        scene_gizmo::paint(
            ui,
            rect,
            self.scene.camera,
            self.session.project(),
            self.editor.selected_nodes(),
            &previewed,
        );
        self.paint_mesh_selection(ui, rect);
        self.paint_selection_outline(ui, rect);
        let paint_target = gizmo_math::Target {
            camera: self.scene.camera,
            project: self.session.project(),
            selection: self.editor.selected_nodes(),
        };
        // The object gizmo is hidden in mesh mode: it acts on the whole selection, not on vertices.
        if let Some(edit) = self.editor.contour_edit() {
            self.contour_gizmo.paint(ui, rect, self.scene.camera, self.session.project(), edit);
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
        } else {
            match self.mode {
                TransformMode::Move => self.translate_gizmo.paint(ui, rect, paint_target),
                TransformMode::Rotate => self.rotate_gizmo.paint(ui, rect, paint_target),
            }
        }
        if self.session.project().active_camera().is_none()
            && let Some(direction) = gizmo::show(ui, rect, self.view_camera)
        {
            self.view_camera.look_from(direction);
            self.scene.camera = scene_build::scene_camera(self.session.project(), self.view_camera);
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

    fn scene_tree(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("scene_tree").resizable(true).default_size(240.0).min_size(180.0).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.editor.show_toolbars(ui, &mut self.session) {
                    Ok(true) => self.draw_gizmo.reset(),
                    Ok(false) => {}
                    Err(error) => self.last_error = Some(error),
                }
                ui.separator();
                ui.heading("Scene");
                ui.separator();
                self.editor.show_scene_tree(ui, &mut self.session);
            });
        });
    }

    fn panel_card(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("panel_card").resizable(true).default_size(300.0).min_size(260.0).show(ui, |ui| {
            ui.heading("Inspector");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.editor.show_card(ui, &mut self.session);
            });
        });
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let mut reset = false;
            let mut toggle_projection = false;
            let mut apply_contour = false;
            let mut cancel_contour = false;
            let mut toggle_grid = false;
            let mut close_drawing = false;
            let mut finish_drawing = false;
            let mut cancel_drawing = false;
            let owns_view = viewport_owns_view(self.session.project());
            let mesh_selection = self.editor.mesh_edit().map(|edit| {
                let count = match edit.element() {
                    MeshElement::Vertex => edit.vertices().len(),
                    MeshElement::Face => edit.faces().len(),
                };
                (edit.element(), count)
            });
            let contour_edit = self
                .editor
                .contour_edit()
                .map(|edit| (edit.draft().segment_count(), edit.is_dirty()));
            let drawing = self.editor.drawing().map(|draft| (draft.tool(), draft.len(), draft.is_closed()));
            let placing = self.editor.pending_primitive();
            ui.horizontal_wrapped(|ui| {
                ui.heading("FurniCraft");
                ui.separator();
                reset = icons::tool_button(ui, icons::view::FRAME_ALL, "Frame all", Some("F"), true, false).clicked();
                reset |= ui.input(|input| input.key_pressed(egui::Key::F));
                ui.separator();
                let (projection_icon, projection_hint) = projection_button(self.scene.camera.projection());
                let projection_response = ui
                    .add_enabled_ui(owns_view, |ui| icons::icon_button(ui, projection_icon, projection_hint))
                    .inner
                    .on_disabled_hover_text("An authored camera owns the projection");
                toggle_projection = projection_response.clicked();
                toggle_projection |= owns_view && ui.input(|input| input.key_pressed(egui::Key::O));
                ui.separator();
                if owns_view {
                    ui.label(format!("Work plane · {}", WorkPlane::from_camera(self.view_camera).label()));
                    ui.separator();
                }
                ui_scale::show(ui);
                ui.separator();
                if let Some((segments, dirty)) = contour_edit {
                    ui.label(format!(
                        "Contour edit · {segments} segments{}",
                        if dirty { " · modified" } else { "" }
                    ));
                    apply_contour = ui.button("Apply").clicked();
                    cancel_contour = ui.button("Cancel").clicked();
                    ui.separator();
                    ui.label("Drag corners  ·  double-click a side to split  ·  Enter apply  ·  Esc cancel");
                } else if let Some((tool, points, closed)) = drawing {
                    ui.label(format!(
                        "Draw · {}{}",
                        tool.label(),
                        if points == 0 { String::new() } else { format!(" · {points} points") }
                    ));
                    ui.separator();
                    toggle_grid |= ui.selectable_label(self.draw_gizmo.grid(), "Grid 10 mm  (G)").clicked();
                    ui.separator();
                    close_drawing |=
                        ui.add_enabled(tool.closes() && points >= 3 && !closed, egui::Button::new("Close")).clicked();
                    finish_drawing |=
                        ui.add_enabled(points >= 2, egui::Button::new("Finish  (Enter)")).clicked();
                    cancel_drawing |= ui.button("Cancel  (Esc)").clicked();
                    ui.separator();
                    ui.label("LMB add point  ·  Shift axis lock  ·  Enter finish  ·  Esc cancel");
                } else if let Some((kind, size)) = placing {
                    let name = match kind {
                        furnicraft_engine::PrimitiveKind::Cube => "Cube",
                        furnicraft_engine::PrimitiveKind::Cylinder => "Cylinder",
                        furnicraft_engine::PrimitiveKind::Sphere => "Sphere",
                    };
                    ui.label(format!("Primitive · {name} {size}"));
                    ui.separator();
                    ui.label("LMB place on the work plane  ·  Esc cancel");
                } else {
                    ui.label("Gizmo");
                    ui.selectable_value(&mut self.mode, TransformMode::Move, "Move");
                    ui.selectable_value(&mut self.mode, TransformMode::Rotate, "Rotate");
                    ui.separator();
                    if let Some((element, count)) = mesh_selection {
                        ui.label(format!("Mesh edit · {count} {} selected", element.noun()));
                        ui.separator();
                        ui.label("LMB select  ·  Shift/Ctrl add  ·  Esc done  ·  MMB orbit");
                    } else {
                        ui.label(
                            "LMB select  ·  double-click enter  ·  Esc leave  ·  MMB orbit  ·  Shift+MMB pan  ·  Ctrl+MMB/wheel zoom",
                        );
                    }
                }
                if let Some(camera) = self
                    .session
                    .project()
                    .active_camera()
                    .and_then(|id| self.session.project().camera(id))
                {
                    ui.separator();
                    ui.label(format!("Camera · {}", camera.name()));
                }
                if let Some(error) = &self.last_error {
                    ui.separator();
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            });
            if toggle_grid {
                self.draw_gizmo.toggle_grid();
            }
            let mut closed = false;
            if close_drawing {
                match self.editor.close_drawing() {
                    Ok(()) => closed = true,
                    Err(error) => self.last_error = Some(error.to_string()),
                }
            }
            if (finish_drawing || closed) && let Err(error) = self.commit_drawing() {
                self.last_error = Some(error);
            }
            if cancel_drawing {
                self.cancel_drawing();
            }
            if apply_contour
                && let Err(error) = self.apply_contour_edit()
            {
                self.last_error = Some(error);
            }
            if cancel_contour {
                self.cancel_contour_edit();
            }
            if reset {
                self.reset_view();
            }
            if toggle_projection && owns_view {
                self.view_camera.toggle_projection();
            }
            ui.separator();
            self.viewport(ui);
        });
    }
}

/// Whether the viewport owns its view.
///
/// An authored camera stores its placement and projection in the document, so looking through one
/// pins both navigation and the projection toggle; a command is the only way to change either.
fn viewport_owns_view(project: &Project) -> bool {
    project.active_camera().is_none()
}

/// The icon and the hint of the projection toggle.
///
/// The icon draws the mode the view is *in*, and the hint names the switch, so the strip never
/// claims a state the camera has not reached. The two travel together, because taking one without
/// the other is what made the button read as the future state.
fn projection_button(projection: Projection) -> (icons::Icon, &'static str) {
    match projection {
        Projection::Perspective { .. } => (icons::view::PERSPECTIVE, "Perspective view; switch to orthographic  (O)"),
        Projection::Orthographic { .. } => (icons::view::ORTHOGRAPHIC, "Orthographic view; switch to perspective  (O)"),
    }
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
            // Escape drops the whole draft; Enter closes a polyline, which is the only close action
            // a keyboard can reach without the strip.
            if !ui.ctx().text_edit_focused() {
                if escape {
                    self.cancel_drawing();
                } else if enter && let Err(error) = self.commit_drawing() {
                    self.last_error = Some(error);
                }
                if ui.input(|input| input.key_pressed(egui::Key::G)) {
                    self.draw_gizmo.toggle_grid();
                }
            }
        } else if self.editor.pending_primitive().is_some() {
            if escape && !ui.ctx().text_edit_focused() {
                self.editor.cancel_primitive();
            }
        } else if escape
            && !self.translate_gizmo.is_dragging()
            && !self.rotate_gizmo.is_dragging()
            && !self.vertex_gizmo.is_dragging()
            && !ui.ctx().text_edit_focused()
        {
            if self.editor.mesh_edit().is_some() {
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
                let Some(key) = furnicraft_ui::toolbars::shortcut(tool) else {
                    continue;
                };
                if ui.input(|input| input.key_pressed(key)) {
                    if !self.editor.start_drawing(tool) {
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
        let dirty = self.session.is_dirty();
        if let Some(action) = self.files.show(ui, dirty) {
            self.handle_file_action(action);
        }
        self.scene_tree(ui);
        self.panel_card(ui);
        self.workspace(ui);
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
}
