# Session

```text
task: U8
goal: Clicking a drawn path in the viewport selects its document node instead of falling through.

steps:
  [ ] scene: a ray-to-segment closest approach and a line chain in `SceneFrame::pick`
  [ ] app: the pointer ray picks solids and paths through one helper
  [ ] tests, ADR 0018 and the full gate

now: `crates/scene/src/lib.rs`, adding `Camera::world_per_point` and the polyline branch of `SceneFrame::pick`
blocked: nothing
run: cargo test -p furnicraft-scene --lib
resume: open `crates/scene/src/lib.rs` at `SceneFrame::pick` and add the line chain branch
map: the "Path line primitive" row names `Polyline` and `SceneFrame::push_polyline`
decisions: a path has no thickness, so it is hit when the ray passes within 6 viewport points of a segment, measured at the segment's depth; the camera supplies millimetres per point, so picking needs no GPU or renderer state
```
