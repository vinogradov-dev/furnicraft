//! The command palette: one search field over the engine's command registry.
//!
//! The palette is a view of the command surface, not a second way into the document. It lists
//! `CommandRegistry::specs()` with the label, the stable id and the parameters a family can build
//! from the current selection; a chosen entry goes through [`crate::command::dispatch`], which is
//! the one path that snapshots the document for undo. History and the last-used order are session
//! state owned by the shell, never document data.

use egui::{Align, Context, Id, Key, Layout, Modifiers, RichText, Sense, TextEdit, Ui, Vec2};
use furnicraft_engine::node_cmds::{
    NODE_COMPONENT_CONVERT_TO_GROUP_ID, NODE_DELETE_ID, NODE_DUPLICATE_ID, NODE_GROUP_ID, NODE_PIVOT_CENTER_ID,
    NODE_RENAME_ID, NODE_UNGROUP_ID,
};
use furnicraft_engine::panel_cmds::CREATE_ID as PANEL_CREATE_ID;
use furnicraft_engine::scene_cmds::{CAMERA_CREATE_ID, LIGHT_CREATE_ID, SCENE_ACTIVE_CAMERA_SET_ID};
use furnicraft_engine::{
    AssemblyCreate, CameraCreate, CommandOutcome, CommandRegistry, CommandSpec, LightCreate,
    NodeComponentConvertToGroup, NodeDelete, NodeDuplicate, NodeGroup, NodePivotCenter, NodeUngroup, PanelCreate,
    SceneActiveCameraSet, Session,
};
use furnicraft_model::{CameraProjection, Id as NodeId, LightKind, Placement};
use furnicraft_text::{Counted, Lang, Message};
use serde_json::Value;

use crate::command::{dispatch, json};
use crate::{icons, theme};

/// How many dispatched command ids the palette remembers.
pub const MAX_HISTORY: usize = 8;

/// The height of one command row, in points.
const ROW_HEIGHT: f32 = 22.0;

/// The selection and creation target the parameter builders read.
///
/// Values are copied out of the editor so the caller can dispatch afterwards without holding a
/// borrow of the panels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteContext {
    /// Selected node ids, oldest first; the last one is the primary selection.
    pub selection: Vec<NodeId>,
    /// The container a creation lands in when nothing container-like is selected.
    pub parent: NodeId,
}

impl PaletteContext {
    /// Builds the context from the primary selection and the editor's creation target.
    pub fn new(selection: Vec<NodeId>, parent: NodeId) -> Self {
        Self { selection, parent }
    }

    fn primary(&self, lang: Lang) -> Result<NodeId, String> {
        self.selection.last().copied().ok_or_else(|| Message::PaletteNeedsSelection.text(lang).to_owned())
    }
}

/// One command the palette can show.
#[derive(Clone, Debug)]
pub struct Entry {
    spec: CommandSpec,
    params: Result<Value, String>,
}

impl Entry {
    /// The stable command id.
    pub fn id(&self) -> &'static str {
        self.spec.id()
    }

    /// The user-facing command label.
    pub fn label(&self, lang: Lang) -> &'static str {
        self.spec.label().text(lang)
    }

    /// The parameters a dispatch would use, or why the palette cannot build them.
    pub fn params(&self) -> Result<&Value, &str> {
        self.params.as_ref().map_err(String::as_str)
    }

    /// Why the command cannot run from here, or `None` when it is ready.
    pub fn reason(&self) -> Option<&str> {
        self.params.as_ref().err().map(String::as_str)
    }

    /// Whether the entry can be dispatched as it stands.
    pub fn is_ready(&self) -> bool {
        self.params.is_ok()
    }
}

/// One keyboard step while the palette is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteKey {
    /// Move the highlight one entry up, wrapping around.
    Up,
    /// Move the highlight one entry down, wrapping around.
    Down,
    /// Dispatch the highlighted entry when it is ready.
    Enter,
    /// Close the palette without dispatching.
    Escape,
}

/// The palette, its search text and the last commands it dispatched.
///
/// The shell owns one of these; it is view state and never appears in a scene file.
#[derive(Debug, Default)]
pub struct Palette {
    open: bool,
    lang: Lang,
    query: String,
    highlight: usize,
    history: Vec<String>,
    notice: Option<String>,
    focus_pending: bool,
}

