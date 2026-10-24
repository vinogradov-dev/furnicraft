# 0013. Geometry edits are never blocked by banding or machining

- Status: Accepted
- Date: 2026-10-24

## Context

The panel card disabled every outline control while a panel had any edge or corner banding or any
machining, and the model rejected some shape changes with `IncompatiblePathBanding`,
`CornerBandingRequiresChamfer`, `CornerFeatureHasBanding` or `UnsupportedContourMachining`. A corner
radius was therefore impossible on a banded panel even when the edit touched no banded edge, and any
shape change was impossible on a milled panel.

The owner's model is segment-first: every end of the panel is a contour segment with its own id and
its own banding. A chamfer adds a new contour segment and leaves the two adjacent edges and their
banding intact. A corner record is valid even when the corner is a point and has no surface to draw
on.

## Decision

A geometry command is accepted whenever the resulting outline is geometrically valid. Banding and
machining never reject it.

- `panel.corner.set`, `panel.corner.clear`, `panel.bow.set`, `panel.bow.clear` and `panel.resize`
  keep every edge-banding record. A chamfer or radius does not move, duplicate or drop a record on an
  adjacent edge.
- A banding belongs to a contour segment. Corner banding is assigned only while the corner has a
  chamfer or radius segment; changing the treatment between a chamfer and a radius keeps it, and
  removing the treatment removes the banding with the segment.
- A shaped panel may keep its machining. The parametric layer subtracts the cutout tools from the
  shaped body with the same 3D boolean it uses for a rectangular core.
- Only invalid geometry still rejects a command: an outline whose offsets self-intersect, a corner
  treatment that consumes its edge, or a non-positive core.

The tangent-path grouping remains for banding *application* and for building a strip; it is no longer
a precondition for a geometry change.

## Consequences

- Geometry is always editable; no "clear banding and machining" gate remains in the panel card.
- Edge records survive a shape change and keep their edge id. Corner records survive a change between
  a chamfer and a radius and are deleted when the corner becomes a point. Undo restores the whole edit
  in one step.
- A banded path can become partially banded after a tangent radius; the panel still resolves and the
  first record on the path drives the current strip. Per-segment banding assignment and strips are the
  next step.
- Adding new machining to a shaped panel stays rejected until the contour-aware cutout union lands;
  shaping a panel that already has machining is now allowed.
