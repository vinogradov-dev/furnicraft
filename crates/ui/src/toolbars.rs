//! The ribbon: one row of tabs over the tool groups of the active tab.
//!
//! The surfaces the tools belong to are tabs — *Furniture, Hardware, Model, Sketch, Scene, View* —
//! and each tab is one row of groups separated by vertical dividers, with the group's own caption
//! under its row. A tab with no group that holds an action is not drawn at all, so the ribbon never
//! shows a greyed "coming soon" tab: *Furniture* and *Hardware* appear only when a tool lands there.
//! Whether the body is open, which tab is active, and what the numeric fields remember is view state
//! in the editor and never reaches the document.
//!
//! Every button is the labelled [`icons::ribbon_button`], because the ribbon row is the one surface
//! tall enough to name a tool under its glyph; the panel headers and the floating viewport cluster
//! keep the icon-only [`icons::tool_button`], whose row could not carry a second line.

use egui::{TextEdit, Ui};
use furnicraft_engine::PrimitiveKind;
use furnicraft_model::Id;
use furnicraft_units::Length;

use crate::contour_edit::{DEFAULT_POLYGON_SIDES, DrawTool};
use crate::icons::{self, Icon, Theme, tool_hint};

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
    /// Drops the armed container, so creation returns to the scene root.
    ClearCreationParent,
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

/// The open-ended half of the drawing tools, in button order.
pub const DRAW_TOOLS: [DrawTool; 4] = [DrawTool::Line, DrawTool::Polyline, DrawTool::Arc, DrawTool::ArcFromCenter];

/// The closed figures, in button order: each produces a ring without a close action.
pub const SHAPE_TOOLS: [DrawTool; 6] = [
    DrawTool::Rectangle,
    DrawTool::RectangleFromCenter,
    DrawTool::RectangleFromCenterDirected,
    DrawTool::Circle,
    DrawTool::CircleThreePoints,
    DrawTool::RegularPolygon,
];

/// Every drawing tool, in ribbon order: the draw group then the shapes.
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

/// Whether the button for one action is the armed one.
///
/// A creation that runs and forgets is never armed, so its button stays idle however long the
/// selection sits there.
fn is_armed(activity: Option<ToolActivity>, action: ToolAction) -> bool {
    match (activity, action) {
        (Some(ToolActivity::Draw(armed)), ToolAction::Draw { tool, .. }) => armed == tool,
        (Some(ToolActivity::Primitive(armed)), ToolAction::CreatePrimitive { kind, .. }) => armed == kind,
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
pub const fn strip_label(tool: DrawTool) -> &'static str {
    match tool {
        DrawTool::Polyline => "Polyline · Tab line/arc · Alt ignores snaps",
        _ => tool.label(),
    }
}

/// The icon of a drawing tool.
const fn tool_icon(tool: DrawTool) -> Icon {
    match tool {
        DrawTool::Line => icons::contour::LINE,
        DrawTool::Polyline => icons::contour::POLYLINE,
        DrawTool::Arc => icons::contour::ARC,
        DrawTool::ArcFromCenter => icons::contour::ARC_CENTRE,
        DrawTool::Rectangle => icons::contour::RECTANGLE,
        DrawTool::RectangleFromCenter => icons::contour::RECTANGLE_CENTRE,
        DrawTool::RectangleFromCenterDirected => icons::contour::RECTANGLE_CENTRE_DIRECTED,
        DrawTool::Circle => icons::contour::CIRCLE,
        DrawTool::CircleThreePoints => icons::contour::CIRCLE_THREE_POINTS,
        DrawTool::RegularPolygon => icons::contour::POLYGON,
    }
}

/// One tab of the ribbon: the surface a group of tools belongs to.
///
/// The tabs are the ribbon's top level, so a tool is found by what the user is doing — a drawing, a
/// solid, the scene — rather than by which theme drew its icon. A tab whose every group is empty is
/// not drawn, which is what keeps *Furniture* and *Hardware* out of the row until they hold a tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonTab {
    /// Cabinet bodies and fronts.
    Furniture,
    /// Hinges, runners and handles.
    Hardware,
    /// Solid primitives and the bodies built from a drawing.
    Model,
    /// Drawing on the work plane.
    Sketch,
    /// Panels, assemblies, cameras and lights.
    Scene,
    /// How the viewport draws and where it looks.
    View,
}

impl RibbonTab {
    /// Every tab, in ribbon order.
    pub const ALL: [Self; 6] = [Self::Furniture, Self::Hardware, Self::Model, Self::Sketch, Self::Scene, Self::View];

    /// The number of tool columns the tab's body draws, so the ribbon can size its cells once.
    ///
    /// The count is structural: one per labelled tool and one per numeric field, for the widest state
    /// of the tab, so the cells do not move as the selection changes. It is the number of cells the tab
    /// needs, not the number of groups, because a group holds several tools.
    pub const fn columns(self) -> usize {
        match self {
            Self::Furniture | Self::Hardware => 0,
            Self::Model => 6,
            Self::Sketch => 13,
            Self::Scene => 4,
            Self::View => 3,
        }
    }

