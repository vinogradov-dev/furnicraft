# Session

```text
task: U13
goal: A drawing tool turns the raw pointer point on the work plane into a grid-aligned, axis-locked or draft-end point, as pure functions with table tests.

steps:
  [ ] the snap types and the 10 mm grid rounding
  [ ] the ends of the draft within a caller-supplied radius
  [ ] the axis lock from the previous point
  [ ] the composed dispatcher and its precedence
  [ ] table tests and ADR 0051
  [ ] the full gate and the closing commit

now: `apps/furnicraft/src/gizmo_math.rs`, adding `SnapMode`, `SnappedPoint` and the grid rounding
blocked: nothing
run: cargo test -p furnicraft
resume: open `apps/furnicraft/src/gizmo_math.rs` after the `WorkPlane` impl and add the snap types and functions
map: the "Work plane" row, because snapping works in its plane coordinates
decisions: grid and axis lock compose — the axis through the previous point stays exact and only the free coordinate is grid-rounded; a draft end wins over both, because joining a chain must not move the joint off the point it joins; the result says why it moved so the preview can draw a marker; the capture radius arrives from the caller in plane millimetres, because the plane-to-screen scale is the camera's job
```
