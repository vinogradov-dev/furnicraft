# 0040. A constructive contour is a ring of directed segments

- Status: Accepted
- Date: 2026-10-29

## Context

A constructive object was created from a closed polyline: a list of points with no arcs
([0037](0037-a-geometry-object-is-a-mesh-or-a-feature-history.md), schema 10). A cabinet part is
rarely a polygon, though — a countertop corner is round, a shelf has a bowed front — and a curve
approximated by 256 corners is both wrong in a drawing and expensive downstream. The panel track
already proved the shape that is needed: a directed line or a circular arc, stored analytically
([0010](0010-analytic-line-arc-contours.md)).

A contoured object also cannot be edited as corners once it holds an arc: sampling the arc into
points and writing those points back turns the curve into a staircase.

## Decision

- `Contour` is a ring of `ContourSegment` values: a line between two points, or an arc with a
  centre, a positive radius, a start angle and a signed sweep of at most one turn.
- Everything stays in document units: micrometres and millidegrees. The segment type is the single
  boundary where a contour becomes geometry millimetres; callers of `furnicraft-model` never see a
  `furnicraft_geom` type ([0003](0003-length-is-integer-micrometres.md)).
- The ring must be closed — each end equals the next start after rounding to the micrometre — must
  enclose a non-zero area and must not cross itself. A line-only ring is measured and stored exactly
  in integers; a ring with arcs is measured from the exact arc integrals.
- Self-intersection is tested on a chord polyline sampled from the curve, so an arc that cuts a line
  or another arc is found. Adjacent segments are allowed to meet at any angle, including tangentially.
- The constructor stores the ring counter-clockwise, so the face winding of a derived mesh never
  depends on how the user drew it.
- `sampled_points(tolerance)` is the only way a contour becomes a polyline, and it is what the
  extrusion evaluation and the kernel profile use. The document keeps the arc.
- Schema 11 writes `segments`. A schema 10 point list still loads as a ring of lines — the same
  shape the file described — and is saved back as segments.
- The object card shows segment and arc counts. Its corner text editor is disabled for a ring that
  holds an arc, because corners cannot describe the curve; a segment editor belongs to the viewport
  sketch work, not to a text box that would silently flatten it.

## Consequences

- A constructive object can carry the shape a cabinet part actually has, and a rounded part stays
  rounded through save, load, evaluation and collapse.
- A contour is no longer trivially equal to its corner list, so every caller that wants corners asks
  for them explicitly (`is_line_only`, `segments`). Code that assumed a point list was removed rather
  than kept as a second representation.
- The self-intersection test is a bounded polyline check, not an exact analytic intersection: a
  crossing finer than the 0.1 mm chord tolerance can slip through. The bound keeps a hostile file
  from buying unbounded work, and a furniture-scale ring is far above that resolution.
- Editing an arc in the UI waits for the sketch phase; until then an arc can be created by a command
  payload or a file, and the text editor refuses to touch it.
- Holes and arcs in a kernel profile are still not modelled: a profile is sampled to a polyline for
  the kernel port ([0010](0010-analytic-line-arc-contours.md)).

## Related

- [`docs/domain/geometry.md`](../domain/geometry.md) — the contour rules and evaluation.
- [`docs/domain/document.md`](../domain/document.md) — the schema 11 migration.
- [0010](0010-analytic-line-arc-contours.md) — line and arc contours for panel offsets.
- [0037](0037-a-geometry-object-is-a-mesh-or-a-feature-history.md) — the constructive payload.