impl Palette {
    /// A closed palette with no history.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the palette is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the palette with an empty search and the first entry highlighted.
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.highlight = 0;
        self.notice = None;
        self.focus_pending = true;
    }

    /// Closes the palette and drops a pending explanation.
    pub fn close(&mut self) {
        self.open = false;
        self.notice = None;
        self.focus_pending = false;
    }

    /// Opens a closed palette and closes an open one.
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// The search text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Replaces the search text and resets the highlight to the first match.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.highlight = 0;
        self.notice = None;
    }

    /// The last dispatched command ids, newest first.
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// The explanation the palette is showing, if any.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The index of the highlighted entry.
    pub fn highlight(&self) -> usize {
        self.highlight
    }

    /// Every command that matches the query, most relevant first.
    ///
    /// A prefix of the label or id ranks above a substring, and with an empty query the
    /// last-used commands come first; the registry order breaks every remaining tie.
    pub fn entries(&self, session: &Session, context: &PaletteContext) -> Vec<Entry> {
        let query = self.query.trim().to_ascii_lowercase();
        let mut ranked = Vec::new();
        for spec in CommandRegistry::new().specs() {
            let Some(rank) = rank(*spec, &query, &self.history, self.lang) else {
                continue;
            };
            ranked.push((rank, self.entry(session, context, *spec)));
        }
        ranked.sort_by_key(|(rank, _)| *rank);
        ranked.into_iter().map(|(_, entry)| entry).collect()
    }

    /// Applies one keyboard step.
    ///
    /// `Enter` on an entry that cannot run leaves the palette open and records its reason; it never
    /// dispatches a command with guessed parameters.
    pub fn handle(&mut self, key: PaletteKey, session: &Session, context: &PaletteContext) -> Option<String> {
        match key {
            PaletteKey::Escape => {
                self.close();
                None
            }
            PaletteKey::Up => {
                self.step(session, context, -1);
                None
            }
            PaletteKey::Down => {
                self.step(session, context, 1);
                None
            }
            PaletteKey::Enter => {
                let entries = self.entries(session, context);
                let index = self.highlight.min(entries.len().saturating_sub(1));
                let entry = entries.get(index)?;
                match entry.reason() {
                    Some(reason) => {
                        self.notice = Some(reason.to_owned());
                        None
                    }
                    None => Some(entry.id().to_owned()),
                }
            }
        }
    }

    /// Builds the parameters, checks the pure `enabled` predicate and dispatches one command.
    ///
    /// A successful dispatch records the id, closes the palette and returns the outcome. A failure
    /// keeps the palette open with the reason, because the user chose an entry the session did not
    /// allow.
    pub fn activate(
        &mut self,
        session: &mut Session,
        context: &PaletteContext,
        id: &str,
    ) -> Result<CommandOutcome, String> {
        let params = match self.prepare(session, context, id) {
            Ok(params) => params,
            Err(error) => {
                self.notice = Some(error.clone());
                return Err(error);
            }
        };
        match dispatch(session, id, &params) {
            Ok(outcome) => {
                self.record(id);
                self.close();
                Ok(outcome)
            }
            Err(error) => {
                self.notice = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Draws the modal for one frame and returns the command the user chose, if any.
    ///
    /// The keys are consumed before the list draws, so an arrow never moves the text cursor and
    /// `Enter` never inserts a newline. `Ctrl+K` closes an open palette; `Esc` closes it without
    /// dispatching.
    pub fn show(&mut self, ctx: &Context, session: &Session, context: &PaletteContext) -> Option<String> {
        if !self.open {
            return None;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::K))
            || ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.close();
            return None;
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            self.handle(PaletteKey::Up, session, context);
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            self.handle(PaletteKey::Down, session, context);
        }
        self.lang = crate::language(ctx);
        let mut chosen = None;
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
            chosen = self.handle(PaletteKey::Enter, session, context);
        }

        let mut clicked = None;
        let response = egui::Modal::new(Id::unique("command_palette")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                let (glyph, _) = ui.allocate_exact_size(Vec2::splat(icons::ICON_POINT_SIZE), Sense::hover());
                icons::draw_icon(ui, icons::actions::COMMAND, glyph, icons::ICON_POINT_SIZE, theme::text(ui));
                ui.heading(Message::PaletteHeading.text(crate::language(ui.ctx())));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("Ctrl+K").small().color(theme::text_dim(ui)));
                });
            });
            let search = ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text(Message::PaletteSearch.text(crate::language(ui.ctx())))
                    .desired_width(f32::INFINITY),
            );
            if self.focus_pending {
                search.request_focus();
                self.focus_pending = false;
            }
            if search.changed() {
                self.highlight = 0;
                self.notice = None;
            }
            ui.separator();

            let entries = self.entries(session, context);
            if entries.is_empty() {
                ui.label(
                    RichText::new(Message::PaletteNoMatch.text(crate::language(ui.ctx()))).color(theme::text_dim(ui)),
                );
            } else {
                let index = self.highlight.min(entries.len().saturating_sub(1));
                self.highlight = index;
                egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    for (row, entry) in entries.iter().enumerate() {
                        let selected = row == index;
                        let button = egui::Button::new(entry_job(ui, entry))
                            .selected(selected)
                            .min_size(egui::vec2(ui.available_width(), ROW_HEIGHT));
                        let response = ui.add(button);
                        if selected {
                            response.scroll_to_me(Some(Align::Center));
                        }
                        if response.clicked() {
                            if let Some(reason) = entry.reason() {
                                self.notice = Some(reason.to_owned());
                            } else {
                                clicked = Some(entry.id().to_owned());
                            }
                        }
                    }
                });
            }
            if let Some(notice) = &self.notice {
                ui.separator();
                ui.colored_label(ui.visuals().warn_fg_color, notice);
            }
            let count: Counted = Message::counted(
                entries.len(),
                Message::PaletteCountOne,
                Message::PaletteCountFew,
                Message::PaletteCountMany,
            );
            ui.label(RichText::new(count.text(crate::language(ui.ctx()))).small().color(theme::text_dim(ui)));
        });
        if response.should_close() {
            self.close();
        }
        clicked.or(chosen)
    }

    /// Builds the parameters for one id and checks that the session allows it.
    fn prepare(&self, session: &Session, context: &PaletteContext, id: &str) -> Result<Value, String> {
        let spec =
            CommandRegistry::new().get(id).ok_or_else(|| Message::PaletteUnknownCommand.with(self.lang, [id]))?;
        let params = params_for(id, session, context, self.lang)?;
        if spec.enabled(session, &params) {
            Ok(params)
        } else {
            Err(Message::PaletteNotAllowed.text(self.lang).to_owned())
        }
    }

    /// One ranked entry, with the reason it cannot run when the builder or the session refuses.
    fn entry(&self, session: &Session, context: &PaletteContext, spec: CommandSpec) -> Entry {
        Entry { spec, params: self.prepare(session, context, spec.id()) }
    }

    fn step(&mut self, session: &Session, context: &PaletteContext, delta: isize) {
        let count = self.entries(session, context).len();
        if count == 0 {
            self.highlight = 0;
            return;
        }
        let current = self.highlight.min(count - 1);
        self.highlight = (current as isize + delta).rem_euclid(count as isize) as usize;
    }

    fn record(&mut self, id: &str) {
        self.history.retain(|known| known != id);
        self.history.insert(0, id.to_owned());
        self.history.truncate(MAX_HISTORY);
    }
}

