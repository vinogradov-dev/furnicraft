# Session

```text
task: U17
goal: A finished drawing becomes one path object in the document, placed on the work plane and selected, in exactly one undo entry.

steps:
  [ ] `WorkPlane::placement`: local XY is the plane, at the world origin
  [ ] `DrawDraft::commit` and `Editor::commit_drawing` dispatch `object.path.create` once and reset the draft
  [ ] a completed tool commits from the viewport; Enter commits, Close and Cancel stay
  [ ] tests for the placement, the selection, one undo entry and a refused draft
  [ ] the full gate and the closing commit

now: `apps/furnicraft/src/gizmo_math.rs`, adding `WorkPlane::placement`
blocked: nothing
run: cargo test -p furnicraft -p furnicraft-ui
resume: open `apps/furnicraft/src/gizmo_math.rs` after `WorkPlane::local` and add `placement`
map: the "Work plane and snapping" row; the "Drawing draft" row names the commit
decisions: the plane runs through the world origin, so the placement keeps a zero translation and only rotates local XY onto u/v; the parent is the selected container, else the selected node's parent, else the root; a fixed tool commits on its last click, a polyline on its close or on Enter, and an open polyline may be committed as drawn; a refused path keeps the draft so the user can undo or cancel
```
