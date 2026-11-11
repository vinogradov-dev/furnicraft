//! FurniCraft desktop application shell.
//!
//! This is the composition root: it creates the window, holds the open tabs, the workspace settings
//! and the window's own view state, and it draws the chrome around the active scene. Everything
//! that belongs to one scene — its session, its editor, its cameras, its renderer target — lives in
//! [`Document`], so activating a tab swaps the workspace without touching a file (U45).
//!
//! The shell dispatches no command itself and never writes to a document it does not own: the
//! header, the menus, the ribbon, the cluster and the keyboard all return what the user asked for,
//! and this file performs it through the active document.

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
mod document;
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
mod tabs;
#[cfg(test)]
mod test_support;
mod translate_gizmo;
mod vertex_gizmo;
mod view_cluster;
mod view_layout;

use std::path::Path;

use eframe::egui;
use eframe::egui_wgpu;
use furnicraft_ui::settings::{Settings, SettingsSection};
use furnicraft_ui::ui_scale;
use furnicraft_ui::{Palette, icons, theme};

use crate::document::Document;
use crate::file_menu::{CloseAnswer, ConfirmClose, FileAction, PreservedSections};
use crate::header::{HeaderAction, HeaderState, TabView};
use crate::scene_build::{empty_session, sample_session};
use crate::settings_store::SettingsStore;
use crate::shortcuts::take_shortcuts;
use crate::side_panels::{PanelHead, PanelSide, side_panel};
use crate::strip::strip_rect;
use crate::tabs::{CloseOutcome, TabId, Tabs};
use crate::view_cluster::ViewCluster;

/// Which transform gizmo the viewport shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformMode {
    /// Three world-axis arrows move the selection.
    Move,
    /// Three local-axis rings turn the selection about its origin.
    Rotate,
}

/// The window's own view state: what every tab shares and no document owns.
///
/// The folds, the floating cluster, the transform mode, the pointer, the measured ribbon band and
/// the viewport rectangle belong to the window, so switching a tab does not move them, while a tab
/// keeps its own camera and its own split layout (ADR 0029). A document borrows this state for the
/// frame it draws; it never stores any of it.
pub(crate) struct Shell {
    /// Whether the scene tree is unfolded.
    pub(crate) tree_open: bool,
    /// Whether the inspector is unfolded.
    pub(crate) card_open: bool,
    /// The floating viewport cluster: navigation, display style and settings.
    pub(crate) cluster: ViewCluster,
    /// Which transform gizmo the viewport shows.
    pub(crate) mode: TransformMode,
    /// The pointer in viewport points from the previous frame, so a border hover can show its resize
    /// cursor and the wheel can tell whether it is over the active view.
    pub(crate) pointer_position: Option<egui::Pos2>,
    /// The band the floating ribbon covered on the last drawn frame. The side panels are inset by its
    /// height and the framing and the orientation widget stay below it (U51, ADR 0080). `None` until
    /// the first real frame, when the deterministic [`furnicraft_ui::toolbars::height`] stands in.
    pub(crate) ribbon_rect: Option<egui::Rect>,
    /// The central area the viewport drew into last frame, so a keyboard *Frame all* knows the view
    /// it fits into without waiting for the viewport to draw first.
    pub(crate) viewport_area: Option<egui::Rect>,
    /// Whether a view border owns the primary button until it is released.
    pub(crate) divider_dragging: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            tree_open: true,
            card_open: true,
            cluster: ViewCluster::default(),
            mode: TransformMode::Move,
            pointer_position: None,
            ribbon_rect: None,
            viewport_area: None,
            divider_dragging: false,
        }
    }
}

impl Shell {
    /// The band the floating ribbon covers at the top of the viewport `area`.
    ///
    /// The measured rectangle from this frame wins, because the ribbon's body can be taller than its
    /// deterministic height in a narrow window; the open state's height stands in on the first frame.
    pub(crate) fn ribbon_band(&self, area: egui::Rect, ribbon_open: bool) -> egui::Rect {
        let height =
            self.ribbon_rect.map_or_else(|| furnicraft_ui::toolbars::height(ribbon_open), |rect| rect.height());
        strip_rect(area, height.min(area.height()))
    }

    /// The height the side panels start below, so the floating ribbon never hides their header.
    pub(crate) fn ribbon_inset(&self, ribbon_open: bool) -> f32 {
        self.ribbon_rect.map_or_else(|| furnicraft_ui::toolbars::height(ribbon_open), |rect| rect.height())
    }
}

