# 0072. A contour joint is judged with the drawing tolerance

- Status: Accepted
- Date: 2026-11-08

## Context
Two values describe a joint between consecutive segments, and they disagreed. `Path` compares the
end of one segment with the start of the next through `within_joint_tolerance`, a bound of one
micrometre (`PATH_JOINT_TOLERANCE_MICROMETRES`). `Contour` compared the same two points with `==`
and refused anything that was not exactly equal (`Contour::assemble`).

The equality held only for whole-micrometre lines that the user placed exactly. An arc is stored as
a centre, a radius, a start angle and a sweep, so its endpoints are **derived**: the centre is
rounded to a micrometre, the radius to a micrometre and the angles to a millidegree, and the point
that comes back can miss the drawn one by a micrometre or two. A polyline closed with an arc, and an
arc edited against a neighbouring segment, therefore produced "path segment 2 does not meet the next
segment" for a figure the user had drawn correctly, while `Path` had already accepted the same
joint. The arc-arc limitation in `plan/debt.md` is the same defect.

## Decision
One rule, used everywhere a joint is checked:

- `Path::assemble` (already) and `Contour::assemble` compare with `within_joint_tolerance`, the same
  one-micrometre bound;
- `Contour::to_geometry` already hands the ring to the geometry layer with
  `DOCUMENT_JOINT_TOLERANCE_MM = 1e-3`, the same micrometre, so the boundary accepts what the
  document accepted;
- the geometry ring keeps its own outward tolerance, and the error keeps its wording, so a real gap
  still reads as "contour segment N does not meet the next segment".

The tolerance is not a licence for a sloppy chain: it is exactly the rounding of one document unit
that an arc endpoint needs. A gap of two micrometres is still a gap.

## Consequences
A ring the user drew with an arc assembles, converts to geometry and extrudes; the closing segment of
a polyline can be an arc; an arc-arc corner survives an edit. The invariant "the end of a segment is
the start of the next" becomes "within one micrometre", and every consumer of a `Contour` must use
the same bound rather than assume bit equality. A future storage change that keeps explicit arc
endpoints would let the tolerance shrink; until then it is the contract.
