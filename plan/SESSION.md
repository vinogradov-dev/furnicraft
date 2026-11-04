# Session

```text
task: U20
goal: The scene tree only shows and restructures nodes: every create button and create menu entry is gone, and the toolbar strip is the one place a new node starts.

steps:
  [ ] drop the create buttons from the tree toolbar and the container menu
  [ ] remove the create variants from `TreeAction`
  [ ] move the creation code behind `ToolAction` with its own dispatch
  [ ] tests that the tree offers no create action and the strip still creates
  [ ] the full gate and the closing commit

now: `crates/ui/src/scene_tree.rs`, removing the creation controls
blocked: nothing
run: cargo test -p furnicraft-ui
resume: strip the `+ Panel`, `+ Assembly`, `+ Cube`, `+ Extrude`, `+ Camera` and `+ Light` buttons and the container menu entries
map: the "Toolbar strip" row and the scene-tree row
decisions: `TreeAction` keeps only structural actions (duplicate, delete, group, ungroup, convert, select, undo, redo); creation moves to `ToolAction`, so the tree cannot create even if a button is added back by mistake; the panel-material check and the default sizes move with it
```
