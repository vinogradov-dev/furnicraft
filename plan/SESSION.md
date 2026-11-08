# Session

```text
task: A8
goal: a ring joins its segments with the one-micrometre tolerance the drawing path already uses

steps:
  [x] Contour::assemble compares joints with within_joint_tolerance
  [x] regression tests: a one-micrometre joint closes, two micrometres do not, and a drawn arc
      start that rounds away still assembles and converts to geometry
  [x] ADR 0072 records the one rule

now: run the full gate and commit A8
blocked: nothing
run: cargo xtask ci
resume: run the gate, then commit the ring tolerance, ADR 0072 and the two tests
map: plan/map.md row for the contour ring
decisions: the tolerance is PATH_JOINT_TOLERANCE_MICROMETRES (1 um) and is now the only joint rule
  for a path, a ring and to_geometry (ADR 0072); the error message keeps its wording.
```