    /// The tab's name, drawn on its own button.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Furniture => "Furniture",
            Self::Hardware => "Hardware",
            Self::Model => "Model",
            Self::Sketch => "Sketch",
            Self::Scene => "Scene",
            Self::View => "View",
        }
    }

    /// The stable id the tab's open state and widget name are keyed by.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Furniture => "ribbon_furniture",
            Self::Hardware => "ribbon_hardware",
            Self::Model => "ribbon_model",
            Self::Sketch => "ribbon_sketch",
            Self::Scene => "ribbon_scene",
            Self::View => "ribbon_view",
        }
    }

    /// The groups this tab draws, in row order.
    ///
    /// The tab-to-action rubric of the design: *Sketch* holds the drawing tools, the closed figures
    /// and the close and cancel for a running draft, *Model* the three primitives and the extrusion,
    /// *Scene* the four scene creations, *View* the framing, the projection and the grid.
    /// *Furniture* and *Hardware* hold no action yet, so they have no group and are never drawn.
    pub const fn groups(self) -> &'static [ToolGroup] {
        match self {
            Self::Furniture | Self::Hardware => &[],
            Self::Model => &[ToolGroup::Primitives, ToolGroup::Bodies],
            Self::Sketch => &[ToolGroup::Draw, ToolGroup::Shapes, ToolGroup::Draft],
            Self::Scene => &[ToolGroup::Scene],
            Self::View => &[ToolGroup::View, ToolGroup::Display],
        }
    }
}

/// One captioned group of tools inside a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolGroup {
    /// The open-ended drawing tools.
    Draw,
    /// The closed figures: rectangles, circles and the polygon.
    Shapes,
    /// Close and cancel for a running draft.
    Draft,
    /// Solid primitives and their size.
    Primitives,
    /// Extrusion and its height.
    Bodies,
    /// Panels, assemblies, cameras and lights.
    Scene,
    /// Frame all and the projection.
    View,
    /// The work-plane grid snap.
    Display,
}

impl ToolGroup {
    /// Every group, in the order the asset tree names them.
    pub const ALL: [Self; 8] =
        [Self::Draw, Self::Shapes, Self::Draft, Self::Primitives, Self::Bodies, Self::Scene, Self::View, Self::Display];

    /// The caption drawn under the group's row, in capitals.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Draw => "Draw",
            Self::Shapes => "Shapes",
            Self::Draft => "Draft",
            Self::Primitives => "Primitives",
            Self::Bodies => "Bodies",
            Self::Scene => "Scene",
            Self::View => "View",
            Self::Display => "Display",
        }
    }

    /// The icon theme this group draws from.
    pub const fn theme(self) -> Theme {
        match self {
            Self::Draw | Self::Shapes | Self::Draft => Theme::Contour,
            Self::Primitives => Theme::Primitives,
            Self::Bodies => Theme::Bodies,
            Self::Scene => Theme::Scene,
            Self::View | Self::Display => Theme::View,
        }
    }
}

/// Whether the ribbon's body is open, which tab is active, and the numeric fields it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: bool,
    tab: RibbonTab,
    primitive_size: String,
    extrude_height: String,
    polygon_sides: String,
}

impl Default for Toolbars {
    fn default() -> Self {
        Self {
            open: true,
            tab: RibbonTab::Sketch,
            primitive_size: DEFAULT_PRIMITIVE_SIZE_MM.to_string(),
            extrude_height: DEFAULT_EXTRUDE_HEIGHT_MM.to_string(),
            polygon_sides: DEFAULT_POLYGON_SIDES.to_string(),
        }
    }
}

impl Toolbars {
    /// The body open on the first tab that holds a tool, the state a fresh editor starts in.
    pub fn new() -> Self {
        Self::default()
    }

    /// The tab whose groups the ribbon draws.
    pub const fn tab(&self) -> RibbonTab {
        self.tab
    }

    /// Opens a tab's body, making it the active tab; returns the tab that is now active.
    pub fn set_tab(&mut self, tab: RibbonTab) -> RibbonTab {
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
}

impl Toolbars {
    /// The typed full extent of a new primitive: the cube's edge, or a curved shape's diameter.
    pub fn primitive_size_text(&self) -> &str {
        &self.primitive_size
    }

    /// The size field, so the group can edit the text in place.
    pub fn primitive_size_mut(&mut self) -> &mut String {
        &mut self.primitive_size
    }

    /// Replaces the typed size.
    pub fn set_primitive_size_text(&mut self, text: impl Into<String>) {
        self.primitive_size = text.into();
    }

    /// The typed size as a document length, or a readable reason it is not one.
    pub fn primitive_size(&self) -> Result<Length, String> {
        let length = Length::parse_millimetres(self.primitive_size.trim())
            .map_err(|error| format!("The primitive size is not a length: {error}"))?;
        if length.micrometres() <= 0 {
            return Err("The primitive size must be positive.".to_owned());
        }
        Ok(length)
    }

    /// The typed extrusion height.
    pub fn extrude_height_text(&self) -> &str {
        &self.extrude_height
    }

    /// The height field, so the group can edit the text in place.
    pub fn extrude_height_mut(&mut self) -> &mut String {
        &mut self.extrude_height
    }

