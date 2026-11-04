# Session

```text
task: U11
goal: The viewport switches between perspective and orthographic as view state, by a strip button and O, without touching an authored camera.

steps:
  [ ] `Camera::toggle_projection`: the orthographic height taken from the visible height at the target
  [ ] `Camera::zoom` scales the orthographic height, so the new view can be navigated
  [ ] the app strip button and the O key, disabled while an authored camera is active
  [ ] ADR 0049 and the view-state documents
  [ ] the full gate and the closing commit

now: `crates/scene/src/lib.rs`, adding `Camera::toggle_projection` and the orthographic zoom
blocked: nothing
run: cargo test -p furnicraft-scene --lib
resume: open `crates/scene/src/lib.rs` at `Camera::with_projection` and add `toggle_projection`
map: add a "View camera and projection" row to `plan/map.md`
decisions: the toggle is view state, so it lives on the view camera and never in the document; the orthographic height is the perspective visible height at the target, so the frame does not jump; zoom scales that height and the orbit distance together, so a round trip returns the same frame; an authored camera is pinned and the strip disables the toggle while one is active
```
