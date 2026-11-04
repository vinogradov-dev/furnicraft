# Session

```text
task: U23
goal: The Bodies panel extrudes the selected closed path into a body at a typed height, and says why an open path cannot be extruded.

steps:
  [ ] a height field in the Bodies panel, with the default 18 mm
  [ ] the Extrude button is enabled only for a selected closed path and sends one action
  [ ] the editor converts the path with `to_contour` and dispatches `object.extrude.create`
  [ ] an open path keeps the message readable and the document untouched
  [ ] tests and the full gate

now: `crates/ui/src/toolbars.rs`, the Bodies panel
blocked: nothing
run: cargo test -p furnicraft-ui
resume: add the extrude height field and the Extrude action to the Bodies panel
map: the "Toolbar strip" row; the path row for `to_contour`
decisions: the body is created in the path's parent at the path's own placement, because the ring is stored in the path's local frame; the path stays in the document so the user can extrude again or edit it; an open or self-crossing path is refused before the command with the `PathError` text, so the message names the real problem
```