/// A close that is waiting for the user to confirm discarding unsaved changes.
///
/// The question belongs to the window, not to the document it names: it stays up while the scene it
/// would drop is still open, and an answer drops exactly that tab.
struct CloseQuestion {
    /// What the answer does: drop one tab, or leave the application.
    request: CloseRequest,
    title: ConfirmClose,
}

/// What a confirmed question performs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseRequest {
    /// Drop this tab, even if another scene was activated while the question was up.
    Tab(TabId),
    /// Close the window; every open scene goes with it.
    Quit,
}

struct FurniCraftApp {
    /// The open scenes, in the order the tab row draws them. Never empty.
    tabs: Tabs<Document>,
    /// The wgpu handles the window and every tab draw with.
    render_state: egui_wgpu::RenderState,
    /// The window's own view state, shared by every tab.
    shell: Shell,
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
    /// Why the defaults were used instead of the saved workspace, shown in the status bar.
    settings_notice: Option<String>,
    /// The tab or the window whose unsaved changes are waiting for the close question.
    pending_close: Option<CloseQuestion>,
    /// Whether the frame should close the window: the context that sends the command is only at
    /// hand there, and quitting is a window operation, not a document one.
    quit_requested: bool,
}

impl FurniCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let settings_store = SettingsStore::for_workspace();
        let loaded = settings_store.load();
        let settings = loaded.settings;
        theme::install_with(&cc.egui_ctx, settings.appearance.theme.preference(), settings.appearance.accent);
        cc.egui_ctx.set_zoom_factor(ui_scale::factor(settings.appearance.ui_scale));
        let render_state =
            cc.wgpu_render_state.clone().ok_or_else(|| "FurniCraft requires the wgpu renderer".to_owned())?;
        let shell = Shell {
            cluster: ViewCluster { all_edges: settings.viewport.all_edges, ..ViewCluster::default() },
            ..Shell::default()
        };
        let session = sample_session()?;
        let document = Document::new(&render_state, &settings, &shell.cluster, session, PreservedSections::new())?;
        Ok(Self {
            tabs: Tabs::new(document),
            render_state,
            shell,
            settings,
            settings_store,
            settings_section: SettingsSection::default(),
            settings_open: false,
            home_open: false,
            palette: Palette::new(),
            settings_notice: loaded.notice,
            pending_close: None,
            quit_requested: false,
        })
    }

    /// Reports a failure in the active tab's status bar.
    ///
    /// An error is the tab's: a scene that could not be evaluated says so on its own status bar and
    /// leaves the neighbours alone.
    fn failed(&mut self, error: String) {
        self.tabs.active_mut().last_error = Some(error);
    }

    /// Performs the header action the row returned.
    ///
    /// Undo, redo and save are the same operations the menu and the keyboard already take, and every
    /// one of them acts on the active tab only, so the header adds a place to click and never a
    /// second code path into a document.
    fn handle_header_action(&mut self, action: HeaderAction) {
        match action {
            HeaderAction::Home => {
                self.home_open = true;
                self.settings_notice = None;
            }
            HeaderAction::Undo => self.tabs.active_mut().undo(),
            HeaderAction::Redo => self.tabs.active_mut().redo(),
            HeaderAction::Save => {
                let requested = self.tabs.active_mut().request_save();
                if let Some(action) = requested {
                    self.handle_file_action(action);
                }
            }
            HeaderAction::Settings => {
                self.settings_open = true;
                self.settings_notice = None;
            }
            HeaderAction::NewTab => {
                if let Err(error) = self.new_tab() {
                    self.failed(error);
                }
            }
            HeaderAction::ActivateTab(id) => {
                self.tabs.activate(id);
            }
            HeaderAction::CloseTab(id) => self.close_tab(id),
        }
    }

    /// Performs the operation the File menu returned and reports a failure in the status bar.
    fn handle_file_action(&mut self, action: FileAction) {
        self.tabs.active_mut().last_error = None;
        let recorded = match &action {
            FileAction::Open(path) | FileAction::Save(path) => Some(path.clone()),
            FileAction::New | FileAction::Close | FileAction::Quit => None,
        };
        let result = match action {
            FileAction::New => self.new_tab(),
            FileAction::Open(path) => self.open_tab(&path),
            FileAction::Save(path) => self.tabs.active_mut().save(&path),
            FileAction::Close => {
                let active = self.tabs.active_id();
                self.close_tab(active);
                Ok(())
            }
            FileAction::Quit => {
                self.request_quit();
                Ok(())
            }
        };
        match result {
            Ok(()) => {
                if let Some(path) = recorded {
                    self.settings.remember(&path);
                    self.persist_settings();
                }
            }
            Err(error) => self.failed(error),
        }
    }

    /// Opens a scene in a new tab, or activates the tab that already holds it.
    ///
    /// The owner's rule: opening a path that is already open activates that tab instead of making a
    /// second copy of one file, so two tabs can never race to save the same scene. The catalogue the
    /// new session gets is the workspace's, taken from the scene in front of the user.
    fn open_tab(&mut self, path: &Path) -> Result<(), String> {
        if let Some(id) = self.tabs.find(|document| document.path() == Some(path)) {
            self.tabs.activate(id);
            return Ok(());
        }
        let catalog = self.tabs.active().session.catalog().clone();
        let document = Document::open(&self.render_state, &self.settings, &self.shell.cluster, path, &catalog)?;
        self.tabs.add(document);
        Ok(())
    }

    /// Opens an empty *Untitled* scene in a new tab.
    fn new_tab(&mut self) -> Result<(), String> {
        let session = empty_session()?;
        let settings = &self.settings;
        let cluster = &self.shell.cluster;
        let document = Document::new(&self.render_state, settings, cluster, session, PreservedSections::new())?;
        self.tabs.add(document);
        Ok(())
    }

    /// Closes one tab, asking first when its scene has unsaved changes.
    ///
    /// The last tab is never removed: its payload becomes an empty scene, so the window always has
    /// one document to draw and the folds, the cluster and the settings keep their home.
    fn close_tab(&mut self, id: TabId) {
        let unsaved = self.tabs.document(id).is_some_and(Document::is_dirty);
        match self.tabs.close(id, unsaved) {
            CloseOutcome::NeedsConfirmation(id) => {
                let title = self.tabs.document(id).map(Document::file_name).unwrap_or_default();
                self.pending_close =
                    Some(CloseQuestion { request: CloseRequest::Tab(id), title: ConfirmClose::tab(title) });
            }
            CloseOutcome::Closed(document) => self.forget(document),
            CloseOutcome::LastTab(id) => self.empty_tab(id),
            CloseOutcome::Missing(_) => {}
        }
    }

    /// Asks to quit when any open scene has unsaved changes, and quits at once when none has.
    ///
    /// One question covers the window: a quit is one gesture, and asking per tab would leave the
    /// user to answer the same question several times for one decision.
    fn request_quit(&mut self) {
        let unsaved = self.tabs.iter().filter(|(_, document)| document.is_dirty()).count();
        match unsaved {
            0 => self.quit_requested = true,
            count => {
                self.pending_close =
                    Some(CloseQuestion { request: CloseRequest::Quit, title: ConfirmClose::quit(count) });
            }
        }
    }

    /// Draws the unsaved-changes question and acts on the answer.
    ///
    /// A discard drops exactly the tab the question named, even if the user activated another scene
    /// while it was up: the answer is about that document, not about what is in front of them.
    fn close_question(&mut self, ctx: &egui::Context) {
        let Some(question) = self.pending_close.as_ref() else {
            return;
        };
        let Some(answer) = question.title.show(ctx) else {
            return;
        };
        let Some(question) = self.pending_close.take() else {
            return;
        };
        if answer == CloseAnswer::Cancel {
            return;
        }
        match question.request {
            CloseRequest::Quit => self.quit_requested = true,
            CloseRequest::Tab(tab) => match self.tabs.discard(tab) {
                CloseOutcome::Closed(document) => self.forget(document),
                CloseOutcome::LastTab(id) => self.empty_tab(id),
                CloseOutcome::NeedsConfirmation(_) | CloseOutcome::Missing(_) => {}
            },
        }
    }

    /// Frees the render target of a closed document.
    ///
    /// The texture is registered on the window's egui renderer and outlives the document that drew
    /// into it, so a workspace that opens and closes scenes has to hand it back; the document then
    /// drops with its own render pass, buffers and cache.
    fn forget(&mut self, document: Document) {
        let texture = document.texture_id();
        self.render_state.renderer.write().free_texture(&texture);
    }

    /// Replaces the last tab with an empty *Untitled* scene.
    fn empty_tab(&mut self, id: TabId) {
        let built = empty_session().and_then(|session| {
            Document::new(&self.render_state, &self.settings, &self.shell.cluster, session, PreservedSections::new())
        });
        match built {
            Ok(document) => {
                if let Some(closed) = self.tabs.replace(id, document) {
                    self.forget(closed);
                }
            }
            Err(error) => self.failed(error),
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
            Err(error) => self.failed(error),
        }
    }

    /// The ribbon over the design space: the tabs and the tool groups of the active tab.
    ///
    /// The strip floats on the middle layer instead of taking height from the viewport, so the scene
    /// image runs under it and a translucent ribbon blends with the furniture (U51, ADR 0080). The
    /// document draws it because the fold is the document's; the band it occupies is the window's.
    fn ribbon(&mut self, ui: &mut egui::Ui, design: egui::Rect) {
        self.tabs.active_mut().show_ribbon(ui, design, &mut self.shell, &self.settings);
    }

    /// The scene tree, as a folding window.
    ///
    /// The panel owns no heading and no tools of its own: the header row names it, counts its nodes
    /// and folds it, and the body is only the tree. Grouping survives in the row context menu.
    fn scene_tree(&mut self, ui: &mut egui::Ui, inset: f32) {
        let Self { shell, tabs, .. } = self;
        let head = PanelHead {
            title: "Scene",
            count: tabs.active().node_count(),
            icon: icons::scene::ASSEMBLY,
            default_width: 240.0,
            min_width: 180.0,
        };
        side_panel(ui, PanelSide::Left, "scene_tree", &mut shell.tree_open, head, inset, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                tabs.active_mut().show_scene_tree(ui);
            });
        });
    }

    /// The inspector, as a folding window over the same header row.
    fn panel_card(&mut self, ui: &mut egui::Ui, inset: f32) {
        let Self { shell, tabs, .. } = self;
        let head = PanelHead {
            title: "Inspector",
            count: tabs.active().editor.selected_nodes().len(),
            icon: icons::actions::SETTINGS,
            default_width: 300.0,
            min_width: 260.0,
        };
        side_panel(ui, PanelSide::Right, "panel_card", &mut shell.card_open, head, inset, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                tabs.active_mut().show_card(ui);
            });
        });
    }

    fn workspace(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let Self { tabs, shell, settings, .. } = self;
            tabs.active_mut().viewport(ui, shell, &settings.viewport);
        });
    }

    /// Draws the command palette and runs the one command it returned.
    ///
    /// The palette lists engine specs and builds their parameters from the current selection; it
    /// never mutates the document itself. A successful dispatch selects a node the command created
    /// and lets the next frame rebuild the scene from the new revision.
    fn command_palette(&mut self, ui: &mut egui::Ui) {
        let Self { palette, tabs, .. } = self;
        let context = tabs.active().palette_context();
        let Some(id) = palette.show(ui.ctx(), &tabs.active().session, &context) else {
            return;
        };
        match palette.activate(&mut tabs.active_mut().session, &context, &id) {
            Ok(outcome) => {
                if let Some(created) = outcome.created_id() {
                    tabs.active_mut().editor.select_node(Some(created));
                }
            }
            Err(error) => tabs.active_mut().last_error = Some(error),
        }
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
    /// Appearance is the window's, so the theme and the zoom apply at once; the viewport section is
    /// pushed into the active scene, because a grid, an edge list or a style default is derived
    /// state that scene has to rebuild. The other tabs pick the new defaults up when they become
    /// active again.
    fn apply_settings(&mut self, before: &Settings, ctx: &egui::Context) {
        self.settings.sanitize();
        if self.settings.appearance != before.appearance {
            theme::install_with(ctx, self.settings.appearance.theme.preference(), self.settings.appearance.accent);
            ctx.set_zoom_factor(ui_scale::factor(self.settings.appearance.ui_scale));
        }
        if self.settings.viewport != before.viewport {
            let Self { tabs, shell, settings, .. } = self;
            tabs.active_mut().apply_viewport_settings(&before.viewport, &settings.viewport, &mut shell.cluster);
        }
    }

    /// Draws the Home surface and performs the one action it asked for.
    ///
    /// Home goes through the same File menu path as the header: *New* and an open file each land in
    /// a tab, so a scene with unsaved changes is never discarded behind the user's back.
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
                self.handle_file_action(FileAction::New);
            }
            furnicraft_ui::home::HomeAction::Open => self.tabs.active_mut().request_open(),
            furnicraft_ui::home::HomeAction::OpenRecent(path) => {
                self.handle_file_action(FileAction::Open(path));
            }
            furnicraft_ui::home::HomeAction::Close => {}
        }
    }
}

