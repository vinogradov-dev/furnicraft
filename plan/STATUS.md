# Status

STATE: milestone=M7 phase=A task=X27 state=idle base=c992043 updated=2026-11-10

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, `FurnType` is a six-digit tree (ADRs 0034–0035). `Contour` is a closed
  ring of stable-id segments (ADR 0040, schema 13) with the drawing-tolerance joint (ADR 0072);
  `Path` is the chain `to_contour` turns into a ring, a drawn figure carries a `PathShape` tag (ADR
  0073), and an extrusion owns its path (ADR 0074). A primitive stores its parameters (ADR 0054);
  every figure shows its grips, a corner moves freely or along the picked segment (ADR 0075), and a
  fresh extrusion pulls its height from an arrow that previews the body as a wireframe prism (X19).
  S5 chose the frame rule and X24 made a drawing use it: `rebase_at_anchor`, `Placement::inverse`,
  and a drawing's creation pivot as its local zero (ADR 0082).
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards (ADRs
  0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps its shape (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus eight
  point lights in watts with a fixed exposure and no tone mapper (ADR 0039).
- The UI plan (U1–U26) is shipped: SVG icons, a work plane from the ortho view and draw/pick/snap
  for a `Path` (ADRs 0047–0052). X3–X6 and U30–U37 are fixed (ADRs 0056–0061): the projection button
  draws its view, a creation lands at the scene root unless a container is armed, the armed tool
  draws pressed, the pole views look down their plane, a drawing's frame is set at creation, every
  contour kind edits as one draft.
- X20 gives a pole view a canonical up: the top is `+Y` up and `+X` right, the bottom `+Y` up and
  `-X` right, and the side and front views keep `+Z` up (ADR 0057). U59 names the six standard views
  with the owner's eye convention and makes a preset or axis snap ortho; `O` still toggles.
- X1, X2 and X7 are fixed: a legal ring past the 1024-vertex cap follows the shape (ADR 0062), and a
  ribbon tool is one measured cell with an elided caption (ADR 0067). U27 rasterises each icon
  master and caches per `(uri, px, colour)`; 47 placeholders are in `plan/debt.md` (ADR 0055).
- U38–U42 landed the visual pass: `theme.rs` owns two palettes and the type scale (ADRs 0063, 0069);
  `header.rs` returns a `HeaderAction` (ADR 0064); U39 landed the ribbon (ADR 0066); U40 folded the
  side panels and moved the view controls into a bottom-centre cluster; U41 added *Settings*, the
  atomic `settings.json` (ADR 0068) and *Home*; U47 added the `Ctrl+K` palette; U42 divided the
  central area into one, two or four views with a split tree and an orientation widget; U60 gives
  every layout its own tree, cameras and active view (ADR 0079), U61 makes *Frame all* fit the
  scene's current bounds, U62 expands a view with *Expand* / *Restore* or `Alt+W`, and U51 floats
  the ribbon over the design space (ADR 0080). U43 made the five display styles real (ADR 0070) and
  U63 gives each view its own style; X10 made their edges mean the form, and its stencil slot shows
  an object's own inside but never the one behind it. U44 gives every floor-standing object a
  contact shadow (ADR 0071); X11 fixed a ring closed by an arc: a joint takes the `arc_slack` of
  either end, so a large arc's derived end closes, extrudes and never blanks the preview (ADR 0072).
  X12 makes a tool switch: another tool replaces the draft, Escape discards it, Enter or RMB
  commits, shortcuts read keys mid-draft. X23 makes every *Close* close and commit (ADR 0052).
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented; `state-contract` (L0)
  checks the plan files. Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

X26 anchored a panel at its contour's base corner (ADR 0083): `add_panel_contour` rebases the ring
and `panel.contour.create` lands it exactly over its source. X27 makes the inspector tell the local
and the world truth: parent-relative labels, a read-only world position and world pivot, and
`node.placement.world_zero`, which clears a nested translation without turning the node.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md`, `plan/questions.md` | accepted gaps; decisions waiting for the owner | a task touches that area |
| `plan/rename.md` | the pre-release rename: inventory, procedure, gotchas | task R1, or a change that adds another place the product name appears |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
