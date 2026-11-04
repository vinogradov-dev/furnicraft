# 0055. The UI rasterises icons itself, and emphasis is tonal

- Status: Accepted
- Date: 2026-11-04

## Context

U24 laid down the icon contract (ADR 0047): one ink on a 24 grid, three families, the button tints the
rasterised texture. U26 drew the first 18 icons. Two assumptions in that contract turned out to be
false in practice.

1. `egui::Image::tint` multiplies into the texture (`out = vertex_colour × texture`), and every master
   is drawn `stroke="#000"`. A tint can only darken, so the product is black whatever the theme is:
   icons ignore the theme entirely and are close to invisible on the dark theme. The test that was
   supposed to cover this inspected the colour handed to the painter, not the pixels.
2. A pure-stroke drawing at 16 pt puts a 1.6–2.4-unit line at 1.07–1.6 px. On a 1× display the ink
   breaks up; the set reads as a sketch next to the filled volume the CAD viewport itself draws.

The owner also asked for the set to look like the one in a neighbouring CAD project, which fills
passive faces and emphasises the result by mass. That project's pipeline (its own usvg subset,
role-coloured tokens, a 64 grid) is not what we want to copy: three of its four mechanisms exist to
serve 391 icons across several designers, and our guide forbids the SVG elements it needs for them.

## Decision

- **The UI owns rasterisation.** `crates/ui/src/icons.rs` parses and rasterises an embedded master
  with `egui_extras::image::load_svg_bytes_with_size` at `points × pixels_per_point`, caches a
  `TextureHandle` per `(uri, pixel size)`, and paints it with `Painter::image` in the theme's icon
  colour. `Image::tint` is no longer used for icons. No dependency is added: `resvg` already arrives
  through `egui_extras`'s `svg` feature.
- **The ink is white.** A master is drawn `#fff`; the UI multiplies it by
  `visuals.widgets.inactive.fg_stroke.color` (hover and press follow the widget state, disabled is an
  alpha multiply). This keeps one master for every theme and makes the tint arithmetic correct.
- **Sizes are a parameter, not a constant.** Toolbar tool buttons draw at 20 pt, inline and menu use at
  16 pt. A size is a point size; the raster follows the UI scale, so no per-size asset exists.
- **Emphasis is tonal, still in one ink.** A shape may carry `fill-opacity`: a passive face at
  12–18 %, the result or changed geometry at 45–60 % with the heavy outline (2.2–2.6). `defs`, `mask`,
  `clipPath`, gradients, filters, CSS and per-theme assets stay forbidden: the tone is alpha, not a
  colour, so a master still serves every theme and still reads without any colour at all.
- **One published axonometry.** The guide fixes the projection (vertical axis vertical, both
  receding axes at 30°, a circle on a face as a `0.577` ellipse) and the optical footprint
  (ink inside `3..21`, coverage 20–45 %), so sibling icons line up instead of each inventing a view.
- **A pixel test guards the path.** Drawing one icon in both themes must produce the theme's colour
  in the uploaded pixels, an identical alpha silhouette, and a raster of `round(pt × ppp)` at 1.0,
  1.25 and 2.0. A future signature change cannot silently make the tint arithmetic wrong again.

`assets/README.md` becomes normative for the white ink and `fill-opacity`; `STYLE_GUIDE.md` §5, §7.3
and §9 for tone and projection. `assets/icons/AUDIT.md` keeps the measurements and the rejected
alternatives.

## Consequences

- The theme finally controls the icons, and a theme change still needs no asset change; the same
  master serves light, dark and any future theme.
- Mass makes the set legible at 16 px on a 1× display, which stroke weight alone cannot do, and
  removes the reason to add a second asset per size.
- The UI rasterises icon textures itself, so the cache key must include the pixel size and the
  colour; the palette lives in `crates/ui`, not in `egui_extras`.
- `fill-opacity` widens the SVG subset we accept, so the icon skill, the README and the tests that
  assert the subset must change in the same commit.
- The 33 remaining placeholder files stay placeholders; the empty-file fallback is unchanged, and
  drawing them is separate work recorded in `plan/debt.md`.
- A single semantic accent (a colour for the "result") is deliberately **not** decided here. If it
  arrives, it supersedes only the "one ink" clause and needs a palette plus a cache key that includes
  the theme.

## Related

- [`assets/README.md`](../../assets/README.md) — file format, naming, theme table.
- [`assets/icons/STYLE_GUIDE.md`](../../assets/icons/STYLE_GUIDE.md) — meaning, composition, tone and projection.
- [`assets/icons/AUDIT.md`](../../assets/icons/AUDIT.md) — measurements and the options that were rejected.
- [ADR 0047](0047-icons-are-one-ink-on-a-24-grid.md) — the contract this record refines.
- [`.opencode/skills/draw-icon/SKILL.md`](../../.opencode/skills/draw-icon/SKILL.md) — the working checklist.
