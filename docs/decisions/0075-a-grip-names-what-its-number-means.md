# 0075. A grip names what its number means

- Status: Accepted
- Date: 2026-11-08

## Context
Every drawn figure needs handles, and the same handle must work the same way whatever shape it is
on. A circle has one radius, a rectangle a width and a height, a polygon a circumradius, an arc a
radius and an end angle. A number typed while dragging a handle has to mean the dimension that
handle belongs to, not a screen distance.

The figure already carries the shape it was drawn as (ADR 0073), and the model already exposes
setters for that shape's dimensions. What is missing is a mapping from a handle to a position and to
that dimension, kept in one place so painting, hit-testing, dragging and typing cannot disagree.

## Decision
The UI owns a **grip model**, and a grip answers two questions: where it sits and what a number at it
measures.

```rust
pub enum ContourGrip {
    Corner(PathCorner),
    CircleCenter, CircleQuadrant(u8),
    ArcCenter(ContourSegmentId), ArcPoint(ContourSegmentId), ArcRadius(ContourSegmentId, [Length; 2]),
    RectangleCenter, RectangleMidpoint(u8),
    PolygonCenter, PolygonRadius,
}
```

- **The list follows the shape.** An untagged ring or chain draws one handle per corner, plus the
  free end of an open chain and the centre and point-on-curve of each arc. A tagged circle draws its
  centre and four quadrants, a rectangle its corners, side midpoints and centre, a polygon its
  vertices, centre and one radius point between the first two corners.
- **A centre translates and takes no number.** Numeric entry belongs to a dimension: a circle
  quadrant, an arc point or a hovered arc curve gives a radius; an arc end gives an angle; a
  rectangle midpoint gives `W` on sides 1 and 3 and `H` on sides 0 and 2; a polygon radius point
  gives its circumradius.
- **Dragging is a separate call.** `ContourDraft::drag_grip` routes a grip to the model setter that
  keeps the figure: a rectangle corner pins the opposite corner and keeps four right angles, a
  rectangle midpoint moves one side and leaves the opposite one, a circle quadrant scales the
  circle, a polygon vertex keeps every side equal, a centre translates the whole figure, an arc
  midpoint (`ArcPoint`) rebuilds the arc through the two endpoints and the new midpoint, and a
  hovered arc curve (`ArcRadius`) and a corner on an arc keep the arc's endpoints and move the
  centre and radius.
- **Free movement is the default; the contour is a toggle.** A corner drag is free unless the status
  bar's switch is on. Then the segment the pointer is nearest at the drag start becomes the support,
  and the model projects onto it for the rest of the drag; the target is not re-resolved when the
  pointer crosses to the other segment at a junction.
- **A number follows the cursor.** Drag start opens a small field next to the pointer, prefilled
  with the grip's current value and labelled with its dimension. Enter parses it through
  `furnicraft-units` and applies it through the same setter the drag uses, without leaving the edit
  mode; Escape puts the value the drag started from back. A refused number leaves the field open and
  the draft untouched.
- **An arc curve offers its radius on hover.** The pointer over an arc creates a transient radius
  grip at the point on the curve; the persistent midpoint handle and the corner handles still win
  when the pointer is on them.

## Consequences
Painting, hit-testing and typing read one list, so a handle cannot be drawn where it cannot be
grabbed or mean a dimension it does not have. The model keeps every figure constraint in one place
and the numeric setters are the only writers, which is what makes Enter and a drag equivalent.

The cost is that a ring has no arc grip yet: a panel outline and a cutout expose corners only,
because the shape tag lives on `Path` and a ring may mix arcs from several figures. The arc curve
hover is derived from the pointer every frame rather than stored, so it is not a stable handle and
does not survive leaving the curve.