/// The match rank of one command, or `None` when the query excludes it.
fn rank(spec: CommandSpec, query: &str, history: &[String], lang: Lang) -> Option<u32> {
    if query.is_empty() {
        let recent = history.iter().position(|id| id == spec.id()).map_or(100, |index| index as u32);
        return Some(recent);
    }
    let label = spec.label().text(lang).to_ascii_lowercase();
    let id = spec.id().to_ascii_lowercase();
    if label.starts_with(query) {
        Some(0)
    } else if id.starts_with(query) {
        Some(1)
    } else if label.contains(query) {
        Some(2)
    } else if id.contains(query) {
        Some(3)
    } else {
        None
    }
}

/// Builds one command's parameters from the selection, or says which value is missing.
///
/// The builder exists for the node families and the camera/light actions first. Every other spec
/// is listed but reports [`NEEDS_SELECTION`] until it gets a builder, so the palette never guesses
/// a size, name or material.
fn params_for(id: &str, session: &Session, context: &PaletteContext, lang: Lang) -> Result<Value, String> {
    match id {
        NODE_DELETE_ID => {
            let node = context.primary(lang)?;
            json(&NodeDelete { node })
        }
        NODE_DUPLICATE_ID => {
            let node = context.primary(lang)?;
            json(&NodeDuplicate { node })
        }
        NODE_RENAME_ID => {
            context.primary(lang)?;
            Err(Message::PaletteNeedsNewName.text(lang).to_owned())
        }
        NODE_GROUP_ID => {
            if context.selection.len() < 2 {
                return Err(Message::PaletteNeedsTwoNodes.text(lang).to_owned());
            }
            json(&NodeGroup { nodes: context.selection.clone(), name: Message::GeneratedGroup.text(lang).to_owned() })
        }
        NODE_UNGROUP_ID => {
            let node = context.primary(lang)?;
            json(&NodeUngroup { node })
        }
        NODE_COMPONENT_CONVERT_TO_GROUP_ID => {
            let node = context.primary(lang)?;
            json(&NodeComponentConvertToGroup { node })
        }
        NODE_PIVOT_CENTER_ID => {
            let node = context.primary(lang)?;
            json(&NodePivotCenter { node })
        }
        SCENE_ACTIVE_CAMERA_SET_ID => {
            let camera = context.primary(lang)?;
            if session.project().camera(camera).is_none() {
                return Err(Message::PaletteNeedsCamera.text(lang).to_owned());
            }
            json(&SceneActiveCameraSet { camera: Some(camera) })
        }
        PANEL_CREATE_ID => {
            let material = session
                .catalog()
                .panel_materials()
                .first()
                .ok_or_else(|| Message::PaletteAddPanelMaterial.text(lang).to_owned())?
                .id();
            json(&PanelCreate {
                parent: create_parent(session, context),
                name: Message::ToolPanel.text(lang).to_owned(),
                nominal_size: PanelCreate::default_nominal_size().map_err(|error| error.to_string())?,
                material,
                placement: Placement::IDENTITY,
                class: None,
            })
        }
        furnicraft_engine::node_cmds::ASSEMBLY_CREATE_ID => json(&AssemblyCreate {
            parent: create_parent(session, context),
            name: Message::ToolAssembly.text(lang).to_owned(),
            placement: Placement::IDENTITY,
        }),
        CAMERA_CREATE_ID => json(&CameraCreate {
            parent: create_parent(session, context),
            name: Message::ToolCamera.text(lang).to_owned(),
            placement: Placement::IDENTITY,
            projection: CameraProjection::DEFAULT,
        }),
        LIGHT_CREATE_ID => json(&LightCreate {
            parent: create_parent(session, context),
            name: Message::ToolLight.text(lang).to_owned(),
            placement: Placement::IDENTITY,
            kind: LightKind::Sun,
            color: [1.0, 0.97, 0.92],
            intensity: 1.0,
        }),
        _ => Err(Message::PaletteNeedsSelection.text(lang).to_owned()),
    }
}

