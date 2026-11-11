//! The ribbon's own state: the fold, the tab, the numeric fields, the pinned method and the list.
//!
//! None of it is document data. The fold and the pin are workspace settings the shell remembers; the
//! tab, the field text and the open list are session view state that resets on restart. The fields
//! parse to [`Length`] here and refuse with a sentence the ribbon draws as the tool's hint, so a
//! number the field accepts is a number the command takes.

use std::collections::BTreeMap;

use furnicraft_text::{Lang, Message};
use furnicraft_units::Length;

use super::list;
use super::spec::{self, FieldKind, GroupSpec, Method, TabSpec, ToolSpec};
use super::{DEFAULT_EXTRUDE_HEIGHT_MM, DEFAULT_PRIMITIVE_SIZE_MM};
use crate::draw::DEFAULT_POLYGON_SIDES;

/// Whether the ribbon's body is open, which tab is active, the numeric fields it keeps, the method
/// each tool is pinned to and the list one cell opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: bool,
    tab: &'static TabSpec,
    primitive_size: String,
    extrude_height: String,
    polygon_sides: String,
    /// The method a click and the shortcut run for a tool that has a list, by tool id. It is the
    /// workspace's setting, not the document's: the editor holds a copy and the shell writes it back.
    pinned: BTreeMap<String, String>,
    /// The variant list a cell opened, if any. Session view state: it is forgotten on restart.
    pub(super) list: Option<list::VariantList>,
}

impl Default for Toolbars {
    fn default() -> Self {
        Self {
            open: true,
            tab: spec::fallback_tab().unwrap_or(&spec::TABS[0]),
            primitive_size: DEFAULT_PRIMITIVE_SIZE_MM.to_string(),
            extrude_height: DEFAULT_EXTRUDE_HEIGHT_MM.to_string(),
            polygon_sides: DEFAULT_POLYGON_SIDES.to_string(),
            pinned: BTreeMap::new(),
            list: None,
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
    ///
    /// A tab change closes an open variant list: the cell that owns it is no longer drawn, and a
    /// list that outlived its tab would reappear on the next visit unasked (U67).
    pub fn set_tab(&mut self, tab: &'static TabSpec) -> &'static TabSpec {
        self.tab = tab;
        self.list = None;
        tab
    }

    /// Whether the body under the tab row is drawn.
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the body and returns its new state.
    pub fn set_open(&mut self, open: bool) -> bool {
        self.open = open;
        if !open {
            self.list = None;
        }
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
    pub(super) fn retain_tab(&mut self) {
        if !self.tab.is_drawn()
            && let Some(tab) = spec::fallback_tab()
        {
            self.tab = tab;
        }
    }

    /// Keeps an open variant list only while its own cell is drawn.
    ///
    /// The list dies with its cell: a folded ribbon, a tab that does not hold the tool, or a tool
    /// that is no longer live leaves nothing to anchor the popup to, so the state is dropped here
    /// instead of drawing a list under a cell the user can no longer see.
    pub(super) fn retain_list(&mut self) {
        let Some(open) = self.list else { return };
        let drawn =
            self.tab.live_groups().flat_map(GroupSpec::tools).any(|tool| tool.id == open.tool && tool.is_live());
        if !self.open || !drawn {
            self.list = None;
        }
    }
}

impl Toolbars {
    /// The methods the workspace pinned, by tool id: the one state of the list that outlives a run.
    pub const fn pinned_variants(&self) -> &BTreeMap<String, String> {
        &self.pinned
    }

    /// Replaces the pinned methods, the copy the shell hands a new tab from the settings file.
    pub fn set_pinned_variants(&mut self, pinned: BTreeMap<String, String>) {
        self.pinned = pinned;
    }

    /// The index of the method a click and the shortcut run for `tool`.
    pub fn pinned_index(&self, tool: &'static ToolSpec) -> usize {
        tool.pinned_index(&self.pinned)
    }

    /// Pins one method of a tool: a plain click and the tool's key run it from now on.
    pub fn pin(&mut self, tool: &'static ToolSpec, method: Method) {
        self.pinned.insert(tool.id.to_owned(), method.id.to_owned());
    }

    /// The variation list a cell opened, if any.
    pub const fn list(&self) -> Option<list::VariantList> {
        self.list
    }

    /// Whether this tool's list is the open one.
    pub fn list_open_for(&self, tool: &'static ToolSpec) -> bool {
        self.list.is_some_and(|list| list.tool == tool.id)
    }

    /// Opens a tool's list; a tool with one method has nothing to open and reports `false`.
    pub fn open_list(&mut self, tool: &'static ToolSpec) -> bool {
        if !tool.has_list() {
            return false;
        }
        self.list = Some(list::VariantList::new(tool));
        true
    }

    /// Closes the list, whichever tool opened it.
    pub fn close_list(&mut self) {
        self.list = None;
    }

    /// Opens the tool's list, or closes it when it is already the open one.
    pub(super) fn toggle_list(&mut self, tool: &'static ToolSpec) {
        if self.list_open_for(tool) {
            self.close_list();
        } else {
            self.open_list(tool);
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
