# 0051. Snapping composes the grid, the axis and the ends of the draft

- Status: Accepted
- Date: 2026-11-04

## Context

A pointer point on the work plane ([0050](0050-the-work-plane-follows-the-view.md)) is almost never
a usable coordinate: the user wants a cabinet at a round 10 mm, an edge exactly horizontal, or the
next segment to close where the first one started. The drawing tools therefore need three different
constraints, and the viewport preview needs to know which one captured the pointer.

Treating every constraint as one exclusive mode would lose the most common combination: a grid point
on a locked horizontal axis. Treating them as independent would let a grid rounding move a joined
point off the joint it just joined.

## Decision

- Snapping is pure and lives in `apps/furnicraft/src/gizmo_math.rs`: `SnapRequest` in, `SnappedPoint`
  and `SnapTarget` out, all coordinates in work-plane millimetres. It touches no document, no
  revision and no undo stack.
- The grid step is 10 mm (`SNAP_GRID_MM`), rounded on both plane axes.
- The capture radius around a draft end arrives from the caller; only the camera knows how many
  millimetres one viewport point covers at the current depth. A radius that is not finite and
  positive captures nothing.
- The constraints apply in a fixed order: a draft end inside the radius wins, then the axis lock,
  then the grid. A join never moves off the joint it joins.
- The grid and the axis lock compose. The axis through the previous point stays exact — its locked
  coordinate is the anchor's — and only the free coordinate is rounded to the grid.
- `SnapMode::{Free, Grid}` is the grid switch. The ends of the draft and the axis lock stay available
  in both modes, because they answer a different question; free means "no grid", not "no snap".
- Ties are deterministic: the nearest end wins, and an earlier entry breaks an equal distance; an
  axis tie locks to `u`.
- `SnapTarget` records why the point moved so the viewport can draw a marker, and it is data only.

## Consequences

- The drawing tool owns the mode, the axis switch and the radius; the viewport owns the camera scale
  that turns a screen radius into a plane radius.
- A later snap to panel vertices, edges or intersections extends the request and `SnapTarget`; it
  does not add a second snapping path or touch the document.
- A very smooth zoom changes the capture radius in plane millimetres, which is why the caller, not
  this module, computes it.
