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
  keep every edge-banding and corner-banding record. A chamfer or radius does not move, duplicate or
  drop a record on an adjacent edge.
- Corner banding may be assigned to any of the four corners, with or without a treatment. On a point
  corner the record is kept but has no surface and is not drawn.
- A shaped panel may keep its machining. The parametric layer subtracts the cutout tools from the
  shaped body with the same 3D boolean it uses for a rectangular core.
- Only invalid geometry still rejects a command: an outline whose offsets self-intersect, a corner
  treatment that consumes its edge, or a non-positive core.

The tangent-path grouping remains for banding *application* and for building a strip; it is no longer
a precondition for a geometry change.

## Consequences

- Geometry is always editable; no "clear banding and machining" gate remains in the panel card.
- Existing records survive a shape change and keep their edge or corner id. Undo restores the whole
  edit in one step.
- A banded path can become partially banded after a tangent radius; the panel still resolves and the
  first record on the path drives the current strip. Per-segment banding assignment and strips are the
  next step.
- Adding new machining to a shaped panel stays rejected until the contour-aware cutout union lands;
  shaping a panel that already has machining is now allowed.
