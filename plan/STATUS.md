# Status

STATE: milestone=M7 phase=U task=U47 state=idle base=HEAD updated=2026-11-07

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of stable-id segments (ADR 0040,
  schema 13); `Path` is the open or closed chain that `to_contour` turns into a ring, and a primitive
  stores the parameters that built it (ADR 0054, schema 18) while its mesh is derived.
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards
  (ADRs 0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps the shape the user saw (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- The UI plan (U1–U26) is shipped: SVG icons rasterised at render size, no creation buttons in the
  scene tree, a work plane from the current ortho view, a projection toggle, and draw/pick/snap for a
  `Path` (ADRs 0047–0052).
- X3–X6 and U30–U37 are all fixed (ADRs 0056–0061): the projection button draws the view it is in, a
  creation lands at the scene root unless a container is armed, the armed tool draws pressed, the pole
  views look down their plane, a new drawing pivots on its own shape, a selected chain is a screen-space
  stroke, and every kind of contour edits through one draft.
- X1, X2 and X7 are fixed: a legal ring sampled past the 1024-vertex cap, so the chord follows the
  shape (ADR 0062); a ribbon tool is one measured cell with an elided caption (ADR 0067).
- The icon pipeline was audited against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`):
  U27 rasterises each white master at `pt × pixels_per_point`, bakes the widget colour into the pixels
  and caches per `(uri, px, colour)`, at 20 pt tools and 16 pt inline; U28 gives a passive face 12–18 %
  fill, the result 45–60 % under its outline, one axonometry, and ink inside `3..21` at 20–45 %
  coverage (ADR 0066 adds a 22 pt ribbon button). 47 placeholders are in `plan/debt.md`.
- U38–U41 landed the visual pass: `theme.rs` owns two palettes (dark and light, the accent folded in),
  the 4 pt step, the row heights and the type scale (ADRs 0063, 0069); `header.rs` draws one row and
  returns a `HeaderAction` (ADR 0064); U39 landed the ribbon (one row of tabs over the tool groups of
  the active tab, each tool a 22 pt icon over a 10.5 pt caption, ADR 0066); U40 made the side panels
  fold over one header row and moved the view controls into a translucent `Order::Middle` cluster at
  the viewport's bottom centre, leaving the bar only the armed tool or gesture, its counts, the
  creation chip and the last error. U41 added the *Settings* modal (appearance, viewport, ribbon,
  shortcuts), the versioned `settings.json` in the OS config directory written atomically (ADR 0068)
  and the *Home* start surface; a change applies at once and a `.fcs` carries none of it. U42–U48
  queue the rest of the owner's design, and ADR 0065 fixes visibility and lock as session state.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) checks
  the three plan files: `updated=` is a real date no older than `base`, `task=` names the first
  unfinished task in `## Now` (P2–P3). Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

U47 is next: a command palette on `Ctrl+K`, one search field over `CommandRegistry::specs()` listing
a command's label, its id and whether it is enabled for the current session. The chosen entry
dispatches it with parameters built from the current selection; an entry that cannot run says why
instead of failing silently. The registry already carries 48 specs with a stable id, a label and a
pure `enabled` check, so the work is a filter, a keyboard loop and a parameter builder per family.

Order: U42 → U43 → U44 → U45 → U48 → U46 (icons last) → U49. Read the U47 entry, the registry in
`crates/engine`, ADRs 0028 and 0064, and `handle_header_action`.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
