# Session

```text
task: A3
goal: edit a panel outline or an object contour as a working copy in the viewport, then apply one command or cancel.

steps:
  [x] read the plan and the contour edit, panel, object and gizmo code
  [ ] add panel.outline.set and the model outline setter with frame re-anchoring
  [ ] add ContourEdit to the editor and entry points in both cards
  [ ] add the viewport contour gizmo: handles, drag, preview, split, apply/cancel
  [ ] tests for the command, the working copy and the gizmo helpers
  [ ] cargo xtask ci, plan files and commit

now: the model and engine outline command
blocked: nothing
run: cargo test -p furnicraft-model -p furnicraft-engine
resume: add Project::set_panel_outline next to set_panel_nominal_size in crates/model/src/lib.rs
map: contour edit | crates/ui/src/lib.rs | Editor
decisions: A3 carries the minimal panel.outline.set so its apply path is real; A6 keeps the command review and the one-undo-entry test.
```
