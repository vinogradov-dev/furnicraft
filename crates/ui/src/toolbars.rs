//! The ribbon: one row of tabs over the tools of the active tab.
//!
//! The map — five tabs that name a domain (*Sketch, Model, Furniture, Scene, View*), the groups of
//! operations under them, the tools and the fields that configure them — is one const table in
//! [`spec`]. This module draws it with one loop: a tab is drawn when it owns a live group, a group is
//! drawn when it owns a live tool or a field, and a planned tool (no command yet) is skipped, so the
//! ribbon never shows a greyed "coming soon" row and a tab with nothing to run is not drawn at all.
//!
//! Whether a tool can run *here* is code, not data: [`ready`] decides it against the
//! [`RibbonContext`] — a catalogue material, a closed path, a running draft, a viewport that owns its
//! projection — and returns the hint that says what is missing, so a disabled cell still explains
//! itself. Whether the body is open, which tab is active and what the numeric fields remember is view
//! state in the editor and never reaches the document; the open state is also the workspace setting
//! the shell remembers (`Settings::ribbon`).
//!
//! A cell is one glyph in a [`RIBBON_CELL_WIDTH`] by [`RIBBON_ROW_HEIGHT`] cell, so a row is one pitch
//! and no tool sizes itself from its name (ADR 0067, revised by ADR 0091). The group's caption sits in
//! the darker band under the row, which is what makes the whole body 72 points tall and the folded
//! ribbon its 29-point tab row.
//!
//! Every button is the icon-only [`icons::ribbon_cell`]: the ribbon names a tool through its hint and,
//! from U67, through the list under the cell; the panel headers and the floating viewport cluster keep
//! the icon-only [`icons::tool_button`], whose row could not carry a second line.

use egui::{TextEdit, Ui};
use furnicraft_engine::PrimitiveKind;
use furnicraft_model::Id;
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use self::spec::{CellSpec, FieldKind, FieldSpec, GroupSpec, TabSpec, ToolKind, ToolSpec};
use crate::draw::{DEFAULT_POLYGON_SIDES, DrawTool};
use crate::icons::{self, Icon, tool_hint};
use crate::theme::{RIBBON_CAPTION_HEIGHT, RIBBON_CELL_WIDTH, RIBBON_ROW_HEIGHT, RIBBON_RULE, RIBBON_TAB_HEIGHT};

pub mod spec;

/// Edge length of a new cube, in millimetres; also the value the field starts with.
pub const DEFAULT_PRIMITIVE_SIZE_MM: i64 = 200;

/// Height of a new extrusion, in millimetres; also the value the field starts with.
pub const DEFAULT_EXTRUDE_HEIGHT_MM: i64 = 100;

/// One action a toolbar panel asks the editor to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolAction {
    /// Starts a drawing tool; `sides` is the regular polygon's side count and is ignored by the
    /// other tools.
    Draw {
        /// Which tool to arm.
        tool: DrawTool,
        /// The side count the polygon tool uses.
        sides: u32,
    },
    /// Creates a panel at the scene root.
    CreatePanel,
    /// Creates an assembly at the scene root.
    CreateAssembly,
    /// Creates a primitive at the scene root once the work plane is clicked.
    CreatePrimitive {
        /// Which primitive to build.
        kind: PrimitiveKind,
        /// Full extent of the primitive.
        size: Length,
    },
    /// Extrudes the selected closed path into a body.
    ExtrudePath {
        /// The path object to extrude.
        object: Id,
        /// Height along the path's local Z.
        height: Length,
    },
    /// Creates a camera at the scene root.
    CreateCamera,
    /// Creates a sun light at the scene root.
    CreateLight,
    /// Closes the running drawing into a ring.
    CloseDrawing,
    /// Drops the running drawing; the document never changed.
    CancelDrawing,
    /// Frames the whole scene in the viewport.
    FrameAll,
    /// Switches the viewport between perspective and orthographic.
    ToggleProjection,
    /// Turns the work-plane grid snap on or off.
    ToggleGrid,
}

/// Every drawing tool, in shortcut order: the draw group then the shapes.
pub const CONTOUR_TOOLS: [DrawTool; 10] = [
    DrawTool::Line,
    DrawTool::Polyline,
    DrawTool::Arc,
    DrawTool::ArcFromCenter,
    DrawTool::Rectangle,
    DrawTool::RectangleFromCenter,
    DrawTool::RectangleFromCenterDirected,
    DrawTool::Circle,
    DrawTool::CircleThreePoints,
    DrawTool::RegularPolygon,
];

/// The tool that owns the next pointer events, drawn as pressed on the strip.
///
/// Only the two arming tools have a state to show: a drawing tool holds the pointer until the chain
/// is committed, and a primitive waits for its work-plane click. A one-shot creation — a panel, an
/// assembly, a camera or a light — runs and forgets, so it is never armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolActivity {
    /// A drawing tool is building a chain.
    Draw(DrawTool),
    /// A primitive is waiting for its work-plane click.
    Primitive(PrimitiveKind),
}

/// The activity whose button is pressed, from what the editor holds.
///
/// The arguments are the two sources of an armed tool, not a decision: a running draft names its
/// tool and a pending primitive names its kind, and neither outranks the other because the editor
/// never holds both at once.
pub fn tool_activity(drawing: Option<DrawTool>, primitive: Option<PrimitiveKind>) -> Option<ToolActivity> {
    match (drawing, primitive) {
        (Some(tool), _) => Some(ToolActivity::Draw(tool)),
        (None, Some(kind)) => Some(ToolActivity::Primitive(kind)),
        (None, None) => None,
    }
}

/// Whether the button for one tool is the armed one.
///
/// A creation that runs and forgets is never armed, so its button stays idle however long the
/// selection sits there.
fn is_armed(activity: Option<ToolActivity>, kind: ToolKind) -> bool {
    match (activity, kind) {
        (Some(ToolActivity::Draw(armed)), ToolKind::Draw(tool)) => armed == tool,
        (Some(ToolActivity::Primitive(armed)), ToolKind::Primitive(kind)) => armed == kind,
        _ => false,
    }
}

/// One key that starts a drawing tool, with the modifier it needs.
///
/// A shifted letter is the alternate of the same family: `A` draws an arc through points and
/// `Shift+A` an arc from its centre, `R` a rectangle from two corners and `Shift+R` one from its
/// centre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    /// The key that starts the tool.
    pub key: egui::Key,
    /// Whether Shift must be held with it.
    pub shift: bool,
}

