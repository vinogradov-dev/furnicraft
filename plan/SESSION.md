# Session

task: S1
goal: split `crates/model/src/lib.rs` by responsibility with the public API, serde and behaviour unchanged

steps:
  [x] plan entry, backlog legend and this note
  [x] ids, placement and furniture type moved into modules
  [x] properties and queries moved into modules
  [x] panel split into geometry, banding and machining
  [x] mesh, contour ring, objects, cameras and lights moved into modules
  [ ] container, node enum and tree traversal moved into modules
  [ ] duplicated tree walkers collapsed into one
  [ ] project wire, errors and derived queries moved into modules
  [ ] project commands split by family
  [ ] gate green, plan closed, `dev` merged into `main`

now: step 4 committed; step 5 starts with the container and node tree
blocked: nothing
run: cargo test -p furnicraft-model
resume: move `Container`, `Node` and the tree walkers out of `lib.rs` into `container.rs` and `node.rs`, re-export them and move their tests
map: `plan/map.md` row "Contour ring, segment ids"
decisions: fields stay private; `Project` commands get `pub(crate)` mutators on value types; tests
  move into the module that owns the behaviour; serde attributes are copied verbatim
