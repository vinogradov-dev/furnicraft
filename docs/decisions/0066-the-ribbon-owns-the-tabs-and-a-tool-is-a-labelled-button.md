# 0066. Tools are reached through a ribbon of tabs, and a tool is a labelled button

- Status: Accepted
- Date: 2026-11-07

## Context
U38 gave the window one theme, a header and a status bar, but the tools still lived where the first
prototype put them: four collapsible sections inside the scene tree. That cost the tree its meaning —
it is the document's tree, not a toolbox — and it cost the tools their names: a 28 pt row could hold
a glyph and nothing else, so every tool was a tooltip away. The owner sent a reference where the
tools sit in a ribbon under the header, the groups of the active tab in one row, and every tool
carries its own caption under its glyph.

Two questions had to be answered. Where does a tool live when it belongs to a surface the design has
not built — *Furniture*, *Hardware* — and how does a row tall enough for a caption relate to the
icon-only rows the header and the panel strips already had?

## Decision
The ribbon is `crates/ui/src/toolbars.rs`, drawn in a `Panel::top` between the header and the side
panels:

- the top level is a tab — *Furniture, Hardware, Model, Sketch, Scene, View* — and a tab is drawn
  only when it owns a group, so *Furniture* and *Hardware* are absent until a tool lands there and no
  greyed "coming soon" tab ever appears;
- a tab is a list of groups, and a group is a row of buttons over a 10.5 pt caption in capitals; the
  groups of a tab are separated by `theme::divider`, so one row holds the whole tab;
- a button is `icons::ribbon_button`: a 22 pt icon over a 10.5 pt caption on a 48 pt row. The header,
  the panel headers and the floating viewport cluster keep `icons::tool_button`, the icon-only button,
  because a caption under a 26 pt row would double it;
- every tool that already existed keeps its one action, and the mapping is explicit — *Sketch* holds
  the five drawing tools plus close and cancel, *Model* the three primitives and the extrusion,
  *Scene* the four scene creations, *View* the framing, the projection and the grid snap. The numeric
  field of a group (primitive size, extrusion height) sits inside that group, not on a row of its own;
- the armed tool draws pressed where it lives, and the container armed to receive the next creation is
  named by a chip in the status bar, not by the ribbon;
- the open body, the active tab and the fields are view state on `Toolbars`. Nothing here is a
  command, a document field or an undo entry.

A tab that owns a group is always drawn, and an unavailable tool is disabled with a hint that names
what is missing; that keeps the ribbon's shape steady across a selection change. Clicking the open
tab collapses the body and clicking it again restores it, and a double-click does the same from any
tab, so a collapsed ribbon keeps only its tab row.

## Consequences
The scene tree holds the scene again, and the window's top edge reads as one control: header, ribbon,
content. The tooling that the design has not built yet has a place to land without a placeholder in
the UI, which is a promise the tab table keeps for us.

The cost is a second button shape in the UI crate, and the rule that decides which one a surface uses
is its row height, not its importance. The ribbon also reads view state from the shell — the
projection, whether the viewport owns it, the grid snap — through `RibbonView`, and returns one
`RibbonViewAction`, so it neither owns the camera nor a second way into it.
