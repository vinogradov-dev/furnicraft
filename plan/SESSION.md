task: S7
goal: the design rules become an ordered refactor plan: one small behaviour-preserving subtask per measured finding, in dependency order

steps:
  [x] `cargo xtask ci` green on the pre-audit tree (layers, plan, fmt, clippy, tests)
  [x] re-measure §2: main 2609, region 1660, contour_edit 1572, render 1446, scene 1277, ui/lib 1159,
      engine/panel_cmds 1143, ui/contour_edit 1112, geom/contour 1046, the two cards 989, toolbars 982
  [x] measure impls and functions: `impl Editor` 966, `impl FurniCraftApp` 1620, `viewport` 635,
      `home_surface` 632, `impl PanelCard` 432, `impl ObjectCard` 469, `show_group_row` 146
  [x] verdicts for F1-F7 and the three newly measured crossings (geom/contour, engine/panel_cmds,
      ui/contour_edit); F4 and the no-action findings routed to `plan/questions.md` and `[design]` debt
  [x] file S8-S22 in `## Now` in dependency order, each with rule ids and a line target
  [ ] S7 Done entry with the verdicts, `plan/STATUS.md` naming S8, and the audit commit

now: the S7 Done entry and the `## Now` entries in `plan/BACKLOG.md`
blocked: nothing
run: cargo xtask plan
resume: read `plan/BACKLOG.md` `## Now`, S8 is next
map: `plan/map.md` `` | Plan gate | state-contract/src/validate.rs
decisions: the audit filed no split that U45 already performs (`viewport` onto `Document`), no split of
  a module under B4's ceiling (the two cards, `toolbars`), and no split of a value family whose header
  already states why its public surface stays together (B6 escape).