/// The shortcut that starts a drawing tool.
///
/// The letters are free in the viewport (`F` frames, `O` toggles the projection, `G` the grid), so
/// the tooltip and the key handler can share one table.
pub const fn shortcut(tool: DrawTool) -> Option<Shortcut> {
    let (key, shift) = match tool {
        DrawTool::Line => (egui::Key::L, false),
        DrawTool::Polyline => (egui::Key::P, false),
        DrawTool::Arc => (egui::Key::A, false),
        DrawTool::ArcFromCenter => (egui::Key::A, true),
        DrawTool::Rectangle => (egui::Key::R, false),
        DrawTool::RectangleFromCenter => (egui::Key::R, true),
        DrawTool::RectangleFromCenterDirected => (egui::Key::D, true),
        DrawTool::Circle => (egui::Key::C, false),
        DrawTool::CircleThreePoints => (egui::Key::C, true),
        DrawTool::RegularPolygon => (egui::Key::N, false),
    };
    Some(Shortcut { key, shift })
}

/// The label of a tool's shortcut, for the button hint.
pub const fn shortcut_label(tool: DrawTool) -> Option<&'static str> {
    match tool {
        DrawTool::Line => Some("L"),
        DrawTool::Polyline => Some("P"),
        DrawTool::Arc => Some("A"),
        DrawTool::ArcFromCenter => Some("Shift+A"),
        DrawTool::Rectangle => Some("R"),
        DrawTool::RectangleFromCenter => Some("Shift+R"),
        DrawTool::RectangleFromCenterDirected => Some("Shift+D"),
        DrawTool::Circle => Some("C"),
        DrawTool::CircleThreePoints => Some("Shift+C"),
        DrawTool::RegularPolygon => Some("N"),
    }
}

/// The strip's label for a drawing tool: its name plus the one gesture the tool does not show.
///
/// The label is the button's tooltip and the text a placeholder asset falls back to; it is not the
/// name of the object the tool creates, which stays [`DrawTool::label`].
pub const fn strip_label(tool: DrawTool) -> Message {
    match tool {
        DrawTool::Polyline => Message::PolylineStripLabel,
        _ => tool.label(),
    }
}

/// Whether the ribbon's body is open, which tab is active, and the numeric fields it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: bool,
    tab: &'static TabSpec,
    primitive_size: String,
    extrude_height: String,
    polygon_sides: String,
}

impl Default for Toolbars {
    fn default() -> Self {
        Self {
            open: true,
            tab: spec::fallback_tab().unwrap_or(&spec::TABS[0]),
            primitive_size: DEFAULT_PRIMITIVE_SIZE_MM.to_string(),
            extrude_height: DEFAULT_EXTRUDE_HEIGHT_MM.to_string(),
            polygon_sides: DEFAULT_POLYGON_SIDES.to_string(),
        }
    }
}

impl Toolbars {
    /// The body open on the first tab that draws a row, the state a fresh editor starts in.
    pub fn new() -> Self {
        Self::default()
    }

    /// The tab whose groups the ribbon draws.
    pub const fn tab(&self) -> &'static TabSpec {
        self.tab
    }

    /// Opens a tab's body, making it the active tab; returns the tab that is now active.
    pub fn set_tab(&mut self, tab: &'static TabSpec) -> &'static TabSpec {
        self.tab = tab;
        tab
    }

    /// Whether the body under the tab row is drawn.
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the body and returns its new state.
    pub fn set_open(&mut self, open: bool) -> bool {
        self.open = open;
        self.open
    }

    /// Flips the body and returns its new state.
    pub fn toggle(&mut self) -> bool {
        self.set_open(!self.open)
    }

    /// Keeps the active tab on a tab that draws a body.
    ///
    /// Only [`Self::set_tab`] can put the ribbon on a planned-only tab, so this is one guard rather
    /// than a per-frame search: a state that names a tab with nothing live falls back to the first
    /// tab that has something, and a ribbon whose every tab is planned draws no body.
    fn retain_tab(&mut self) {
        if !self.tab.is_drawn()
            && let Some(tab) = spec::fallback_tab()
        {
            self.tab = tab;
        }
    }
}

impl Toolbars {
    /// The text of one field, so its group can edit it in place.
    pub fn field_text_mut(&mut self, kind: FieldKind) -> &mut String {
        match kind {
            FieldKind::PrimitiveSize => &mut self.primitive_size,
            FieldKind::ExtrudeHeight => &mut self.extrude_height,
            FieldKind::PolygonSides => &mut self.polygon_sides,
        }
    }

    /// The typed full extent of a new primitive: the cube's edge, or a curved shape's diameter.
    pub fn primitive_size_text(&self) -> &str {
        &self.primitive_size
    }

    /// Replaces the typed size.
    pub fn set_primitive_size_text(&mut self, text: impl Into<String>) {
        self.primitive_size = text.into();
    }

    /// The typed size as a document length, or a readable reason it is not one.
    pub fn primitive_size(&self) -> Result<Length, FieldRefusal> {
        let length = Length::parse_millimetres(self.primitive_size.trim())
            .map_err(|error| FieldRefusal::SizeNotLength(error.to_string()))?;
        if length.micrometres() <= 0 {
            return Err(FieldRefusal::SizeNotPositive);
        }
        Ok(length)
    }

    /// The typed extrusion height.
    pub fn extrude_height_text(&self) -> &str {
        &self.extrude_height
    }

    /// The typed extrusion height as a positive document length.
    ///
    /// The upper bound is the document's contour limit, the same one `object.extrude.create`
    /// applies, so a field the group accepts is a field the command takes.
    pub fn extrude_height(&self) -> Result<Length, FieldRefusal> {
        let height = Length::parse_millimetres(self.extrude_height.trim())
            .map_err(|error| FieldRefusal::HeightNotLength(error.to_string()))?;
        if !height.is_positive() {
            return Err(FieldRefusal::HeightNotPositive);
        }
        let limit = furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres();
        if height.micrometres() > limit {
            return Err(FieldRefusal::HeightTooLong { max_mm: limit / 1_000 });
        }
        Ok(height)
    }

    /// Replaces the typed extrusion height.
    pub fn set_extrude_height_text(&mut self, text: impl Into<String>) {
        self.extrude_height = text.into();
    }

    /// The typed regular polygon side count.
    pub fn polygon_sides_text(&self) -> &str {
        &self.polygon_sides
    }

    /// The typed side count, clamped to the range the model accepts.
    ///
    /// A field the user is halfway through typing is never a reason to disable the tool: the value
    /// falls back to [`DEFAULT_POLYGON_SIDES`] so a cleared field still draws a hexagon.
    pub fn polygon_sides(&self) -> u32 {
        clamp_polygon_sides(self.polygon_sides.trim().parse().unwrap_or(DEFAULT_POLYGON_SIDES))
    }

    /// Replaces the typed side count.
    pub fn set_polygon_sides_text(&mut self, text: impl Into<String>) {
        self.polygon_sides = text.into();
    }
}

/// Clamps a side count to the range `Path::regular_polygon` accepts.
const fn clamp_polygon_sides(sides: u32) -> u32 {
    if sides < furnicraft_model::MIN_POLYGON_SIDES {
        furnicraft_model::MIN_POLYGON_SIDES
    } else if sides > furnicraft_model::MAX_POLYGON_SIDES {
        furnicraft_model::MAX_POLYGON_SIDES
    } else {
        sides
    }
}

