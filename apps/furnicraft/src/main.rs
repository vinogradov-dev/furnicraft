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

mod gizmo;
mod scene_build;
mod translate_gizmo;

use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use furnicraft_engine::node_cmds::NODE_PLACEMENT_SET_ID;
use furnicraft_engine::{NodePlacementSet, Session};
use furnicraft_geom::DVec3;
use furnicraft_kernel_truck::TruckKernel;
use furnicraft_model::{Id, Revision};
use furnicraft_render::ViewportRenderer;
use furnicraft_scene::{Camera, SceneFrame};
use furnicraft_ui::Editor;

use crate::scene_build::{evaluate, home_camera, sample_session};
use crate::translate_gizmo::{Commit, TranslateGizmo};

/// Orbit sensitivity in radians per point dragged.
const ORBIT_RADIANS_PER_POINT: f64 = 0.008;

/// Zoom sensitivity per scroll point.
const ZOOM_PER_SCROLL_POINT: f64 = 0.004;

struct FurniCraftApp {
    session: Session,
    editor: Editor,
    kernel: TruckKernel,
    scene: SceneFrame,
    home_camera: Camera,
    scene_revision: Revision,
    renderer: ViewportRenderer,
    render_state: egui_wgpu::RenderState,
    texture_id: egui::TextureId,
    translate_gizmo: TranslateGizmo,
    highlighted: Option<Id>,
    uploaded_preview: Option<(Id, DVec3)>,
    last_error: Option<String>,
}