/// The container a create command lands in: a selected container, else the editor's target.
fn create_parent(session: &Session, context: &PaletteContext) -> NodeId {
    context.selection.last().copied().filter(|id| session.project().is_container(*id)).unwrap_or(context.parent)
}

/// One row: label, stable id and, when it cannot run, the reason in the warning colour.
fn entry_job(ui: &Ui, entry: &Entry) -> egui::WidgetText {
    let body = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (ui.available_width() - 16.0).max(1.0);
    job.wrap.max_rows = 1;
    job.append(
        entry.label(crate::language(ui.ctx())),
        0.0,
        egui::TextFormat { font_id: body.clone(), color: theme::text(ui), ..Default::default() },
    );
    job.append(
        "  ",
        0.0,
        egui::TextFormat { font_id: small.clone(), color: theme::text_dim(ui), ..Default::default() },
    );
    job.append(
        entry.id(),
        0.0,
        egui::TextFormat { font_id: small.clone(), color: theme::text_dim(ui), ..Default::default() },
    );
    if let Some(reason) = entry.reason() {
        job.append(
            "  ·  ",
            0.0,
            egui::TextFormat { font_id: small.clone(), color: theme::text_dim(ui), ..Default::default() },
        );
        job.append(
            reason,
            0.0,
            egui::TextFormat { font_id: small, color: ui.visuals().warn_fg_color, ..Default::default() },
        );
    }
    job.into()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_testkit::{sample_catalog, sample_project};

    use super::*;

    fn fixture() -> (Session, PaletteContext, NodeId) {
        let session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        let shelf = NodeId::new(2).expect("the sample shelf id");
        (session, PaletteContext::new(vec![shelf], root), shelf)
    }

    #[test]
    fn a_prefix_and_a_substring_both_find_node_delete() {
        let (session, context, _) = fixture();
        let mut palette = Palette::new();

        palette.set_query("node.del");
        assert!(palette.entries(&session, &context).iter().any(|entry| entry.id() == "node.delete"));

        palette.set_query("delete");
        assert!(palette.entries(&session, &context).iter().any(|entry| entry.id() == "node.delete"));

        palette.set_query("DELETE");
        assert!(palette.entries(&session, &context).iter().any(|entry| entry.id() == "node.delete"));
    }

    #[test]
    fn a_disabled_command_is_listed_but_not_runnable_and_explains_itself() {
        let (session, context, _) = fixture();
        let mut palette = Palette::new();
        palette.open();
        palette.set_query("Resize");

        let entries = palette.entries(&session, &context);
        let resize = entries.iter().find(|entry| entry.id() == "panel.resize").expect("panel.resize is listed");
        assert!(!resize.is_ready());
        assert!(resize.reason().is_some(), "a disabled entry names the missing value");

        let revision = session.project().revision();
        assert!(palette.handle(PaletteKey::Enter, &session, &context).is_none(), "nothing is dispatched");
        assert_eq!(session.project().revision(), revision);
        assert_eq!(palette.notice(), resize.reason());
    }

    #[test]
    fn escape_closes_without_dispatching() {
        let (session, context, _) = fixture();
        let mut palette = Palette::new();
        palette.open();
        let revision = session.project().revision();

        let ctx = Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))),
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            palette.show(ui.ctx(), &session, &context);
        });
        output.drop_without_applying_deltas();

        assert!(!palette.is_open());
        assert_eq!(session.project().revision(), revision);
        assert!(!session.can_undo());
    }

    #[test]
    fn a_chosen_entry_dispatches_one_command_and_one_undo_entry() {
        let (mut session, context, _shelf) = fixture();
        let before = session.project().clone();
        let mut palette = Palette::new();
        palette.open();
        let revision = session.project().revision();

        let outcome = palette.activate(&mut session, &context, NODE_DUPLICATE_ID).expect("duplicate the shelf");
        assert!(outcome.created_id().is_some());
        assert!(session.project().revision() > revision);
        assert_eq!(palette.history(), [NODE_DUPLICATE_ID.to_owned()]);
        assert!(!palette.is_open(), "a successful command closes the palette");

        session.undo().expect("one undo reverses the whole command");
        assert!(before.same_content(session.project()));
        assert!(!session.can_undo(), "the dispatch left exactly one undo entry");
    }

    #[test]
    fn last_used_commands_come_first_when_the_search_is_empty() {
        let (mut session, _context, _shelf) = fixture();
        // The pivot commands author a container's pivot; a panel's is derived.
        let drawer = NodeId::new(3).expect("the sample drawer id");
        let context = PaletteContext::new(vec![drawer], session.project().root().id());
        let mut palette = Palette::new();
        palette.open();
        palette.activate(&mut session, &context, NODE_PIVOT_CENTER_ID).expect("centre the drawer pivot");
        palette.open();

        let entries = palette.entries(&session, &context);
        assert_eq!(entries.first().map(Entry::id), Some(NODE_PIVOT_CENTER_ID));
        assert_eq!(
            entries.iter().filter(|entry| entry.id() == NODE_PIVOT_CENTER_ID).count(),
            1,
            "history reorders an entry instead of repeating it"
        );
        assert_eq!(palette.history().len(), 1);
    }

    #[test]
    fn the_history_is_bounded_and_newest_first() {
        let mut palette = Palette::new();
        for index in 0..(MAX_HISTORY + 4) {
            palette.record(&format!("node.test.{index}"));
        }
        assert_eq!(palette.history().len(), MAX_HISTORY);
        assert_eq!(palette.history()[0], format!("node.test.{}", MAX_HISTORY + 3));

        palette.record("node.test.5");
        assert_eq!(palette.history()[0], "node.test.5");
        assert_eq!(palette.history().iter().filter(|id| *id == "node.test.5").count(), 1);
    }

    #[test]
    fn history_and_the_query_never_reach_the_document() {
        let (session, context, _) = fixture();
        let mut palette = Palette::new();
        palette.open();
        palette.set_query("delete");
        let _ = palette.entries(&session, &context);
        palette.record(NODE_DUPLICATE_ID);

        let document = serde_json::to_string(session.project()).expect("serialise the project");
        for key in ["history", "query", "highlight", "palette"] {
            assert!(!document.contains(key), "`{key}` is session state and must stay out of the .fcs");
        }
    }

    #[test]
    fn the_entry_list_has_a_reason_for_every_unbuilt_family() {
        let (session, context, _) = fixture();
        let mut palette = Palette::new();
        palette.set_query("resize");
        let entries = palette.entries(&session, &context);
        assert!(entries.iter().all(|entry| entry.reason().is_some()));
    }
}
