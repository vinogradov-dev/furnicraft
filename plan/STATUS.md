# Status

STATE: milestone=M7 phase=A task=S11 state=idle base=186a5bc updated=2026-11-10

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
  S5 chose the frame rule; X24, X29 and X30 applied it to a drawing, a group and a panel (ADRs 0082,
  0083), and S6 proves it once in `crates/model/src/project/frame_contract.rs`.
- S7 audited the design rules and filed S8–S22 under `plan/refactor.md` (geom, model, engine, scene,
  render, then `Editor`, then the shell; `main.rs` and `ui/lib.rs` land before U45). S8 gave the shared
  test fixtures one definition in `model::testing` behind a `testing` feature the testkit re-exports
  (ADR 0084); S9 split off `geom/segment.rs`, so `geom/contour.rs` keeps the ring that joins, areas,
  offsets and samples it; S10 split off `geom/rect.rs` (the exact orthogonal boolean and its tracer),
  so `geom/region.rs` keeps the ring model and the general contour boolean. F4
  (`Project::apply(Change)`) waits in `plan/questions.md`; the no-action verdicts are `[design]` debt.
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
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented; `state-contract` (L0)
  checks the plan. Gate: `cargo xtask ci`, green; `dev` → `main` with `--no-ff`.

## Next

S11 is next: `geom/region.rs` (1126 production lines, still over B4's ceiling) gives up the ring model
to `geom/ring.rs` and what remains becomes the one named algorithm in `geom/boolean.rs`. Then S12–S13
take the trigonometry into `geom` and split `model/contour_edit.rs`, S14 in `engine`, S15–S17 in
`scene` and `render`, S18–S20 in `ui`, S21–S22 in the shell — rule ids and line targets in
`plan/BACKLOG.md` `## Now`.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | `state=doing` |
| `plan/debt.md`, `plan/questions.md` | accepted gaps; decisions for the owner | a task touches that area |
| `plan/rename.md` | the pre-release rename: inventory, procedure, gotchas | task R1, or a change that adds a place the product name appears |
| `plan/refactor.md` | design rules, subtask template, order (S7, S8–S22) | any S subtask |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
