# 0067. A ribbon tool is one measured cell, and a caption never sizes it

- Status: Accepted
- Date: 2026-11-07

## Context
U39 put the tools on a ribbon and gave each one an icon over a caption, but the button took its width
from the caption's own layout: `galley.width + 2 * button_padding`, with a floor of one icon. The row
that produced had buttons from 41 to 83 pt wide, so `Rectangle` pushed its neighbours aside and `Arc`
left a gap; the widths changed again when the window was maximised, because nothing tied them to the
row. The dividers between the groups used `theme::divider`, the 16 pt header hairline, so a group
boundary stopped a third of the way down a 48 pt row. The header had the same two defects in
miniature: `File` was a text button 34 × 20 next to icon buttons 26 × 26, and its hairlines were
16 pt tall beside 26 pt controls.

The owner's reference settles what "right" is: every tool of a group is the same width, the row keeps
one pitch, a caption too long is cut with `…` (`Выдавли…`, `Враг…`), the group divider runs the whole
body, and a maximised window makes the cells wider — up to a point, not without one.

## Decision
`crates/ui/src/theme.rs` owns the numbers and the measurement:

- `RIBBON_BUTTON_MIN_WIDTH` and `RIBBON_BUTTON_MAX_WIDTH` bound the cell, and `RIBBON_BUTTON_PADDING`
  is the air inside it;
- `RibbonMetrics::measure(ui, columns)` takes the width available to the body, subtracts the group
  hairlines and the cell padding, divides by the number of columns, and clamps the result to the band.
  The columns are the **widest** tab's count (`RibbonTab::columns`), not the active tab's, so switching
  tab never moves the cells;
- `RIBBON_ROW_HEIGHT` is the cell's height, `RIBBON_CAPTION_GAP` the air between the glyph and the
  caption, and the icon-plus-caption block is centred in the cell rather than pinned to the top, so a
  one-line and a two-line caption share their optical centre;
- the floor is derived from the longest caption the interface will ever draw: the Russian
  `Прямоугольник` at the 10.5 pt caption size measures 91 pt, and U49's longest caption measures
  98, so the cell floor is 108, not a guess.

`icons::ribbon_button` takes that width and paints one cell for every caption:

- the caption is laid out in a `LayoutJob` of `RIBBON_CAPTION_ROWS` (1) rows at the cell's inner width,
  so a caption that does not fit is elided with `…` by `epaint` itself and never widens the button;
- a cell at rest is transparent, so a row of tools reads as one surface; a pointer or the armed tool
  gives it the hover fill or the selection fill, which is the reference's plinth;
- a placeholder asset draws its caption in the same cell, because a half-drawn icon set must not leave
  a ragged row.

The header follows the same rule with its own numbers: `HEADER_CONTROL_HEIGHT` (26) is one control, an
icon button and the `File` menu button both take it (`icons::icon_button_sized`), and
`theme::divider_of_height` draws every hairline as tall as the controls it separates. The ribbon calls
it with the tool row plus the caption band, so a group divider runs the whole body.

A cell has a floor, so a tab of seven tools can be wider than the window: the body scrolls sideways
(`ScrollArea::horizontal`) rather than clipping the last group.

## Consequences
The row is one pitch at every window width, a colour change or a new caption no longer moves a
neighbour, and the longest caption the interface will ever draw — 98 pt at the 10.5 pt size —
fits the 108 pt floor, which is the U49 requirement. The costs are a second measurement per frame
(one division and a clamp), a horizontal scroll on a very narrow window, and the rule that a cell's
width is a property of the ribbon rather than of a button, which every caller now has to respect.

The cells also grow with the window, which the owner asked for and the reference does: at a maximised
window the row reads as eight wide plates with one pitch, at a small one it keeps the floor and the body
scrolls sideways. Nothing about the width reaches the document.

Revisited only if a tool needs a caption longer than the floor's inner width: the fix then is to
widen `RIBBON_BUTTON_MIN_WIDTH` and accept a wider ribbon, not to let the caption size its button.