/// Why a ribbon field cannot be used, in the words of its own tool.
///
/// The refusal is the hint of every tool the field configures, so a bad size disables a primitive and
/// says which number is wrong instead of leaving the button silently off. The sentence is chosen when
/// the ribbon draws, so one field reads in the workspace's language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldRefusal {
    /// The typed size is not a length; the parser's reason is carried for the sentence.
    SizeNotLength(String),
    /// The typed size is zero or negative.
    SizeNotPositive,
    /// The typed extrusion height is not a length.
    HeightNotLength(String),
    /// The typed extrusion height is zero or negative.
    HeightNotPositive,
    /// The typed extrusion height is past the document's limit.
    HeightTooLong {
        /// The largest height the command takes, in millimetres.
        max_mm: i64,
    },
}

impl FieldRefusal {
    /// The sentence the hint draws.
    pub fn text(&self, lang: Lang) -> String {
        match self {
            Self::SizeNotLength(reason) => Message::PrimitiveSizeNotLength.with(lang, [reason]),
            Self::SizeNotPositive => Message::PrimitiveSizePositive.text(lang).to_owned(),
            Self::HeightNotLength(reason) => Message::ExtrudeHeightNotLength.with(lang, [reason]),
            Self::HeightNotPositive => Message::ExtrudeHeightPositive.text(lang).to_owned(),
            Self::HeightTooLong { max_mm } => Message::ExtrudeHeightMax.with(lang, [max_mm]),
        }
    }
}

/// The view state the ribbon reads from the shell: how the camera and the work plane are set.
///
/// The ribbon is a UI surface in `furnicraft-ui`, but the projection, the grid snap and the framing
/// belong to the shell that owns the view camera and the drawing gizmo. They arrive as this small
/// snapshot, and a click comes back as a [`RibbonViewAction`], so the ribbon neither reads nor
/// writes the viewport.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RibbonView {
    /// The view camera is perspective rather than orthographic.
    pub perspective: bool,
    /// The viewport owns its projection; an authored camera pins it.
    pub owns_view: bool,
    /// The work-plane grid snap is on.
    pub grid: bool,
}

/// What one ribbon click asked the shell's view to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonViewAction {
    /// Frame the whole scene.
    FrameAll,
    /// Switch between perspective and orthographic.
    ToggleProjection,
    /// Turn the work-plane grid snap on or off.
    ToggleGrid,
}

/// What drawing the ribbon changed: whether a tool started, and the one view action it asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RibbonOutcome {
    /// A drawing tool started, so the shell resets its pointer state.
    pub started_drawing: bool,
    /// The running ring must be closed and committed.
    ///
    /// The ribbon reports the *Close* instead of running it, because closing owns the work plane,
    /// the gizmo state and the scene rebuild — all of them the shell's. One close path serves the
    /// ribbon, the status bar and a click on the first draft point (ADR 0052).
    pub close_draft: bool,
    /// The view action the ribbon asked for, if any.
    pub view: Option<RibbonViewAction>,
    /// The body a path extrusion just created, with the height it used.
    ///
    /// The shell starts the pull arrow on that body; the height is session view state, so the next
    /// extrusion starts here instead of at the default.
    pub extruded: Option<(Id, Length)>,
}

/// What the ribbon reads from the scene, the pointer and the view camera while it draws.
///
/// The values travel together because every tab reads the same picture: what the catalogue offers,
/// what is selected, whether that selection is a closed path, which tool is armed, how the viewport
/// draws, and what the running draft has built so far.
#[derive(Clone, Copy, Debug, Default)]
pub struct RibbonContext {
    /// The catalogue has at least one panel material.
    pub has_panel_material: bool,
    /// The primary selection, if any.
    pub selected: Option<Id>,
    /// Whether the selection is a path, and whether that path is closed.
    pub selected_path: Option<bool>,
    /// The tool that owns the next click.
    pub active: Option<ToolActivity>,
    /// The view is perspective rather than orthographic.
    pub perspective: bool,
    /// Whether the viewport owns its projection; an authored camera pins it.
    pub owns_view: bool,
    /// Whether the work-plane grid snap is on.
    pub grid: bool,
    /// A running draft: its tool, how many points it has, and whether it is closed.
    pub draft: Option<(DrawTool, usize, bool)>,
}

/// Draws the ribbon: one row of tabs and, while it is open, the groups of the active tab.
///
/// Every tab that owns a live group is drawn, and the one whose groups are shown carries the accent.
/// Clicking the active tab, or double-clicking any tab label, toggles the body, so a collapsed ribbon
/// keeps only its tab row. The armed tool draws pressed and a tool whose action needs a draft or a
/// viewport that owns its projection is drawn disabled; the hint says what is missing.
pub fn show(ui: &mut Ui, state: &mut Toolbars, context: RibbonContext, actions: &mut Vec<ToolAction>) {
    let lang = crate::language(ui.ctx());
    ui.scope(|ui| {
        ui.set_width(ui.available_width());
        // The rows sit edge to edge: the drawn height is the sum of the metrics and nothing else,
        // because the shell reserves exactly [`height`] over the design space (ADR 0080).
        ui.spacing_mut().item_spacing.y = 0.0;
        state.retain_tab();
        tab_row(ui, state, lang);
        rule(ui);
        if state.is_open() {
            ribbon_body(ui, state, context, actions, lang);
            rule(ui);
        }
    });
}

/// The outer margin of the ribbon's overlay frame, in points (horizontal, vertical).
///
/// The ribbon floats over the design space instead of taking height from it (U51), so its height and
/// the band the shell reserves for it must agree exactly: this constant is the single source the
/// frame the ribbon draws and [`height`] both read. The vertical margin is zero, because the height
/// is the approved 29/72 metric itself and every row of it is a control row.
pub const RIBBON_FRAME_MARGIN: [i8; 2] = [6, 0];

/// The height the ribbon overlay covers over the design space, in points.
///
/// The shell reserves this band for the ribbon and never lets the scene, the side panels or the
/// framing use it, so a translucent ribbon blends with the furniture it covers (U51). It is
/// deterministic from the open state so the strip and the reserved inset agree without a frame of
/// lag when the body opens or closes: the tab row and its rule folded, plus the tool row, the caption
/// band and the rule under it when the body is open.
pub fn height(open: bool) -> f32 {
    let body = if open { RIBBON_ROW_HEIGHT + RIBBON_CAPTION_HEIGHT + RIBBON_RULE } else { 0.0 };
    RIBBON_TAB_HEIGHT + RIBBON_RULE + body
}

/// One hairline across the strip, the rule that closes a row.
fn rule(ui: &mut Ui) {
    let width = ui.available_width().max(0.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, RIBBON_RULE), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, crate::theme::line(ui));
}

