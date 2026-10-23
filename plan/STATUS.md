# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M4 — Panel geometry
- Working on: edge-banding geometry and derived panel diagnostics
- Gate: `cargo xtask ci` — green

## Done

- `feat(engine): add catalogue-aware panel commands` (uncommitted until the checkpoint)
- `feat(model): store panel nominal geometry and materials`
- `feat(model): add the material catalogue`
- `docs(decisions): define panel, material and banding model`
- `docs(decisions): add architecture decision records`
- `docs(vendor): add rules for vendored code`
- `docs: add the agent status file`
- `build(xtask): add the ci gate`

## Next

1. Build separate core and edge-band bodies in `param`, and render banding in the sample scene with the panel id on every group part.
2. Add derived panel diagnostics: missing materials, non-positive core and machining that no longer fits.
3. Show panel issues in the UI when the panel card lands.

## Open questions

- Contour radii/chamfers, edge-banding milling and grain constraints; see `docs/domain/panels.md` › Open questions and `PROJECT.md` › Open model questions.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions for the future database.

## Known debt

- Existing banding records still use stored fallbacks; a catalogue refresh path is not wired yet.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
