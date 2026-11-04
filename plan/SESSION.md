# Session

```text
task: U7
goal: A stored path draws as a line chain in the viewport, so an open contour is visible without a solid.

steps:
  [x] a `Polyline` line primitive in the scene frame, with the preview and highlight helpers
  [x] the renderer's line pipeline uploads and draws the frame's polylines
  [x] the shell samples a path object into a polyline and includes it in the bounds
  [ ] the full gate and the closing commit

now: `cargo xtask ci`
blocked: nothing
run: cargo xtask ci
resume: run the gate, then commit U7 and close the plan
map: the "Flat path value" row names `Path`; drawing adds a scene row for the line primitive
decisions: the frame stores local points plus a rigid transform, like `Drawable`, so previews stay cheap; line colours are gamma-space like the grid because the line shader is unlit; the shell samples with the geom chain and the kernel's 0.05 mm chord tolerance
```
