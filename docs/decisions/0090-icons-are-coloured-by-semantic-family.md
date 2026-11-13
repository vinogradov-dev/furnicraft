# 0090. An icon is coloured by its semantic family, and the theme maps the literal

- Status: Accepted
- Date: 2026-11-12

## Context

ADR 0047 and ADR 0055 made "one ink, and it is white" normative: a master carries no colour, the UI
multiplies its alpha by the widget's foreground, and colour never carries meaning. The set has since
grown to 81 icons across seven themes, and the owner approved colouring it by what an icon means —
drawn geometry, an edit, a solid, a scene object, navigation, a destructive command — instead of one
grey silhouette on every surface. A colour per theme directory would have been wrong (the surface is
not the meaning: `actions/delete` belongs to no toolbar), a second master per theme would have
doubled the set and broken the "one file, one drawing" rule, and CSS tokens or masks are forbidden by
the SVG contract.

The RASTER size also matters: at 16 px a two-tone fill reads as a muddy blob and a hue cannot be told
apart, while at 20 px the tones separate. The owner's line was "below 20 px, one ink is fine".

## Decision

- A master writes up to three **role literals** from one closed table, and its tagged family's literal
  is always one of them: `draw #58C8DC`, `modify #A98BF0`, `model #6E9EF7`, `scene #55C7A4`,
  `view #B8C2CE`, `danger #E06A6A`. The dark column is the master's value; no other colour, class,
  token or gradient enters the file.
- **The family is tagged on the icon, not on the theme directory.** `contour/arc` is `draw`,
  `primitives/cuboid` is `model`, `actions/delete` is `danger`; the theme says where the icon is shown,
  the family says what it means. `danger` is for destructive commands only.
- **The rasteriser maps the literals to the active variant's ink before the upload.** `RoleRamp` holds
  the six dark inks (the identity mapping) and the six folded light inks; the texture is built from
  the substituted SVG, so the alpha silhouette is the drawing itself and dark and light differ in hue,
  never in shape. The preset changes neutrals and the accent, so the ramp is per variant and a preset
  switch re-rasterises nothing.
- **The one-ink path stays as the fallback.** Below a 20 px raster, or when `appearance.icon_color` is
  `Never`, the same master is rasterised and its alpha multiplied by the widget's colour — the U27
  path, byte for byte. `Always` keeps the role inks at every size.
- State is tone, not artwork: hover and press move the role ink one step away from the surface,
  disabled is one alpha multiply, and a chosen toggle draws the theme's foreground because the accent
  fill behind it is chrome. The accent is never a role ink.
- The cache is keyed by `(uri, px, variant, tone)` for a role raster and `(uri, px, tint)` for a
  one-ink raster.
- Every role ink clears 3:1 against the panel and the ribbon in all three presets and both variants;
  a test measures it, along with the silhouette identity, the 16 px equality with the tint path and
  the closedness of the literal table.

`assets/README.md` is normative for the table and the format, `STYLE_GUIDE.md` §5/§7 for the meaning
and the threshold, `AUDIT.md` §8 for the measurements.

## Consequences

- The set has a hue that says what an icon is for, and a preset or a variant never needs a second
  asset; the 80 drawn files were re-inked mechanically and their geometry did not move.
- "One ink" is no longer absolute: the meaning still has to survive in one ink, and the tests keep
  the role hue redundant rather than load-bearing.
- A new icon now requires two decisions (grammar and family) and a literal from the closed table; a
  colour outside it fails the asset test.
- The light column is a second table to maintain, but it is six values folded once, and the contrast
  test fails before a hue becomes illegible.

## Related

- [ADR 0047](0047-icons-are-one-ink-on-a-24-grid.md) — the grid, the weights and the families this
  record keeps; its one-ink clause is superseded here.
- [ADR 0055](0055-the-ui-rasterises-icons-and-emphasis-is-tonal.md) — the rasteriser, the tonal
  emphasis and the pixel tests this record extends.
- [ADR 0089](0089-three-palettes-are-data-and-derive-their-states.md) — the presets the role inks are
  measured against.
- [`assets/README.md`](../../assets/README.md) — the role table and the file format.
- [`assets/icons/STYLE_GUIDE.md`](../../assets/icons/STYLE_GUIDE.md) — §5, §7.5.
- [`assets/icons/AUDIT.md`](../../assets/icons/AUDIT.md) — §8, the measurements.
