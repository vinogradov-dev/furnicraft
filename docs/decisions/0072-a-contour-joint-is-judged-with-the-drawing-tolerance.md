# 0072. A contour joint is judged with the drawing tolerance

- Status: Accepted
- Date: 2026-11-08

## Context
Two values describe a joint between consecutive segments, and they disagreed. `Path` compares the
end of one segment with the start of the next through a one-micrometre bound
(`PATH_JOINT_TOLERANCE_MICROMETRES`). `Contour` compared the same two points with `==` and refused
anything that was not exactly equal (`Contour::assemble`).

The equality held only for whole-micrometre lines that the user placed exactly. An arc is stored as
a centre, a radius, a start angle and a sweep, so its endpoints are **derived**: the centre is
rounded to a micrometre, the radius to a micrometre and the angles to a millidegree, and the point
that comes back can miss the drawn one. The miss is not a constant: half a millidegree of angle is
`8.73e-6` radians, so on a 600 mm arc the endpoint can be about **5 µm** away from the click. A
fixed one-micrometre bound therefore refused the very figure the user drew — "path segment 2 does
not meet the next segment" — and a polyline closed by an arc could never become a ring. The arc-arc
limitation in `plan/debt.md` was the same defect.

## Decision
One rule, applied to every joint and scaled by the segments on either side of it:

```text
tolerance = 1 µm + arc_slack(first) + arc_slack(next)
arc_slack = radius × 8.73e-6   (0 for a line)
```

- The one-micrometre base is the rounding of one document unit that any two stored points need; it
  also covers the fraction lost when the slack is rounded down to whole micrometres.
- The slack is the endpoint rounding of an arc of that radius: the stored angle is a whole
  millidegree, and the endpoint moves by `radius × Δangle`.
- The rule is one function. In document units `ContourSegment::end_meets` (and `Path::assemble` and
  `Contour::assemble` through it) compares with it in integer micrometres; at the geometry boundary
  `joint_tolerance_mm` hands the same rule to
  `furnicraft_geom::Contour::new_with_joint_tolerance`, which judges every pair separately. A wide
  arc on a ring therefore never loosens a line-to-line joint beside it.
- `Contour::to_geometry` and every other converter of stored segments to a geometry chain pass the
  rule, so the boundary accepts exactly what the document accepted.
- The error keeps its wording, so a real gap still reads as "contour segment N does not meet the
  next segment".

The tolerance is not a licence for a sloppy chain: on a 100 mm arc it is one micrometre, and a
20 µm gap is still a gap. On a 600 mm arc it is 6 µm, which is one stored unit plus the rounding of
the arc itself.

## Consequences
A ring the user drew with an arc assembles, converts to geometry and extrudes; the closing click of
a polyline in arc mode finishes the arc on the first point and closes the ring, and the draft adds
no sliver segment in front of the rounded end. An arc-arc corner survives an edit. The invariant
"the end of a segment is the start of the next" becomes "within the joint tolerance of the two
segments", and every consumer of a `Contour` must use the same rule rather than assume bit
equality.

A future storage change that keeps explicit arc endpoints (schema 20, recorded in `plan/debt.md`)
would let the rule shrink back to one unit for every pair.