    /// The typed extrusion height as a positive document length.
    ///
    /// The upper bound is the document's contour limit, the same one `object.extrude.create`
    /// applies, so a field the group accepts is a field the command takes.
    pub fn extrude_height(&self) -> Result<Length, String> {
        let height = Length::parse_millimetres(self.extrude_height.trim())
            .map_err(|error| format!("The extrusion height is not a length: {error}"))?;
        if !height.is_positive() {
            return Err("The extrusion height must be positive.".to_owned());
        }
        if height.micrometres() > furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() {
            return Err(format!(
                "The extrusion height must be at most {} mm.",
                furnicraft_model::MAX_CONTOUR_COORDINATE.micrometres() / 1_000
            ));
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

    /// The side field, so the group can edit the text in place.
    pub fn polygon_sides_mut(&mut self) -> &mut String {
        &mut self.polygon_sides
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

/// Whether a tab is drawn at all: a tab is drawn exactly when it owns a group.
///
/// The rule is structural, not situational. A group of implemented tools is always drawn and an
/// unavailable tool is disabled with a hint that names what is missing, which is what keeps the
/// ribbon's shape steady across a selection; a tab with no group is not drawn, which is what keeps
/// *Furniture* and *Hardware* out of the row until they own a tool.
fn tab_is_drawn(tab: RibbonTab) -> bool {
    !tab.groups().is_empty()
}

/// Draws the ribbon: one row of tabs and, while it is open, the groups of the active tab.
///
/// Every tab that holds a tool is drawn, and the one whose groups are shown carries the accent.
/// Clicking the active tab, or double-clicking any tab label, toggles the body, so a collapsed ribbon
/// keeps only its tab row. The armed tool draws pressed and a group whose action needs a draft or a
/// viewport that owns its projection is drawn only when it can act; the hint says what is missing
/// when a tool is merely unavailable for the selection.
pub fn show(ui: &mut Ui, state: &mut Toolbars, context: RibbonContext, actions: &mut Vec<ToolAction>) {
    ui.scope(|ui| {
        ui.set_width(ui.available_width());
        ui.add_space(crate::theme::STEP);
        retain_tab(state);
        tab_row(ui, state);
        if state.is_open() {
            ribbon_body(ui, state, context, actions);
        }
        ui.add_space(crate::theme::STEP);
    });
}

/// The outer margin of the ribbon's overlay frame, in points (horizontal, vertical).
///
/// The ribbon floats over the design space instead of taking height from it (U51), so its height and
/// the band the shell reserves for it must agree exactly: this constant is the single source the
/// frame the ribbon draws and [`height`] both read.
pub const RIBBON_FRAME_MARGIN: [i8; 2] = [6, 2];

/// The inner margin of the ribbon body below the tab row, in points (horizontal, vertical).
const RIBBON_BODY_MARGIN: [i8; 2] = [6, 4];

/// The height the ribbon overlay covers over the design space, in points.
///
/// The shell reserves this band for the ribbon and never lets the scene, the side panels or the
/// framing use it, so a translucent ribbon blends with the furniture it covers (U51). It is
/// deterministic from the open state so the strip and the reserved inset agree without a frame of
/// lag when the body opens or closes.
pub fn height(open: bool) -> f32 {
    let outer = 2.0 * f32::from(RIBBON_FRAME_MARGIN[1]);
    // The air band above and below plus the one gap the vertical rhythm puts before the tab row;
    // the body adds its own gap, its margins and the tool row with its caption band.
    let top = 3.0 * crate::theme::STEP;
    let body = if open {
        crate::theme::STEP
            + 2.0 * f32::from(RIBBON_BODY_MARGIN[1])
            + crate::theme::RIBBON_ROW_HEIGHT
            + crate::theme::RIBBON_CAPTION_HEIGHT
    } else {
        0.0
    };
    outer + top + crate::theme::RIBBON_TAB_HEIGHT + body
}

/// Keeps the active tab on a tab that is drawn.
///
/// Only `set_tab` can put the ribbon on an empty tab, so this is one guard rather than a per-frame
/// search: a state that names a tab without a group falls back to the first tab that has one, and a
/// ribbon whose every tab is empty draws nothing.
fn retain_tab(state: &mut Toolbars) {
    if !tab_is_drawn(state.tab())
        && let Some(tab) = RibbonTab::ALL.into_iter().find(|tab| tab_is_drawn(*tab))
    {
        state.set_tab(tab);
    }
}

/// The one row of tabs, each drawn in the ribbon's own rhythm.
fn tab_row(ui: &mut Ui, state: &mut Toolbars) {
    let height = crate::theme::RIBBON_TAB_HEIGHT;
    ui.horizontal(|ui| {
        ui.set_height(height);
        let active = state.tab();
        let open = state.is_open();
        for tab in RibbonTab::ALL {
            if !tab_is_drawn(tab) {
                continue;
            }
            let selected = tab == active && open;
            let response = tab_button(ui, tab, selected, height);
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
fn apply_tab_gesture(state: &mut Toolbars, tab: RibbonTab, double: bool) {
    if double {
        state.set_tab(tab);
        state.toggle();
    } else if tab == state.tab() {
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
fn tab_button(ui: &mut Ui, tab: RibbonTab, selected: bool, height: f32) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let galley = ui.fonts_mut(|fonts| fonts.layout_no_wrap(tab.label().to_owned(), font, egui::Color32::PLACEHOLDER));
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
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, tab.label()));
    }
    response
}

/// The body under the tab row: the active tab's groups in one row, separated by vertical dividers.
fn ribbon_body(ui: &mut Ui, state: &mut Toolbars, context: RibbonContext, actions: &mut Vec<ToolAction>) {
    let groups = state.tab().groups();
    if groups.is_empty() {
        return;
    }
    // One measurement for the whole row, taken before anything is laid out so every cell of every
    // group shares it (ADR 0067): a caption never sizes its own button. It is measured on the widest
    // tab, not the active one, so switching tab does not move the cells.
    let columns = RibbonTab::ALL.into_iter().map(RibbonTab::columns).max().unwrap_or(1);
    egui::Frame::new().inner_margin(egui::Margin::symmetric(RIBBON_BODY_MARGIN[0], RIBBON_BODY_MARGIN[1])).show(
        ui,
        |ui| {
            let cell = crate::theme::RibbonMetrics::measure(ui, columns).cell;
            // One row for the tools and the caption band under it; the divider between two groups is
            // drawn at the height of both, so a group boundary runs the whole body.
            let body_height = crate::theme::RIBBON_ROW_HEIGHT + crate::theme::RIBBON_CAPTION_HEIGHT;
            // A cell has a floor, so a tab of seven tools is wider than a small window; the body scrolls
            // sideways rather than losing its last group to the window edge.
            egui::ScrollArea::horizontal().id_salt("ribbon_body").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_height(body_height);
                    for (index, group) in groups.iter().enumerate() {
                        if index > 0 {
                            crate::theme::divider_of_height(ui, body_height);
                        }
                        show_group(ui, cell, *group, state, context, actions);
                    }
                });
            });
        },
    );
}

/// One group: its row of labelled buttons over the caption that names it.
///
/// The caption is centred on the group's own width, so a vertical divider separates two groups and
/// each caption names the tools above it without spanning the whole ribbon.
fn show_group(
    ui: &mut Ui,
    cell: f32,
    group: ToolGroup,
    state: &mut Toolbars,
    context: RibbonContext,
    actions: &mut Vec<ToolAction>,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let inner = ui.horizontal(|ui| {
            // Cells sit edge to edge inside a group: the whole cell is the button, and the only air
            // between two controls is the padding the cell already carries (ADR 0067). A layout gap
            // on top of that padding is what made the row read as loose plates.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.set_height(crate::theme::RIBBON_ROW_HEIGHT);
            show_group_row(ui, cell, group, state, context, actions);
        });
        let caption = group.label().to_uppercase();
        let galley = ui.fonts_mut(|fonts| {
            fonts.layout_no_wrap(caption, egui::FontId::proportional(10.5), egui::Color32::PLACEHOLDER)
        });
        let width = inner.response.rect.width().max(galley.size().x);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(width, crate::theme::RIBBON_CAPTION_HEIGHT), egui::Sense::hover());
        let pos = egui::pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, crate::theme::text_dim(ui));
    });
}

