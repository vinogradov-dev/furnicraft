# Session

```text
task: U55
goal: a fresh extrusion shows a pull arrow, and dragging it or typing a number sets its height

steps:
  [x] plan close-out for U54 and U55 opened
  [ ] ribbon reports the body it just extruded, with the height it used
  [ ] ExtrudeGizmo: arrow from the top-face centre along local +Z, hit-test and drag to height
  [ ] commit object.extrude.height.set on release or Enter; a click or Enter finishes
  [ ] the next body starts at the last height used in the session

now: the ribbon outcome must carry the extruded body so the shell can start the pull
blocked: nothing
run: cargo test -p furnicraft --bin furnicraft
resume: add RibbonOutcome::extruded and set it in Editor::show_ribbon after extrude_path succeeds
map: plan/map.md row for the contour edit mode; the extrusion arrow sits beside the other gizmos
decisions: the default height is last-used session view state (100 mm at first); the pull only
  previews, and one object.extrude.height.set is the undo entry; Esc drops the pull and leaves the
  body at the height it was created with.
```
