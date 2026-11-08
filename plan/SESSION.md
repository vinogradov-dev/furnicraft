# Session

```text
task: U54
goal: every drawn figure shows its grips, takes a number at the cursor, and moves freely or along
  its own contour

steps:
  [ ] read the shape tag, the numeric setters and CornerMove the model already offers
  [ ] grips: circle quadrant points and centre; arc start, end, centre, a point on the arc and a
      radius grip on hover; rectangle corners, midpoints and centre; polygon vertices, centre and
      radius point
  [ ] a numeric field follows the cursor; its meaning comes from the grip, Enter applies it without
      leaving the edit, Esc restores the value the drag started from
  [ ] free/along-contour toggle in the edit HUD, free by default, target resolved at drag start
  [ ] each figure keeps its identity through a grip drag: a circle stays round, a rectangle keeps
      right angles, a polygon keeps its sides
  [ ] ADR 0075, docs, map, tests

now: read crates/model/src/contour_edit.rs (CornerMove, numeric setters) and the shape tag
blocked: nothing
run: cargo test -p furnicraft-model --lib contour_edit
resume: list the setters the model has and the grips the gizmo has to add
map: plan/map.md row for the contour gizmo
decisions: free movement is the default for every kind; the along-contour target is fixed at drag
  start; Arc numeric keeps both stored endpoints and moves the centre on the existing bulge side;
  a degenerate free arc step stays refused.
```
