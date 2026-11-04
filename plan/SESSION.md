# Session

```text
task: U10
goal: The drawing tools can build a rectangle, a circle and a three-point arc as a valid `Path`.

steps:
  [ ] `Path::rectangle` from two opposite corners
  [ ] `Path::circle` as four quarter arcs with exact cardinal joints
  [ ] `Path::arc_through` from three points, with a readable error on degenerate input
  [ ] tests for every shape and its degenerate cases
  [ ] the full gate and the closing commit

now: `crates/model/src/contour.rs`, adding the three constructors after `Path::new`
blocked: nothing
run: cargo test -p furnicraft-model --lib
resume: open `crates/model/src/contour.rs` at `impl Path` and add `rectangle`, `circle`, `arc_through`
map: the "Flat path value" row names `Path` in `crates/model/src/contour.rs`
decisions: shapes return a `Path`, not a `Contour`, because a drawing is not yet a ring; the circle uses cardinal quarter arcs so every joint is the centre plus or minus the radius with no rounding; an arc through three points rounds its centre, radius and angles to document units and reports collinear or coincident input as a typed error
```
