# Session

task: U58
goal: the corner widget gains a four-node roll ring whose hand turn follows the step in the settings

steps:
  [ ] gizmo: ring geometry, node degrees, visual angle, quantize, hit test
  [ ] gizmo: Gizmo drag state, RollTo/RollBy actions, accent handle
  [ ] app: Leaf::roll_to/roll_by, pass the step, keep the ring drag out of orbit
  [ ] settings: roll_step_degrees, clamp 1..=15, DragValue
  [ ] tests, map row, then the gate

now: ring geometry and the Gizmo state in apps/furnicraft/src/gizmo.rs
blocked: nothing
run: cargo test -p furnicraft --bin furnicraft gizmo
resume: add the ring constants and geometry helpers to gizmo.rs
map: Corner orientation widget — apps/furnicraft/src/gizmo.rs (Gizmo, widget_rect, show, ring, quantize)
decisions: nodes set the absolute roll; the drag accumulates the unwrapped visual angle; the step is workspace settings, default 1°, clamped 1..=15
