# Session

```text
task: S12
goal: the contour-edit maths over a segment lives in `geom/segment.rs`, and the document only converts

steps:
  [x] measured the baseline: contour_edit 1571 production lines, 25 `millimetres_f64`/`radians_f64`, suite green
  [x] `geom/segment.rs` gained `Arc::with_radius_through`, `Arc::through_ends`, `Arc::sweep_to`, `Arc::midpoint`, `Segment::clamp_to_extent`, `Segment::support_point_within` and `point_on_circle`
  [x] `contour_edit.rs` calls them; `arc_with_radius`, `arc_between`, `arc_midpoint`, `arc_rebuilt`, `clamp_to_curve` and the circle-point trigonometry are gone (1571 -> 1481 lines)
  [x] `model/contour.rs` header says the maths lives in `geom`; `plan/map.md` names both files
  [x] X31 filed: an along-contour drag on a sub-millimetre segment panics in `f64::clamp`, reproduced before filing and not fixed inside the move

now: nothing; the move is committed, the plan edit is next
blocked: nothing
run: cargo xtask ci
resume: mark S12 `[x]` with the code commit's hash, rewrite `plan/STATUS.md` to name S13 and delete this note
map: `| Contour edit math | crates/model/src/contour_edit.rs ..., crates/geom/src/segment.rs ... |`
decisions: error variants are unchanged — every geom failure maps to the model's own `InvalidValue`, `PointOffSegment` or `PointOffCircle`; `split_document` keeps the millidegree quantisation in the document (it is what makes both halves share one end) and asks `geom` only for the signed sweep
```
