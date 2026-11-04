# Session

```text
task: U19
goal: Every toolbar panel offers its tools: five drawing tools, the four scene creators, the cube and the extrude, each with a hinted icon button and a text fallback.

steps:
  [x] `ToolAction` and the per-panel tool buttons
  [x] `Editor::show_toolbars` runs an action through the tree path
  [x] the drawing shortcuts, in the same table as the hints
  [x] tests for the buttons, the fallback and the shortcuts
  [x] the full gate and the closing commit

now: close the plan
blocked: nothing
run: cargo xtask ci
resume: close U19 and point the plan at U20
map: the "Toolbar strip" row names `ToolAction` and `CONTOUR_TOOLS`
decisions: the panels call the same `run_action` the scene tree uses, so one creation path survives U20; the drawing shortcuts are L, P, A, R and C from the one table the hints read; a scene button uses N for Panel and leaves the other creators without a key until they earn one
```