/// The buttons of one group, before its caption.
fn show_group_row(
    ui: &mut Ui,
    cell: f32,
    group: ToolGroup,
    state: &mut Toolbars,
    context: RibbonContext,
    actions: &mut Vec<ToolAction>,
) {
    let RibbonContext { has_panel_material, selected, selected_path, active, perspective, owns_view, grid, draft } =
        context;
    match group {
        ToolGroup::Draw => {
            for tool in DRAW_TOOLS {
                let action = ToolAction::Draw { tool, sides: state.polygon_sides() };
                let on = is_armed(active, action);
                let caption = tool.label();
                let hint = tool_hint(strip_label(tool), shortcut_label(tool));
                if icons::ribbon_button(ui, cell, tool_icon(tool), caption, &hint, true, on).clicked() {
                    actions.push(action);
                }
            }
        }
        ToolGroup::Shapes => {
            // Every figure but the polygon takes exactly what the ribbon shows; the polygon reads
            // the side field, so the field sits next to it as part of the same group.
            let sides = state.polygon_sides();
            for tool in SHAPE_TOOLS {
                if tool == DrawTool::RegularPolygon {
                    labelled_field(ui, "Sides", "3–128", state.polygon_sides_mut());
                }
                let action = ToolAction::Draw { tool, sides };
                let on = is_armed(active, action);
                let caption = tool.label();
                let hint = tool_hint(strip_label(tool), shortcut_label(tool));
                if icons::ribbon_button(ui, cell, tool_icon(tool), caption, &hint, true, on).clicked() {
                    actions.push(action);
                }
            }
        }
        ToolGroup::Draft => {
            // Close needs a running draft that can close, cancel needs any running draft; both are
            // drawn so the pair never vanishes between clicks and the hint says why one is off.
            let (tool, points, closed) = draft.unwrap_or((DrawTool::Line, 0, false));
            let close = draft_can_close(tool, points, closed);
            let close_hint = if close {
                "Close the draft into a ring".to_owned()
            } else {
                "A draft needs two segments to close".to_owned()
            };
            if icons::ribbon_button(ui, cell, icons::contour::CLOSE, "Close", &close_hint, close, false).clicked()
                && close
            {
                actions.push(ToolAction::CloseDrawing);
            }
            let cancel = draft.is_some();
            let cancel_hint = tool_hint(if cancel { "Cancel the draft" } else { "No draft is running" }, Some("Esc"));
            if icons::ribbon_button(ui, cell, icons::contour::CANCEL, "Cancel", &cancel_hint, cancel, false).clicked()
                && cancel
            {
                actions.push(ToolAction::CancelDrawing);
            }
        }
        ToolGroup::Primitives => {
            labelled_field(ui, "Size", "mm", state.primitive_size_mut());
            let size = state.primitive_size();
            let enabled = size.is_ok();
            let armed_size = size.as_ref().ok().copied();
            let refusal = size.as_ref().err().cloned();
            for (kind, icon, label) in [
                (PrimitiveKind::Cube, icons::primitives::CUBE, "Cube"),
                (PrimitiveKind::Cylinder, icons::primitives::CYLINDER, "Cylinder"),
                (PrimitiveKind::Sphere, icons::primitives::SPHERE, "Sphere"),
            ] {
                let on = armed_size.is_some_and(|size| is_armed(active, ToolAction::CreatePrimitive { kind, size }));
                // The hint carries the field's refusal, so a bad size disables the tool and says
                // why instead of leaving the button silently off.
                let hint = match &refusal {
                    Some(reason) => reason.clone(),
                    None => format!("{label}: click the work plane"),
                };
                if icons::ribbon_button(ui, cell, icon, label, &hint, enabled, on).clicked()
                    && let Ok(size) = size
                {
                    actions.push(ToolAction::CreatePrimitive { kind, size });
                }
            }
        }
        ToolGroup::Bodies => {
            labelled_field(ui, "Height", "mm", state.extrude_height_mut());
            let height = state.extrude_height();
            // Only a closed path encloses a ring, so the button is off for anything else and the
            // hint names what is missing: a selection, a path, a closed chain, or a usable height.
            let hint = match &height {
                Ok(_) => extrude_hint(selected, selected_path).to_owned(),
                Err(reason) => reason.clone(),
            };
            let enabled = matches!((selected, selected_path), (Some(_), Some(true))) && height.is_ok();
            if icons::ribbon_button(ui, cell, icons::bodies::EXTRUDE, "Extrude", &hint, enabled, false).clicked()
                && let (Some(object), Ok(height)) = (selected, height)
            {
                actions.push(ToolAction::ExtrudePath { object, height });
            }
        }
        ToolGroup::Scene => {
            for (icon, label, shortcut, enabled, action) in [
                (icons::scene::PANEL, "Panel", Some("N"), has_panel_material, ToolAction::CreatePanel),
                (icons::scene::ASSEMBLY, "Assembly", None, true, ToolAction::CreateAssembly),
                (icons::scene::CAMERA, "Camera", None, true, ToolAction::CreateCamera),
                (icons::scene::LIGHT, "Light", None, true, ToolAction::CreateLight),
            ] {
                let hint = tool_hint(label, shortcut);
                if icons::ribbon_button(ui, cell, icon, label, &hint, enabled, false).clicked() {
                    actions.push(action);
                }
            }
        }
        ToolGroup::View => {
            if icons::ribbon_button(
                ui,
                cell,
                icons::view::FRAME_ALL,
                "Frame all",
                &tool_hint("Frame all", Some("F")),
                true,
                false,
            )
            .clicked()
            {
                actions.push(ToolAction::FrameAll);
            }
            let (icon, caption) = projection_button(perspective);
            let hint = tool_hint("Switch the projection", Some("O"));
            if icons::ribbon_button(ui, cell, icon, caption, &hint, owns_view, false).clicked() && owns_view {
                actions.push(ToolAction::ToggleProjection);
            }
        }
        ToolGroup::Display => {
            let hint = tool_hint("Grid snap", Some("G"));
            if icons::ribbon_button(ui, cell, icons::view::GRID_SNAP, "Grid snap", &hint, owns_view, grid).clicked()
                && owns_view
            {
                actions.push(ToolAction::ToggleGrid);
            }
        }
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

/// The icon and the short caption of the projection toggle.
///
/// The icon draws the mode the view is *in* and the caption names the switch it would take, so the
/// ribbon never claims a state the camera has not reached.
fn projection_button(perspective: bool) -> (Icon, &'static str) {
    if perspective { (icons::view::PERSPECTIVE, "Orthographic") } else { (icons::view::ORTHOGRAPHIC, "Perspective") }
}

/// A labelled numeric field inside a group: the small name over the field, with its unit as a hint.
///
/// The field is one control in the group's own row, not a separate band: a primitive's size and an
/// extrusion's height read as part of the tool they configure, which is what the owner's reference
/// does and the floating row did not.
fn labelled_field(ui: &mut Ui, label: &str, hint: &str, text: &mut String) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(egui::RichText::new(label).size(10.5).color(crate::theme::text_dim(ui)));
        ui.add(TextEdit::singleline(text).desired_width(64.0).hint_text(hint));
    });
}