/// The one row of tabs, each drawn in the ribbon's own rhythm.
fn tab_row(ui: &mut Ui, state: &mut Toolbars, lang: Lang) {
    let height = RIBBON_TAB_HEIGHT;
    ui.horizontal(|ui| {
        ui.set_height(height);
        let active = state.tab();
        let open = state.is_open();
        for tab in spec::tabs() {
            if !tab.is_drawn() {
                continue;
            }
            let selected = tab.id == active.id && open;
            let response = tab_button(ui, tab, selected, height, lang);
            // A double-click also reports a click in the same frame, so it is tested first: the two
            // gestures would otherwise cancel each other out and the body would never close.
            if response.double_clicked() {
                apply_tab_gesture(state, tab, true);
            } else if response.clicked() {
                apply_tab_gesture(state, tab, false);
            }
        }
    });
}

/// What one gesture on a tab does: open its body, or collapse and restore it.
///
/// A single click on another tab opens that tab; a single click on the open tab collapses the body,
/// because the tab already says where the tools are. A double-click collapses or restores whichever
/// tab it lands on, which is the office-ribbon habit and works without aiming at the open tab.
fn apply_tab_gesture(state: &mut Toolbars, tab: &'static TabSpec, double: bool) {
    if double {
        state.set_tab(tab);
        state.toggle();
    } else if tab.id == state.tab().id {
        state.toggle();
    } else {
        state.set_tab(tab);
        state.set_open(true);
    }
}

/// One tab: its name in the body type, on a control fill with the accent underline while it is open.
///
/// The open tab is marked by the accent, not by a second palette entry: the whole ribbon is chrome,
/// and one saturated line on the row already says which group row belongs to which tab. Every tab
/// has the same inner padding and a minimum width, so a four-letter name does not read as a stamp
/// next to a long one.
fn tab_button(ui: &mut Ui, tab: &'static TabSpec, selected: bool, height: f32, lang: Lang) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let galley =
        ui.fonts_mut(|fonts| fonts.layout_no_wrap(tab.label.text(lang).to_owned(), font, egui::Color32::PLACEHOLDER));
    let width = (galley.size().x + 6.0 * crate::theme::STEP).max(crate::theme::RIBBON_TAB_MIN_WIDTH);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        if selected {
            ui.painter().rect_filled(rect, egui::CornerRadius::same(crate::theme::RADIUS), crate::theme::control(ui));
            let underline = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 2.0, rect.bottom() - 2.0),
                egui::pos2(rect.right() - 2.0, rect.bottom()),
            );
            ui.painter().rect_filled(underline, egui::CornerRadius::ZERO, crate::theme::accent(ui));
        } else if response.hovered() {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(crate::theme::RADIUS),
                crate::theme::control_hover(ui),
            );
        }
        let color = if selected { crate::theme::text(ui) } else { crate::theme::text_dim(ui) };
        let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, color);
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, tab.label.text(lang)));
    }
    response
}

/// The body under the tab row: the active tab's live groups in one row, separated by dividers.
///
/// The row is as wide as the map says — the tab's [`TabSpec::columns`] cells — and a group widens to
/// its own caption if the caption is wider than its cells, so a one-tool group still names itself.
/// The widest row of the map is ten cells, well inside the smallest window, so the body needs no
/// scroll area.
fn ribbon_body(ui: &mut Ui, state: &mut Toolbars, context: RibbonContext, actions: &mut Vec<ToolAction>, lang: Lang) {
    let tab = state.tab();
    if !tab.is_drawn() {
        return;
    }
    let body_height = RIBBON_ROW_HEIGHT + RIBBON_CAPTION_HEIGHT;
    ui.horizontal(|ui| {
        // Cells sit edge to edge inside a group: the whole cell is the button, and the only air
        // between two controls is the padding the cell already carries (ADR 0067).
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.set_height(body_height);
        for (index, group) in tab.live_groups().enumerate() {
            if index > 0 {
                crate::theme::divider_of_height(ui, body_height);
            }
            show_group(ui, group, state, context, actions, lang);
        }
    });
}

/// One group: its row of cells over the darker band that captions it.
fn show_group(
    ui: &mut Ui,
    group: &'static GroupSpec,
    state: &mut Toolbars,
    context: RibbonContext,
    actions: &mut Vec<ToolAction>,
    lang: Lang,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.set_height(RIBBON_ROW_HEIGHT);
            for cell in group.cells.iter().filter(|cell| cell.is_live()) {
                match cell {
                    CellSpec::Tool(tool) => show_tool(ui, tool, state, context, actions, lang),
                    CellSpec::Field(field) => show_field(ui, field, state),
                }
            }
        });
        caption_band(ui, row.response.rect.width(), group.label.text(lang));
    });
}

/// The caption band under one group: the darker strip with the group's name in capitals.
fn caption_band(ui: &mut Ui, row_width: f32, caption: &str) {
    let galley = ui.fonts_mut(|fonts| {
        fonts.layout_no_wrap(
            caption.to_uppercase(),
            egui::FontId::proportional(icons::RIBBON_CAPTION_POINT_SIZE),
            egui::Color32::PLACEHOLDER,
        )
    });
    let width = row_width.max(galley.size().x);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, RIBBON_CAPTION_HEIGHT), egui::Sense::hover());
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, crate::theme::band(ui));
    let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, crate::theme::text_dim(ui));
}

/// One tool cell: its glyph, and the action a click asks the editor to run.
fn show_tool(
    ui: &mut Ui,
    tool: &'static ToolSpec,
    state: &mut Toolbars,
    context: RibbonContext,
    actions: &mut Vec<ToolAction>,
    lang: Lang,
) {
    let Some(kind) = tool.primary() else {
        return;
    };
    let Some(icon) = tool.icon else {
        return;
    };
    // The projection button draws the mode the view is *in*, so the glyph flips with the camera while
    // the hint names the switch.
    let icon = if kind == ToolKind::Projection { projection_icon(context.perspective) } else { icon };
    let ready = ready(kind, tool, state, context, lang);
    if icons::ribbon_cell(ui, RIBBON_CELL_WIDTH, icon, &ready.hint, ready.enabled, ready.armed).clicked()
        && let Some(action) = ready.action
    {
        actions.push(action);
    }
}

/// One numeric field: two cells wide, named by its hint, with the unit in the field itself.
fn show_field(ui: &mut Ui, field: &'static FieldSpec, state: &mut Toolbars) {
    let size = egui::vec2(FieldSpec::COLUMNS as f32 * RIBBON_CELL_WIDTH, RIBBON_ROW_HEIGHT);
    ui.allocate_ui_with_layout(size, egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
        let text = state.field_text_mut(field.kind);
        let edit = TextEdit::singleline(text).desired_width(size.x - 2.0 * crate::theme::STEP).hint_text(field.hint);
        ui.add(edit).on_hover_text(field.label.text(crate::language(ui.ctx())));
    });
}

/// What one ribbon cell draws and what a click on it runs.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ready {
    /// The tool can run here.
    enabled: bool,
    /// The button draws pressed: the tool owns the next click, or the toggle is on.
    armed: bool,
    /// The tooltip and the accessible name, which is also the sentence of a refusal.
    hint: String,
    /// The action a click asks for, unless the tool cannot run.
    action: Option<ToolAction>,
}

