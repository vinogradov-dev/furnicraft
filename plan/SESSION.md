# Session

```text
task: U15
goal: Five drawing tools — line, polyline, arc, rectangle and circle — each turn their clicks into a document Path, with a table of tests per tool.

steps:
  [ ] `Path::polyline` and `Path::circle_through` in the model, with tests
  [ ] `DrawTool` with its point count, completion check and per-tool path
  [ ] `DrawDraft::to_path` delegates to `Path::polyline`
  [ ] tests per tool and the full gate

now: `crates/model/src/contour.rs`, adding `Path::polyline` and `Path::circle_through`
blocked: nothing
run: cargo test -p furnicraft-model -p furnicraft-ui
resume: open `crates/model/src/contour.rs` after `Path::circle` and add the two constructors
map: the "Flat path value" row and a new UI row for the tools
decisions: a circle takes its centre and a rim point, because a pointer gives points not a radius; the rim distance is rounded to whole micrometres in the model, so the UI never does unit conversion; line and polyline are open, rectangle and circle closed, an arc one open arc; closing a chain stays the draft's job
```
