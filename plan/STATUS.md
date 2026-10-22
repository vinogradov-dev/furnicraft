# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M4 — Panel geometry
- Working on: the material catalogue and the panel schema from ADR 0005–0008
- Gate: `cargo xtask ci` — green at 5239ff5

## Done

- `docs(decisions): define panel, material and banding model`
- `docs(decisions): add architecture decision records`
- `docs(vendor): add rules for vendored code`
- `docs: add the agent status file`
- `build(xtask): add the ci gate`
- `docs: add panel, material and scene specifications`

## Next

1. Add `MaterialId`, `PanelMaterial`, `EdgeBandingMaterial` and `Catalog` to `crates/model/src/catalog.rs` with validation and serde tests.
2. Split the stored `PanelSize` into `PanelNominalSize` and resolved `PanelGeometry`; update `crates/engine` commands and migrations.

## Open questions

- Contour radii/chamfers, edge-banding milling and grain constraints; see `docs/domain/panels.md` › Open questions and `PROJECT.md` › Open model questions.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions for the future database.

## Known debt

- The panel/material/banding model is accepted but not implemented; `crates/model` still stores `PanelSize`.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
