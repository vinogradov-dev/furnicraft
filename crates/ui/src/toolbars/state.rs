//! The ribbon's own state: the fold, the tab, the typed defaults, the pinned method and the list.
//!
//! None of it is document data. The fold and the pin are workspace settings the shell remembers; the
//! tab, the defaults and the open list are session view state that resets on restart. The values are
//! typed ([`Length`], a side count), so the ribbon never parses a number and a tool always has one.

use std::collections::BTreeMap;

use furnicraft_units::Length;

use super::list;
use super::spec::{self, GroupSpec, Method, TabSpec, ToolSpec};
use super::{DEFAULT_EXTRUDE_HEIGHT_MM, DEFAULT_PRIMITIVE_SIZE_MM};
use crate::draw::DEFAULT_POLYGON_SIDES;

/// Whether the ribbon's body is open, which tab is active, the typed defaults it remembers, the
/// method each tool is pinned to and the list one cell opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolbars {
    open: bool,
    tab: &'static TabSpec,
    /// The full extent of a new primitive, typed once at construction.
    primitive_size: Length,
    /// The height the next extrusion starts at.
    extrude_height: Length,
    /// The side count a new regular polygon starts with.
    polygon_sides: u32,
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
            primitive_size: Length::from_micrometres(DEFAULT_PRIMITIVE_SIZE_MM.saturating_mul(1_000)),
            extrude_height: Length::from_micrometres(DEFAULT_EXTRUDE_HEIGHT_MM.saturating_mul(1_000)),
            polygon_sides: DEFAULT_POLYGON_SIDES,
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
            self.tab.live_groups().flat_map(GroupSpec::live_tools).any(|tool| tool.id == open.tool && tool.is_live());
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
    /// The full extent a new primitive is built at: the cube's edge, or a curved shape's diameter.
    pub const fn primitive_size(&self) -> Length {
        self.primitive_size
    }

    /// The height the next extrusion starts at.
    pub const fn extrude_height(&self) -> Length {
        self.extrude_height
    }

    /// Remembers the height a finished extrusion used, so the next one starts there.
    pub fn set_extrude_height(&mut self, height: Length) {
        self.extrude_height = height;
    }

    /// The side count a new regular polygon is built with.
    pub const fn polygon_sides(&self) -> u32 {
        self.polygon_sides
    }
}
