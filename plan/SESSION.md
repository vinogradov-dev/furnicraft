# Session

```text
task: U16
goal: While a drawing tool runs, the viewport shows the draft, the snapped cursor with a marker and a live size, and the pointer appends points instead of selecting.

steps:
  [ ] `DrawDraft::preview_path` for the shape a next click would make
  [ ] `DrawGizmo`: pointer to work plane, snap, append or close, hover state
  [ ] paint the draft, the marker and the size; hide the other gizmos
  [ ] strip controls to start a tool, toggle the grid and cancel
  [ ] tests and the full gate

now: `apps/furnicraft/src/draw_gizmo.rs`, the new module
blocked: nothing
run: cargo test -p furnicraft -p furnicraft-ui
resume: create `apps/furnicraft/src/draw_gizmo.rs` with `DrawGizmo`
map: add a "Drawing tool preview" row
decisions: the preview builds the real `Path` per frame, so a circle, rectangle and arc look final before the last click; only the polyline closes by clicking its first point; a click that snaps to the anchor is ignored instead of becoming an error; Shift locks the axis and G toggles the 10 mm grid; the strip controls move to the toolbar in U19
```
