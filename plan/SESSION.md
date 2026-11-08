# Session

```text
task: U53
goal: every drawing tool finishes its own shape, and a committed shape opens for editing

steps:
  [ ] DrawTool gains ArcFromCenter, CircleThreePoints, RectangleFromCenter (+ Alt directed),
      RegularPolygon; points(), closes(), is_closed() and label() follow
  [ ] DrawDraft builds each new tool from its clicked points, N from the ribbon for the polygon
  [ ] Tab switches the polyline between a straight segment and an arc; Alt only overrides snaps
  [ ] the closing segment of a polyline is an arc in arc mode and starts at the stored end
  [ ] a preview that cannot become a path keeps the last valid one and the status names the reason
  [ ] committing a figure opens its edit mode

now: read DrawTool/DrawDraft in crates/ui/src/contour_edit.rs and draw_gizmo.rs
blocked: nothing
run: cargo test -p furnicraft-ui --lib contour_edit
resume: add the new DrawTool variants and their construction to DrawDraft
map: plan/map.md row for the drawing draft
decisions: Tab owns line/arc inside a polyline and Alt is only the snap override; N-gon takes two
  clicks (centre + radius/orientation); a degenerate free arc step is refused, never turned into a
  line.
```