/// Decides one tool against the picture the ribbon was handed.
///
/// This is the one place where a row's [`ToolKind`] meets the session: everything the map says about
/// *where* a tool sits is data, and everything about *whether* it can run is here, next to the words
/// that say why not.
fn ready(kind: ToolKind, tool: &ToolSpec, state: &Toolbars, context: RibbonContext, lang: Lang) -> Ready {
    let RibbonContext { has_panel_material, selected, selected_path, active, owns_view, grid, draft, .. } = context;
    match kind {
        ToolKind::Draw(draw) => Ready {
            enabled: true,
            armed: is_armed(active, kind),
            hint: tool_hint(strip_label(draw).text(lang), shortcut_label(draw)),
            action: Some(ToolAction::Draw { tool: draw, sides: state.polygon_sides() }),
        },
        ToolKind::CloseDraft => {
            // Close needs a running draft that can close, cancel needs any running draft; both are
            // drawn so the pair never vanishes between clicks and the hint says why one is off.
            let (drawing, points, closed) = draft.unwrap_or((DrawTool::Line, 0, false));
            let close = draft_can_close(drawing, points, closed);
            let hint = if close { Message::DraftCloseHint.text(lang) } else { Message::DraftCloseNeedsTwo.text(lang) };
            Ready {
                enabled: close,
                armed: false,
                hint: hint.to_owned(),
                action: close.then_some(ToolAction::CloseDrawing),
            }
        }
        ToolKind::CancelDraft => {
            let running = draft.is_some();
            let hint = tool_hint(
                if running { Message::DraftCancelHint.text(lang) } else { Message::DraftNotRunning.text(lang) },
                Some("Esc"),
            );
            Ready { enabled: running, armed: false, hint, action: running.then_some(ToolAction::CancelDrawing) }
        }
        ToolKind::Primitive(primitive) => {
            let size = state.primitive_size();
            let armed = is_armed(active, ToolKind::Primitive(primitive));
            // The hint carries the field's refusal, so a bad size disables the tool and says why
            // instead of leaving the button silently off.
            let hint = match &size {
                Ok(_) => Message::PrimitiveClickPlane.with(lang, [tool.label.text(lang)]),
                Err(refusal) => refusal.text(lang),
            };
            let action = size.ok().map(|size| ToolAction::CreatePrimitive { kind: primitive, size });
            Ready { enabled: action.is_some(), armed, hint, action }
        }
        ToolKind::Extrude => {
            let height = state.extrude_height();
            // Only a closed path encloses a ring, so the button is off for anything else and the
            // hint names what is missing: a selection, a path, a closed chain, or a usable height.
            let hint = match &height {
                Ok(_) => extrude_hint(selected, selected_path).text(lang).to_owned(),
                Err(refusal) => refusal.text(lang),
            };
            let action = match (selected, height) {
                (Some(object), Ok(height)) => Some(ToolAction::ExtrudePath { object, height }),
                _ => None,
            };
            let enabled = matches!((selected, selected_path), (Some(_), Some(true))) && action.is_some();
            Ready { enabled, armed: false, hint, action }
        }
        ToolKind::Panel => Ready {
            enabled: has_panel_material,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreatePanel),
        },
        ToolKind::Assembly => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateAssembly),
        },
        ToolKind::Camera => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateCamera),
        },
        ToolKind::Light => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::CreateLight),
        },
        ToolKind::FrameAll => Ready {
            enabled: true,
            armed: false,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: Some(ToolAction::FrameAll),
        },
        ToolKind::Projection => Ready {
            enabled: owns_view,
            armed: false,
            hint: tool_hint(Message::SwitchProjection.text(lang), tool.shortcut),
            action: owns_view.then_some(ToolAction::ToggleProjection),
        },
        ToolKind::GridSnap => Ready {
            enabled: owns_view,
            armed: grid,
            hint: tool_hint(tool.label.text(lang), tool.shortcut),
            action: owns_view.then_some(ToolAction::ToggleGrid),
        },
    }
}

/// Whether a running draft can be closed into a ring: only a tool that closes, and only once the
/// chain has the two segments a ring needs, and only while it is still open.
///
/// Two segments are the whole requirement: a straight close adds the third, and a pending arc *is*
/// the closing segment (ADR 0077). This is the rule `DrawDraft::can_close` applies, so the button
/// and the first-node highlight never disagree with the command that runs.
pub const fn draft_can_close(tool: DrawTool, points: usize, closed: bool) -> bool {
    tool.closes() && points >= 2 && !closed
}

/// The icon of the projection toggle: the mode the view is in.
fn projection_icon(perspective: bool) -> Icon {
    if perspective { icons::view::PERSPECTIVE } else { icons::view::ORTHOGRAPHIC }
}

