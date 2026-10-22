# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M4 — Panel geometry
- Working on: the panel schema migration from ADR 0006
- Gate: `cargo xtask ci` — green at 61e6b88

## Done

- `feat(model): add the material catalogue`
- `docs(decisions): define panel, material and banding model`
- `docs(decisions): add architecture decision records`
- `docs(vendor): add rules for vendored code`
- `docs: add the agent status file`
- `build(xtask): add the ci gate`

## Next

1. Split the stored `PanelSize` into `PanelNominalSize` and resolved `PanelGeometry`; update `crates/model` and `crates/engine` commands and migrations.
2. Hold the `Catalog` in `Session` and add `panel.material.set`, `panel.edge_banding.set` and `panel.edge_banding.clear`.

## Open questions

- Contour radii/chamfers, edge-banding milling and grain constraints; see `docs/domain/panels.md` › Open questions and `PROJECT.md` › Open model questions.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions for the future database.

## Known debt

- The panel schema migration is pending: `Panel` still stores `PanelSize` and has no material or banding fields.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
