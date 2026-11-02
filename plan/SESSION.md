# Session

task: S1
goal: split `crates/model/src/lib.rs` by responsibility with the public API, serde and behaviour unchanged

steps:
  [x] plan entry, backlog legend and this note
  [x] ids, placement and furniture type moved into modules
  [x] properties and queries moved into modules
  [ ] panel split into geometry, banding and machining
  [ ] mesh, contour ring, objects, cameras and lights moved into modules
  [ ] container, node enum and tree traversal moved into modules
  [ ] duplicated tree walkers collapsed into one
  [ ] project wire, errors and derived queries moved into modules
  [ ] project commands split by family
  [ ] gate green, plan closed, `dev` merged into `main`

now: step 2 committed; step 3 starts with `crates/model/src/panel/mod.rs`
blocked: nothing
run: cargo test -p furnicraft-model
resume: create `crates/model/src/panel/`, move `Panel` and `PanelOutline` into `panel/mod.rs`, split the nominal geometry, banding and machining rules into sibling modules, re-export and move their tests
map: `plan/map.md` row "Contour ring, segment ids"
decisions: fields stay private; `Project` commands get `pub(crate)` mutators on value types; tests
  move into the module that owns the behaviour; serde attributes are copied verbatim
