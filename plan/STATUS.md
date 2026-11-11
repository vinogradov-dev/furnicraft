# Status

STATE: milestone=M7 phase=A task=S18 state=idle base=1f361e1 updated=2026-11-11

Read this file first; the two commands print the state and one task, `AGENTS.md` §1.1 orders a broken plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, `FurnType` is a six-digit tree (ADRs 0034–0035). `Contour` is a closed
  ring of stable-id segments (ADR 0040, schema 13) with the drawing-tolerance joint (ADR 0072);
  `Path` is the chain `to_contour` turns into a ring, a drawn figure carries a `PathShape` tag (ADR
  0073), and an extrusion owns its path (ADR 0074). A primitive stores its parameters (ADR 0054);
  every figure shows its grips, a corner moves freely or along the picked segment (ADR 0075), and a
  fresh extrusion pulls its height from an arrow that previews the body as a wireframe prism (X19).
  S5 chose the frame rule; X24, X29 and X30 applied it to a drawing, a group and a panel (ADRs 0082,
  0083), and S6 proves it once in `crates/model/src/project/frame_contract.rs`.
- S7 audited the design rules and filed S8–S22 under `plan/refactor.md` (geom, model, engine, scene,
  render, then `Editor` and the shell; `main.rs` and `ui/lib.rs` before U45): S8 gave the shared
  test fixtures one definition in `model::testing` (ADR 0084); S9 split off `geom/segment.rs` and
  `geom/contour.rs`; S10 `geom/rect.rs` (the orthogonal boolean and its tracer); S11 `geom/ring.rs`
  and `geom/boolean.rs` (ADR 0043 amended); S12 the segment maths back to `geom/segment.rs` (X31);
  S13 the figure frames into `contour_frames.rs`; S14 `engine/panel_cmds` into `panel`, `machining`
  and `banding`; S15 `scene`'s camera and ray, S16 `render`'s pipelines and buffers, S17 the vertex
  builders; F4 waits in `plan/questions.md`.
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards (ADRs
  0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps its shape (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); one authored sun plus eight point lights (0039).
- The UI plan (U1–U26) is shipped: SVG icons, a work plane from the ortho view and draw/pick/snap
  for a `Path` (ADRs 0047–0052). X3–X6 and U30–U37 are fixed (ADRs 0056–0061): the projection button
  draws its view, a creation lands at the scene root unless a container is armed, the armed tool
  draws pressed, the pole views look down their plane, a drawing's frame is set at creation, every
  contour kind edits as one draft.
- X20 gives a pole view a canonical up (ADR 0057) and U59 names the six views; X1, X2 and X7 are fixed
  — a ring past the cap follows the shape (ADR 0062), a ribbon tool is one measured cell (ADR 0067).
  U27 caches icons per `(uri, px, colour)`; 47 placeholders sit in `plan/debt.md` (ADR 0055).
- U38–U42 and the X-fixes landed the visual pass: `theme.rs` owns two palettes and the type scale
  (ADRs 0063, 0069), `header.rs` returns a `HeaderAction` (ADR 0064), U39 the ribbon (ADR 0066), U40
  the folded side panels and the bottom-centre cluster, U41 *Settings*, `settings.json` and *Home*
  (ADR 0068), U47 the `Ctrl+K` palette, U42 the one/two/four-view split with an orientation widget,
  U60 a tree, cameras and active view per layout (ADR 0079), U61/U62/U51 framing, expand and the
  floating ribbon (ADR 0080). U43/U63 gave each view its display style (ADR 0070), X10 made the edges
  mean the form, U44 gave every floor-standing object a contact shadow (ADR 0071), X11 fixed a ring
  closed by an arc (ADR 0072), X12 makes a tool switch replace the draft and X23 every *Close* commit.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) is the
  whole plan tool — gate, report, writer, dashboard, ADR index, `agents`/`init`, the command table
  (P4–P7, P9, ADR 0085); `xtask` keeps the layering rules and `ci` (P8). P10 completed the write
  surface (`--repair`, `task add --at top`, `state where`, `file`) and the gate now follows every
  `files:` hint, a closed entry's ADR, the `STATE:` keys, the reference files and a stale `base`.
  Gate: `cargo xtask ci`, green; `dev` → `main` with `--no-ff`.

## Next

S18 is next: the three document rules `ui/lib.rs`'s `Editor` holds — the frame composition, the
600 x 400 panel default and the picking and creation-target rules — move down to `model` and
`engine`; S19–S22 follow.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | `state=doing` |
| `plan/debt.md`, `plan/questions.md` | accepted gaps; decisions for the owner | a task touches that area |
| `plan/rename.md` | the pre-release rename: inventory, procedure, gotchas | task R1, or a change that adds a place the product name appears |
| `plan/refactor.md` | design rules, subtask template, order (S7, S8–S22) | any S subtask |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
