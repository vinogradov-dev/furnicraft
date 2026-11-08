# Status

STATE: milestone=M7 phase=A task=X16 state=doing base=916b15e updated=2026-11-08

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of stable-id segments (ADR 0040,
  schema 13) whose joints take the drawing tolerance (ADR 0072); `Path` is the chain `to_contour`
  turns into a ring, a drawn figure carries a `PathShape` tag at schema 19 (ADR 0073), and an
  extrusion owns its path (ADR 0074). A primitive stores its parameters (ADR 0054); every figure
  shows its grips, a corner moves freely or along the segment picked at the drag start, a grip keeps
  its figure (ADR 0075), and a fresh extrusion pulls its height from an arrow.
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards
  (ADRs 0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps the shape the user saw (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus eight
  point lights in watts with a fixed exposure and no tone mapper (ADR 0039).
- The UI plan (U1–U26) is shipped: SVG icons, a work plane from the ortho view, a projection toggle
  and draw/pick/snap for a `Path` (ADRs 0047–0052). X3–X6 and U30–U37 are fixed (ADRs 0056–0061):
  the projection button draws the view it is in, a creation lands at the scene root unless a
  container is armed, the armed tool draws pressed, the pole views look down their plane, a drawing
  pivots on its own shape, a selected chain is a screen-space stroke, every contour kind edits as one draft.
- X1, X2 and X7 are fixed: a legal ring past the 1024-vertex cap follows the shape (ADR 0062), and
  a ribbon tool is one measured cell with an elided caption (ADR 0067). The icon pipeline was audited
  against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`): U27 rasterises each white master
  at `pt × pixels_per_point`, bakes the widget colour into the pixels and caches per `(uri, px,
  colour)`, at 20 pt tools and 16 pt inline; 47 placeholders are in `plan/debt.md`.
- U38–U42 landed the visual pass: `theme.rs` owns two palettes and the type scale (ADRs 0063, 0069);
  `header.rs` returns a `HeaderAction` (ADR 0064); U39 landed the ribbon (ADR 0066); U40 folded the
  side panels and moved the view controls into a bottom-centre cluster; U41 added *Settings*, the
  atomic `settings.json` (ADR 0068) and *Home*; U47 added the `Ctrl+K` palette; U42 divided the
  central area into one, two or four views with a split tree, a scissored pass per leaf and an
  orientation widget with a home button. U43 made the five display styles real (ADR 0070) and X10
  made their edges mean the form; X10's stencil slot lets an object show its own inside but never
  the one behind it. U44 gives every floor-standing object a contact shadow (ADR 0071). X11 fixed a
  ring closed by an arc: a joint is `1 µm + arc_slack(first) + arc_slack(next)`, so a large arc's
  derived end closes, reaches a contour and extrudes, and the preview never blanks (ADR 0072). X12
  made a drawing tool switch and cancel: another tool replaces the running draft, Escape discards
  it, Enter and the right button commit, and the shortcut table reads keys while a draft runs.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented; `state-contract` (L0)
  checks the plan files. Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

X14 is done: the polygon grip set has no phantom node outside the figure,
and a circle's rim grip works from any arc segment and keeps it round.
X16 is next: a rectangle keeps right angles and its axes at any angle,
a midpoint drag never yields zero, and a corner drag stays in range.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md`, `plan/questions.md` | accepted gaps; decisions waiting for the owner | a task touches that area |
| `plan/rename.md` | the pre-release rename: inventory, procedure, gotchas | task R1, or a change that adds another place the product name appears |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
