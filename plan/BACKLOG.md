# Backlog

One line per task, stable id in front. `[ ]` open, `[~]` in progress (exactly one, and it must match
`plan/SESSION.md`), `[x]` done with the commit that closed it. A finished task is removed from `[~]`
and marked `[x]`; older entries are kept as a short tail so the current position reads at a glance.

The `files:` hint names where the work starts. It is a hint, not a boundary: read those files and their
callers, not the workspace.

Order is the order of work. Renumbering is not allowed; a new task takes the next free id in its
phase. Phases come from the shape-base plan: A contour, B contour operations, C sketch, D panel
variants, E 2D primitives, F revolution and sweep, G 2D booleans.

## Now

- `[ ]` **A3** arcs in the object card and a viewport contour editor — a ring with an arc has no editor
  yet. `files: crates/ui/src/object_card.rs, crates/ui/src/lib.rs`
- `[ ]` **A4** stable element ids for contour segments, so a command names a segment, not an index.
  `files: crates/model/src/contour.rs, crates/model/src/lib.rs`
- `[ ]` **A5** contour edit math: move a corner, trim, split, and rely on the segment ring.
  `files: crates/model/src/lib.rs, crates/geom/src/contour.rs`

## Next

- `[ ]` **A6** contour edit commands in `furnicraft-engine` (`object.contour.*`), one undo entry each.
  `files: crates/engine/src/object_cmds.rs`
- `[ ]` **A7** document the contour editing model in `docs/domain/geometry.md` and an ADR.
  `files: docs/domain/geometry.md, docs/decisions/`
- `[ ]` **B1** fillet and chamfer on a contour corner. `files: crates/geom/src/contour.rs, crates/param`
- `[ ]` **B2** offset a contour inward and outward as a new ring. `files: crates/geom/src/contour.rs`
- `[ ]` **C1** a sketch on a chosen plane: grid, vertex snapping, translation and rotation steps.
  `files: crates/ui, apps/furnicraft`
- `[ ]` **D1** a contour panel variant beside the rectangular one. `files: crates/model, crates/param`
- `[ ]` **L1** scene exposure and tone mapping, then shadow maps, so point lights stop clipping.
  `files: crates/render`

## Done

- `[x]` **A2** contour as a ring of segments — `0833b4c`
- `[x]` **A1** the contour segment type — `e7551ca`
