# Session

```text
task: A11
goal: an extrusion is the body that owns the path it grew from

steps:
  [ ] Constructive stores { path, features } and Extrude keeps only its height
  [ ] schema 19 loads a version 18 extrude.contour into constructive.path
  [ ] object.path.extrude turns the Path node into the body in place
  [ ] object.contour.replace edits the path inside the body, the mesh cache keys on it
  [ ] object.extrude.height.set drives the pull and keeps the path
  [ ] tests: node replacement, contour edit rebuild, height, old file round trip

now: read crates/model/src/object.rs and the Constructive/Extrude/Feature shapes
blocked: nothing
run: cargo test -p furnicraft-model --lib object
resume: rewrite Constructive as { path, features } in crates/model/src/object.rs
map: plan/map.md row for the constructive body
decisions: schema 19 already landed with the path shape tag; the extrusion migration rides in
  the same version because no file written by a release exists yet.
```