impl eframe::App for FurniCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        {
            let Self { tabs, shell, settings, .. } = self;
            if let Err(error) = tabs.active_mut().sync_scene(&settings.viewport, &shell.cluster) {
                tabs.active_mut().last_error = Some(error);
            }
        }
        // A modal — the Settings window, the Home surface, the palette — owns the keys for the frame
        // it is up, so typing a path or a name never saves, undoes, reframes or leaves a tool.
        let modals = self.settings_open || self.home_open || self.palette.is_open();
        {
            let Self { tabs, shell, .. } = self;
            tabs.active_mut().handle_keys(ui, modals, shell);
        }
        if let Err(error) = self.tabs.active_mut().sync_selection() {
            self.failed(error);
        }
        if !ui.ctx().text_edit_focused() && !modals {
            let shortcuts = take_shortcuts(ui.ctx());
            if shortcuts.palette {
                self.palette.open();
            }
            let requested = {
                let Self { tabs, shell, .. } = self;
                tabs.active_mut().handle_shortcuts(shortcuts, shell)
            };
            if let Some(action) = requested {
                self.handle_file_action(action);
            }
            if shortcuts.new_tab {
                self.handle_file_action(FileAction::New);
            }
            if shortcuts.close_tab {
                let active = self.tabs.active_id();
                self.close_tab(active);
            }
        }

        let header_state = {
            let active = self.tabs.active_id();
            HeaderState {
                tabs: self
                    .tabs
                    .iter()
                    .map(|(id, document)| TabView {
                        id,
                        title: document.file_name(),
                        dirty: document.is_dirty(),
                        active: id == active,
                    })
                    .collect(),
                can_undo: self.tabs.active().can_undo(),
                can_redo: self.tabs.active().can_redo(),
            }
        };
        let mut header_action = None;
        let mut file_action = None;
        {
            let Self { tabs, .. } = self;
            egui::Panel::top("header").show(ui, |ui| {
                header_action = header::show(ui, &header_state, |ui| {
                    file_action = tabs.active_mut().show_file_menu(ui);
                });
            });
        }
        if let Some(action) = file_action {
            self.handle_file_action(action);
        }
        if let Some(action) = self.tabs.active_mut().show_dialogs(ui.ctx()) {
            self.handle_file_action(action);
        }
        if let Some(action) = header_action {
            self.handle_header_action(action);
        }
        self.close_question(ui.ctx());

        let design = ui.available_rect_before_wrap();
        self.ribbon(ui, design);
        let inset = self.shell.ribbon_inset(self.tabs.active().ribbon_open());
        self.scene_tree(ui, inset);
        self.panel_card(ui, inset);
        self.status_bar(ui);
        self.workspace(ui);
        self.command_palette(ui);
        self.settings_window(ui.ctx());
        self.home_surface(ui.ctx());
        // The close command needs the context, which only the frame has: everything else a quit
        // touches — the questions, the settings write — has already happened by now.
        if self.quit_requested {
            self.quit_requested = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
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

    use furnicraft_scene::Projection;

    use super::*;
    use crate::view_cluster::{cluster_anchor, projection_button};

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
    fn the_ribbon_band_follows_the_measured_strip() {
        // The band the tools must stay clear of is the window's, not the scene's: it is the same
        // rectangle whichever tab is active, and the measured strip wins over the nominal height.
        let mut shell = Shell::default();
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 600.0));
        let unfolded = shell.ribbon_band(area, true).height();
        assert!(unfolded > 0.0, "an unfolded ribbon covers part of the viewport");
        assert!((shell.ribbon_inset(true) - unfolded).abs() < f32::EPSILON, "the panels start below the same band");
        assert_eq!(shell.ribbon_band(area, false).height(), furnicraft_ui::toolbars::height(false));

        shell.ribbon_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 91.0)));
        assert!((shell.ribbon_inset(true) - 91.0).abs() < f32::EPSILON, "the strip that was drawn wins");
        assert!((shell.ribbon_band(area, true).height() - 91.0).abs() < f32::EPSILON);
        assert!(
            (shell.ribbon_band(area, false).height() - 91.0).abs() < f32::EPSILON,
            "a measured strip is the band whatever its nominal height was"
        );
    }
}
