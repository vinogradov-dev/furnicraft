# Session

```text
task: D1
goal: a panel whose outline is a selected contour ring, with banding per ring segment and schema 13.

steps:
  [x] `Outline::Rect | Contour` on `Panel`, serde default = Rect, schema 13, old files load
  [ ] `Panel::nominal_contour` and the `param` build accept a contour panel and use its ring
  [ ] banding addressed by a ring segment for a contour outline
  [ ] `panel.contour.create` in the engine, from a contour selected in the scene
  [ ] tests: model invariants, project round trip, panel build from a ring

now: `nominal_contour` for a contour panel in `crates/model/src/panel_contour.rs`, then `build_panel` in `crates/param`
blocked: nothing
run: cargo test -p furnicraft-model --lib && cargo test -p furnicraft-param --lib
resume: read `Panel::geometry_for_with` in `crates/model/src/lib.rs`, then add the ring branch to `nominal_contour` in `crates/model/src/panel_contour.rs`
decisions: |
  A contour panel is created by its own command, never converted from a rectangle.
  Banding on a contour panel is per ring segment; no bows, no corner features, no parametric cutouts.
  The outline is edited only in a contour edit mode; Apply is one command, Cancel writes nothing.
```