impl FurniCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let session = sample_session()?;
        let mut kernel = TruckKernel::new();
        let evaluated = evaluate(&mut kernel, &session)?;
        let home_camera = home_camera(evaluated.bounds)?;
        let mut scene = evaluated.frame;
        scene.camera = home_camera;

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
            scene,
            home_camera,
            renderer,
            render_state,
            texture_id,
            translate_gizmo: TranslateGizmo::default(),
            highlighted: None,
            uploaded_preview: None,
            last_error: None,
        })
    }

    fn sync_scene(&mut self) -> Result<(), String> {
        let revision = self.session.project().revision();
        if revision == self.scene_revision {
            return Ok(());
        }
        let evaluated = evaluate(&mut self.kernel, &self.session)?;
        let mut scene = evaluated.frame;
        scene.camera = self.scene.camera;
        self.scene = scene;
        self.scene_revision = revision;
        self.upload_scene()
    }

    fn upload_scene(&mut self) -> Result<(), String> {
        let selected = self.editor.selected_node();
        let panels = selected.and_then(|id| self.session.project().panel_ids_under(id)).unwrap_or_default();
        let frame = self.scene.with_selection_highlight(&panels);
        let frame = match self.translate_gizmo.preview() {
            Some((panels, delta)) if delta != DVec3::ZERO => frame.with_world_translation(panels, delta),
            _ => frame,
        };
        self.renderer.upload(&self.render_state.device, &frame).map_err(|error| error.to_string())?;
        self.highlighted = selected;
        self.uploaded_preview = self.preview_key();
        Ok(())
    }

    /// The preview currently shown by the renderer, ignoring a zero delta.
    fn preview_key(&self) -> Option<(Id, DVec3)> {
        self.translate_gizmo.preview_key().filter(|(_, delta)| *delta != DVec3::ZERO)
    }

    /// Re-uploads the scene when the drag preview moved to a new position.
    ///
    /// A finished or cancelled drag also lands here with no preview, which restores the base
    /// upload. The key comparison keeps a still pointer from rebuilding the geometry every frame.
    fn sync_preview(&mut self) -> Result<(), String> {
        if self.preview_key() == self.uploaded_preview {
            return Ok(());
        }
        self.upload_scene()
    }

    /// Dispatches one `node.placement.set` for a finished drag and rebuilds the scene.
    fn apply_gizmo_commit(&mut self, commit: Commit) -> Result<(), String> {
        let params = serde_json::to_value(NodePlacementSet { node: commit.node, placement: commit.placement })
            .map_err(|error| format!("could not encode the placement: {error}"))?;
        self.session.dispatch(NODE_PLACEMENT_SET_ID, &params).map_err(|error| error.to_string())?;
        // The preview already showed the new position; rebuilding here keeps the document, the
        // scene and the inspector in agreement without waiting a frame.
        self.sync_scene()
    }

    fn reset_view(&mut self) {
        self.scene.camera = self.home_camera;
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
        let selected = self.editor.selected_node();
        let target = translate_gizmo::Target { camera: self.scene.camera, project: self.session.project(), selected };
        match self.translate_gizmo.update(ui, &response, rect, target, pointer_over_widget) {
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
        let dragging = self.translate_gizmo.is_dragging();

        if response.hovered() && !dragging {
            let scroll = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.scene.camera.zoom((-f64::from(scroll) * ZOOM_PER_SCROLL_POINT).exp());
            }
        }

        let drag = response.drag_delta();
        // An axis drag owns the pointer; rotating the camera mid-drag would move the axis under the
        // pointer and change the drag parameter under the user's hand.
        if !dragging && response.dragged_by(egui::PointerButton::Middle) {
            let modifiers = ui.input(|input| input.modifiers);
            if modifiers.shift {
                self.scene.camera.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(rect.height()));
            } else if modifiers.ctrl {
                let factor = (-f64::from(drag.y) * ZOOM_PER_SCROLL_POINT).exp();
                self.scene.camera.zoom(factor);
            } else {
                self.scene
                    .camera
                    .orbit(-f64::from(drag.x) * ORBIT_RADIANS_PER_POINT, -f64::from(drag.y) * ORBIT_RADIANS_PER_POINT);
            }
        } else if !dragging && response.dragged_by(egui::PointerButton::Secondary) {
            self.scene.camera.pan_pixels(f64::from(drag.x), f64::from(drag.y), f64::from(rect.height()));
        }

        if dragging || response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }

        let pointer_over_gizmo = response
            .interact_pointer_pos()
            .is_some_and(|pointer| gizmo_rect.is_some_and(|widget| widget.contains(pointer)));
        if (response.clicked() || response.double_clicked())
            && !pointer_over_gizmo
            && !self.translate_gizmo.consumed_click()
            && let Some(pointer) = response.interact_pointer_pos()
            && rect.width() > 0.0
            && rect.height() > 0.0
        {
            let ndc_x = f64::from((pointer.x - rect.min.x) / rect.width()) * 2.0 - 1.0;
            let ndc_y = 1.0 - f64::from((pointer.y - rect.min.y) / rect.height()) * 2.0;
            let ray = self.scene.camera.ray_ndc(ndc_x, ndc_y, f64::from(rect.width() / rect.height()));
            let hit = self.scene.pick(ray);
            if response.double_clicked() {
                self.editor.enter_viewport_level(self.session.project(), hit);
            } else {
                self.editor.select_viewport_hit(self.session.project(), hit);
            }
            if let Err(error) = self.upload_scene() {
                self.last_error = Some(error);
            }
        }

        self.renderer.render(&self.render_state.device, &self.render_state.queue, &self.scene);
        ui.painter().with_clip_rect(rect).image(
            self.texture_id,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        self.translate_gizmo.paint(
            ui,
            rect,
            translate_gizmo::Target { camera: self.scene.camera, project: self.session.project(), selected },
        );
        if let Some(direction) = gizmo::show(ui, rect, self.scene.camera) {
            self.scene.camera.look_from(direction);
        }
    }

    fn scene_tree(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("scene_tree").resizable(true).default_size(240.0).min_size(180.0).show(ui, |ui| {
            ui.heading("Scene");
            ui.separator();
            egui::ScrollArea::vertical().show(ui, |ui| {
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
            ui.horizontal_wrapped(|ui| {
                ui.heading("FurniCraft");
                ui.separator();
                reset = ui.button("Frame all  (F)").clicked();
                reset |= ui.input(|input| input.key_pressed(egui::Key::F));
                ui.separator();
                ui.label("LMB select  ·  double-click enter  ·  Esc leave  ·  MMB orbit  ·  Shift+MMB pan  ·  Ctrl+MMB/wheel zoom");
                if let Some(error) = &self.last_error {
                    ui.separator();
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
            });
            if reset {
                self.reset_view();
            }
            ui.separator();
            self.viewport(ui);
        });
    }
}

impl eframe::App for FurniCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Err(error) = self.sync_scene() {
            self.last_error = Some(error);
        }
        let escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
        if escape && !self.translate_gizmo.is_dragging() && !ui.ctx().text_edit_focused() {
            let before = self.editor.selected_node();
            self.editor.exit_level(self.session.project());
            if self.editor.selected_node() != before
                && let Err(error) = self.upload_scene()
            {
                self.last_error = Some(error);
            }
        }
        if self.editor.selected_node() != self.highlighted
            && let Err(error) = self.upload_scene()
        {
            self.last_error = Some(error);
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
