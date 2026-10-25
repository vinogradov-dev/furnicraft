# 0016. Arc band joints use the same rules as straight ones

- Status: Accepted
- Date: 2026-10-25

## Context

ADR 0015 extends and trims band strips at sharp joints, but it deliberately stopped at straight
segments: an arc or radius band kept its butt joint because intersecting a line with a circle and two
circles with each other was not implemented. A panel with a bowed edge or a radius corner therefore
still shows a step where a band meets its neighbour, and the step depends on the direction in which
the two boundaries were offset.

## Decision

Every band strip is built from a pair of boundary segments: an included band between its nominal
segment and the core offset, a non-included band between the outward offset and its nominal segment.
The joint rule runs on those segments, whatever their kind:

- the supporting curves of the two boundaries and the neighbouring face are intersected
  analytically — line-line, line-arc and arc-arc, up to two candidate points;
- a candidate is accepted only if it lies on the neighbouring face's own span. For a straight face
  the supporting line stands for the span, as before; for an arc face the point must be on its
  sweep, with a `1e-6 mm` tolerance at the endpoints;
- the nearest accepted candidate wins, and the strip is trimmed or extended to it with the same
  cap of `50 mm` from the nominal endpoint;
- an arc may only be extended by a quarter turn (`geom::MAX_ARC_EXTENSION`), so a joint can never
  wrap around the supporting circle and claim a point on the far side;
- an included band with no usable earlier face keeps its radial butt joint at the nominal endpoint;
  a non-included band is mitred against the adjacent segment, and falls back to its own endpoint
  when that mitre does not exist.

The end cap of a strip stays a straight connector between the two boundary points, so the strip is a
quadrilateral in the line case and a generalised strip in the arc case.

## Consequences

- Bowed edges and radius corners now close the same hanging step that straight joints do, and the
  later band is visible over the earlier one on any pair of segment kinds.
- A band on an arc is no longer a plain offset band: its ends depend on the neighbours, so the same
  bow can produce a slightly different strip depending on the band order. That is the intended rule.
- The core outline at a line-arc or arc-arc joint still joins per-segment offsets with a straight
  connector. Only the band strips are intersected analytically; the shaped-core tracer is a separate
  step and will need its own joint handling.
- Revisit if a joint needs a curved end cap (a true mitre between two arcs) instead of the straight
  connector, or if the quarter-turn extension cap rejects a legitimate deep joint.
