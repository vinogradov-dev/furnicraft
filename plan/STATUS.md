# Status

STATE: milestone=M7 phase=U task=U33 state=idle base=0e289e0 updated=2026-11-05

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the current task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken or interrupted plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of stable-id line and arc segments
  (ADR 0040, schema 13); `Path` is the open or closed chain that `to_contour` turns into a ring, and
  a primitive stores the parameters that built it (ADR 0054, schema 18) while its mesh is derived.
  `object.primitive.replace` is one revision and `object.collapse` bakes either payload.
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards
  (ADRs 0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps the shape the user saw (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- The UI plan (U1–U26) is shipped: themed toolbars — Scene, Contour, Primitives, Bodies — with SVG
  icons rasterised at render size, no creation buttons in the scene tree, a work plane from the
  current ortho view, a projection toggle, and draw/pick/snap for a `Path` (ADRs 0047–0052).
- The owner reviewed the editor and queued seven defects and two changes ahead of the icon work
  (X3–X6, U30–U37): the projection icon, the creation parent, tool highlight, the pole views, a
  centre pivot for new drawings, a visible selection outline for line chains, and editing every kind
  of contour. X3, X4, X6, U30 and U31 are fixed: the projection button draws the view it is in, every
  creation tool lands at the scene root unless a container is armed, the strip draws the armed drawing
  tool or primitive pressed while the duplicate "Draw" row is gone, the top and bottom views look
  straight down their plane (ADR 0057), and *Add child* arms a group or an assembly so the next
  created nodes land inside it while the strip names the arm (ADR 0056). U32 is fixed too: a new
  drawing stores the pivot of its own shape — the bounds centre when it is closed, the first stored
  segment's start when it is open — and an extruded body takes the ring's centre (ADR 0058). X5 is
  fixed: a selected chain takes one explicit colour and the shell repaints it over the viewport as a
  thick stroke with a dark underlay, because a one-pixel `LineList` cannot be widened (ADR 0059).
- Defects live in `## Bugs` with an `X` id: X1 (the concave cap flap) is fixed; X2 — a legal ring over
  1024 sampled points cannot be extruded — is filed behind the icon work, its options in
  `plan/questions.md`.
- The icon pipeline was audited against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`):
  `Image::tint` multiplies into the texture, so a `#000` master stayed black in every theme and the
  theme never reached a glyph; the set is 18 drawn files of 51 and pure stroke, so it has no mass at
  16 pt. U27 moves rasterisation into the UI with a pixel test, U28 adds tonal fills and one
  axonometry; the deferred accent, tooling and the 33 placeholders are recorded there and in
  `plan/debt.md`.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) checks
  the three plan files; `updated=` must be a real date no older than `base`, and `task=` must name the
  first unfinished task in `## Now` (P2–P3). Gate: `cargo xtask ci` — green. Branches: `dev`,
  merged to `main` with `--no-ff` (AGENTS §6.2).

## Next

U33 is next: path edit math in the model — `PathCorner` addressing, plus move, split, remove and
close that share the ring helpers, so an open chain can be edited without welding its last segment
to the first (ADR 0060). U34–U37 follow: the path draft, the open-chain gizmo, the rectangle-to-ring
conversion (ADR 0061) and cutout rings. U27, U28, X2 and U29 come after them.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
