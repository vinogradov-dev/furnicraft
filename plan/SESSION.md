# Session

```text
task: D1
goal: a panel whose outline is a selected contour ring, with banding per ring segment and schema 13.

steps:
  [ ] `Outline::Rect | Contour` on `Panel`, serde default = Rect, schema 13, old files load
  [ ] a rectangle panel keeps building exactly as today; a contour panel builds from its ring
  [ ] banding addressed by a ring segment for a contour outline
  [ ] `panel.contour.create` in the engine, from a contour selected in the scene
  [ ] tests: model invariants, project round trip, panel build from a ring

now: the `Outline` type and the `Panel` field in `crates/model/src/lib.rs`
blocked: nothing
run: cargo xtask plan && cargo test -p furnicraft-model --lib
resume: add `Outline` to `crates/model/src/lib.rs` next to `Panel`, then make `nominal_size` a Rect-only accessor
decisions: |
  A contour panel is created by its own command, never converted from a rectangle.
  Banding on a contour panel is per ring segment; no bows, no corner features, no parametric cutouts.
  The outline is edited only in a contour edit mode; Apply is one command, Cancel writes nothing.
```
