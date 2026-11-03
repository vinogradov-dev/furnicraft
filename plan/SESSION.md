# Session

```text
task: U1
goal: the assets/icons tree with one empty SVG per toolbar icon, and assets/README.md with the naming and format rules

steps:
  [x] assets/icons/{contour,primitives,bodies,scene,view}/ hold the 21 placeholder files
  [x] assets/README.md states the naming, the 24x24 format and the empty-file fallback
  [x] plan/map.md names the icon resource row
  [ ] the gate is green and the resource tree is committed

now: running cargo xtask ci before the commit
blocked: nothing
run: cargo xtask ci
resume: `find assets -type f | sort` then compare the list with the U1 entry in plan/BACKLOG.md
map: plan/map.md row "UI icon resources"
decisions: the placeholders stay zero bytes, so the loader in U2 can detect "not drawn yet" with is_empty and fall back to a text label.
```
