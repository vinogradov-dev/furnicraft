//! The ribbon: one row of tabs over the tools of the active tab.
//!
//! The map — five tabs that name a domain (*Sketch, Model, Furniture, Scene, View*), the groups of
//! operations under them and the tools inside them — is one const table in [`spec`]. This module
//! draws it with one loop: a tab is drawn when it owns a live group, a group when it owns a live
//! tool, and a planned tool (no command yet) is skipped, so the ribbon never shows a greyed "coming
//! soon" row and a tab with nothing to run is not drawn at all.
//!
//! Whether a tool can run *here* is code, not data: [`ready::ready`] decides it against the
//! [`RibbonContext`] — a catalogue material, a closed path, a running draft, a viewport that owns its
//! projection — and returns the hint that says what is missing, so a disabled cell still explains
//! itself. Whether the body is open, which tab is active, the typed defaults it remembers, which
//! method each tool is pinned to and which list is open is view state in [`state::Toolbars`]: never
//! document data; the fold and the pin are also workspace settings the shell remembers.
//!
//! The ribbon holds tools only (U76): a number a tool needs is a typed session default, and the
//! object card or the operation's own command bar is where the user edits it, so no cell is a form.
//!
//! This module is the drawing: [`show`] and the row, cell and list wiring. The map is [`spec`], the
//! state is [`state`], the readiness table is [`ready`] and the popup under a cell is [`list`].
//!
//! A cell is one glyph in a [`RIBBON_CELL_WIDTH`] by [`RIBBON_ROW_HEIGHT`] cell, so a row is one pitch
//! and no tool sizes itself from its name (ADR 0067, revised by ADR 0091). The group's caption sits in
//! the quieter band under the row, which is what makes the whole body 76 points tall and the folded
//! ribbon its 29-point tab row.
//!
//! Every button is the icon-only [`icons::ribbon_cell`]: the ribbon names a tool through its hint and
//! through the list under the cell a click opens ([`list`], U67); the panel headers and the floating
//! viewport cluster keep the icon-only [`icons::tool_button`], whose row could not carry a second
//! line.
//!
//! A tool with more than one input method is a split button (U74): [`icons::ribbon_split_cell`] gives
//! it a main face that is a whole cell and runs the method the workspace pinned, plus a caret face
//! added beside it that opens its variant list, while a tool with one method keeps the whole cell and
//! runs on the click. The pin is a workspace setting, the open list is view state, and the key of the
//! tool runs the pinned method without opening anything.

use std::collections::BTreeMap;

use egui::Ui;
use furnicraft_engine::PrimitiveKind;
use furnicraft_model::{ClassCode, FillMode, Id};
use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use self::spec::{GroupSpec, TabSpec, ToolKind, ToolSpec};
use crate::box_tool::BoxOwner;
use crate::draw::DrawTool;
use crate::icons::{self, Icon};
use crate::theme::{RIBBON_CAPTION_HEIGHT, RIBBON_CELL_WIDTH, RIBBON_ROW_HEIGHT, RIBBON_RULE, RIBBON_TAB_HEIGHT};

pub mod list;
pub mod ready;
pub mod spec;
pub mod state;

pub use self::ready::draft_can_close;
pub use self::state::Toolbars;

use self::ready::{projection_icon, ready};

/// Full extent of a new primitive, in millimetres; the size a cylinder or a sphere starts at and
/// the equal-sided box a one-extent creation builds.
pub const DEFAULT_PRIMITIVE_SIZE_MM: i64 = 200;

/// Height of a new extrusion, in millimetres; also the height the next extrusion starts at.
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
    /// Opens the product-frame card; the card dispatches `product.create`.
    CreateProductCard,
    /// Arms the box tool with the frame as its owner: the next clicks draw the reserved box and the
    /// ✓ dispatches `product.create`.
    DrawProductFrame,
    /// Creates a panel at the scene root.
    CreatePanel,
    /// Arms the empty-space tool with the class whose `fill` trait picks the axis pair.
    Fill {
        /// The class a post or a shelf fills the void with.
        class: ClassCode,
        /// How the part reads the section the walls leave.
        mode: FillMode,
    },
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
/// Only the arming tools have a state to show: a drawing tool holds the pointer until the chain is
/// committed, a primitive waits for its work-plane click and a box waits for its base rectangle. A
/// one-shot creation — a panel, an assembly, a camera or a light — runs and forgets, so it is never
/// armed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolActivity {
    /// A drawing tool is building a chain.
    Draw(DrawTool),
    /// A primitive is waiting for its work-plane click.
    Primitive(PrimitiveKind),
    /// The box tool is drawing a product frame's reserved box.
    ProductFrame,
    /// The empty-space tool is armed with a fill class and its mode.
    Fill {
        /// The class the armed cell fills the void with.
        class: ClassCode,
        /// How the armed cell reads the section.
        mode: FillMode,
    },
}

/// The activity whose button is pressed, from what the editor holds.
///
/// The arguments are the sources of an armed tool, not a decision: a running draft names its tool, a
/// pending primitive names its kind, a box draft names the field it will write and a fill arm names
/// its class. None outranks another because the editor never holds two at once.
pub fn tool_activity(
    drawing: Option<DrawTool>,
    primitive: Option<PrimitiveKind>,
    box_owner: Option<BoxOwner>,
    fill: Option<(ClassCode, FillMode)>,
) -> Option<ToolActivity> {
    match (drawing, primitive, box_owner, fill) {
        (Some(tool), _, _, _) => Some(ToolActivity::Draw(tool)),
        (None, Some(kind), _, _) => Some(ToolActivity::Primitive(kind)),
        (None, None, Some(BoxOwner::Cuboid), _) => Some(ToolActivity::Primitive(PrimitiveKind::Cuboid)),
        (None, None, Some(BoxOwner::Product), _) => Some(ToolActivity::ProductFrame),
        (None, None, None, Some((class, mode))) => Some(ToolActivity::Fill { class, mode }),
        (None, None, None, None) => None,
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
        // The frame cell runs the card as its primary method, so the drawn method's row carries the
        // pressed state and the cell shows it through the list (U74).
        (Some(ToolActivity::ProductFrame), ToolKind::ProductDraw) => true,
        (Some(ToolActivity::Fill { class: armed, mode: armed_mode }), ToolKind::Fill { class, mode }) => {
            armed == class && armed_mode == mode
        }
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
    /// The *Product frame* cell asked for its card.
    ///
    /// The card belongs to the window, like Settings and the File dialogs, so the ribbon reports
    /// the request and the shell draws it (ADR 0100).
    pub open_product_card: bool,
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
    /// A product is open, so a niche is offered (ADR 0098).
    pub product_open: bool,
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
        state.retain_list();
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
/// is the approved 29/76 metric itself and every row of it is a control row.
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
            for tool in group.live_tools() {
                show_tool(ui, tool, state, context, actions, lang);
            }
        });
        caption_band(ui, row.response.rect.width(), group.label.text(lang));
    });
}

/// The caption band under one group: the quiet strip with the group's name in capitals.
///
/// The band is [`crate::theme::caption_band`], one step from a control toward the chrome: the darkest
/// surface alone read as a hole under every group (U75). The name is centred in the band, which is
/// tall enough at [`RIBBON_CAPTION_HEIGHT`] to keep air above and below the capitals.
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
    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, crate::theme::caption_band(ui));
    let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - capitals_centre(&galley));
    ui.painter().galley(pos, galley, crate::theme::text_dim(ui));
}

/// The middle of a caption's capitals, measured from the top of the galley.
///
/// The line box of a galley carries the descent, which capitals never use, so centring the box leaves
/// the name two points high in the band (U75). Centring the block between the tallest glyph and the
/// baseline puts every caption of the strip on one baseline — a `Д` leg or a `Ц` tail hangs as type
/// does — instead of lifting the words that have one. A layout with no glyph falls back to the box.
fn capitals_centre(galley: &egui::Galley) -> f32 {
    let baseline = galley.rows.first().and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y));
    match baseline {
        Some(baseline) => (galley.mesh_bounds.top() + baseline) / 2.0,
        None => galley.size().y / 2.0,
    }
}

