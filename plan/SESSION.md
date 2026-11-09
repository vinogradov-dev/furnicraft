# Session

```text
task: U59
goal: the six standard views follow the owner's eye convention and a standard view is orthographic by default

steps:
  [x] OpeningView gains Left and Bottom with the owner's directions, labels, ALL and is_standard
  [x] Camera::set_orthographic converts a perspective view by the visible-height rule
  [x] a standard preset and an axis-ball snap set ortho; axonometric and free directions keep the projection
  [ ] full gate `cargo xtask ci` and the plan close

now: run the gate, then commit U59 and rewrite the plan
blocked: nothing
run: cargo xtask ci
resume: run `cargo xtask ci`; if green, commit the U59 change and close the task in the plan
map: View camera and projection (crates/ui/src/display.rs, crates/scene/src/lib.rs, view_layout.rs)
decisions: is_standard names the five non-axonometric views; only an exact world-axis direction is a standard snap
```
