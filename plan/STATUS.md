# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing
- Working on: the outliner, panel card and derived diagnostics in the UI
- Gate: `cargo xtask ci` — green

## Done

- `feat(engine): rename panels`
- `feat(param): build edge bands and expose panel diagnostics`
- `feat(engine): add catalogue-aware panel material commands`
- `feat(model): store panel nominal geometry and materials`
- `feat(model): add the material catalogue`
- `docs(decisions): define panel, material and banding model`
- `docs(decisions): add architecture decision records`
- `docs(vendor): add rules for vendored code`
- `docs: add the agent status file`
- `build(xtask): add the ci gate`

## Next

1. Resolve panel and banding geometry from the session catalogue before scene evaluation.
2. Add the sample material catalogue to the application and assign materials to the sample panels.
3. Build the outliner and panel card on the existing commands, with diagnostics and undo/redo.

## Open questions

- Contour radii/chamfers, edge-banding milling and grain constraints; see `docs/domain/panels.md` › Open questions and `PROJECT.md` › Open model questions.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions for the future database.

## Known debt

- Existing panels and banding still use stored fallbacks during scene evaluation; catalogue resolution is the next task.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
