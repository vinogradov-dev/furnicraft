# Session

task: U56
goal: a flat projection icon under the corner widget switches ortho and perspective like the ribbon and the popover

steps:
  [ ] ui: flat icon button, no fill at rest, a faint wash on hover or press
  [ ] gizmo: button row under the cube, projection_rect, ToggleProjection
  [ ] shell: one toggle_view_projection shared by the cluster and the widget
  [ ] tests, then the gate

now: plan files, then icons::flat_icon_button
blocked: nothing
run: cargo test -p furnicraft-ui --lib && cargo test -p furnicraft --lib gizmo
resume: add flat_icon_button in crates/ui/src/icons.rs
map: Corner orientation widget — apps/furnicraft/src/gizmo.rs (widget_rect, show, GizmoAction)
decisions: the button is centred under the cube; Home keeps the top-right corner; one toggle path
