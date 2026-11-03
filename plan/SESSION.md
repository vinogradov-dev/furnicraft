# Session

```text
task: D4
goal: one panel outline source, so a rectangle and a contour ring both yield one geom::Region that PanelGeometry and every machining rule read

steps:
  [ ] PanelOutline owns the rectangle's corner features and bows, and both variants produce one nominal region
  [ ] PanelGeometry holds the core and overall regions and derives every size from their bounds
  [ ] machining validates and traces the core region without branching on the outline kind
  [ ] param extrudes the core region for every panel and shaped panels accept new cutouts
  [ ] schema 15 moves the outline parameters into the outline, older files still load, docs and ADR updated

now: reading the panel and param call sites before the first edit
blocked: nothing
run: cargo test -p furnicraft-model --lib
resume: edit crates/model/src/panel/mod.rs to move corner_features and edge_bows into PanelOutline
map: plan/map.md row "Model panel"
decisions: PanelGeometry keeps the regions themselves and loses Copy; callers pass it by reference.
```
