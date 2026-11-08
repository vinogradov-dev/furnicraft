# 0076. A drawing tool separates the segment kind from the snaps

- Status: Accepted
- Date: 2026-11-08

## Context
The first drawing tools overloaded Alt: a click with Alt held started an arc through the point, so a
polyline mixed straight and curved segments without leaving the tool. The shape set grew to arc from
centre, circle through three points, rectangle from its centre, a directed rectangle and a regular
polygon. Every one of them is a distinct construction with its own point count, and a modifier is a
poor place to hide a construction the user cannot see in the ribbon.

At the same time the owner asked for a snap override: every click snaps to a draft end or the grid by
default, Shift already locks a segment to an axis, and Alt is the conventional key for "do not snap
this one". One key cannot both choose an arc and suppress the snaps.

## Decision
A drawing tool reads its input in two independent layers.

- **Tab chooses the segment kind.** In a polyline it flips the next segment between a straight line
  and an arc; the chosen kind stays until Tab flips it back. The closing click uses the same kind,
  and an arc closing click starts at the stored end of the previous segment. Other tools have one
  construction and ignore Tab.
- **Alt only suppresses the snaps.** A click with Alt held skips the draft-end capture and the grid,
  and takes the pointer where it is. Shift's axis lock survives, because it is an explicit request
  for a straight run rather than an implicit snap.
- **A second construction is a second tool.** The directed rectangle is a separate ribbon button
  with its own icon and shortcut, not a hidden mode of the rectangle-from-centre tool. A shifted
  letter is the alternate of the same family (`Shift+R` beside `R`), and the shortcut table remains
  the one the button hints and the key handler share.
- **A build ends or is cancelled explicitly.** Escape and the right button finish the figure and
  commit it; Ctrl+Esc and the ribbon's Cancel button throw the draft away. A committed figure opens
  its own contour edit mode at once, so the shape can be reshaped without a second gesture.

## Consequences
The gesture and the snap state stop competing for one key, and every construction is visible in the
ribbon. The cost is more buttons in the Sketch tab (the row scrolls sideways rather than shrinking
cells) and one rule to remember: Tab belongs to the polyline, every other tool is complete on its
last click. Escape no longer discards a draft, which is why the status bar, the ribbon hint and the
key handler all name Ctrl+Esc as the cancel.
