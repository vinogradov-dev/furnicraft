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
  [x] project wire, errors and derived queries moved into modules
  [ ] project commands split by family
  [ ] gate green, plan closed, `dev` merged into `main`

now: step 7 committed; step 8 splits the remaining commands in `project/mod.rs` by family
blocked: nothing
run: cargo test -p furnicraft-model
resume: move the panel, object, node, camera/light and property commands with their validators into `project/{panel_cmds,object_cmds,node_cmds,camera_light_cmds,property_cmds}.rs`
map: `plan/map.md` row "Contour ring, segment ids"
decisions: fields stay private; `Project` commands get `pub(crate)` mutators on value types; tests
  move into the module that owns the behaviour; serde attributes are copied verbatim
