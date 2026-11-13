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
/// The shell's frame, asserted without a window (U78).
#[cfg(test)]
mod frame_tests;
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
/// The interface rendered offscreen, one PNG per cell of the look matrix (U81).
#[cfg(test)]
mod ui_shots;
mod vertex_gizmo;
mod view_cluster;
mod view_layout;

use std::path::Path;

use eframe::egui;
use eframe::egui_wgpu;
use furnicraft_scene::SceneColors;
use furnicraft_ui::product_card::ProductCard;
use furnicraft_ui::settings::{Settings, SettingsSection};
use furnicraft_ui::ui_scale;
use furnicraft_ui::{Palette, icons, theme};

use crate::document::Document;
use crate::file_menu::{CloseAnswer, ConfirmClose, FileAction, PreservedSections};
use crate::header::{HeaderAction, HeaderState, TabView};
use crate::scene_build::{empty_session, sample_session};
use crate::settings_store::SettingsStore;
use crate::shortcuts::take_shortcuts;
use crate::side_panels::{
    ANIMATION_SECONDS, Chrome, EXPANDED_WIDTH, PanelEvent, PanelHead, PanelRects, PanelSide, PanelSlot, Panels,
    STRIP_WIDTH, panel_top, scroll_body, show as show_panel,
};
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
    /// The two side panels: their visibility, their fold and the memories of the last *hide all*
    /// and *collapse all* press.
    pub(crate) panels: Panels,
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
    /// Whether a view border owns the primary button until it is released.
    pub(crate) divider_dragging: bool,
    /// Whether the frame being drawn rebuilt its scene, so the status bar can say «Перестроение…».
    pub(crate) rebuilding: bool,
    /// The cluster asked for Settings › View this frame; the shell opens it and clears this.
    pub(crate) open_settings: bool,
    /// The background, grid and line ink of the theme variant the window draws with.
    ///
    /// The viewport is a `wgpu` surface, so it cannot read the `egui` palette at draw time; the
    /// shell resolves the variant once per frame and every tab paints its frame with it (X8).
    pub(crate) colors: SceneColors,
}