/// One tool cell: its glyph, and the action a click asks the editor to run.
///
/// A tool with more than one input method is a split button (U74): its main face is a full cell that
/// runs the method the workspace pinned and names it in the hint, and a caret face beside it opens
/// the list of the other methods, so the row keeps one glyph pitch and one glyph size whether a tool
/// has a choice or not (X36). A tool with one method keeps the whole cell and runs on the click, so a
/// tool that has no choice carries no caret.
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
    if !tool.has_list() {
        // The projection button draws the mode the view is *in*, so the glyph flips with the camera
        // while the hint names the switch.
        let icon = if kind == ToolKind::Projection { projection_icon(context.perspective) } else { icon };
        let ready = ready(kind, tool, state, context, lang);
        let response = icons::ribbon_cell(ui, RIBBON_CELL_WIDTH, icon, &ready.hint, ready.enabled, ready.armed);
        if response.clicked()
            && let Some(action) = ready.action
        {
            actions.push(action);
        }
        return;
    }
    let rows = method_rows(tool, state, context, lang);
    let pinned = state.pinned_index(tool);
    // The armed method is what the cell shows: picking a mode from the list must change the face
    // and the pressed state, not just the ghost, or the choice looks like it did not take (ADR 0110).
    let shown = rows.iter().position(|row| row.armed).unwrap_or(pinned);
    let Some(face) = main_face(tool, &rows, shown) else {
        return;
    };
    let caret_hint = Message::ToolMethods.text(lang);
    let cell =
        icons::ribbon_split_cell(ui, RIBBON_CELL_WIDTH, face.icon, &face.hint, caret_hint, face.enabled, face.armed);
    if face.enabled && cell.main.clicked() {
        if let Some(action) = face.action {
            actions.push(action);
        }
        // The main face runs its method whether the list is open or not; an open list goes away with
        // the run, because the click answered the question the list was asking.
        state.close_list();
    }
    if cell.caret.clicked() {
        state.toggle_list(tool);
    }
    if state.list_open_for(tool) {
        let choice = list::show(ui, &mut state.list, tool, &rows, &cell.main, pinned, lang);
        apply_choice(state, tool, &rows, choice, actions);
    }
}

/// What the main face of a split cell draws and runs.
///
/// It is the pinned method, not the tool: the glyph is the method's artwork when it has any and the
/// tool's otherwise, because a method that shares its tool's shape must not leave the face blank,
/// and the hint is the row's own, so a method that cannot run here still says what is missing.
struct MainFace {
    /// The glyph of the face.
    icon: Icon,
    /// The hint, which names the method and its key rather than the tool.
    hint: String,
    /// Whether the method can run here.
    enabled: bool,
    /// Whether the method owns the next click of the viewport.
    armed: bool,
    /// What a click asks the editor to run.
    action: Option<ToolAction>,
}

/// The main face of a split cell: the row of the pinned or armed method, read as one cell.
///
/// A fill cell always draws the tool's own glyph: the two rows are two builds of one panel, and the
/// cell must keep saying «Post» or «Shelf» as the mode changes (ADR 0110). Any other method draws
/// its own artwork, falling back to the tool's when it shares it.
fn main_face(tool: &'static ToolSpec, rows: &[list::Row], index: usize) -> Option<MainFace> {
    let row = rows.get(index)?;
    let icon = match row.method.run {
        Some(ToolKind::Fill { .. }) => tool.icon?,
        _ => row.method.icon.or(tool.icon)?,
    };
    Some(MainFace { icon, hint: row.hint.clone(), enabled: row.enabled, armed: row.armed, action: row.action })
}
/// The rows of one tool's list: every method the map declares, in order, planned ones included.
///
/// A planned method stays in the list as a disabled row, unlike a planned tool, which the ribbon
/// skips: the map has room to say «a circle can also be drawn tangent to two lines» before the
/// command exists, and the row says so without pretending to run (U67).
fn method_rows(tool: &'static ToolSpec, state: &Toolbars, context: RibbonContext, lang: Lang) -> Vec<list::Row> {
    tool.methods()
        .map(|method| {
            let ready = method.run.map(|kind| ready(kind, tool, state, context, lang));
            let hint = match ready.as_ref() {
                None => method.label.text(lang).to_owned(),
                // A method that cannot run here says what is missing.
                Some(ready) if !ready.enabled => ready.hint.clone(),
                // A fill's own hint names the tool and then the method — «Post: Largest rectangle» —
                // because the two fills are two cells whose methods would otherwise read the same.
                Some(ready) if matches!(method.run, Some(ToolKind::Fill { .. })) => ready.hint.clone(),
                // Every other tool names the method alone: the glyph already says which tool it is.
                Some(_) => icons::tool_hint(method.label.text(lang), method.shortcut),
            };
            list::Row {
                method,
                hint,
                enabled: ready.as_ref().is_some_and(|ready| ready.enabled),
                armed: ready.as_ref().is_some_and(|ready| ready.armed),
                action: ready.and_then(|ready| ready.action),
            }
        })
        .collect()
}

/// Applies what one open list asked for: run a method, pin it, or nothing.
fn apply_choice(
    state: &mut Toolbars,
    tool: &'static ToolSpec,
    rows: &[list::Row],
    choice: Option<list::ListChoice>,
    actions: &mut Vec<ToolAction>,
) {
    match choice {
        Some(list::ListChoice::Run(index)) => {
            if let Some(action) = rows.get(index).and_then(|row| row.action) {
                actions.push(action);
            }
            state.close_list();
        }
        Some(list::ListChoice::Pin(index)) => {
            if let Some(row) = rows.get(index) {
                state.pin(tool, row.method);
            }
        }
        None => {}
    }
}

/// The drawing tool a key press runs.
///
/// A key that names a tool's own method runs the method the workspace pinned for that tool, without
/// opening the list; a key that names one of the other methods (`Shift+C` for a circle through three
/// points) still runs exactly that method. The map is the only source: a tool whose pinned id is
/// unknown in this build falls back to its first method.
pub fn shortcut_method(tool: DrawTool, pinned: &BTreeMap<String, String>) -> DrawTool {
    let Some(spec) = tool_of_default(tool) else { return tool };
    let index = spec.pinned_index(pinned);
    match spec.methods().nth(index).and_then(|method| method.run) {
        Some(ToolKind::Draw(pinned_tool)) => pinned_tool,
        _ => tool,
    }
}

/// The tool whose own method is `tool`, the one its key is the shortcut of.
fn tool_of_default(tool: DrawTool) -> Option<&'static ToolSpec> {
    for tab in spec::tabs() {
        for group in tab.live_groups() {
            for tool_spec in group.live_tools() {
                if tool_spec.default_method().run == Some(ToolKind::Draw(tool)) {
                    return Some(tool_spec);
                }
            }
        }
    }
    None
}

