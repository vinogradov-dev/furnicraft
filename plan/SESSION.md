task: X4
goal: every creation tool lands at the scene root, never in the selected container

steps:
  [x] `ToolAction` carries no parent; `show`/`show_panel` take none
  [x] `creation_root` replaces `drawing_parent` in `commit_drawing` and `show_toolbars`
  [x] regression: all six creations land at the root beside a selected nested assembly
  [x] ADR 0056 written, ADR 0052 marked "parent rule superseded by 0056"
  [ ] `cargo xtask ci` and the closing commit

now: run the full gate, then commit X4 and close the plan
blocked: nothing
run: cargo xtask ci
resume: run `cargo xtask ci`; on green, commit crates/ui, docs/decisions, plan/map.md and the plan closure
map: plan/map.md "Toolbar strip" row
decisions: the root is the only default parent; `extrude_path` stays a sibling of its path because it derives a body, and U31 adds the explicit armed child target
