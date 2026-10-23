# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M4 — Panel geometry
- Working on: catalogue-aware material and banding commands
- Gate: `cargo xtask ci` — green

## Done

- `feat(model): store panel nominal geometry and materials` (uncommitted until the checkpoint)
- `feat(model): add the material catalogue`
- `docs(decisions): define panel, material and banding model`
- `docs(decisions): add architecture decision records`
- `docs(vendor): add rules for vendored code`
- `docs: add the agent status file`
- `build(xtask): add the ci gate`

## Next

1. Add `panel.material.set`, `panel.edge_banding.set` and `panel.edge_banding.clear` with catalogue resolution and tests.
2. Build separate core and edge-band bodies in `param`, and render banding in the sample scene.
3. Add derived panel diagnostics: missing materials, non-positive core and machining that no longer fits.

## Open questions

- Contour radii/chamfers, edge-banding milling and grain constraints; see `docs/domain/panels.md` › Open questions and `PROJECT.md` › Open model questions.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions for the future database.

## Known debt

- Panel commands still read banding fallbacks; a catalogue refresh path is not wired yet.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
