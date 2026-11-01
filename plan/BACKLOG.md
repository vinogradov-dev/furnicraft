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
L is scene lighting, X is a defect in behaviour that already shipped.

A defect is filed under `## Bugs` with an `X` id next to the feature that owns it. Its entry states
the observation, the expectation and the suspected cause, and never a guess at a fix that has not
been reproduced. Taking it moves the line into `## Now` at its place in the work order, so `STATE` can
name it; fixing it moves it to `## Done` with the commit, like any task.

## Now

- `[ ]` **A7** document the contour panel and the contour edit mode in `docs/domain/geometry.md`,
  `docs/domain/panels.md` and an ADR. `files: docs/domain/geometry.md, docs/decisions/`

## Bugs

- `[ ]` **X2** a valid contour with more than 1024 sampled points cannot become an extrusion.
  Observed: the command now refuses it with "one extrusion cap holds at most 1024", where before
  A6 it was stored and drew nothing. Expected: any simple ring the user can draw becomes a solid.
  Cause: one extrusion cap is one document face, capped by `MAX_FACE_VERTICES`; the ring samples at
  50 µm, so a smooth ring of a few turns exceeds it while the ring itself is legal (at most 256
  segments). `object.collapse` fails on such an object and `object_mesh` cannot draw it.
  `files: crates/param/src/lib.rs, crates/model/src/lib.rs`

## Next

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

- `[x]` **D3** a cutout by an arbitrary contour: `CutoutShape::Contour`, kept inside the core, through
  and blind, the tool built by extruding the sampled ring; the schema moves to 14 — `2167e2b`
- `[x]` **D2** the contour panel card: the ring summary, the material-driven thickness, the material
  and one banding row per ring segment through `panel.ring_banding.set`/`clear`, no corners, bows or
  cutouts, an `Edit contour` entry; a missing ring-band material is a diagnostic — `f88a995`
- `[x]` **A6** contour edit commands: `panel.outline.set` and `object.contour.replace` share their
  `enabled` check with `run`, validate the ring before the document changes, and record one undo
  entry per apply; a contour too detailed for one extrusion cap is refused instead of stored — `6f3e82b`
- `[x]` **X1** a concave contour cap drew a flap triangle across the notch: `object_mesh` fanned
  every face, so the cap of an extruded object left its own contour; faces are ear-clipped now —
  `a2859fc`
- `[x]` **A3** the contour edit mode: enter on a panel outline or an object contour, edit a working copy
  in the viewport with handles and a preview, leave by applying one command or by cancelling — `1428441`
- `[x]` **A5** contour edit math: move a corner, trim, split, and rely on the segment ring — `dda986c`
- `[x]` **D1** a panel whose outline is a contour ring, banding per ring segment — `d9827ed`
- `[x]` **A4** stable ids for contour segments — `ea3153a`
- `[x]` **A2** contour as a ring of segments — `0833b4c`
- `[x]` **A1** the contour segment type — `e7551ca`