/// What one ribbon cell draws and what a click on it runs.
#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::draw::DEFAULT_POLYGON_SIDES;
    use crate::toolbars::ready::extrude_hint;
    use crate::toolbars::spec::{self, TabSpec, VariantSpec};

    /// A context where every live tool can run and the viewport owns its view.
    fn full_context() -> RibbonContext {
        RibbonContext {
            has_panel_material: true,
            product_open: true,
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
        groups: &[GroupSpec { id: "test.group", label: Message::GroupCreate, tools: &[] }],
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

    /// Runs the ribbon for `frames` frames on one context, so the popup the ribbon opens can finish
    /// its sizing pass and draw its contents.
    fn draw_frames(
        state: &mut Toolbars,
        context: RibbonContext,
        frames: usize,
    ) -> (Vec<String>, Vec<String>, Vec<ToolAction>) {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::set_language(&ctx, Lang::En);
        let mut actions = Vec::new();
        let mut out = (Vec::new(), Vec::new(), Vec::new());
        for _ in 0..frames {
            let output = ctx.run_ui(Default::default(), |ui| {
                show(ui, state, context, &mut actions);
            });
            out = (drawn_text(&output.shapes), accessible_labels(&output), actions.clone());
            output.drop_without_applying_deltas();
        }
        out
    }

    /// The tool `id` of the map.
    fn tool(id: &str) -> &'static ToolSpec {
        spec::tool(id).expect("the map declares this tool")
    }

    /// The hints of the cells a tab draws, with its own tab buttons and the caret faces filtered out.
    ///
    /// A split cell publishes two labels — the method and the list behind the caret — and this helper
    /// answers the tool row, not the caret; [`caret_hints`] counts those.
    fn tool_hints(state: &mut Toolbars, context: RibbonContext) -> Vec<String> {
        let (_, labels, _) = draw(state, context);
        let tabs = spec::tabs().iter().map(|tab| tab.label.text(Lang::En)).collect::<Vec<_>>();
        let caret = Message::ToolMethods.text(Lang::En);
        let mut hints = labels
            .into_iter()
            .filter(|label| !tabs.iter().any(|tab| tab == label) && label != caret)
            .collect::<Vec<_>>();
        hints.sort();
        hints
    }

    /// The number of listed methods a caret face asks the list for.
    fn caret_hints(state: &mut Toolbars, context: RibbonContext) -> usize {
        let (_, labels, _) = draw(state, context);
        let caret = Message::ToolMethods.text(Lang::En);
        labels.iter().filter(|label| label.as_str() == caret).count()
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
                    "By side count  (N)",
                    "Centre and radius  (C)",
                    "From corner  (R)",
                    "No draft is running  (Esc)",
                    "Polyline · Tab line/arc · Alt ignores snaps  (P)",
                    "Through 2 points  (A)",
                    "Two points  (L)",
                ],
            ),
            (
                "model",
                &[
                    "Box: draw its base on the work plane",
                    "Cylinder: click the work plane",
                    "Select a closed path first",
                    "Sphere: click the work plane",
                ],
            ),
            ("furniture", &["Panel", "Post: Largest rectangle", "Product frame", "Shelf: Largest rectangle"]),
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
            let listed = tab(id).live_groups().flat_map(GroupSpec::live_tools).filter(|tool| tool.has_list()).count();
            assert_eq!(caret_hints(&mut state, full_context()), listed, "one caret face per tool with a list");
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
        // The approved body is the 30 pt tool row and the 16 pt caption band: a row that grows an
        // extra field band is a regression (U68, revised by U75).
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
        assert!((drawn - 76.0).abs() < 0.01, "the approved open ribbon is 76 pt, not {drawn}");
    }

    #[test]
    fn every_group_caption_sits_centred_with_air_above_and_below() {
        // U75: at 12 pt a 9.5 pt line of capitals touched the tool row above it and the rule below.
        // The band is 16 pt now, painted a step off the control tone instead of the darkest chrome,
        // and every caption of every drawn tab keeps its air on both sides in both languages.
        for lang in [Lang::En, Lang::Ru] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            crate::set_language(&ctx, lang);
            let mut checked = 0;
            for spec in spec::tabs().iter().filter(|tab| tab.is_drawn()) {
                let mut state = Toolbars::new();
                state.set_tab(spec);
                let mut fill = egui::Color32::TRANSPARENT;
                let output = ctx.run_ui(Default::default(), |ui| {
                    show(ui, &mut state, full_context(), &mut Vec::new());
                    fill = crate::theme::caption_band(ui);
                });
                let band = band_of(&output.shapes, fill);
                let band = band.unwrap_or_else(|| panic!("the {} tab captions no group", spec.id));
                assert!(
                    (band.height() - RIBBON_CAPTION_HEIGHT).abs() < 0.01,
                    "the {} band is {:.2} pt tall",
                    spec.id,
                    band.height()
                );
                let mut baseline: Option<f32> = None;
                for group in spec.live_groups() {
                    let name = group.label.text(lang).to_uppercase();
                    let caps = text_cap_line(&output.shapes, &name);
                    let (top, line) = caps.unwrap_or_else(|| panic!("{} is not captioned", group.id));
                    let ink = text_ink(&output.shapes, &name).expect("a drawn caption has glyphs");
                    let above = top - band.top();
                    let below = band.bottom() - line;
                    assert!(
                        ((top + line) / 2.0 - band.center().y).abs() < 0.5,
                        "{}: the capitals are not centred: {above:.2} pt above, {below:.2} pt below",
                        group.id
                    );
                    assert!(above >= 3.0, "{}: the capitals keep {above:.2} pt above them", group.id);
                    assert!(below >= 3.0, "{}: the capitals keep {below:.2} pt below them", group.id);
                    assert!(
                        ink.top() >= band.top() && ink.bottom() <= band.bottom(),
                        "{}: the glyphs leave the band: {ink:?} in {band:?}",
                        group.id
                    );
                    match baseline {
                        Some(first) => assert!(
                            (line - first).abs() < 0.01,
                            "{}: its baseline is {:.2} pt off the group beside it",
                            group.id,
                            line - first
                        ),
                        None => baseline = Some(line),
                    }
                    checked += 1;
                }
                output.drop_without_applying_deltas();
            }
            assert!(checked >= 5, "every drawn tab was read, not {checked} captions");
        }
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
        assert_eq!(height(true), 76.0, "the open ribbon is the tab row, the tool row and the band");
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
        let post = ClassCode::VERTICAL_PANEL;
        let cases = [
            (None, None, None, None, None),
            (Some(DrawTool::Line), None, None, None, Some(ToolActivity::Draw(DrawTool::Line))),
            (None, Some(PrimitiveKind::Sphere), None, None, Some(ToolActivity::Primitive(PrimitiveKind::Sphere))),
            (None, None, Some(BoxOwner::Cuboid), None, Some(ToolActivity::Primitive(PrimitiveKind::Cuboid))),
            (None, None, Some(BoxOwner::Product), None, Some(ToolActivity::ProductFrame)),
            (
                None,
                None,
                None,
                Some((post, FillMode::Fit)),
                Some(ToolActivity::Fill { class: post, mode: FillMode::Fit }),
            ),
            (Some(DrawTool::Arc), Some(PrimitiveKind::Sphere), None, None, Some(ToolActivity::Draw(DrawTool::Arc))),
        ];
        for (drawing, primitive, box_owner, fill, expected) in cases {
            assert_eq!(
                tool_activity(drawing, primitive, box_owner, fill),
                expected,
                "drawing={drawing:?} primitive={primitive:?} box={box_owner:?} fill={fill:?}"
            );
        }
    }

    #[test]
    fn a_button_is_pressed_only_for_its_own_activity() {
        let cases = [
            (None, ToolKind::Draw(DrawTool::Line), false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolKind::Draw(DrawTool::Line), true),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolKind::Draw(DrawTool::Circle), false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolKind::Panel, false),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cuboid)), ToolKind::Primitive(PrimitiveKind::Cuboid), true),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cuboid)), ToolKind::Primitive(PrimitiveKind::Sphere), false),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cuboid)), ToolKind::Light, false),
            (Some(ToolActivity::ProductFrame), ToolKind::ProductDraw, true),
            (Some(ToolActivity::ProductFrame), ToolKind::Product, false),
            (Some(ToolActivity::ProductFrame), ToolKind::Primitive(PrimitiveKind::Cuboid), false),
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
        assert_eq!(state.extrude_height(), Length::from_millimetres(100).expect("height"));
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
        state.set_extrude_height(Length::from_millimetres(24).expect("height"));
        let node = Id::new(7).expect("id");
        let context = RibbonContext { selected: Some(node), selected_path: Some(true), ..full_context() };
        let (_, labels, actions) = draw(&mut state, context);
        assert!(labels.iter().any(|label| label.contains("Extrude the selected path")));
        assert_eq!(state.extrude_height(), Length::from_millimetres(24).expect("height"));
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn the_ribbon_starts_on_the_typed_defaults() {
        // The three numbers a tool needs are typed session defaults (U76): always valid, never text.
        let state = Toolbars::new();
        assert_eq!(
            state.primitive_size(),
            Length::from_millimetres(DEFAULT_PRIMITIVE_SIZE_MM).expect("the default size is a length")
        );
        assert_eq!(
            state.extrude_height(),
            Length::from_millimetres(DEFAULT_EXTRUDE_HEIGHT_MM).expect("the default height is a length")
        );
        assert_eq!(state.polygon_sides(), DEFAULT_POLYGON_SIDES);
    }

    #[test]
    fn a_primitive_cell_runs_at_the_default_size() {
        let tool = tool("model.primitives.cuboid");
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        let (actions, _) = click_cell(&mut state, tool, main_point());
        assert_eq!(
            actions,
            vec![ToolAction::CreatePrimitive {
                kind: PrimitiveKind::Cuboid,
                size: Length::from_millimetres(DEFAULT_PRIMITIVE_SIZE_MM).expect("the default size is a length"),
            }],
            "the cell hands the command the typed default"
        );
    }

    #[test]
    fn a_polygon_is_started_with_the_default_side_count() {
        let polygon = tool("sketch.create.polygon");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        let (actions, _) = click_cell(&mut state, polygon, main_point());
        assert_eq!(
            actions,
            vec![ToolAction::Draw { tool: DrawTool::RegularPolygon, sides: DEFAULT_POLYGON_SIDES }],
            "the polygon starts at the typed side count and the object card edits it afterwards"
        );
    }

    #[test]
    fn no_tab_draws_a_text_field() {
        // The ribbon is tools only: a tab that drew a `TextEdit` would be a form in the tool row.
        for tab in spec::tabs() {
            let mut state = Toolbars::new();
            state.set_tab(tab);
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            crate::set_language(&ctx, Lang::En);
            let output = ctx.run_ui(Default::default(), |ui| {
                show(ui, &mut state, full_context(), &mut Vec::new());
            });
            let text_inputs =
                accessible_roles(&output).into_iter().filter(|role| *role == egui::Role::TextInput).count();
            output.drop_without_applying_deltas();
            assert_eq!(text_inputs, 0, "{} draws a text field in its tool row", tab.id);
        }
    }

    #[test]
    fn the_three_primitive_buttons_are_filled() {
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        let (_, labels, actions) = draw(&mut state, full_context());
        for label in
            ["Box: draw its base on the work plane", "Cylinder: click the work plane", "Sphere: click the work plane"]
        {
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
    fn the_empty_space_tool_arms_a_fill_class_only_inside_an_open_product() {
        let state = Toolbars::new();
        let closed = RibbonContext { product_open: false, ..full_context() };
        for tool_id in ["furniture.panels.post", "furniture.panels.shelf"] {
            let tool = tool(tool_id);
            let kind = tool.primary().expect("a live tool");
            let cell = ready(kind, tool, &state, closed, Lang::En);
            assert!(!cell.enabled, "{tool_id} cannot fill a void with no product open");
            assert!(cell.hint.contains(Message::FillNeedsProduct.text(Lang::En)), "got {}", cell.hint);
            assert_eq!(cell.action, None);
        }

        let post = tool("furniture.panels.post");
        let cell = ready(post.primary().expect("a live tool"), post, &state, full_context(), Lang::En);
        assert!(cell.enabled, "an open piece offers its niche");
        assert_eq!(cell.action, Some(ToolAction::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Fit }));

        let shelf = tool("furniture.panels.shelf");
        let cell = ready(shelf.primary().expect("a live tool"), shelf, &state, full_context(), Lang::En);
        assert_eq!(cell.action, Some(ToolAction::Fill { class: ClassCode::HORIZONTAL_PANEL, mode: FillMode::Fit }));
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
    fn a_row_and_a_tab_count_tools_only() {
        for tab in spec::tabs() {
            let mut columns = 0;
            for group in tab.live_groups() {
                assert_eq!(group.columns(), group.live_tools().count(), "{} counts more than its tools", group.id);
                columns += group.columns();
            }
            assert_eq!(tab.columns(), columns, "{} disagrees with its groups", tab.id);
        }
    }

    #[test]
    fn every_drawn_row_fits_the_narrowest_window() {
        // A split cell adds its caret column beside the cell (X36), so the body is wider than its cell
        // count by one caret per listed tool. The narrowest window FurniCraft opens is 640 x 480, and
        // the ribbon has no scroll area by design (ADR 0091), so a row that outgrew the window would
        // be clipped and lose a tool.
        const MIN_WINDOW_WIDTH: f32 = 640.0;
        for spec in spec::tabs().iter().filter(|tab| tab.is_drawn()) {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut state = Toolbars::new();
            state.set_tab(spec);
            let mut drawn = 0.0;
            let output = ctx.run_ui(Default::default(), |ui| {
                let left = ui.min_rect().left();
                ribbon_body(ui, &mut state, full_context(), &mut Vec::new(), Lang::En);
                drawn = ui.min_rect().right() - left;
            });
            output.drop_without_applying_deltas();
            assert!(
                drawn <= MIN_WINDOW_WIDTH,
                "the {} body draws {drawn} pt, wider than the {MIN_WINDOW_WIDTH} pt window",
                spec.id
            );
        }
    }

    #[test]
    fn a_tool_with_more_than_one_method_has_a_list_and_one_method_has_none() {
        let line = tool("sketch.create.line");
        let polyline = tool("sketch.create.polyline");
        let circle = tool("sketch.create.circle");
        assert!(line.has_list(), "a line names a method besides two points");
        assert!(!polyline.has_list(), "polyline has one way to be drawn");
        assert!(!tool("sketch.create.polyline").variants.is_empty() || !polyline.has_list());
        assert!(circle.has_list());

        let mut state = Toolbars::new();
        assert!(state.open_list(circle));
        assert!(!state.open_list(polyline), "a one-method tool opens nothing");
        assert!(state.list_open_for(circle), "the refused open left the open list alone");
        state.close_list();
        assert!(state.list().is_none());
    }

    #[test]
    fn the_pinned_method_falls_back_to_the_first_and_drives_the_shortcut() {
        let circle = tool("sketch.create.circle");
        let mut pinned = BTreeMap::new();
        assert_eq!(circle.pinned_index(&pinned), 0, "an unpinned tool runs its own method");
        assert_eq!(shortcut_method(DrawTool::Circle, &pinned), DrawTool::Circle);

        pinned.insert(circle.id.to_owned(), "sketch.create.circle.three-points".to_owned());
        assert_eq!(circle.pinned_index(&pinned), 2);
        assert_eq!(
            shortcut_method(DrawTool::Circle, &pinned),
            DrawTool::CircleThreePoints,
            "the tool's key runs the pinned method with the list closed"
        );

        pinned.insert(circle.id.to_owned(), "sketch.create.circle.retired".to_owned());
        assert_eq!(circle.pinned_index(&pinned), 0, "an unknown id falls back to the first method");
        assert_eq!(shortcut_method(DrawTool::Circle, &pinned), DrawTool::Circle);
        assert_eq!(
            shortcut_method(DrawTool::Polyline, &pinned),
            DrawTool::Polyline,
            "a tool with one method is never redirected"
        );
    }

    #[test]
    fn the_rows_are_the_map_in_order_and_a_planned_method_stays_disabled() {
        let circle = tool("sketch.create.circle");
        let rows = method_rows(circle, &Toolbars::new(), full_context(), Lang::En);
        let ids = rows.iter().map(|row| row.method.id).collect::<Vec<_>>();
        assert_eq!(
            ids,
            [
                "sketch.create.circle",
                "sketch.create.circle.two-points",
                "sketch.create.circle.three-points",
                "sketch.create.circle.tangent",
            ],
            "the list is the map, in order, without a duplicate row"
        );
        assert_eq!(rows.iter().map(|row| row.enabled).collect::<Vec<_>>(), [true, false, true, false]);
        assert_eq!(
            rows[0].method.label,
            Message::ToolCircleCentreRadius,
            "the first row names the method, not the tool"
        );
        assert_eq!(rows[0].method.section, Some(Message::SectionBuild), "the tool's row joins the first section");
        assert_eq!(rows[2].method.section, Some(Message::SectionBuild));
        assert_eq!(rows[3].method.section, Some(Message::SectionTangents));
        assert!(rows[1].action.is_none(), "a planned method runs nothing");
    }

    #[test]
    fn a_row_choice_runs_its_action_and_a_pin_keeps_the_list_open() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        assert!(state.open_list(circle));
        let rows = method_rows(circle, &state, full_context(), Lang::En);
        let mut actions = Vec::new();

        apply_choice(&mut state, circle, &rows, Some(list::ListChoice::Pin(2)), &mut actions);
        assert!(actions.is_empty(), "a pin runs nothing");
        assert_eq!(
            state.pinned_variants().get(circle.id).map(String::as_str),
            Some("sketch.create.circle.three-points")
        );
        assert!(state.list_open_for(circle), "pinning keeps the list open under the new accent");

        apply_choice(&mut state, circle, &rows, Some(list::ListChoice::Run(2)), &mut actions);
        assert_eq!(actions, vec![ToolAction::Draw { tool: DrawTool::CircleThreePoints, sides: DEFAULT_POLYGON_SIDES }]);
        assert!(!state.list_open_for(circle), "running a row closes the list");
    }

    #[test]
    fn a_main_face_click_runs_the_pinned_method_and_opens_nothing() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        state.pin(circle, circle.methods().nth(2).expect("the three-point method"));
        let (actions, _) = click_cell(&mut state, circle, main_point());
        assert_eq!(
            actions,
            vec![ToolAction::Draw { tool: DrawTool::CircleThreePoints, sides: DEFAULT_POLYGON_SIDES }],
            "the main face runs exactly one method"
        );
        assert!(state.list().is_none(), "a main-face click opens nothing");
    }

    #[test]
    fn a_caret_click_opens_and_closes_the_list_without_running_anything() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        let (actions, _) = click_cell(&mut state, circle, caret_point());
        assert!(actions.is_empty(), "the caret opens the list instead of running a method: {actions:?}");
        assert!(state.list_open_for(circle), "the caret opens the list");

        let (actions, _) = click_cell(&mut state, circle, caret_point());
        assert!(actions.is_empty(), "closing runs nothing either: {actions:?}");
        assert!(state.list().is_none(), "the second caret click closes the list");
    }

    #[test]
    fn a_main_face_click_closes_the_open_list() {
        // The list is a question; answering it by running the method takes the question away, so a
        // second click on the main face does not leave a popup hanging under the cell.
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        assert!(state.open_list(circle));
        let (actions, _) = click_cell(&mut state, circle, main_point());
        assert_eq!(actions, vec![ToolAction::Draw { tool: DrawTool::Circle, sides: DEFAULT_POLYGON_SIDES }]);
        assert!(state.list().is_none(), "the main face closes the list it answered");
    }

    #[test]
    fn the_fill_cell_runs_the_mode_the_workspace_pinned() {
        // Two behaviours, one cell: the pin is the choice between fitting the void and taking its
        // whole shape, and nothing else stores it (N10).
        let post = tool("furniture.panels.post");
        let mut state = Toolbars::new();
        state.set_tab(tab("furniture"));
        let methods = post.methods().collect::<Vec<_>>();
        assert_eq!(methods.len(), 2, "one cell, two methods");
        assert_eq!(
            methods.iter().map(|method| method.run).collect::<Vec<_>>(),
            vec![
                Some(ToolKind::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Fit }),
                Some(ToolKind::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Shape }),
            ],
            "the cell's own method fits, its second takes the shape"
        );
        assert_eq!(post.id, methods[0].id);
        assert_eq!(post.variants[0].id, methods[1].id, "a method keeps its namespaced id");

        let (actions, labels) = click_cell(&mut state, post, main_point());
        assert_eq!(
            actions,
            vec![ToolAction::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Fit }],
            "an unpinned cell fits the void"
        );
        assert!(
            labels.iter().any(|label| label == "Post: Largest rectangle"),
            "the hint names the tool and the mode: {labels:?}"
        );

        state.pin(post, methods[1]);
        let (actions, labels) = click_cell(&mut state, post, main_point());
        assert_eq!(
            actions,
            vec![ToolAction::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Shape }],
            "the pinned method runs, and the click still arms nothing by itself"
        );
        assert!(labels.iter().any(|label| label == "Post: Niche shape"), "the face follows the pin: {labels:?}");
    }

    #[test]
    fn the_fill_methods_read_as_two_builds_of_one_panel() {
        // The Shape row used to be a blank line named like a second panel. It now carries its own
        // glyph and sits under a caption that says both rows are builds of the same cell (ADR 0110).
        for id in ["furniture.panels.post", "furniture.panels.shelf"] {
            let tool = tool(id);
            let methods = tool.methods().collect::<Vec<_>>();
            assert_eq!(methods.len(), 2, "{id} has one cell with two builds");
            assert!(methods[1].icon.is_some_and(Icon::is_drawn), "{id}'s shape row has artwork");
            assert_eq!(methods[0].section, Some(Message::SectionFillMethod), "{id} captions its build");
            assert_eq!(methods[1].section, Some(Message::SectionFillMethod));
        }
    }

    #[test]
    fn the_open_fill_list_draws_the_build_caption_and_both_rows() {
        let post = tool("furniture.panels.post");
        let mut state = Toolbars::new();
        state.set_tab(tab("furniture"));
        assert!(state.open_list(post));
        let (texts, _, _) = draw_frames(&mut state, full_context(), 3);
        assert!(texts.iter().any(|text| text == "BUILD METHOD"), "the caption says what the rows are: {texts:?}");
        assert!(texts.iter().any(|text| text == "Largest rectangle"), "the fit row is drawn: {texts:?}");
        assert!(texts.iter().any(|text| text == "Niche shape"), "the shape row is drawn: {texts:?}");
    }

    #[test]
    fn the_fill_cell_shows_the_armed_mode_even_when_another_is_pinned() {
        // Picking Shape from the list arms it; the cell must then show Shape pressed, instead of
        // going on showing the pinned Fit and making the choice look like it did not take (ADR 0110).
        let post = tool("furniture.panels.post");
        let mut state = Toolbars::new();
        state.set_tab(tab("furniture"));
        let context = RibbonContext {
            active: Some(ToolActivity::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Shape }),
            ..full_context()
        };
        let rows = method_rows(post, &state, context, Lang::En);
        assert!(!rows[0].armed, "the pinned Fit is not what the viewport will build");
        assert!(rows[1].armed, "the shape is armed");
        let shown = rows.iter().position(|row| row.armed).unwrap_or(state.pinned_index(post));
        let face = main_face(post, &rows, shown).expect("a face");
        assert!(face.armed, "the cell draws pressed");
        assert_eq!(face.icon, icons::scene::POST, "the cell keeps the panel kind's glyph");
        assert_eq!(face.hint, "Post: Niche shape", "the face names the mode the click will use");
        assert_eq!(
            face.action,
            Some(ToolAction::Fill { class: ClassCode::VERTICAL_PANEL, mode: FillMode::Shape }),
            "a second click on the armed mode drops it"
        );
        assert_eq!(
            state.pinned_index(post),
            0,
            "showing the armed mode does not silently change the workspace default"
        );
    }

    #[test]
    fn a_fill_face_keeps_the_tool_glyph_in_both_modes() {
        let post = tool("furniture.panels.post");
        let rows = method_rows(post, &Toolbars::new(), full_context(), Lang::En);
        for index in 0..rows.len() {
            let face = main_face(post, &rows, index).expect("a face");
            assert_eq!(face.icon, icons::scene::POST, "row {index} draws the post, not a mode glyph");
        }
    }

    #[test]
    fn a_disabled_fill_face_says_what_is_missing_rather_than_naming_its_method() {
        let post = tool("furniture.panels.post");
        let state = Toolbars::new();
        let rows = method_rows(post, &state, RibbonContext { product_open: false, ..full_context() }, Lang::En);
        let face = main_face(post, &rows, 0).expect("a face");
        assert!(!face.enabled);
        assert_eq!(face.hint, Message::FillNeedsProduct.text(Lang::En), "the cell explains itself");
        assert_eq!(face.action, None);
    }

    #[test]
    fn a_pin_carries_the_main_face_to_the_new_method() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        let rows = method_rows(circle, &state, full_context(), Lang::En);
        let own = main_face(circle, &rows, state.pinned_index(circle)).expect("the tool's own method");
        assert_eq!(own.icon, icons::contour::CIRCLE, "the face draws the method the workspace pinned");
        assert_eq!(own.hint, "Centre and radius  (C)", "the hint names the method and its key, not the tool");

        state.pin(circle, rows[2].method);
        let rows = method_rows(circle, &state, full_context(), Lang::En);
        let pinned = main_face(circle, &rows, state.pinned_index(circle)).expect("a face");
        assert_eq!(pinned.icon, icons::contour::CIRCLE_THREE_POINTS, "the face follows the pin at once");
        assert_eq!(pinned.hint, "Circle through 3 points  (Shift+C)", "the hint follows the pin at once");

        let (actions, labels) = click_cell(&mut state, circle, main_point());
        assert_eq!(
            actions,
            vec![ToolAction::Draw { tool: DrawTool::CircleThreePoints, sides: DEFAULT_POLYGON_SIDES }],
            "the main face runs the method the pin put there"
        );
        assert!(
            labels.iter().any(|label| label == "Circle through 3 points  (Shift+C)"),
            "the cell publishes the pinned method: {labels:?}"
        );
        assert!(labels.iter().any(|label| label == Message::ToolMethods.text(Lang::En)), "the caret names the list");
    }

    #[test]
    fn a_pinned_method_without_artwork_keeps_the_tool_glyph() {
        // A method that shares its tool's shape has no drawing of its own; the face must fall back
        // to the tool rather than leave a blank cell.
        static ARTWORKLESS: ToolSpec = ToolSpec {
            id: "test.artworkless",
            label: Message::ToolCircle,
            icon: Some(icons::contour::CIRCLE),
            shortcut: Some("C"),
            run: Some(ToolKind::Draw(DrawTool::Circle)),
            method: Some(Message::ToolCircleCentreRadius),
            variants: &[VariantSpec {
                id: "test.artworkless.three-points",
                label: Message::ToolCircleThreePoints,
                icon: None,
                run: Some(ToolKind::Draw(DrawTool::CircleThreePoints)),
                section: None,
            }],
        };
        let rows = method_rows(&ARTWORKLESS, &Toolbars::new(), full_context(), Lang::En);
        let face = main_face(&ARTWORKLESS, &rows, 1).expect("a face");
        assert_eq!(face.icon, icons::contour::CIRCLE, "a method without artwork keeps the tool's glyph");
        assert_eq!(face.hint, "Circle through 3 points  (Shift+C)", "the hint still names the method");
    }

    #[test]
    fn the_pin_and_the_key_keep_their_columns_apart_in_both_languages() {
        // The pin is a control inside the row, and the key is a label: the columns are measured once
        // for the whole list, so the pin can never be drawn over a key (U74).
        let row = egui::Rect::from_min_size(egui::pos2(4.0, 40.0), egui::vec2(list::WIDTH, list::ROW_HEIGHT));
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            for lang in [Lang::En, Lang::Ru] {
                for id in ["sketch.create.circle", "sketch.create.line", "sketch.create.arc", "sketch.create.rectangle"]
                {
                    let rows = method_rows(tool(id), &Toolbars::new(), full_context(), lang);
                    let columns = list::columns(row, list::shortcut_column(ui, &rows));
                    assert!(
                        columns.shortcut.right() < columns.pin.left(),
                        "{id} {lang:?}: the key {columns:?} reaches the pin"
                    );
                    assert!(
                        columns.label.right() <= columns.shortcut.left(),
                        "{id} {lang:?}: the name {columns:?} reaches the key"
                    );
                    assert_eq!(columns.glyph.left(), row.left() + 10.0, "the glyph keeps the row's inset");
                    assert!(columns.label.left() > columns.glyph.right(), "the name follows the glyph");
                    assert_eq!(columns.pin.width(), 20.0, "the pin's column is the pin's control");
                }
            }
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn a_tab_change_and_a_folded_body_close_the_list() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        assert!(state.open_list(circle));
        apply_tab_gesture(&mut state, tab("model"), false);
        assert!(state.list().is_none(), "a tab change closes the list");

        state.set_tab(tab("sketch"));
        assert!(state.open_list(circle));
        state.set_open(false);
        assert!(state.list().is_none(), "a folded body has no cell to anchor the list to");
    }

    #[test]
    fn a_list_that_outlived_its_tab_is_dropped_before_it_draws() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("model"));
        // A state put out of step by hand, the way a lost tab would leave it.
        state.list = Some(list::VariantList::new(circle));
        let (texts, _, _) = draw(&mut state, full_context());
        assert!(state.list().is_none(), "the ribbon drops a list it cannot anchor");
        assert!(!texts.iter().any(|text| text == "TANGENTS"), "no list is drawn: {texts:?}");
    }

    #[test]
    fn the_open_list_draws_its_header_its_rows_and_no_footer() {
        let circle = tool("sketch.create.circle");
        let mut state = Toolbars::new();
        state.set_tab(tab("sketch"));
        assert!(state.open_list(circle));
        let (texts, labels, actions) = draw_frames(&mut state, full_context(), 3);
        assert!(texts.iter().any(|text| text == "Circle"), "the header names the tool: {texts:?}");
        assert!(texts.iter().any(|text| text == "C"), "the header carries the shortcut");
        assert!(texts.iter().any(|text| text == "Centre and radius"), "the tool's own method is a row: {texts:?}");
        assert!(texts.iter().any(|text| text == "Circle through 3 points"), "a live method is a row: {texts:?}");
        assert!(texts.iter().any(|text| text == "BUILD"), "the section caption is drawn: {texts:?}");
        assert!(
            !texts.iter().any(|text| text.contains("Enter") || text.contains("Esc")),
            "the list has no footer: {texts:?}"
        );
        assert!(labels.iter().any(|label| label.contains("Circle")), "the rows are announced: {labels:?}");
        assert!(actions.is_empty(), "opening and drawing the list runs nothing");
    }

    #[test]
    fn every_open_list_stays_inside_the_window_at_both_horizontal_edges() {
        // The list is anchored to its cell and flips to the other side of the anchor at an edge;
        // this is the approved anatomy's promise, and a window narrower than the list is out of
        // scope because the shell enforces a larger minimum.
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        for anchor_x in [8.0, 752.0] {
            let ctx = egui::Context::default();
            crate::set_language(&ctx, Lang::En);
            let mut slot = Some(list::VariantList::new(tool("sketch.create.circle")));
            let mut anchor = None;
            for _ in 0..2 {
                anchor = list_frame(&ctx, screen, anchor_x, &mut slot, Vec::new());
            }
            let anchor = anchor.expect("the cell drew");
            let circle = tool("sketch.create.circle");
            let rect = egui::Popup::from_response(&anchor)
                .id(egui::Id::unique(("ribbon.variants", circle.id)))
                .get_popup_rect()
                .expect("the list drew once and reports its rect");
            assert!(screen.contains_rect(rect), "the list at x = {anchor_x} leaves the window: {rect:?} in {screen:?}");
        }
    }

    #[test]
    fn escape_closes_the_open_list() {
        let circle = tool("sketch.create.circle");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        crate::set_language(&ctx, Lang::En);
        let mut slot = Some(list::VariantList::new(circle));
        for _ in 0..2 {
            list_frame(&ctx, screen, 8.0, &mut slot, Vec::new());
        }
        assert!(slot.is_some(), "the list is open");
        list_frame(&ctx, screen, 8.0, &mut slot, vec![key_event(egui::Key::Escape)]);
        assert!(slot.is_none(), "Escape closes the list");
    }

    #[test]
    fn a_click_outside_the_list_closes_it() {
        let circle = tool("sketch.create.circle");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        crate::set_language(&ctx, Lang::En);
        let mut slot = Some(list::VariantList::new(circle));
        for _ in 0..3 {
            list_frame(&ctx, screen, 8.0, &mut slot, Vec::new());
        }
        assert!(slot.is_some(), "the list is open");
        let outside = egui::pos2(600.0, 500.0);
        list_frame(&ctx, screen, 8.0, &mut slot, vec![pointer_button(outside, true)]);
        assert!(slot.is_some(), "the press alone is not a click");
        list_frame(&ctx, screen, 8.0, &mut slot, vec![pointer_button(outside, false)]);
        assert!(slot.is_none(), "a click outside the list closes it");
    }

    #[test]
    fn a_click_inside_the_list_keeps_it_open() {
        // The pin is inside the list: only a click elsewhere closes it, so a pin does not dismiss
        // the list the user is still choosing from.
        let circle = tool("sketch.create.circle");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        crate::set_language(&ctx, Lang::En);
        let mut slot = Some(list::VariantList::new(circle));
        for _ in 0..3 {
            list_frame(&ctx, screen, 8.0, &mut slot, Vec::new());
        }
        let inside = egui::pos2(150.0, 110.0);
        list_frame(&ctx, screen, 8.0, &mut slot, vec![pointer_button(inside, true)]);
        list_frame(&ctx, screen, 8.0, &mut slot, vec![pointer_button(inside, false)]);
        assert!(slot.is_some(), "a click inside the list leaves it open");
    }

    #[test]
    fn the_pin_is_drawn_only_on_the_hovered_or_the_highlighted_row() {
        // The pin is not part of the resting row: it appears on the row under the pointer and on the
        // row the keyboard reached, and its column stays reserved either way (U74).
        let circle = tool("sketch.create.circle");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        crate::set_language(&ctx, Lang::En);
        // Row 1 is a planned method, so an idle list draws no pin at all.
        let mut slot = Some(list::VariantList { tool: circle.id, highlighted: 1 });
        let (_, idle) = warm_list(&ctx, screen, &mut slot);
        let base = count_images(&idle.shapes);
        assert_eq!(base, 3, "the header's glyph and the two drawn methods, and no pin at rest");
        let idle_label = text_place(&idle.shapes, "Centre and radius");
        let idle_key = text_place(&idle.shapes, "Shift+C");
        // Hovering is resolved from the previous pass, so the pointer moves and the next frame paints.
        let hover = row_of(&idle.shapes, "Circle through 3 points").center();
        idle.drop_without_applying_deltas();
        let (_, moved) = list_paint(&ctx, screen, 8.0, &mut slot, vec![pointer_moved(hover)]);
        moved.drop_without_applying_deltas();
        let (_, hovered) = list_paint(&ctx, screen, 8.0, &mut slot, Vec::new());
        assert_eq!(count_images(&hovered.shapes), base + 1, "the pin appears on the row under the pointer");
        assert_eq!(text_place(&hovered.shapes, "Centre and radius"), idle_label, "the name does not move");
        assert_eq!(text_place(&hovered.shapes, "Shift+C"), idle_key, "the key does not move");
        hovered.drop_without_applying_deltas();

        // The keyboard on the same row, with the pointer away from the list.
        slot.as_mut().expect("the list is open").highlighted = 2;
        let (_, away) = list_paint(&ctx, screen, 8.0, &mut slot, vec![pointer_moved(egui::pos2(700.0, 500.0))]);
        away.drop_without_applying_deltas();
        let (_, keyed) = list_paint(&ctx, screen, 8.0, &mut slot, Vec::new());
        assert_eq!(count_images(&keyed.shapes), base + 1, "the pin appears on the row the keyboard reached");
        keyed.drop_without_applying_deltas();
    }

    #[test]
    fn the_pin_stays_under_a_pointer_that_reaches_it() {
        // The pin is a second control inside the row: a pointer that reaches it must keep the pin
        // drawn and the row washed, or the control disappears from under the hand (U74).
        let circle = tool("sketch.create.circle");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        crate::set_language(&ctx, Lang::En);
        let mut slot = Some(list::VariantList { tool: circle.id, highlighted: 1 });
        let (_, idle) = warm_list(&ctx, screen, &mut slot);
        let base = count_images(&idle.shapes);
        let row = row_of(&idle.shapes, "Circle through 3 points");
        idle.drop_without_applying_deltas();
        assert_eq!(base, 3, "an idle list draws no pin");

        // One frame per step: the pointer reaches the row, then the pin at the row's right edge.
        let pin = egui::pos2(row.right() - list::ROW_INSET - list::PIN_COLUMN / 2.0, row.center().y);
        for pos in [row.center(), pin, pin] {
            let (_, output) = list_paint(&ctx, screen, 8.0, &mut slot, vec![pointer_moved(pos)]);
            output.drop_without_applying_deltas();
        }
        let (_, reached) = list_paint(&ctx, screen, 8.0, &mut slot, Vec::new());
        assert_eq!(count_images(&reached.shapes), base + 1, "the pin stays under the pointer that reached it");
        assert_eq!(fills_at(&reached.shapes, row).len(), 1, "the row stays washed under the pin");
        reached.drop_without_applying_deltas();
    }

    #[test]
    fn the_pinned_row_keeps_its_mark_when_the_pin_is_hidden() {
        // The accent bar and the fill are what says "this is the main method"; the pin only says what
        // a click on it would do, so it can hide without taking the mark away (U74).
        let circle = tool("sketch.create.circle");
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let ctx = egui::Context::default();
        crate::set_language(&ctx, Lang::En);
        let mut slot = Some(list::VariantList { tool: circle.id, highlighted: 1 });
        let (_, idle) = warm_list(&ctx, screen, &mut slot);
        assert_eq!(count_images(&idle.shapes), 3, "an idle list draws no pin");
        let mut marked = fills_at(&idle.shapes, row_of(&idle.shapes, "Centre and radius"));
        assert_eq!(marked.len(), 2, "the pinned row keeps its fill and its accent bar");
        assert!(marked.iter().all(|fill| *fill != egui::Color32::TRANSPARENT), "the mark is painted");
        marked.sort_unstable_by_key(|fill| fill.to_array());
        assert_ne!(marked[0], marked[1], "the bar is the accent, not the row's own fill");
        assert!(
            fills_at(&idle.shapes, row_of(&idle.shapes, "Circle through 3 points")).is_empty(),
            "a row that is neither hovered nor pinned carries no mark"
        );
        idle.drop_without_applying_deltas();
    }

    /// Warms the open circle list for three frames and returns the anchor and the last paint.
    fn warm_list(
        ctx: &egui::Context,
        screen: egui::Rect,
        slot: &mut Option<list::VariantList>,
    ) -> (egui::Response, egui::FullOutput) {
        for _ in 0..2 {
            let (_, output) = list_paint(ctx, screen, 8.0, slot, Vec::new());
            output.drop_without_applying_deltas();
        }
        let (anchor, output) = list_paint(ctx, screen, 8.0, slot, Vec::new());
        (anchor.expect("the cell drew"), output)
    }

    /// The rect of the row whose name is `label`, read from where the frame painted that name.
    ///
    /// The list's rows are not on a fixed pitch — a section caption takes its own band — so a test
    /// asks the paint where a row is instead of recomputing the layout a second time.
    fn row_of(shapes: &[egui::epaint::ClippedShape], label: &str) -> egui::Rect {
        let name = text_rect(shapes, label).unwrap_or_else(|| panic!("the frame does not draw {label:?}"));
        egui::Rect::from_center_size(name.center(), egui::vec2(list::WIDTH, list::ROW_HEIGHT))
    }

    /// The fills of the rects painted over the height of `row`: its own fill and its marks.
    fn fills_at(shapes: &[egui::epaint::ClippedShape], row: egui::Rect) -> Vec<egui::Color32> {
        fn walk(shape: &egui::Shape, row: egui::Rect, out: &mut Vec<egui::Color32>) {
            match shape {
                egui::Shape::Rect(painted)
                    if (painted.rect.top() - row.top()).abs() < 0.5
                        && (painted.rect.bottom() - row.bottom()).abs() < 0.5
                        && painted.fill != egui::Color32::TRANSPARENT =>
                {
                    out.push(painted.fill);
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, row, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, row, &mut out);
        }
        out
    }

    /// The ink of one drawn text: the tight box around its glyphs, or `None` when it is not drawn.
    ///
    /// The line box of a galley is taller than its capitals, so a clearance measured from
    /// [`text_rect`] would be the air around the line, not around the name.
    fn text_ink(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<egui::Rect> {
        fn walk(shape: &egui::Shape, text: &str, out: &mut Option<egui::Rect>) {
            match shape {
                egui::Shape::Text(drawn) if drawn.galley.text() == text => {
                    *out = Some(drawn.galley.mesh_bounds.translate(drawn.pos.to_vec2()));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, text, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = None;
        for clipped in shapes {
            walk(&clipped.shape, text, &mut out);
        }
        out
    }

    /// The top of one drawn text's capitals and its baseline, in frame coordinates.
    ///
    /// A caption is centred on this block, not on its line box or its whole ink: the box carries the
    /// descent and the ink may carry a `Д` leg or a `Ц` tail, and both would move the name off the
    /// baseline its neighbours share (U75).
    fn text_cap_line(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<(f32, f32)> {
        fn walk(shape: &egui::Shape, text: &str, out: &mut Option<(f32, f32)>) {
            match shape {
                egui::Shape::Text(drawn) if drawn.galley.text() == text => {
                    let baseline = drawn
                        .galley
                        .rows
                        .first()
                        .and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y));
                    if let Some(baseline) = baseline {
                        *out = Some((drawn.pos.y + drawn.galley.mesh_bounds.top(), drawn.pos.y + baseline));
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, text, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = None;
        for clipped in shapes {
            walk(&clipped.shape, text, &mut out);
        }
        out
    }

    /// The tallest rect painted with `fill`, which for a caption is its band.
    fn band_of(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> Option<egui::Rect> {
        fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut Option<egui::Rect>) {
            match shape {
                egui::Shape::Rect(painted) if painted.fill == fill => {
                    let taller = out.is_none_or(|rect: egui::Rect| painted.rect.height() > rect.height());
                    if taller {
                        *out = Some(painted.rect);
                    }
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, fill, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = None;
        for clipped in shapes {
            walk(&clipped.shape, fill, &mut out);
        }
        out
    }

    /// The rect of one drawn text, or `None` when the frame does not draw it.
    fn text_rect(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<egui::Rect> {
        fn walk(shape: &egui::Shape, text: &str, out: &mut Option<egui::Rect>) {
            match shape {
                egui::Shape::Text(drawn) if drawn.galley.text() == text => {
                    *out = Some(egui::Rect::from_min_size(drawn.pos, drawn.galley.size()));
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, text, out);
                    }
                }
                _ => {}
            }
        }

        let mut out = None;
        for clipped in shapes {
            walk(&clipped.shape, text, &mut out);
        }
        out
    }

    /// One frame of a window with a single open list under a cell at `anchor_x`.
    fn list_frame(
        ctx: &egui::Context,
        screen: egui::Rect,
        anchor_x: f32,
        slot: &mut Option<list::VariantList>,
        events: Vec<egui::Event>,
    ) -> Option<egui::Response> {
        let (anchor, output) = list_paint(ctx, screen, anchor_x, slot, events);
        output.drop_without_applying_deltas();
        anchor
    }

    /// One frame of the open circle list under a cell at `anchor_x`, with its paint.
    ///
    /// The pin, the accent bar and the columns are shapes rather than state, so a test reads them off
    /// the frame instead of asking the model to repeat what it drew.
    fn list_paint(
        ctx: &egui::Context,
        screen: egui::Rect,
        anchor_x: f32,
        slot: &mut Option<list::VariantList>,
        events: Vec<egui::Event>,
    ) -> (Option<egui::Response>, egui::FullOutput) {
        let circle = tool("sketch.create.circle");
        let mut anchor = None;
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let output = ctx.run_ui(input, |ui| {
            egui::Area::new(egui::Id::unique("ribbon.list.test")).fixed_pos(egui::pos2(anchor_x, 32.0)).show(
                ui.ctx(),
                |ui| {
                    let (_, response) =
                        ui.allocate_exact_size(egui::vec2(RIBBON_CELL_WIDTH, RIBBON_ROW_HEIGHT), egui::Sense::click());
                    let rows = method_rows(circle, &Toolbars::new(), full_context(), Lang::En);
                    list::show(ui, slot, circle, &rows, &response, 0, Lang::En);
                    anchor = Some(response);
                },
            );
        });
        (anchor, output)
    }

    /// One key press, as the platform hands it to `egui`.
    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    /// One pointer move, as the platform hands it to `egui`.
    fn pointer_moved(pos: egui::Pos2) -> egui::Event {
        egui::Event::PointerMoved(pos)
    }

    /// The rect a tool cell takes when a test draws it on its own: a full cell and the caret column a
    /// split cell adds beside it, so a cell of either kind fits where the row would put it.
    fn cell_rect() -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(16.0, 16.0),
            egui::vec2(RIBBON_CELL_WIDTH + icons::RIBBON_CARET_WIDTH, RIBBON_ROW_HEIGHT),
        )
    }

    /// The middle of a split cell's main face, which is the middle of a plain cell.
    fn main_point() -> egui::Pos2 {
        let rect = cell_rect();
        egui::pos2(rect.left() + RIBBON_CELL_WIDTH / 2.0, rect.center().y)
    }

    /// The middle of a split cell's caret face, which begins where the main face ends.
    fn caret_point() -> egui::Pos2 {
        let rect = cell_rect();
        egui::pos2(rect.left() + RIBBON_CELL_WIDTH + icons::RIBBON_CARET_WIDTH / 2.0, rect.center().y)
    }

    /// Draws one tool cell at [`cell_rect`], clicks `pos`, and reports what it ran and what it said.
    ///
    /// The cell is drawn alone, so a test aims at a face by its own metric instead of counting the
    /// cells before it on the tab.
    fn click_cell(state: &mut Toolbars, tool: &'static ToolSpec, pos: egui::Pos2) -> (Vec<ToolAction>, Vec<String>) {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::set_language(&ctx, Lang::En);
        let mut actions = Vec::new();
        let mut labels = Vec::new();
        for pressed in [None, Some(true), Some(false)] {
            let events = match pressed {
                Some(pressed) => vec![pointer_button(pos, pressed)],
                None => vec![pointer_moved(pos)],
            };
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
                events,
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(cell_rect()), |ui| {
                    show_tool(ui, tool, state, full_context(), &mut actions, Lang::En);
                });
            });
            labels = accessible_labels(&output);
            output.drop_without_applying_deltas();
        }
        (actions, labels)
    }

    /// The drawn text of one frame with the place of its galley, so a test can see a label move.
    fn text_places(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Pos2)> {
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Text(text) => out.push((text.galley.text().to_owned(), text.pos)),
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

    /// The place of one drawn text in a frame, or a failure that names what was drawn instead.
    fn text_place(shapes: &[egui::epaint::ClippedShape], text: &str) -> egui::Pos2 {
        text_places(shapes)
            .into_iter()
            .find(|(drawn, _)| drawn == text)
            .map(|(_, pos)| pos)
            .unwrap_or_else(|| panic!("the frame does not draw {text:?}"))
    }

    /// One mouse-button change at `pos`.
    fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
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

    /// The accessible roles this frame published, so a test can see a text field among the cells.
    fn accessible_roles(output: &egui::FullOutput) -> Vec<egui::Role> {
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| update.nodes.iter().map(|(_, node)| node.role()).collect())
            .unwrap_or_default()
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
        assert!(english.iter().any(|hint| hint.contains("Two points  (L)")));
        assert!(russian.iter().any(|hint| hint.contains("По двум точкам  (L)")), "the hint is translated: {russian:?}");
    }
}
