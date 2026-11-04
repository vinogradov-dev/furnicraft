# Session

```text
task: U21
goal: The Cube tool asks for an edge length and then places one cube where the user clicks on the work plane.

steps:
  [ ] a size field in the Primitives panel, with the default 200 mm
  [ ] a pending primitive mode in `Editor`, so the next viewport click commits
  [ ] the click maps to the work plane and creates through one `object.primitive.create`
  [ ] tests for the size, the pending mode and the placement
  [ ] the full gate and the closing commit

now: `crates/ui/src/toolbars.rs`, the size field and the Cube action
blocked: nothing
run: cargo test -p furnicraft-ui -p furnicraft
resume: add `primitive_size` to the toolbar state and a Cube `ToolAction` that starts a pending placement
map: the "Toolbar strip" row and the "Drawing tool preview" row
decisions: the panel keeps the edge length as text parsed with `Length::parse_millimetres`, so a partly typed number is not lost; the Cube button arms a placement instead of creating at the origin, and the next click on the work plane creates the node; Escape disarms; the placement is the work plane's own placement with the click point as translation, so the cube sits on the plane
```
