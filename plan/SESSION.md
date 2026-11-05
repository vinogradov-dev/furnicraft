# Session

```text
task: X3
goal: the projection button draws the mode the view is in, and its hint names the switch

steps:
  [ ] `projection_button(Projection)` beside `workspace`, with a table test for both modes
  [ ] the strip draws the current mode and keeps the authored-camera state
  [ ] `cargo xtask ci` green and the plan closed

now: writing `projection_button` in `apps/furnicraft/src/main.rs`
blocked: nothing
run: cargo test -p furnicraft
resume: add `projection_button` and its table test, then draw it in `workspace`
decisions: the icon is the current mode (an orthographic view shows the orthographic glyph), the
  hint is the action; `add_enabled_ui(owns_view)` stays where it is, so an authored camera still
  disables the button without a second code path
```
