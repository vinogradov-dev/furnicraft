# 0047. Icons are one ink on a 24 grid, in three families

- Status: Accepted
- Date: 2026-11-04

## Context

U1 laid out `assets/icons/` and `assets/README.md` before any icon was drawn. The design guide that
arrived with the tree described a 64-unit canvas, a semantic multi-colour palette, CSS variables and
masks. The actual pipeline is `resvg` behind `egui`, the button tints the rasterised texture with the
theme's text colour, and the UI has no theme system at all. Two documents defined the same facts, and
the first icon would have had to choose between them. The 64-unit weights were also sized for a 64 px
display: at a 16–24 px button a 2.4-unit stroke becomes 0.6–0.9 px and disappears.

## Decision

- Icons are drawn on a `24 × 24` grid with the ink inside `2..22`, in one ink. The file carries no
  colour; the button applies the theme's text colour when it rasterises, so one master serves every
  theme and a theme change needs no asset change.
- Emphasis is structural: weight, size, solidity and a real gap. The stroke width varies by role
  (`1.5` auxiliary, `1.6–1.8` main, `2.2–2.6` result) instead of one fixed width.
- Icons belong to one of three families with separate grammars: `geometry` (flat 2D or volumetric 3D),
  `view` (flat, monochrome navigation) and `service` (flat chrome). Service actions — confirm,
  cancel, undo, delete, file, warning — live in the `actions` theme and follow §19 of the guide: one
  glyph per verb, bare glyph = action, enclosed glyph = status, at most one modifier mark, readable at
  16 px without colour.
- `STYLE_GUIDE.md` is normative for meaning and composition; `assets/README.md` is normative for the
  file format, naming and the theme table. When they disagree, the guide wins on meaning and the
  README wins on format, and the losing file is fixed in the same change.
- The guide is read only when adding, editing or reviewing an icon (`AGENTS.md` §4.12); it does not
  constrain code that merely displays one.
- No masks, `defs`, `clipPath`, gradients, filters, CSS or tokens: gaps are real path breaks. The
  master is hand-authored SVG; automation, if ever needed, is a Rust `xtask` subcommand.

## Consequences

- The rasteriser stays the only colour consumer; there is no token substitution or per-theme asset
  set, and no second SVG per state or size.
- A status colour, if a theme later applies one, is redundant: the shape must already carry the
  meaning, which keeps the icons readable in one ink and for colour-blind users.
- The `actions` theme starts as 30 zero-byte placeholders; the existing empty-file fallback draws the
  button's text label until an icon is drawn, so no surface is blocked.
- Icons are reviewed against one checklist (16 px, one ink, real gap, one emphasis), and the service
  grammar keeps chrome from borrowing the CAD geometry language.
- The guide and the README must be updated together whenever the format changes; the ADR records why
  they are two documents rather than one.

## Related

- [`assets/icons/STYLE_GUIDE.md`](../../assets/icons/STYLE_GUIDE.md) — meaning, composition, §19 service grammar.
- [`assets/README.md`](../../assets/README.md) — file format, naming, theme table.
- [`AGENTS.md`](../../AGENTS.md) §4.12 — when the guide is read.
- [`.opencode/skills/draw-icon/SKILL.md`](../../.opencode/skills/draw-icon/SKILL.md) — the working checklist.
