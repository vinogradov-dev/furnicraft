# UI icon resources

One SVG per toolbar icon, grouped by the theme of the toolbar that shows it. `crates/ui/src/icons.rs`
embeds this tree and draws an icon into a themed button; the theme and the icon name are the keys the
UI asks for, so this layout is part of the code's contract.

## Layout

```text
assets/icons/<theme>/<name>.svg
```

| Theme | Icons | Shown by |
|---|---|---|
| `contour` | `line`, `polyline`, `arc`, `rectangle`, `circle`, `close`, `cancel` | Contour toolbar |
| `primitives` | `cube`, `cylinder`, `sphere`, `cone` | Primitives toolbar |
| `bodies` | `extrude`, `revolve`, `sweep` | Bodies toolbar |
| `scene` | `panel`, `assembly`, `camera`, `light` | Scene toolbar |
| `view` | `orthographic`, `perspective`, `frame-all` | Viewport strip |

A theme has no other files: no sprites, no sheets, no per-scale variants. The button rasterises one
file at the pixel size it draws, so a second size would only drift from the first.

## Naming

- lowercase ASCII `kebab-case`, extension `.svg`; the file stem is the icon id.
- no `icon-` prefix, no numeric prefix, no size suffix: the theme directory already says what the icon
  is for, and the UI scale already says how large it is drawn.
- one drawing per file, one file per id. If one drawing serves two themes, it is two files: a theme
  owns its icons and may change one without touching the other.
- a theme directory is named after the toolbar panel. Renaming a theme or an icon is an API change:
  the same commit updates `crates/ui/src/icons.rs` and the table above.

## Format

- SVG 1.1, one root `<svg>` with `xmlns="http://www.w3.org/2000/svg"`, `viewBox="0 0 24 24"`,
  `width="24"` and `height="24"`. The file is self-contained: no references to sibling files.
- a 24 × 24 unit drawing grid with a 2 unit margin, so the ink stays inside `2..22` on both axes.
  The rasteriser runs at 16–48 px; a shape thinner than 1.5 units disappears.
- stroke geometry: `fill="none"`, `stroke="#000"`, `stroke-width="1.5"`, `stroke-linecap="round"`,
  `stroke-linejoin="round"`. A dot, an arrowhead or a solid silhouette may use `fill="#000"`.
- one ink colour only. The button tints the rasterised texture with the theme's text colour, so
  colour never carries meaning and a theme change needs no asset change.
- geometry only: `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `g` and their
  `transform`. No `<text>` (a glyph that must be a character belongs in the UI, not in an icon), no
  `<image>`, `<foreignObject>`, `<script>`, `<style>`, font, filter, mask, pattern or gradient — the
  loader is `resvg`, and the effects it can do either cost raster time or read wrong at button size.
- two decimals at most, no editor metadata namespaces, no `<title>`/`<desc>`: the button's tooltip is
  the accessible name, and the icon carries no prose.

## Placeholders

Every file is empty (zero bytes) until the toolbar that owns it is built. An empty file means "not
drawn yet": the icon button draws its text label instead, so a toolbar works while its artwork is
still arriving. Keep the file empty rather than half-drawn — a whitespace-only file is still a broken
drawing, not a placeholder.

To fill an icon, replace the whole file with a drawing that follows the format above. The path, the
theme and the file name stay unchanged.

## Ownership

The icons are drawn for FurniCraft and carry the project license, MIT OR Apache-2.0. Do not copy an
icon set from another product (`AGENTS.md` §4.6): a third-party icon keeps its license file next to
the asset and gets a row in `ATTRIBUTION.md` in the same change.
