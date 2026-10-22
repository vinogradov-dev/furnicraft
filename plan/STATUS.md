# Status

Updated: 2026-10-22. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M4 — Panel geometry
- Working on: operational layer docs, then edge banding and the material catalogue
- Gate: `cargo xtask ci` — green at c2cb175

## Done

- `build(xtask): add the ci gate`
- `docs: add panel, material and scene specifications`
- `feat(app): show the panel scene in a native viewport`
- `feat(render): draw shaded scenes into an offscreen target`
- `feat(scene): describe camera, grid and drawables`
- `feat(param): subtract panel cutouts`

## Next

1. Add `Catalog`, `MaterialId`, `Panel.material` and `EdgeBanding` to `crates/model/src/lib.rs` with validation and serde tests.
2. Hold the catalogue in `Session` and add `panel.material.set`, `panel.edge_banding.set` and `panel.edge_banding.clear` in `crates/engine`.

## Open questions

- Corner trimming for a single banded edge and the machining invalidation policy; see `docs/domain/panels.md` › Open questions.
- Whether the material catalogue stays a mock file behind a trait or becomes the database schema first; see `docs/domain/materials.md` › Open questions.

## Known debt

- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- Edge banding, material references and panel groups are specified but not implemented.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
