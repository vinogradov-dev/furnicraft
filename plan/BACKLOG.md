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
variants, E 2D primitives, F revolution and sweep, G 2D booleans, H hardware and accessories; P is
the plan and `xtask` itself, L is scene lighting, S is a structural refactor that changes module
boundaries only, X is a defect in behaviour that already shipped.

A defect is filed under `## Bugs` with an `X` id next to the feature that owns it. Its entry states
the observation, the expectation and the suspected cause, and never a guess at a fix that has not
been reproduced. Taking it moves the line into `## Now` at its place in the work order, so `STATE` can
name it; fixing it moves it to `## Done` with the commit, like any task.

## Now

- `[ ]` **S3** one contour conversion: `model::Contour` to `geom::Region` to `kernel::Profile` with one
  sampling tolerance and one place where arcs become a polyline; route the rectangular core, shaped
  core, cutout tool and constructive extrusion through it.
  `files: crates/geom/src/region.rs, crates/param/src`

## Bugs

- `[ ]` **X2** a valid contour with more than 1024 sampled points cannot become an extrusion.
  Observed: the command now refuses it with "one extrusion cap holds at most 1024", where before
  A6 it was stored and drew nothing. Expected: any simple ring the user can draw becomes a solid.
  Cause: one extrusion cap is one document face, capped by `MAX_FACE_VERTICES`; the ring samples at
  50 µm, so a smooth ring of a few turns exceeds it while the ring itself is legal (at most 256
  segments). `object.collapse` fails on such an object and `object_mesh` cannot draw it.
  `files: crates/param/src/lib.rs, crates/model/src/object.rs`

## Next

- `[ ]` **G1** `geom::Region` holds exact `Contour` rings (outer and holes): rectangle constructor,
  bounds, area, containment and sampling, with the rectangle boolean helpers still passing.
  `files: crates/geom/src/region.rs, crates/geom/src/contour.rs`
- `[ ]` **G2** exact 2D region booleans on `Contour` rings: union, difference and intersection, used
  to union through cutouts and subtract them from the core instead of the rectangle-only tracer.
  `files: crates/geom/src/region.rs`
- `[ ]` **D4** one panel outline source: a rectangle keeps its corner and bow parameters as a contour
  generator, a contour panel stores the ring, both produce a `geom::Region`; `PanelGeometry` derives
  core and overall from the region, and no machining code branches on the outline kind.
  `files: crates/model/src/panel, crates/param/src`
- `[ ]` **D5** a typed machining list: ordered `Machining` records with stable `MachiningId`s, a
  tagged `MachiningParams` and shared `FaceSide`/`EdgeRef` anchors; `panel.machining.add/update/
  remove/move` commands; migrate `Cutout` and the schema, keeping existing cutout files loading.
  `files: crates/model/src/panel, crates/model/src/project/wire.rs, crates/engine/src/panel_cmds.rs`
- `[ ]` **D6** the machining pipeline: `CompileMachining` and `OpEffect` (cut region, subtract, add,
  attachment), a `PanelPlan` that folds operations, and a static descriptor table with kind,
  category, stage, capabilities and default; the panel builder contains no kind match, and a
  post/deform stage stays reserved for the deferred bend.
  `files: crates/param/src, crates/model/src/panel`
- `[ ]` **D7** panel card sections from the registry: common operation chrome plus a per-kind section
  module, an add menu built from the descriptor table, diagnostics bound to `MachiningId`, collapsed
  sections in session view state.
  `files: crates/ui/src/panel_card.rs, crates/ui/src/lib.rs, crates/model/src/panel`
- `[ ]` **D8** face groove (a groove on a panel face): a region on face A/B with depth and
  through/blind mode, compiled as a subtractive tool; card section and tests.
  `files: crates/model/src/panel, crates/param/src, crates/ui/src/panel_card.rs`
- `[ ]` **D9** edge groove (a groove on a panel edge): follows an `EdgeRef` with offset, depth and
  width, compiled as a swept or extruded tool; card section and tests.
  `files: crates/model/src/panel, crates/param/src, crates/ui/src/panel_card.rs`
- `[ ]` **D10** face-anchored milling: a path or region anchored to face A/B with cutter diameter and
  depth, compiled as a swept tool; card section and tests.
  `files: crates/model/src/panel, crates/param/src, crates/ui/src/panel_card.rs`
- `[ ]` **D11** banding as an operation: `MachiningParams::Banding` references `EdgeRef`, the joint
  solver returns attachment and cut-region effects, and the separate edge/corner/ring banding
  vectors migrate with old files loading.
  `files: crates/model/src/panel, crates/param/src, crates/ui/src/panel_card.rs`
- `[ ]` **S4** revision-keyed evaluation cache: `(Revision, Id)` to evaluated body and mesh in
  `furnicraft-mesh`; `scene_build` and `object.collapse` share one evaluator and a rebuild touches
  only changed nodes.
  `files: crates/mesh/src/lib.rs, apps/furnicraft/src/scene_build.rs, crates/engine/src/session.rs`
- `[ ]` **H1** hardware and accessories as existing nodes: a catalogue definition, an instance that
  is a single object or a component/group, and a body built through `SolidPlan`; no separate
  hardware geometry pipeline. `files: crates/model/src, crates/param/src, crates/ui/src`
- `[ ]` **H2** typed hardware mounts: host panel, anchor, orientation and quantity; the panel
  evaluator derives machining from mounts and the card shows it read-only; holes are not duplicated
  on the panel. `files: crates/model/src, crates/engine/src, crates/param/src, crates/ui/src/panel_card.rs`
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

- `[x]` **A7** the contour panel and the contour edit mode are written down in `docs/domain/` and
  ADR 0042: the outline variant, the anchored ring, ring banding, the cutout ring rules and the
  working-copy edit mode — `020a6cb`
- `[x]` **S2** one domain-free solid plan: `SolidPlan` is a base extrusion plus boolean steps, a
  `PlanBuilder` assembles it, and one evaluator drives both the kernel and the analytic
  single-extrusion backend; panels, bands, cutout tools and constructive objects compile to it,
  `object.collapse` keeps the exact mesh, and ADR 0041 records the boundary — `bd33056`
- `[x]` **S1** decompose `crates/model/src/lib.rs` by responsibility: one module per value family,
  panel rules split into geometry, banding and machining, `Project` split into wire, error, derived
  queries and command families, tree traversal in one place; public API, serde and behaviour
  unchanged — `43583b4`
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