/// The hint of the extrude button: what the tool does, or why it is off.
///
/// `selected_path` is `None` when the selection is not a path object, so a path that is merely
/// open gets its own message instead of being called "not a path".
fn extrude_hint(selected: Option<Id>, selected_path: Option<bool>) -> Message {
    match (selected, selected_path) {
        (None, _) => Message::ExtrudeSelectPath,
        (Some(_), Some(true)) => Message::ExtrudeSelectedPath,
        (Some(_), Some(false)) => Message::ExtrudePathOpen,
        (Some(_), None) => Message::ExtrudeNotPath,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::toolbars::spec::{self, CellSpec, TabSpec};

    /// A context where every live tool can run and the viewport owns its view.
    fn full_context() -> RibbonContext {
        RibbonContext {
            has_panel_material: true,
            selected: None,
            selected_path: None,
            active: None,
            perspective: true,
            owns_view: true,
            grid: true,
            draft: None,
        }
    }

    /// A tab that holds only planned tools, so the ribbon's own rules can be tested without editing
    /// the approved map.
    static PLANNED_ONLY: TabSpec = TabSpec {
        id: "test",
        label: Message::RibbonScene,
        groups: &[GroupSpec { id: "test.group", label: Message::GroupCreate, cells: &[] }],
    };

    /// The tab `id`, as the map declares it.
    fn tab(id: &str) -> &'static TabSpec {
        spec::tab(id).expect("the map declares this tab")
    }

    /// Runs the ribbon with `context` and returns its text shapes and accessible labels.
    fn draw(state: &mut Toolbars, context: RibbonContext) -> (Vec<String>, Vec<String>, Vec<ToolAction>) {
        draw_in(state, context, Lang::En)
    }

    /// Runs the ribbon in one language, so a switch can be compared without a document.
    fn draw_in(
        state: &mut Toolbars,
        context: RibbonContext,
        lang: Lang,
    ) -> (Vec<String>, Vec<String>, Vec<ToolAction>) {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::set_language(&ctx, lang);
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, state, context, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        (texts, labels, actions)
    }

    /// The hints of the tools a tab draws, with its own tab buttons filtered out.
    fn tool_hints(state: &mut Toolbars, context: RibbonContext) -> Vec<String> {
        let (_, labels, _) = draw(state, context);
        let tabs = spec::tabs().iter().map(|tab| tab.label.text(Lang::En)).collect::<Vec<_>>();
        let mut hints = labels.into_iter().filter(|label| !tabs.iter().any(|tab| tab == label)).collect::<Vec<_>>();
        hints.sort();
        hints
    }

    #[test]
    fn the_ribbon_starts_open_on_a_tab_that_draws_a_row() {
        let mut state = Toolbars::new();
        assert!(state.is_open(), "a fresh ribbon shows its body");
        assert_eq!(state.tab().id, "sketch", "a fresh ribbon opens on the first drawn tab");
        assert!(!state.toggle());
        assert!(!state.is_open());
        assert!(state.toggle());
        assert!(state.is_open());
        assert!(!state.set_open(false));
        assert!(!state.is_open());
    }

    #[test]
    fn every_tab_draws_its_live_tools_and_nothing_else() {
        // The map is the contract: these are the hints of the live tools of each tab, and a planned
        // row must not reach the ribbon at all.
        let expected: [(&str, &[&str]); 5] = [
            (
                "sketch",
                &[
                    "A draft needs two segments to close",
                    "Arc  (A)",
                    "Circle  (C)",
                    "Line  (L)",
                    "No draft is running  (Esc)",
                    "Polygon  (N)",
                    "Polyline · Tab line/arc · Alt ignores snaps  (P)",
                    "Rectangle  (R)",
                    "Sides",
                ],
            ),
            (
                "model",
                &[
                    "Cube: click the work plane",
                    "Cylinder: click the work plane",
                    "Height",
                    "Select a closed path first",
                    "Size",
                    "Sphere: click the work plane",
                ],
            ),
            ("furniture", &["Panel"]),
            ("scene", &["Assembly", "Camera", "Light"]),
            ("view", &["Frame all  (F)", "Grid snap  (G)", "Switch the projection  (O)"]),
        ];
        for (id, hints) in expected {
            let mut state = Toolbars::new();
            state.set_tab(tab(id));
            let drawn = tool_hints(&mut state, full_context());
            let mut wanted = hints.to_vec();
            wanted.sort_unstable();
            assert_eq!(drawn, wanted, "the {id} tab draws a different set of tools");
        }
    }

    #[test]
    fn a_group_of_planned_tools_is_skipped_with_its_caption() {
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        let (texts, _, _) = draw(&mut state, full_context());
        for caption in ["СОЗДАТЬ", "ЧЕРНОВИК"] {
            let caption = if caption == "СОЗДАТЬ" { "CREATE" } else { "DRAFT" };
            assert!(texts.iter().any(|text| text == caption), "{caption} names a live group: {texts:?}");
        }
        for caption in ["MODIFY", "CONSTRUCTION", "DIMENSIONS", "BOOLEANS"] {
            assert!(!texts.iter().any(|text| text == caption), "{caption} holds no live tool: {texts:?}");
        }
    }

    #[test]
    fn a_tab_with_nothing_live_is_not_drawn_and_is_never_active() {
        assert!(!PLANNED_ONLY.is_drawn());
        assert_eq!(PLANNED_ONLY.columns(), 0);
        let mut state = Toolbars::new();
        state.set_tab(&PLANNED_ONLY);
        let (texts, _, _) = draw(&mut state, full_context());
        assert_eq!(state.tab().id, "sketch", "the ribbon fell back to the first drawn tab");
        assert!(!texts.iter().any(|text| text == PLANNED_ONLY.label.text(Lang::En) && text == "Scene" && false));
    }

    #[test]
    fn the_body_is_one_tool_row_and_one_caption_band() {
        // The approved body is the 30 pt tool row and the 12 pt caption band: a row that grows an
        // extra field band is a regression (U68).
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        let ctx = egui::Context::default();
        let mut drawn = 0.0;
        let output = ctx.run_ui(Default::default(), |ui| {
            let before = ui.min_rect().bottom();
            show(ui, &mut state, full_context(), &mut Vec::new());
            drawn = ui.min_rect().bottom() - before;
        });
        output.drop_without_applying_deltas();
        assert!((drawn - self::height(true)).abs() < 0.01, "the open ribbon draws {drawn} pt");
        assert!((drawn - 72.0).abs() < 0.01, "the approved open ribbon is 72 pt, not {drawn}");
    }

    #[test]
    fn the_reserved_height_is_the_ribbon_it_draws() {
        // The shell reserves `height(open)` for the floating ribbon and the strip draws inside it;
        // a mismatch would leave a side panel header under the strip or a gap above it (U51).
        for open in [false, true] {
            let mut state = Toolbars::new();
            state.set_open(open);
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut height = 0.0;
            let output = ctx.run_ui(Default::default(), |ui| {
                let before = ui.min_rect().bottom();
                show(ui, &mut state, full_context(), &mut Vec::new());
                height = ui.min_rect().bottom() - before;
            });
            output.drop_without_applying_deltas();
            let expected = self::height(open) - 2.0 * f32::from(RIBBON_FRAME_MARGIN[1]);
            assert!(
                (height - expected).abs() < 0.01,
                "the ribbon draws {height} pt inside its frame but reserves {expected} pt (open: {open})"
            );
        }
        assert_eq!(height(false), 29.0, "the folded ribbon is its tab row and one hairline");
        assert_eq!(height(true), 72.0, "the open ribbon is the tab row, the tool row and the band");
    }

    #[test]
    fn collapsing_keeps_the_tab_row_and_drops_the_body() {
        let mut state = Toolbars::new();
        state.set_tab(tab("scene"));
        let (open_texts, _, _) = draw(&mut state, full_context());
        assert!(open_texts.iter().any(|text| text == "OBJECTS"), "the group caption is drawn: {open_texts:?}");
        state.set_open(false);
        let (closed_texts, _, _) = draw(&mut state, full_context());
        assert!(closed_texts.iter().any(|text| text == "Scene"), "the tab row survives: {closed_texts:?}");
        assert!(!closed_texts.iter().any(|text| text == "OBJECTS"), "no caption without a body: {closed_texts:?}");
    }

    #[test]
    fn clicking_the_open_tab_collapses_and_restores_it() {
        let mut state = Toolbars::new();
        assert_eq!(state.tab().id, "sketch");
        apply_tab_gesture(&mut state, tab("sketch"), false);
        assert!(!state.is_open(), "a click on the open tab collapses the body");
        apply_tab_gesture(&mut state, tab("sketch"), false);
        assert!(state.is_open(), "the same click restores it");
    }

    #[test]
    fn clicking_another_tab_opens_it_and_a_double_click_collapses_it() {
        let mut state = Toolbars::new();
        apply_tab_gesture(&mut state, tab("model"), false);
        assert_eq!(state.tab().id, "model");
        assert!(state.is_open(), "switching tabs shows the new body");
        apply_tab_gesture(&mut state, tab("model"), true);
        assert!(!state.is_open(), "a double-click collapses the body");
        apply_tab_gesture(&mut state, tab("model"), true);
        assert!(state.is_open(), "a double-click restores it");
        apply_tab_gesture(&mut state, tab("scene"), true);
        assert_eq!(state.tab().id, "scene", "a double-click also switches tabs");
        assert!(!state.is_open());
    }

    #[test]
    fn only_the_arming_tool_owns_the_next_click() {
        let cases = [
            (None, None, None),
            (Some(DrawTool::Line), None, Some(ToolActivity::Draw(DrawTool::Line))),
            (None, Some(PrimitiveKind::Cube), Some(ToolActivity::Primitive(PrimitiveKind::Cube))),
            (Some(DrawTool::Arc), Some(PrimitiveKind::Sphere), Some(ToolActivity::Draw(DrawTool::Arc))),
        ];
        for (drawing, primitive, expected) in cases {
            assert_eq!(tool_activity(drawing, primitive), expected, "drawing={drawing:?} primitive={primitive:?}");
        }
    }

    #[test]
    fn a_button_is_pressed_only_for_its_own_activity() {
        let cases = [
            (None, ToolKind::Draw(DrawTool::Line), false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolKind::Draw(DrawTool::Line), true),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolKind::Draw(DrawTool::Circle), false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolKind::Panel, false),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), ToolKind::Primitive(PrimitiveKind::Cube), true),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), ToolKind::Primitive(PrimitiveKind::Sphere), false),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), ToolKind::Light, false),
        ];
        for (activity, kind, expected) in cases {
            assert_eq!(is_armed(activity, kind), expected, "activity={activity:?} kind={kind:?}");
        }
    }

    #[test]
    fn the_ribbon_marks_exactly_the_armed_tool() {
        let cases = [
            (None, "sketch", 0, "an idle ribbon presses nothing"),
            (Some(ToolActivity::Draw(DrawTool::Line)), "sketch", 1, "the armed Line button is pressed"),
            (Some(ToolActivity::Primitive(PrimitiveKind::Sphere)), "model", 1, "the armed Sphere button is pressed"),
        ];
        for (activity, id, expected, reason) in cases {
            let ctx = egui::Context::default();
            let mut state = Toolbars::new();
            state.set_tab(tab(id));
            let mut actions = Vec::new();
            let mut pressed_fill = None;
            let context = RibbonContext { active: activity, ..full_context() };
            let output = ctx.run_ui(Default::default(), |ui| {
                pressed_fill = Some(ui.visuals().selection.bg_fill);
                show(ui, &mut state, context, &mut actions);
            });
            let pressed_fill = pressed_fill.expect("the ribbon ran");
            let pressed = drawn_fill(&output.shapes, pressed_fill);
            output.drop_without_applying_deltas();
            assert_eq!(pressed, expected, "{reason}");
        }
    }

    #[test]
    fn the_bodies_group_only_offers_extrude_for_a_closed_path() {
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        let (_, labels, _) = draw(&mut state, full_context());
        assert!(labels.iter().any(|label| label.contains("Select a closed path first")));
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(100).expect("height")));
    }

    #[test]
    fn a_cell_paints_one_icon_and_keeps_its_size() {
        // A ribbon cell is a glyph in a fixed cell: one image, 22 pt of artwork, 34 x 30 of cell
        // (ADR 0067 revised by ADR 0091).
        let icon = icons::contour::LINE;
        assert!(icon.is_drawn(), "the Line asset is drawn, so the test exercises the image path");
        let ctx = egui::Context::default();
        let mut size = None;
        let output = ctx.run_ui(Default::default(), |ui| {
            let response = icons::ribbon_cell(ui, RIBBON_CELL_WIDTH, icon, "Line  (L)", true, false);
            size = Some(response.rect.size());
        });
        let images = count_images(&output.shapes);
        let texts = drawn_text(&output.shapes);
        output.drop_without_applying_deltas();
        assert_eq!(size, Some(egui::vec2(RIBBON_CELL_WIDTH, RIBBON_ROW_HEIGHT)), "the cell is the metric");
        assert_eq!(images, 1, "the icon is painted as one image");
        assert!(texts.is_empty(), "a cell carries no caption: {texts:?}");
    }

    #[test]
    fn the_extrude_hint_names_what_is_missing() {
        let node = Id::new(7).expect("id");
        assert_eq!(extrude_hint(None, None).text(Lang::En), "Select a closed path first");
        assert_eq!(extrude_hint(None, Some(true)).text(Lang::En), "Select a closed path first");
        assert_eq!(extrude_hint(Some(node), None).text(Lang::En), "The selected object is not a path");
        assert_eq!(extrude_hint(Some(node), Some(false)).text(Lang::En), "The selected path is open; close it first");
        assert_eq!(extrude_hint(Some(node), Some(true)).text(Lang::En), "Extrude the selected path");
    }

    #[test]
    fn an_open_path_leaves_the_extrude_button_off() {
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        let node = Id::new(7).expect("id");
        let context = RibbonContext { selected: Some(node), selected_path: Some(false), ..full_context() };
        let (_, labels, actions) = draw(&mut state, context);
        assert!(
            labels.iter().any(|label| label.contains("The selected path is open; close it first")),
            "the ribbon says why an open path cannot be extruded: {labels:?}"
        );
        assert!(actions.is_empty(), "a disabled button asks for no action");
    }

    #[test]
    fn a_closed_path_offers_one_extrusion_at_the_typed_height() {
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        state.set_extrude_height_text("24");
        let node = Id::new(7).expect("id");
        let context = RibbonContext { selected: Some(node), selected_path: Some(true), ..full_context() };
        let (_, labels, actions) = draw(&mut state, context);
        assert!(labels.iter().any(|label| label.contains("Extrude the selected path")));
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(24).expect("height")));
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn a_blank_extrusion_height_is_refused_in_the_group() {
        let mut state = Toolbars::new();
        state.set_extrude_height_text("   ");
        assert!(state.extrude_height().is_err());
        state.set_extrude_height_text("0");
        assert!(state.extrude_height().is_err());
        state.set_extrude_height_text("12");
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(12).expect("height")));
    }

    #[test]
    fn the_height_field_stops_where_the_command_does() {
        let mut state = Toolbars::new();
        // The document's contour limit is 10 m; the field must not accept a height the command
        // would refuse, or the group would offer a tool that always fails.
        let max = furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() / 1_000;
        state.set_extrude_height_text(max.to_string());
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(max).expect("height")));
        state.set_extrude_height_text((max + 1).to_string());
        assert!(state.extrude_height().is_err(), "one millimetre past the limit is refused");
    }

    #[test]
    fn a_refusal_reads_in_the_workspace_language() {
        let refusal = FieldRefusal::HeightTooLong { max_mm: 10_000 };
        assert_eq!(refusal.text(Lang::En), "The extrusion height must be at most 10000 mm.");
        assert_eq!(refusal.text(Lang::Ru), "Высота выдавливания должна быть не больше 10000 мм.");
        assert_eq!(
            FieldRefusal::SizeNotPositive.text(Lang::En),
            "The primitive size must be positive.",
            "a refusal is a whole sentence, not half of one"
        );
    }

    #[test]
    fn the_three_primitive_buttons_are_filled() {
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        let (_, labels, actions) = draw(&mut state, full_context());
        for label in ["Cube: click the work plane", "Cylinder: click the work plane", "Sphere: click the work plane"] {
            assert!(labels.iter().any(|text| text.contains(label)), "{label} is missing: {labels:?}");
        }
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn a_panel_without_a_catalogue_material_cannot_create_a_panel() {
        let mut state = Toolbars::new();
        state.set_tab(tab("furniture"));
        let context = RibbonContext { has_panel_material: false, ..full_context() };
        let (_, _, actions) = draw(&mut state, context);
        assert!(actions.is_empty(), "the Panel tool is disabled without a material");
    }

    #[test]
    fn every_tool_has_a_distinct_shortcut() {
        // A shifted letter is a different shortcut from the bare one, so the table is unique on
        // the pair and not on the key alone.
        let mut seen = std::collections::HashSet::new();
        for tool in CONTOUR_TOOLS {
            let key = shortcut(tool).expect("every drawing tool has a key");
            let name = match key.shift {
                true => format!("Shift+{}", key.key.name()),
                false => key.key.name().to_owned(),
            };
            assert!(seen.insert(name.clone()), "{name} is reused by {}", tool.label());
            assert_eq!(shortcut_label(tool), Some(name.as_str()));
        }
        assert_eq!(shortcut(DrawTool::Line), Some(Shortcut { key: egui::Key::L, shift: false }));
        assert_eq!(shortcut(DrawTool::Rectangle), Some(Shortcut { key: egui::Key::R, shift: false }));
        assert_eq!(shortcut(DrawTool::RectangleFromCenter), Some(Shortcut { key: egui::Key::R, shift: true }));
        assert_eq!(shortcut(DrawTool::RegularPolygon), Some(Shortcut { key: egui::Key::N, shift: false }));
    }

    #[test]
    fn the_draft_group_offers_close_only_for_a_closable_draft() {
        assert!(draft_can_close(DrawTool::Polyline, 2, false), "two segments and the closing one make a ring");
        assert!(!draft_can_close(DrawTool::Polyline, 1, false), "one segment cannot close");
        assert!(!draft_can_close(DrawTool::Polyline, 4, true), "a closed ring cannot close again");
        assert!(!draft_can_close(DrawTool::Line, 2, false), "a line never closes");

        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        // A pending arc is a segment the draft has drawn, so a chain of two lines plus the arc can
        // close; the ribbon cannot know it is pending, so two segments is the whole test.
        let context = RibbonContext { draft: Some((DrawTool::Polyline, 2, false)), ..full_context() };
        let (_, labels, _) = draw(&mut state, context);
        assert!(labels.iter().any(|label| label.contains("Close the draft into a ring")), "the draft group is drawn");
        assert!(labels.iter().any(|label| label.contains("Cancel the draft")), "a draft can always cancel");
    }

    #[test]
    fn a_group_divider_spans_the_whole_body() {
        // The owner's complaint: the divider stopped in the middle of the tool row, so a group
        // boundary looked like a stray tick. It is as tall as the tool row plus its caption band.
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let body = RIBBON_ROW_HEIGHT + RIBBON_CAPTION_HEIGHT;
        let mut dividers = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, full_context(), &mut Vec::new());
        });
        collect_vertical_lines(&output.shapes, &mut dividers);
        output.drop_without_applying_deltas();
        assert!(!dividers.is_empty(), "the Sketch tab separates its two live groups");
        for height in dividers {
            assert!((height - body).abs() < 0.5, "a group divider is {height} pt tall, the body is {body} pt");
        }
    }

    #[test]
    fn every_live_tool_draws_exactly_one_cell() {
        for group in spec::tabs().iter().flat_map(TabSpec::live_groups) {
            let tools = group.tools().count();
            let fields = group.cells.iter().filter(|cell| matches!(cell, CellSpec::Field(_)) && cell.is_live()).count();
            assert_eq!(group.columns(), tools + fields * FieldSpec::COLUMNS, "{} miscounts its row", group.id);
        }
    }

    /// Counts the painted rects filled with `fill`, the selection fill of a pressed button.
    fn drawn_fill(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> usize {
        fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut usize) {
            match shape {
                egui::Shape::Rect(rect) if rect.fill == fill => *out += 1,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, fill, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = 0;
        for clipped in shapes {
            walk(&clipped.shape, fill, &mut out);
        }
        out
    }

    /// Counts painted images, the icon of a tool cell.
    fn count_images(shapes: &[egui::epaint::ClippedShape]) -> usize {
        fn walk(shape: &egui::Shape, out: &mut usize) {
            match shape {
                egui::Shape::Mesh(_) => *out += 1,
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = 0;
        for clipped in shapes {
            walk(&clipped.shape, &mut out);
        }
        out
    }

    /// Collects the height of every painted vertical line segment, a divider or a `vline`.
    fn collect_vertical_lines(shapes: &[egui::epaint::ClippedShape], out: &mut Vec<f32>) {
        fn walk(shape: &egui::Shape, out: &mut Vec<f32>) {
            match shape {
                egui::Shape::LineSegment { points, .. } => {
                    if (points[0].x - points[1].x).abs() < 0.5 {
                        out.push((points[0].y - points[1].y).abs());
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        for clipped in shapes {
            walk(&clipped.shape, out);
        }
    }

    /// The accessible labels this frame published: a drawn icon carries its hint here, not as text.
    fn accessible_labels(output: &egui::FullOutput) -> Vec<String> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| update.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

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

    #[test]
    fn switching_the_language_redraws_the_ribbon() {
        let mut state = Toolbars::new();
        let (english, _, _) = draw_in(&mut state, full_context(), Lang::En);
        let (russian, _, _) = draw_in(&mut state, full_context(), Lang::Ru);
        assert!(!english.iter().any(|text| text == "СОЗДАТЬ"));
        assert!(russian.iter().any(|text| text == "СОЗДАТЬ"), "the Russian group caption is drawn: {russian:?}");
        assert_ne!(english, russian, "the same ribbon draws different words");

        let mut state = Toolbars::new();
        let (_, english, _) = draw_in(&mut state, full_context(), Lang::En);
        let mut state = Toolbars::new();
        let (_, russian, _) = draw_in(&mut state, full_context(), Lang::Ru);
        assert!(english.iter().any(|hint| hint.contains("Line  (L)")));
        assert!(russian.iter().any(|hint| hint.contains("Линия  (L)")), "the hint is translated: {russian:?}");
    }
}
