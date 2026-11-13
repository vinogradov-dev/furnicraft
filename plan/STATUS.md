# Status

STATE: milestone=M7 phase=T task=P12 state=doing base=6f8b62c updated=2026-11-13

Read this file first; the two commands print the state and one task, `AGENTS.md` §1.1 orders a broken plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, `FurnType` is a six-digit tree (ADRs 0034–0035). `Contour` is a closed
  ring of stable-id segments (0040, schema 13) with the drawing-tolerance joint (0072); `Path` is the
  chain `to_contour` turns into a ring, a drawn figure carries a `PathShape` tag (0073), an extrusion
  owns its path (0074) and a primitive stores its parameters (0054). Every figure shows its grips, a
  corner moves freely or along the picked segment (0075), a fresh extrusion pulls its height from an
  arrow previewing a wireframe prism (X19), and S5's frame rule reached a drawing, a group and a panel
  (0082, 0083), proved once in `frame_contract.rs`; cameras and lights are scene objects (0036).
- S7 audited the design rules and filed S8–S22 under `plan/refactor.md`: S8–S13 split `geom` and gave
  the tests one fixture definition (ADR 0084), S14–S17 `engine/panel_cmds`, `scene`'s camera and ray
  and `render`'s pipelines, S18–S19 the document rules and the `Editor` out of `ui/lib.rs` (50 lines),
  S20–S22 the drawing draft and the shell's chrome (2608 -> 2037), then U45 the scene itself —
  `Document` per tab, `main.rs` 2037 -> 455 (ADR 0081); S21 predates its filing, F4 is in questions.
- U45 opens several scenes at once: one tab per open file in the header, with its dirty marker and
  its cross. *New* and *Open* open a tab (a path already open activates that tab), *Save*, *Undo* and
  *Redo* act on the active tab only, a close with unsaved changes asks and the last close leaves an
  empty *Untitled* scene; history, selection, camera and split layout travel with the tab (ADR 0081).
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards (0044–0046,
  schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every untouched node and
  a collapsed object keeps its shape (ADR 0053).
- The UI plan (U1–U26) is shipped: SVG icons, a work plane from the ortho view and draw/pick/snap for
  a `Path` (ADRs 0047–0052). X3–X6 and U30–U37 are fixed (ADRs 0056–0061), X20 gives a pole view a
  canonical up (0057) and U59 names the six views; X1, X2 and X7 are fixed too — a ring past the cap
  follows the shape (0062), a ribbon tool is one measured cell (0067). U27 caches icons per `(uri,
  px, colour)`; U46 drew every declared icon but `primitives/cone` (0055).
- U38–U42 and the X-fixes landed the visual pass: `theme.rs` owns two palettes and the type scale
  (ADRs 0063, 0069), `header.rs` returns a `HeaderAction` (0064), U39 the ribbon (0066), U40 the
  folded side panels and the bottom-centre cluster, U41 *Settings*, `settings.json` and *Home* (0068),
  U47 the `Ctrl+K` palette, U42 the one/two/four-view split with an orientation widget, U60 a tree,
  cameras and active view per layout (0079), U61/U62/U51 framing, expand and the floating ribbon
  (0080). U43/U63 gave each view its display style (0070), X10 made the edges mean the form, U44 a
  shadow under every floor-standing object (0071), X11 a ring closed by an arc (0072), X12 a tool
  switch replaces the draft and X23 makes every *Close* commit. U52 made the UI scale a scrubbing
  field, and every numeric field reads the drag's delta and clamps what is typed. U68 drew the
  ribbon from one const map (0091), U74 made a multi-method tool a split cell, and U75 put the group
  caption in a 16 pt band a step off the control tone, every name on one baseline, so the ribbon is
  76 pt. U65 gave every number one 76 x 22 field, right-aligned with its unit outside the string, one
  commit policy and one undo entry per drag or typing run (0092).
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) is the
  whole plan tool — gate, report, writer, dashboard, ADR index, `agents`/`init`, the command table
  (P4–P7, P9, ADR 0085); `xtask` keeps the layering rules and `ci` (P8), and the gate follows every
  `files:` hint, a closed entry's ADR, the `STATE:` keys, the reference files and a stale `base`.
  Gate: `cargo xtask ci`, green; `dev` → `main` with `--no-ff`.

## Next

P12: a phase can be declared through the plan tool, so the legend in `plan/BACKLOG.md` can never
drift from the ids the file uses. K2–K4 wait in `## Next` with R1 until the export work is wanted.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | `state=doing` |
| `plan/debt.md`, `plan/questions.md` | accepted gaps; decisions for the owner | a task touches that area |
| `plan/rename.md` | the pre-release rename: inventory, procedure, gotchas | task R1, or a change that adds a place the product name appears |
| `plan/refactor.md` | design rules, subtask template, order (S7, S8–S22) | any S subtask |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