impl Default for Shell {
    fn default() -> Self {
        Self {
            panels: Panels::default(),
            cluster: ViewCluster::default(),
            mode: TransformMode::Move,
            pointer_position: None,
            ribbon_rect: None,
            divider_dragging: false,
            rebuilding: false,
            open_settings: false,
            colors: SceneColors::DARK,
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
    /// The wgpu handles the window and every tab draw with; `None` in a headless run, where a test
    /// drives the same frame with no device, no adapter and no window (U77).
    render_state: Option<egui_wgpu::RenderState>,
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
    /// The product-frame card: window chrome, like the File dialogs, so it outlives a tab switch.
    product_card: ProductCard,
    /// Why the defaults were used instead of the saved workspace, shown in the status bar.
    settings_notice: Option<crate::settings_store::Notice>,
    /// The tab or the window whose unsaved changes are waiting for the close question.
    pending_close: Option<CloseQuestion>,
    /// Whether the frame should close the window: the context that sends the command is only at
    /// hand there, and quitting is a window operation, not a document one.
    quit_requested: bool,
}

impl FurniCraftApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, String> {
        let render_state =
            cc.wgpu_render_state.clone().ok_or_else(|| "FurniCraft requires the wgpu renderer".to_owned())?;
        let settings_store = SettingsStore::for_workspace();
        let loaded = settings_store.load();
        let (preference, preset, accent) = loaded.settings.install_args();
        theme::install_with(&cc.egui_ctx, preference, preset, accent);
        cc.egui_ctx.set_zoom_factor(ui_scale::factor(loaded.settings.appearance.ui_scale));
        Self::build(&cc.egui_ctx, Some(render_state), settings_store, loaded, sample_session()?)
    }

    /// Builds the application without a window and without a GPU.
    ///
    /// A test's [`egui::Context`] stands in for the window and `settings_path` for the workspace
    /// file, so the real frame runs against the real interface while nothing is drawn into and no
    /// file in the user's config directory is read or written (U77).
    #[cfg(test)]
    fn headless(
        ctx: &egui::Context,
        settings_path: std::path::PathBuf,
        settings: Settings,
        session: furnicraft_engine::Session,
    ) -> Result<Self, String> {
        Self::test_app(ctx, None, settings_path, settings, session)
    }

    /// Builds the application for a test: no window, and a GPU only when one was handed over.
    ///
    /// A headless frame test passes `None` and draws no scene image; a shot passes a device and
    /// queue with no surface, so the real viewport renders into its target and egui paints the same
    /// interface a window would show, offscreen (U77, U81).
    #[cfg(test)]
    fn test_app(
        ctx: &egui::Context,
        render_state: Option<egui_wgpu::RenderState>,
        settings_path: std::path::PathBuf,
        settings: Settings,
        session: furnicraft_engine::Session,
    ) -> Result<Self, String> {
        let (preference, preset, accent) = settings.install_args();
        theme::install_with(ctx, preference, preset, accent);
        ctx.set_zoom_factor(ui_scale::factor(settings.appearance.ui_scale));
        let loaded = crate::settings_store::Loaded { settings, notice: None };
        Self::build(ctx, render_state, SettingsStore::at(settings_path), loaded, session)
    }

    /// Composes the shell around one document, one workspace and, when the window has one, its GPU.
    ///
    /// This is the one place the window and a headless test part ways: everything below it — the
    /// frame, the chrome, the keys, the documents — runs the same code (U77).
    fn build(
        ctx: &egui::Context,
        render_state: Option<egui_wgpu::RenderState>,
        settings_store: SettingsStore,
        loaded: crate::settings_store::Loaded,
        session: furnicraft_engine::Session,
    ) -> Result<Self, String> {
        let settings = loaded.settings;
        let shell = Shell {
            panels: Panels::from_settings(&settings.panels),
            colors: theme::scene_colors(ctx.theme()),
            ..Shell::default()
        };
        let document = Document::new(render_state.as_ref(), &settings, &shell, session, PreservedSections::new())?;
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
            product_card: ProductCard::new(),
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
    fn handle_header_action(&mut self, action: HeaderAction, lang: furnicraft_text::Lang) {
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
                    self.handle_file_action(action, lang);
                }
            }
            HeaderAction::Settings => {
                self.settings_open = true;
                self.settings_notice = None;
            }
            HeaderAction::CollapsePanels => self.shell.panels.toggle_collapse_all(),
            HeaderAction::HidePanels => self.shell.panels.toggle_hide_all(),
            HeaderAction::ToggleRibbon => {
                let open = !self.tabs.active().ribbon_open();
                self.tabs.active_mut().set_ribbon_open(open);
                self.sync_ribbon_fold();
            }
            HeaderAction::NewTab => {
                if let Err(error) = self.new_tab() {
                    self.failed(error);
                }
            }
            HeaderAction::ActivateTab(id) => {
                self.tabs.activate(id);
            }
            HeaderAction::CloseTab(id) => self.close_tab(id, lang),
        }
    }

    /// Performs the operation the File menu returned and reports a failure in the status bar.
    fn handle_file_action(&mut self, action: FileAction, lang: furnicraft_text::Lang) {
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
                self.close_tab(active, lang);
                Ok(())
            }
            FileAction::Quit => {
                self.request_quit(lang);
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
        let document = Document::open(self.render_state.as_ref(), &self.settings, &self.shell, path, &catalog)?;
        self.tabs.add(document);
        Ok(())
    }

    /// Opens an empty *Untitled* scene in a new tab.
    fn new_tab(&mut self) -> Result<(), String> {
        let session = empty_session()?;
        let settings = &self.settings;
        let shell = &self.shell;
        let document = Document::new(self.render_state.as_ref(), settings, shell, session, PreservedSections::new())?;
        self.tabs.add(document);
        Ok(())
    }

    /// Closes one tab, asking first when its scene has unsaved changes.
    ///
    /// The last tab is never removed: its payload becomes an empty scene, so the window always has
    /// one document to draw and the folds, the cluster and the settings keep their home.
    fn close_tab(&mut self, id: TabId, lang: furnicraft_text::Lang) {
        let unsaved = self.tabs.document(id).is_some_and(Document::is_dirty);
        match self.tabs.close(id, unsaved) {
            CloseOutcome::NeedsConfirmation(id) => {
                let title = self.tabs.document(id).map(|document| document.file_name(lang)).unwrap_or_default();
                self.pending_close =
                    Some(CloseQuestion { request: CloseRequest::Tab(id), title: ConfirmClose::tab(title, lang) });
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
    fn request_quit(&mut self, lang: furnicraft_text::Lang) {
        let unsaved = self.tabs.iter().filter(|(_, document)| document.is_dirty()).count();
        match unsaved {
            0 => self.quit_requested = true,
            count => {
                self.pending_close =
                    Some(CloseQuestion { request: CloseRequest::Quit, title: ConfirmClose::quit(count, lang) });
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
        document.release();
    }

    /// Replaces the last tab with an empty *Untitled* scene.
    fn empty_tab(&mut self, id: TabId) {
        let built = empty_session().and_then(|session| {
            Document::new(self.render_state.as_ref(), &self.settings, &self.shell, session, PreservedSections::new())
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
        if self.tabs.active_mut().show_ribbon(ui, design, &mut self.shell, &self.settings) {
            self.product_card.start();
        }
        self.sync_ribbon_fold();
        self.sync_pinned_variants();
    }

    /// Draws the product-frame card and creates the frame it describes.
    ///
    /// The card is the numeric path to `product.create`: the editor dispatches the command and
    /// opens the new frame as a session view operation, so the next part lands inside it (ADR 0100).
    /// A refused command leaves the card closed and the sentence on the status bar.
    fn product_frame_card(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.product_card.show(ctx) else {
            return;
        };
        let lang = furnicraft_ui::language(ctx);
        if let Err(error) = self.tabs.active_mut().create_product(draft, lang) {
            self.failed(error);
        }
    }

    /// Remembers the ribbon's fold as a workspace setting and hands it to every open scene.
    ///
    /// The fold is chrome for the whole window rather than a property of one tab, so a tab that folds
    /// the ribbon folds it everywhere, and the next launch opens the way this one was left. The write
    /// happens on the one frame the fold changes, not per frame (U68).
    fn sync_ribbon_fold(&mut self) {
        let open = self.tabs.active().ribbon_open();
        if open == self.settings.ribbon.open {
            return;
        }
        self.settings.ribbon.open = open;
        // The measured band belongs to the fold it was drawn with. A remembered measurement would
        // keep the band of the state before — the strip's own height is a floor, so a folded ribbon
        // would hold the open band forever — and the fold would give the scene nothing (U78).
        self.shell.ribbon_rect = None;
        for (_, document) in self.tabs.iter_mut() {
            document.set_ribbon_open(open);
        }
        self.persist_settings();
    }

    /// Remembers the method the user pinned as a workspace setting and hands it to every open scene.
    ///
    /// The pin is global chrome, like the fold: one click in one tab changes the method every tab
    /// runs, and the next launch starts the same way. The write happens on the one frame the map
    /// changes, not per frame (U67).
    fn sync_pinned_variants(&mut self) {
        let active = self.tabs.active().pinned_variants();
        if *active == self.settings.ribbon.pinned_variants {
            return;
        }
        let pinned = active.clone();
        self.settings.ribbon.pinned_variants = pinned.clone();
        for (_, document) in self.tabs.iter_mut() {
            document.set_pinned_variants(pinned.clone());
        }
        self.persist_settings();
    }

    /// Remembers the panels' visibility and fold as workspace settings.
    ///
    /// The write happens on the one frame a panel gesture changes them, not per frame. The memory
    /// a collapse-all or hide-all press keeps stays in [`Panels`]; only the state the user sees is
    /// written, so a restart opens exactly the layout the window was left in.
    fn sync_panels(&mut self) {
        let panels = self.shell.panels;
        if self.settings.panels.scene == panels.scene && self.settings.panels.inspector == panels.inspector {
            return;
        }
        self.settings.panels.scene = panels.scene;
        self.settings.panels.inspector = panels.inspector;
        self.persist_settings();
    }

    /// The scene tree, as a floating panel over the left edge of the scene.
    ///
    /// The panel owns no heading and no tools of its own: the header row names it and folds it, and
    /// the body is only the tree. Grouping survives in the row context menu.
    fn scene_tree(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Self { shell, tabs, settings, .. } = self;
        let head = PanelHead { title: furnicraft_text::Message::PanelScene, icon: icons::scene::ASSEMBLY };
        let slot = PanelSlot { side: PanelSide::Left, id: "scene_tree", rect, state: shell.panels.scene, head };
        match show_panel(ctx, slot, settings.appearance.panel_opacity, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                tabs.active_mut().show_scene_tree(ui);
            });
        }) {
            PanelEvent::Fold => shell.panels.scene.expanded = false,
            PanelEvent::Restore => shell.panels.scene.expanded = true,
            PanelEvent::None => {}
        }
    }

    /// The inspector, as a floating panel over the right edge of the scene.
    fn panel_card(&mut self, ctx: &egui::Context, rect: egui::Rect) {
        let Self { shell, tabs, settings, .. } = self;
        let head = PanelHead { title: furnicraft_text::Message::PanelInspector, icon: icons::actions::SETTINGS };
        let slot = PanelSlot { side: PanelSide::Right, id: "panel_card", rect, state: shell.panels.inspector, head };
        // The inspector's rows (the name field and its button, the property rows) are wider than a
        // 226 pt panel, so the body scrolls sideways instead of widening the panel (U73).
        match show_panel(ctx, slot, settings.appearance.panel_opacity, |ui| {
            scroll_body(ui, |ui| tabs.active_mut().show_card(ui));
        }) {
            PanelEvent::Fold => shell.panels.inspector.expanded = false,
            PanelEvent::Restore => shell.panels.inspector.expanded = true,
            PanelEvent::None => {}
        }
    }

    fn workspace(&mut self, ui: &mut egui::Ui, chrome: &Chrome) {
        egui::CentralPanel::default().show(ui, |ui| {
            let Self { tabs, shell, settings, .. } = self;
            tabs.active_mut().viewport(ui, shell, &settings.viewport, chrome, settings.appearance.panel_opacity);
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
        // A locked node refuses a destructive command here, before the dispatch: the lock is a
        // guard on a gesture, so it never becomes a document rule (ADR 0065).
        if let Some(refusal) = tabs.active().locked_command_refusal(&id, furnicraft_ui::language(ui.ctx())) {
            tabs.active_mut().last_error = Some(refusal);
            return;
        }
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
    /// state that scene has to rebuild, and a theme switch repaints that scene's own colours in the
    /// same frame. The other tabs pick the new defaults up when they become active again.
    fn apply_settings(&mut self, before: &Settings, ctx: &egui::Context) {
        self.settings.sanitize();
        let colors_before = self.shell.colors;
        // The panels page edits the same state the header gestures write, so a change there replaces
        // the runtime pair and drops a memory that would restore an older one by surprise.
        if self.settings.panels != before.panels {
            self.shell.panels = Panels::from_settings(&self.settings.panels);
            self.shell.panels.forget_memory();
        }
        if self.settings.appearance != before.appearance {
            let (preference, preset, accent) = self.settings.install_args();
            theme::install_with(ctx, preference, preset, accent);
            ctx.set_zoom_factor(ui_scale::factor(self.settings.appearance.ui_scale));
            furnicraft_ui::set_language(ctx, self.settings.appearance.language.lang());
            furnicraft_ui::set_icon_color(ctx, self.settings.appearance.icon_color);
        }
        // The active variant is read after the install, so a theme switch in this very window
        // repaints the viewport now instead of on the next frame.
        self.shell.colors = theme::scene_colors(ctx.theme());
        if self.settings.viewport != before.viewport || self.shell.colors != colors_before {
            let Self { tabs, shell, settings, .. } = self;
            tabs.active_mut().apply_viewport_settings(&before.viewport, &settings.viewport, shell);
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
                self.handle_file_action(FileAction::New, furnicraft_ui::language(ctx));
            }
            furnicraft_ui::home::HomeAction::Open => self.tabs.active_mut().request_open(),
            furnicraft_ui::home::HomeAction::OpenRecent(path) => {
                self.handle_file_action(FileAction::Open(path), furnicraft_ui::language(ctx));
            }
            furnicraft_ui::home::HomeAction::Close => {}
        }
    }

    /// Draws one frame of the whole interface and performs what the user asked for.
    ///
    /// Everything the window shows happens here: the header, the menus, the ribbon, the floating
    /// panels, the scene, the cluster, the palette, the Settings window and the Home surface. The
    /// eframe trait method delegates to it, so a test drives the real interface with an
    /// [`egui::Context`] and no window at all (U77), and the chrome the frame gave the window comes
    /// back, so the presses it keeps and the rectangles it hands out are asserted without pixels
    /// (U78).
    fn frame(&mut self, ui: &mut egui::Ui) -> Chrome {
        // The language is workspace settings, published once per frame so every surface draws the
        // same words without threading a `Lang` through every signature (U49); the icon-colour mode
        // is published the same way, so the rasteriser never needs the settings threaded to it.
        furnicraft_ui::set_language(ui.ctx(), self.settings.appearance.language.lang());
        furnicraft_ui::set_icon_color(ui.ctx(), self.settings.appearance.icon_color);
        // The theme variant is the window's, and `System` can follow the OS while the window is
        // open, so the shell re-reads it every frame instead of trusting the last settings change.
        self.shell.colors = theme::scene_colors(ui.ctx().theme());
        {
            let Self { tabs, shell, settings, .. } = self;
            // A scene whose last rebuild overran the budget says so before it runs, so the frame
            // that paints the message and the last good geometry is the frame before the work.
            shell.rebuilding = tabs.active().busy(&settings.viewport, &shell.cluster, shell.colors);
            if let Err(error) = tabs.active_mut().sync_scene(ui.ctx(), &settings.viewport, &shell.cluster, shell.colors)
            {
                shell.rebuilding = false;
                tabs.active_mut().last_error = Some(error);
            }
        }
        // A modal — the Settings window, the Home surface, the palette — owns the keys for the frame
        // it is up, so typing a path or a name never saves, undoes, reframes or leaves a tool.
        let modals = self.settings_open || self.home_open || self.palette.is_open() || self.product_card.is_open();
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
            let requested = self.tabs.active_mut().handle_shortcuts(shortcuts);
            if let Some(action) = requested {
                self.handle_file_action(action, furnicraft_ui::language(ui.ctx()));
            }
            if shortcuts.new_tab {
                self.handle_file_action(FileAction::New, furnicraft_ui::language(ui.ctx()));
            }
            if shortcuts.close_tab {
                let active = self.tabs.active_id();
                self.close_tab(active, furnicraft_ui::language(ui.ctx()));
            }
            if shortcuts.hide_panels {
                self.shell.panels.toggle_hide_all();
            }
            if shortcuts.collapse_panels {
                self.shell.panels.toggle_collapse_all();
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
                        title: document.file_name(furnicraft_ui::language(ui.ctx())),
                        dirty: document.is_dirty(),
                        active: id == active,
                    })
                    .collect(),
                can_undo: self.tabs.active().can_undo(),
                can_redo: self.tabs.active().can_redo(),
                ribbon_open: self.tabs.active().ribbon_open(),
                panels_hidden: !self.shell.panels.any_visible(),
                panels_collapsed: self.shell.panels.all_collapsed(),
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
            self.handle_file_action(action, furnicraft_ui::language(ui.ctx()));
        }
        if let Some(action) = self.tabs.active_mut().show_dialogs(ui.ctx()) {
            self.handle_file_action(action, furnicraft_ui::language(ui.ctx()));
        }
        if let Some(action) = header_action {
            self.handle_header_action(action, furnicraft_ui::language(ui.ctx()));
        }
        self.close_question(ui.ctx());

        let design = ui.available_rect_before_wrap();
        self.ribbon(ui, design);
        self.status_bar(ui);
        // The viewport is the whole area under the header and above the status bar: the panels float
        // over it, so the rendered scene runs under their translucent fill (M2). The frame's
        // geometry is computed once and handed to the viewport, so picking and the split views read
        // exactly the rectangles the panels cover.
        let viewport = ui.available_rect_before_wrap().intersect(ui.clip_rect());
        let ribbon_open = self.tabs.active().ribbon_open();
        let band = self.shell.ribbon_band(viewport, ribbon_open);
        let band_height = self.shell.ribbon_inset(ribbon_open);
        let ctx = ui.ctx().clone();
        let animated_band =
            ctx.animate_value_with_time(egui::Id::unique("ribbon_band_height"), band_height, ANIMATION_SECONDS);
        let width = |id: &str, state: furnicraft_ui::settings::PanelState| {
            let target = if state.expanded { EXPANDED_WIDTH } else { STRIP_WIDTH };
            ctx.animate_value_with_time(egui::Id::unique((id, "panel_width")), target, ANIMATION_SECONDS)
        };
        let top = viewport.top() + panel_top(animated_band);
        let panels = &self.shell.panels;
        let rects = PanelRects::of(
            viewport,
            top,
            panels,
            (width("scene_tree", panels.scene), width("panel_card", panels.inspector)),
        );
        let chrome = Chrome { panels: rects, ribbon: band };
        self.workspace(ui, &chrome);
        if let Some(rect) = rects.left {
            self.scene_tree(&ctx, rect);
        }
        if let Some(rect) = rects.right {
            self.panel_card(&ctx, rect);
        }
        self.sync_panels();
        self.command_palette(ui);
        // The cluster's *Configure…* link is drawn inside the viewport, which only has the shell;
        // the window owns the Settings modal, so it opens it here on the same frame (U64).
        if std::mem::take(&mut self.shell.open_settings) {
            self.settings_open = true;
            self.settings_section = SettingsSection::Viewport;
            self.settings_notice = None;
        }
        self.settings_window(ui.ctx());
        self.product_frame_card(ui.ctx());
        self.home_surface(ui.ctx());
        // The close command needs the context, which only the frame has: everything else a quit
        // touches — the questions, the settings write — has already happened by now.
        if self.quit_requested {
            self.quit_requested = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        chrome
    }
}

impl eframe::App for FurniCraftApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
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
            assert!(
                hint.text(furnicraft_text::Lang::En).contains(action),
                "the hint names the switch to `{action}`: {hint:?}"
            );
            assert!(hint.text(furnicraft_text::Lang::En).contains("(O)"), "the hint keeps the shortcut: {hint:?}");
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
        let default_strength = furnicraft_ui::settings::DEFAULT_SHADOW_STRENGTH;
        assert!(cluster.shadow_settings(default_strength).draws(), "the soft ground shadow is on by default");
        cluster.contact_shadows = false;
        assert!(!cluster.shadow_settings(default_strength).visible, "the switch reaches the frame");
        assert!(!cluster.shadow_settings(default_strength).draws());
        cluster.contact_shadows = true;
        assert!((cluster.shadow_settings(50).strength - 0.5).abs() < 1.0e-6);
        assert!(!cluster.shadow_settings(0).draws(), "a zero strength draws nothing either");
        assert!((cluster.shadow_settings(140).strength - 1.0).abs() < 1.0e-6, "a hand-edited strength is clamped");
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
