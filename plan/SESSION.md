# Session

task: S1
goal: split `crates/model/src/lib.rs` by responsibility with the public API, serde and behaviour unchanged

steps:
  [x] plan entry, backlog legend and this note
  [x] ids, placement and furniture type moved into modules
  [x] properties and queries moved into modules
  [x] panel split into geometry, banding and machining
  [x] mesh, contour ring, objects, cameras and lights moved into modules
  [x] container, node enum and tree traversal moved into modules
  [x] duplicated tree walkers collapsed into one
  [ ] project wire, errors and derived queries moved into modules
  [ ] project commands split by family
  [ ] gate green, plan closed, `dev` merged into `main`

now: steps 5 and 6 committed; step 7 starts with `ProjectWire`, `ProjectError` and the derived queries
blocked: nothing
run: cargo test -p furnicraft-model
resume: move `ProjectWire`, `ProjectError` and the derived project queries out of `lib.rs` into `project/wire.rs`, `project/error.rs` and `project/query.rs`
map: `plan/map.md` row "Contour ring, segment ids"
decisions: fields stay private; `Project` commands get `pub(crate)` mutators on value types; tests
  move into the module that owns the behaviour; serde attributes are copied verbatim