/// The hint of the extrude button: what the tool does, or why it is off.
///
/// `selected_path` is `None` when the selection is not a path object, so a path that is merely
/// open gets its own message instead of being called "not a path".
fn extrude_hint(selected: Option<Id>, selected_path: Option<bool>) -> &'static str {
    match (selected, selected_path) {
        (None, _) => "Select a closed path first",
        (Some(_), Some(true)) => "Extrude the selected path",
        (Some(_), Some(false)) => "The selected path is open; close it first",
        (Some(_), None) => "The selected object is not a path",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    /// A context where every implemented group holds a tool and the viewport owns its view.
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

    /// Runs the ribbon with `context` and returns its text shapes and accessible labels.
    fn draw(state: &mut Toolbars, context: RibbonContext) -> (Vec<String>, Vec<String>, Vec<ToolAction>) {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut actions = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, state, context, &mut actions);
        });
        let texts = drawn_text(&output.shapes);
        let labels = accessible_labels(&output);
        output.drop_without_applying_deltas();
        (texts, labels, actions)
    }

    #[test]
    fn the_ribbon_starts_open_on_a_tab_that_holds_a_tool() {
        let mut state = Toolbars::new();
        assert!(state.is_open(), "a fresh ribbon shows its body");
        assert_eq!(state.tab(), RibbonTab::Sketch, "a fresh ribbon opens on Sketch");
        assert!(!state.toggle());
        assert!(!state.is_open());
        assert!(state.toggle());
        assert!(state.is_open());
        assert!(!state.set_open(false));
        assert!(!state.is_open());
    }

    #[test]
    fn every_tab_names_itself_and_its_stable_id() {
        let cases = [
            (RibbonTab::Furniture, "Furniture", "ribbon_furniture"),
            (RibbonTab::Hardware, "Hardware", "ribbon_hardware"),
            (RibbonTab::Model, "Model", "ribbon_model"),
            (RibbonTab::Sketch, "Sketch", "ribbon_sketch"),
            (RibbonTab::Scene, "Scene", "ribbon_scene"),
            (RibbonTab::View, "View", "ribbon_view"),
        ];
        assert_eq!(RibbonTab::ALL.len(), cases.len());
        for (tab, label, id) in cases {
            assert_eq!(tab.label(), label);
            assert_eq!(tab.id(), id);
        }
    }

    #[test]
    fn every_implemented_action_sits_in_exactly_one_group() {
        // The ribbon's promise: no tool is reachable twice and none is lost in the move. Each tab
        // is drawn with a draft running and a view that owns its projection, so every group is
        // present; the hints name the tools, and a tool must appear on exactly one tab.
        let mut context = full_context();
        context.draft = Some((DrawTool::Polyline, 3, false));
        let mut seen: Vec<String> = Vec::new();
        for tab in RibbonTab::ALL {
            if tab.groups().is_empty() {
                continue; // a tab with no group is never drawn, so it owns no tool
            }
            let mut state = Toolbars::new();
            state.set_tab(tab);
            let (_, labels, _) = draw(&mut state, context);
            for label in labels {
                // A tab names itself on the row, so its accessible name is not a tool.
                if RibbonTab::ALL.iter().any(|tab| tab.label() == label) {
                    continue;
                }
                assert!(seen.iter().all(|tool| tool != &label), "{label} is reachable from two tabs");
                seen.push(label);
            }
        }
        for tab in RibbonTab::ALL {
            for group in tab.groups() {
                assert!(!group.label().is_empty(), "{tab:?} has a nameless group");
            }
        }
        // Every tool of the design, under the caption it draws, on the one tab that owns it. The
        // `Close` and `Cancel` of a draft are the one pair whose drawing depends on a live draft.
        let mut expected: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
        for tool in CONTOUR_TOOLS {
            expected.insert(strip_label(tool), "Sketch");
        }
        expected.insert("Close the draft into a ring", "Sketch");
        expected.insert("Cancel the draft", "Sketch");
        for name in ["Cube", "Cylinder", "Sphere"] {
            expected.insert(name, "Model");
        }
        // With nothing selected the extrude button is drawn but off; its hint is the reason.
        expected.insert("Select a closed path first", "Model");
        for name in ["Panel", "Assembly", "Camera", "Light"] {
            expected.insert(name, "Scene");
        }
        expected.insert("Frame all", "View");
        expected.insert("Switch the projection", "View");
        expected.insert("Grid snap", "Display");
        for (tool, tab) in expected {
            assert!(seen.iter().any(|label| label.starts_with(tool)), "{tool} is drawn nowhere; drawn: {seen:?}");
            let mut state = Toolbars::new();
            state.set_tab(match tab {
                "Sketch" => RibbonTab::Sketch,
                "Model" => RibbonTab::Model,
                "Scene" => RibbonTab::Scene,
                "View" => RibbonTab::View,
                _ => RibbonTab::View,
            });
            let (_, labels, _) = draw(&mut state, context);
            assert!(labels.iter().any(|label| label.starts_with(tool)), "{tool} is not on the {tab} tab");
        }
    }

    #[test]
    fn the_body_is_one_tool_row_and_one_caption_band() {
        // The owner's reference is a body of about 90 pt: the tool row, the caption band and the
        // surface's own margins. A body that grows an extra row of fields is a regression.
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Model);
        let ctx = egui::Context::default();
        let mut height = 0.0;
        let mut body_top = None;
        let output = ctx.run_ui(Default::default(), |ui| {
            let before = ui.min_rect().bottom();
            show(ui, &mut state, full_context(), &mut Vec::new());
            height = ui.min_rect().bottom() - before;
            body_top = Some(before);
        });
        output.drop_without_applying_deltas();
        assert!(body_top.is_some());
        let one_row = crate::theme::RIBBON_ROW_HEIGHT + crate::theme::RIBBON_CAPTION_HEIGHT;
        // The tab row, the body row and caption, the air bands above and below the content, and the
        // body's own margins.
        let budget = crate::theme::RIBBON_TAB_HEIGHT + one_row + 6.0 * crate::theme::STEP;
        assert!(height <= budget, "the ribbon body is {height} pt, over its {budget} pt budget");
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
    }

    #[test]
    fn every_group_draws_at_least_one_button() {
        // One group at a time, on the tab that owns it, so a group whose row is empty fails here
        // rather than as an empty band in the ribbon.
        for tab in RibbonTab::ALL {
            for group in tab.groups() {
                let mut context = full_context();
                context.draft = Some((DrawTool::Polyline, 3, false));
                let mut state = Toolbars::new();
                state.set_tab(tab);
                let (_, labels, actions) = draw(&mut state, context);
                assert!(actions.is_empty(), "drawing alone runs no tool");
                assert!(labels.iter().any(|label| !label.is_empty()), "{tab:?} / {group:?} drew no control");
            }
        }
    }

    #[test]
    fn an_empty_tab_is_not_drawn() {
        let mut state = Toolbars::new();
        state.set_open(false);
        let (texts, _, _) = draw(&mut state, full_context());
        assert!(!texts.iter().any(|text| text == "Furniture"), "a tab with no tool is not drawn: {texts:?}");
        assert!(!texts.iter().any(|text| text == "Hardware"), "the empty hardware tab is hidden: {texts:?}");
        for tab in [RibbonTab::Model, RibbonTab::Sketch, RibbonTab::Scene, RibbonTab::View] {
            assert!(texts.iter().any(|text| text == tab.label()), "{tab:?} holds a tool and is drawn");
        }
    }

    #[test]
    fn collapsing_keeps_the_tab_row_and_drops_the_body() {
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Scene);
        let (open_texts, _, _) = draw(&mut state, full_context());
        assert!(open_texts.iter().any(|text| text == "SCENE"), "the group caption is drawn: {open_texts:?}");
        assert!(open_texts.iter().any(|text| text == "Panel"), "the body draws its tools: {open_texts:?}");

        state.set_open(false);
        let (closed_texts, _, _) = draw(&mut state, full_context());
        assert!(closed_texts.iter().any(|text| text == "Scene"), "the tab row survives: {closed_texts:?}");
        assert!(!closed_texts.iter().any(|text| text == "Panel"), "a collapsed ribbon draws no body: {closed_texts:?}");
        assert!(!closed_texts.iter().any(|text| text == "SCENE"), "no caption without a body: {closed_texts:?}");
    }

    #[test]
    fn clicking_the_open_tab_collapses_and_restores_it() {
        let mut state = Toolbars::new();
        assert_eq!(state.tab(), RibbonTab::Sketch);
        apply_tab_gesture(&mut state, RibbonTab::Sketch, false);
        assert!(!state.is_open(), "a click on the open tab collapses the body");
        apply_tab_gesture(&mut state, RibbonTab::Sketch, false);
        assert!(state.is_open(), "the same click restores it");
    }

    #[test]
    fn clicking_another_tab_opens_it_and_a_double_click_collapses_it() {
        let mut state = Toolbars::new();
        apply_tab_gesture(&mut state, RibbonTab::Model, false);
        assert_eq!(state.tab(), RibbonTab::Model);
        assert!(state.is_open(), "switching tabs shows the new body");
        apply_tab_gesture(&mut state, RibbonTab::Model, true);
        assert!(!state.is_open(), "a double-click collapses the body");
        apply_tab_gesture(&mut state, RibbonTab::Model, true);
        assert!(state.is_open(), "a double-click restores it");
        apply_tab_gesture(&mut state, RibbonTab::Scene, true);
        assert_eq!(state.tab(), RibbonTab::Scene, "a double-click also switches tabs");
        assert!(!state.is_open());
    }

    #[test]
    fn an_empty_tab_is_never_the_active_one() {
        // Only `set_tab` can land on a tab without a group; the ribbon repairs that state instead
        // of drawing a bodyless accent, and a ribbon whose every tab is empty draws nothing.
        assert!(!tab_is_drawn(RibbonTab::Furniture));
        assert!(!tab_is_drawn(RibbonTab::Hardware));
        for tab in [RibbonTab::Model, RibbonTab::Sketch, RibbonTab::Scene, RibbonTab::View] {
            assert!(tab_is_drawn(tab), "{tab:?} owns a group");
        }
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Hardware);
        assert_eq!(state.tab(), RibbonTab::Hardware, "the state keeps what it was given");
        let (texts, _, _) = draw(&mut state, full_context());
        assert_eq!(state.tab(), RibbonTab::Model, "the ribbon fell back to the first drawn tab");
        assert!(!texts.iter().any(|text| text == "Hardware"), "an empty tab is not drawn: {texts:?}");
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
        let size = Length::from_millimetres(DEFAULT_PRIMITIVE_SIZE_MM).expect("default size");
        let cube = ToolAction::CreatePrimitive { kind: PrimitiveKind::Cube, size };
        let cases = [
            (None, ToolAction::Draw { tool: DrawTool::Line, sides: 6 }, false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolAction::Draw { tool: DrawTool::Line, sides: 6 }, true),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolAction::Draw { tool: DrawTool::Circle, sides: 6 }, false),
            (Some(ToolActivity::Draw(DrawTool::Line)), ToolAction::CreatePanel, false),
            (Some(ToolActivity::Draw(DrawTool::Line)), cube, false),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), cube, true),
            (
                Some(ToolActivity::Primitive(PrimitiveKind::Cube)),
                ToolAction::CreatePrimitive { kind: PrimitiveKind::Sphere, size },
                false,
            ),
            (Some(ToolActivity::Primitive(PrimitiveKind::Cube)), ToolAction::CreateLight, false),
        ];
        for (activity, action, expected) in cases {
            assert_eq!(is_armed(activity, action), expected, "activity={activity:?} action={action:?}");
        }
    }

    #[test]
    fn the_ribbon_marks_exactly_the_armed_tool() {
        let cases = [
            (None, RibbonTab::Sketch, 0, "an idle ribbon presses nothing"),
            (Some(ToolActivity::Draw(DrawTool::Line)), RibbonTab::Sketch, 1, "the armed Line button is pressed"),
            (
                Some(ToolActivity::Primitive(PrimitiveKind::Sphere)),
                RibbonTab::Model,
                1,
                "the armed Sphere button is pressed",
            ),
        ];
        for (activity, tab, expected, reason) in cases {
            let ctx = egui::Context::default();
            let mut state = Toolbars::new();
            state.set_tab(tab);
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
        state.set_tab(RibbonTab::Model);
        let (_, labels, _) = draw(&mut state, full_context());
        assert!(labels.iter().any(|label| label.contains("Select a closed path first")));
        assert_eq!(state.extrude_height(), Ok(Length::from_millimetres(100).expect("height")));
    }

    #[test]
    fn a_labelled_button_paints_both_its_icon_and_its_caption() {
        // The ribbon's whole reason for a taller row: the glyph and its name travel together. A
        // placeholder asset has no glyph, so this test draws the real Line asset, which is one of
        // the drawn icons in the set.
        let icon = icons::contour::LINE;
        assert!(icon.is_drawn(), "the Line asset is drawn, so the test exercises the image path");
        let ctx = egui::Context::default();
        let output = ctx.run_ui(Default::default(), |ui| {
            icons::ribbon_button(ui, crate::theme::RIBBON_BUTTON_MAX_WIDTH, icon, "Line", "Line  (L)", true, false);
        });
        let texts = drawn_text(&output.shapes);
        let images = count_images(&output.shapes);
        output.drop_without_applying_deltas();
        assert!(texts.iter().any(|text| text == "Line"), "the caption is painted: {texts:?}");
        assert_eq!(images, 1, "the icon is painted as one image");
    }

    #[test]
    fn the_extrude_hint_names_what_is_missing() {
        let node = Id::new(7).expect("id");
        assert_eq!(extrude_hint(None, None), "Select a closed path first");
        assert_eq!(extrude_hint(None, Some(true)), "Select a closed path first");
        assert_eq!(extrude_hint(Some(node), None), "The selected object is not a path");
        assert_eq!(extrude_hint(Some(node), Some(false)), "The selected path is open; close it first");
        assert_eq!(extrude_hint(Some(node), Some(true)), "Extrude the selected path");
    }

    /// The Scene tab, where the Panel, Assembly, Camera and Light buttons live.
    fn scene_context() -> RibbonContext {
        RibbonContext { has_panel_material: true, ..full_context() }
    }

    #[test]
    fn an_open_path_leaves_the_extrude_button_off() {
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Model);
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
        state.set_tab(RibbonTab::Model);
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
    fn the_three_primitive_buttons_are_filled() {
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Model);
        let (_, labels, actions) = draw(&mut state, full_context());
        for label in ["Cube: click the work plane", "Cylinder: click the work plane", "Sphere: click the work plane"] {
            assert!(labels.iter().any(|text| text.contains(label)), "{label} is missing: {labels:?}");
        }
        assert!(actions.is_empty(), "drawing alone runs no tool");
    }

    #[test]
    fn a_panel_without_a_catalogue_material_cannot_create_a_panel() {
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Scene);
        let context = RibbonContext { has_panel_material: false, ..scene_context() };
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
        state.set_tab(RibbonTab::Sketch);
        // A pending arc is a segment the draft has drawn, so a chain of two lines plus the arc can
        // close; the ribbon cannot know it is pending, so two segments is the whole test.
        let context = RibbonContext { draft: Some((DrawTool::Polyline, 2, false)), ..full_context() };
        let (_, labels, _) = draw(&mut state, context);
        assert!(labels.iter().any(|label| label.contains("Close")), "the draft group is drawn: {labels:?}");
        assert!(labels.iter().any(|label| label.contains("Cancel")), "a draft can always cancel: {labels:?}");
    }

    #[test]
    fn the_cell_grows_with_the_window_within_its_band() {
        // The owner's second observation: on a maximised window the reference's buttons get wider. So
        // the cell is not one number — it is measured and clamped, and every tab uses the same one.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let cell = |width: f32| {
            let mut cell = 0.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 600.0))),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                ui.set_width(width);
                cell = crate::theme::RibbonMetrics::measure(ui, RibbonTab::Sketch.columns()).cell;
            });
            output.drop_without_applying_deltas();
            cell
        };
        let small = cell(640.0);
        let large = cell(2560.0);
        assert_eq!(small, crate::theme::RIBBON_BUTTON_MIN_WIDTH, "a narrow window keeps the floor");
        assert_eq!(large, crate::theme::RIBBON_BUTTON_MAX_WIDTH, "a maximised window stops at the ceiling");
        assert!(small < large, "the cells grow with the window, within the band");
        const { assert!(crate::theme::RIBBON_BUTTON_MIN_WIDTH < crate::theme::RIBBON_BUTTON_MAX_WIDTH) };
    }

    #[test]
    fn every_tab_of_the_ribbon_is_counted() {
        // A tab's column count drives the cell, so a tab that undercounts would draw past its share.
        assert_eq!(RibbonTab::Sketch.columns(), 13, "four open tools, six figures, the side field and the draft pair");
        assert_eq!(RibbonTab::Model.columns(), 6, "three primitives, the height field and the extrusion");
        assert_eq!(RibbonTab::Scene.columns(), 4);
        assert_eq!(RibbonTab::View.columns(), 3);
        assert_eq!(RibbonTab::Furniture.columns(), 0, "an empty tab never reaches the body");
        assert_eq!(RibbonTab::Hardware.columns(), 0);
        assert_eq!(RibbonTab::Sketch.groups().len(), 3, "the Sketch tab draws draw, shapes and draft");
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

    /// Counts painted images, the icon half of a labelled button.
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

    #[test]
    fn a_group_divider_spans_the_whole_body() {
        // The owner's complaint: the divider stopped in the middle of the tool row, so a group
        // boundary looked like a stray tick. It is as tall as the tool row plus its caption band.
        let mut state = Toolbars::new();
        state.set_tab(RibbonTab::Sketch);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let body = crate::theme::RIBBON_ROW_HEIGHT + crate::theme::RIBBON_CAPTION_HEIGHT;
        let mut dividers = Vec::new();
        let output = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut state, full_context(), &mut Vec::new());
        });
        collect_vertical_lines(&output.shapes, &mut dividers);
        output.drop_without_applying_deltas();
        assert!(!dividers.is_empty(), "the Sketch tab separates its two groups");
        for height in dividers {
            assert!((height - body).abs() < 0.5, "a group divider is {height} pt tall, the body is {body} pt");
        }
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
}
