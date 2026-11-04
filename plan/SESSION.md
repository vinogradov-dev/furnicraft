# Session

```text
task: U9
goal: A drawing reaches the document through `object.path.create` and `object.path.replace`, one undo entry each.

steps:
  [ ] `object.path.create`: a path payload under a container, one revision
  [ ] `object.path.replace`: a new drawing on the same node, one revision
  [ ] tests for both commands and their `enabled`/`run` agreement
  [ ] ADR 0048 and the path payload in `docs/domain/geometry.md`
  [ ] the full gate and the closing commit

now: `crates/engine/src/object_cmds.rs`, adding the create and replace specs
blocked: nothing
run: cargo test -p furnicraft-engine --lib
resume: open `crates/engine/src/object_cmds.rs` after `OBJECT_CONTOUR_REPLACE` and add the path specs
map: the "Flat path value" row names `Path`; the commands belong in `crates/engine/src/object_cmds.rs`
decisions: a command carries the whole `Path` value, as `object.extrude.create` carries a whole contour; the document validates the chain on deserialize, so the command validates only the name and the parent
```
