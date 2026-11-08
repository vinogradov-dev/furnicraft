# 0073. A drawn figure remembers its shape

- Status: Accepted
- Date: 2026-11-08

## Context
A rectangle, a circle and a regular polygon are drawn as ordinary chains of lines and arcs, and the
document stores only those segments. Editing them well needs more than the segments: dragging a
rectangle corner must keep four right angles, dragging a polygon vertex must keep every side equal,
and a circle has one radius no matter which quadrant the user grabs.

The information needed is not in the geometry. A closed four-line chain that looks like a rectangle
may be any quadrilateral, a 4-gon and a square are the same number of segments, and a circle drawn
as four quarter arcs is indistinguishable from a closed chain of arcs that happens to look round.
Inferring the figure from the numbers is a heuristic that quietly turns a parallelogram into a
rectangle when the user rounds a corner.

Storing the dimensions as well (a radius, a width, a side count) would duplicate the geometry: a
field can then contradict the segments that are supposed to be the truth, and every edit has to keep
both in step.

## Decision
A closed `Path` carries an optional **edit-intent tag**, not parameters:

```rust
pub enum PathShape {
    Rectangle,
    Circle,
    RegularPolygon,
}
```

- Every dimension is derived from the segments; the tag only names the constraint set the grips
  apply (ADR 0075). A side count is the segment count, a radius is the stored arc radius, a width is
  the distance between two stored sides.
- `Path::rectangle`, `Path::rectangle_from_center`, `Path::rectangle_from_center_directed`,
  `Path::circle`, `Path::circle_through`, `Path::circle_three_points` and `Path::regular_polygon`
  set the tag; a polyline, a line and an arc are untagged.
- A **reshape** (`Path::rebuild`, used by `move_corner_at`) keeps the tag: the figure is still that
  figure. A **structural** change (`Path::rebuild_topology` for a split or a removal,
  `Path::rebuild_as` for closing a chain) drops it: the result is a new figure.
- On load the tag is checked against the segments (`PathShape::fits`): a rectangle needs four lines,
  a circle needs arcs that share one centre and one radius and add up to a full turn, a polygon
  needs at least three lines of equal length. A tag that does not fit is a readable
  `PathError::ShapeMismatch`, never a silent downgrade; a file without a tag loads untagged
  (schema 19, older files migrate by absence).

## Consequences
The grips can keep a figure regular without the document ever holding a dimension twice. The cost is
a rule every edit must respect: a reshape keeps the tag and a topology change drops it, and a future
edit kind has to choose one explicitly. A tag that a file claims can still describe a figure the
geometry does not support — that is a load error, which is the honest answer to a corrupt file.
