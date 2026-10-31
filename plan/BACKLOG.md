# Backlog

One entry per task, stable id in front; a long entry may wrap onto indented lines. `[ ]` open, `[~]`
in progress (exactly one, in `## Now`, matching `plan/SESSION.md`), `[x]` done and in `## Done` with
the commit that closed it. Older entries are kept as a short tail so the current position reads at a
glance.

The `files:` hint names where the work starts and is required on every unfinished task: each path
must exist, so name a directory for a file the task creates. The hint is a hint, not a boundary: read
those files and their callers, not the workspace.

Order is the order of work. Renumbering is not allowed; a new task takes the next free id in its
phase. Phases come from the shape-base plan: A contour, B contour operations, C sketch, D panel
variants, E 2D primitives, F revolution and sweep, G 2D booleans; P is the plan and `xtask` itself,
L is scene lighting.

## Now

- `[ ]` **A3** the contour edit mode: enter on a panel outline or an object contour, edit a working copy
  in the viewport with handles and a preview, leave by applying one command or by cancelling.
  `files: crates/ui, apps/furnicraft`

## Next

- `[ ]` **A6** contour edit commands: `panel.outline.set` and `object.contour.replace`, one undo entry
  per apply. `files: crates/engine/src/panel_cmds.rs, crates/engine/src/object_cmds.rs`
- `[ ]` **D2** the card of a contour panel: thickness, material and banding per ring segment; no bows,
  no corner features, no parametric cutouts; an `Edit contour` button.
  `files: crates/ui/src/panel_card.rs`
- `[ ]` **D3** a cutout by an arbitrary contour: `CutoutShape::Contour`, kept inside the outline,
  through and blind, tool built by extrusion. `files: crates/model, crates/param`
- `[ ]` **A7** document the contour panel and the contour edit mode in `docs/domain/geometry.md`,
  `docs/domain/panels.md` and an ADR. `files: docs/domain/geometry.md, docs/decisions/`
- `[ ]` **B1** fillet and chamfer on a contour corner. `files: crates/geom/src/contour.rs, crates/param`
- `[ ]` **B2** offset a contour inward and outward as a new ring. `files: crates/geom/src/contour.rs`
- `[ ]` **C1** a sketch on a chosen plane: grid, vertex snapping, translation and rotation steps.
  `files: crates/ui, apps/furnicraft`
- `[ ]` **L1** scene exposure and tone mapping, then shadow maps, so point lights stop clipping.
  `files: crates/render`
- `[ ]` **P1** `cargo xtask status` prints `head <hash>` and, while `state=doing`, the note's
  `task`/`now`/`blocked`/`run` block, so an interrupted session resumes in one command.
  `files: state-contract/src/status.rs, xtask/src/main.rs`
- `[ ]` **P2** `updated=` must be `YYYY-MM-DD` and no earlier than the date of the `base` commit.
  `files: state-contract/src/validate.rs`
- `[ ]` **P3** the current task must be the first unfinished one in `## Now`; promote the rule into
  the gate once the backlog has settled. `files: state-contract/src/validate.rs`

## Done

- `[x]` **A5** contour edit math: move a corner, trim, split, and rely on the segment ring — `dda986c`
- `[x]` **D1** a panel whose outline is a contour ring, banding per ring segment — `d9827ed`
- `[x]` **A4** stable ids for contour segments — `ea3153a`
- `[x]` **A2** contour as a ring of segments — `0833b4c`
- `[x]` **A1** the contour segment type — `e7551ca`
