# 0091. The ribbon is one const map, and one tool is one glyph in a 34 x 30 cell

- Status: Accepted; supersedes the cell metrics and the caption rule of 0067
- Date: 2026-11-12

## Context

`toolbars.rs` described the ribbon three times: `RibbonTab` named the tabs, `ToolGroup` named the
groups and `show_group_row` matched the group again to draw its tools. Adding one tool meant an arm in
each of the three tables, and the map the owner approved could not be written down at all — twenty-two
tools in *Sketch*, namespaced ids, groups with no command yet, and a column count hardcoded per tab
(`0/6/13/4/3`) so the cell measurement and the drawing could disagree without anyone noticing.

The owner also settled a new look (`plan/notes/U71.md`): five noun tabs — *Sketch, Model, Furniture,
Scene, View* — a 28 pt tab row, a 30 pt tool row of 34 x 30 cells holding a 22 pt glyph, a 12 pt
caption band in the darker chrome under each group, so the open ribbon is 72 pt and the folded one is
29 pt. Tools carry no caption of their own: the name lives in the hint and, from U67 on, in the list
under the cell.

ADR 0067 derived the cell from the longest caption (108–112 pt) so that a caption could never size its
own button. With no caption under the glyph that floor has nothing left to protect.

## Decision

`crates/ui/src/toolbars/spec.rs` holds the map as constants, one level per row:

- `TabSpec { id, label, groups }`, `GroupSpec { id, label, cells }`, `CellSpec::Tool(ToolSpec)` or
  `CellSpec::Field(FieldSpec)`, `ToolSpec { id, label, icon, shortcut, run, variants }` and
  `VariantSpec { id, label, icon, run }`. Every id is namespaced by its parent — `sketch`,
  `sketch.create`, `sketch.create.line`, `sketch.create.line.angle` — so nothing depends on a position
  in the table;
- `run: Option<ToolKind>` is the one code a row carries: what a click asks for. The data of an action
  (a size, a height, the selected path) is not stored; the renderer reads it from the ribbon state and
  the context when the cell is drawn, so a row is one constant for every document;
- a tool with no `run` and no live variant is **planned**. A planned tool is not drawn, a group of only
  planned tools is not drawn with its caption, and a tab with no live group is not drawn at all — the
  map can therefore record the whole approved design without a single greyed "coming soon" row;
- whether a live tool can run *here* stays code: `ready(kind, …)` matches the `ToolKind` once against
  `RibbonContext` and returns the enabled state, the pressed state, the hint and the action. A refusal
  is the hint, so a disabled cell says what is missing;
- the body is one loop over the active tab's live groups, and its width is counted from the spec
  (`columns()`), not from a per-tab constant. Each cell is `RIBBON_CELL_WIDTH` (34) wide and
  `RIBBON_ROW_HEIGHT` (30) tall, a field takes two cells, and a group widens to its caption when the
  caption is wider than its cells;
- the drawn height is the metric itself: `height(open)` is the tab row plus one hairline, plus the tool
  row, the caption band and the closing hairline when the body is open — 29 or 72 points, with
  `RIBBON_FRAME_MARGIN`'s vertical margin at zero so the strip fills the band it reserves exactly;
- the fold is a workspace setting (`ribbon.open`, `#[serde(default)]`, on by default). A tab that folds
  the ribbon folds it in every open scene and the shell writes the setting on that frame; the active
  tab and the numeric fields stay session view state.

## Consequences

Adding a tool is one row and, for a command that did not exist yet, one `ToolKind` arm — the tests fail
if an id repeats, if a live tool has no artwork, or if a `ToolKind` is reachable twice or not at all.
The map now reads as the design document it is: the owner's groups, in order, including the rows that
are only planned.

The costs are deliberate. A tool is named by its hint and by U67's list rather than by a caption, so
discoverability rests on the tooltips until that list lands; the caption-derived cell floor, the elision
machinery and the horizontal scroll area are gone, because the widest row of the map is ten cells —
about 400 points, well inside the 640-point minimum window; and a group of one tool widens to its
caption, which is the one place the drawn row is wider than its cells.

Revisited when a tab grows past what one 30 pt row can hold: the answer then is a second row, not a
wider cell.
