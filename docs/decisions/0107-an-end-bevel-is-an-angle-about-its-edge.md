# 0107. An end bevel is an angle about its outline edge

- Status: Accepted
- Date: 2026-11-14

## Context

A board that butts a wall turned in two planes cannot meet it with a square end: its end face has to
lie in the wall's plane. A panel is one region extruded along its thickness, so each of its end faces
is perpendicular to the two large faces — the outline can express any angle *in* the panel's plane,
and none *across* it. Without a parameter for that tilt the shaped fill can only leave a gap at the
wall or cut into it.

The kernel already cuts a solid with a prism (`PlanStep::Difference`), and the outline already names
each side by a stable topology reference (`EdgeRef`: a rectangle `EdgeId` or a ring
`ContourSegmentId`), which is what banding keys on.

## Decision

A panel stores at most one end bevel per outline edge: the topology source plus a signed angle in
millidegrees. The outline is the edge on face `B` (`z = 0`); the end face tilts across the thickness,
so face `A`'s edge is set back by `thickness * tan(angle)` towards the middle of the panel.

- **An angle, not a plane.** One angle through the edge line determines the plane exactly, and the
  edge line supplies the rest, so no normal or offset is stored. The value is one a maker can read and
  edit, and it follows the edge when the outline is edited.
- **Positive removes material.** The body never grows past its outline, so the nominal size stays the
  stock size and the nominal outline stays the reference for machining and banding. A negative angle
  is refused rather than unioned on; making a board *grow* at an end is a different construction, and
  which face is the reference belongs to the shaped fill (N12).
- **Straight segments only.** An arc's end face is a ruled surface, not a plane, so a bowed edge and a
  ring arc are refused with a sentence. The angle is capped below 89°, and the record must still
  leave a far face: the core eroded by every bevel's full setback has to stay one positive piece.
- **Banding excludes a bevel on the same edge, in both directions.** A band follows the nominal edge
  and would hang in the air beside the tilted face.
- **The cutter is a wedge prism.** `param` folds one `Difference` step per bevel after the core
  extrusion, so the core region, the analytic rectangle path, the cutouts and the other edges'
  attachments are untouched. The wedge is closed outside the panel on every side that does not cut.
  Its plane sits one micrometre outside the material: a plane exactly through the panel's own edge is
  tangent to that edge, and the kernel answers such a cut with nothing. The difference is below the
  document's own resolution.
- **The record travels with its source.** Baking a rectangle into a ring moves each bevel onto the
  ring segment its edge became, and a ring edit that drops a beveled segment is refused the way a
  banded one is.

## Consequences

A panel's nominal size stays its stock size and every existing reference — cutouts, banding on other
edges, the core region — keeps its meaning; the visible result is a body whose end faces can lie in
the planes a niche or a turned wall asks for. The cost is a second edge record beside banding, with a
rule for each excluding the other, and one boolean step per bevel in the panel plan, which is the
stage where it belongs. A beveled end is not a thickness change: two bevels on opposite edges are
two independent cuts and accumulate. The cap and the far-face check are the guard against an angle
that would eat the panel, and they are refused before the document changes.

If the sign ever has to grow material instead of removing it, that is a reference-face decision
recorded on its own, not a reinterpretation of this angle.

## References

- [Panels and edge banding](../domain/panels.md) — the record, the geometry and the commands.
- ADR [0007](0007-edge-banding-v1.md) — banding, the other record keyed on an outline edge.
- ADR [0045](0045-machining-is-an-ordered-list-of-anchored-operations.md) — the plan a body is folded from.
- ADR [0096](0096-before-the-first-release-the-format-has-no-past.md) — schema 24 carries the record
  and the golden was regenerated with it.
