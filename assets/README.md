# UI icon resources

One SVG per icon, grouped by the theme of the surface that shows it. `crates/ui/src/icons.rs` embeds
this tree and draws an icon into a themed button; the theme and the icon name are the keys the UI
asks for, so this layout is part of the code's contract.

This file is normative for the **file format** — viewBox, allowed elements, naming, the theme table.
`STYLE_GUIDE.md` is normative for **meaning and composition**. When the two disagree, the guide wins
on meaning and this file wins on format; fix the losing file in the same change. Read both only when
adding, editing or reviewing an icon. `AUDIT.md` in this directory holds the measured state of the
set and the pipeline, not the rules.

## Layout

```text
assets/icons/<theme>/<name>.svg
```

| Theme | Icons | Shown by |
|---|---|---|
| `contour` | `line`, `polyline`, `arc`, `arc-centre`, `rectangle`, `rectangle-centre`, `rectangle-centre-directed`, `circle`, `circle-three-points`, `polygon`, `close`, `cancel` | The ribbon *Sketch* tab |
| `primitives` | `cube`, `cylinder`, `sphere`, `cone` | The ribbon *Model* tab |
| `bodies` | `extrude`, `revolve`, `sweep` | The ribbon *Model* tab |
| `scene` | `panel`, `assembly`, `camera`, `light` | The ribbon *Scene* tab |
| `view` | `orthographic`, `perspective`, `frame-all`, `maximize`, `restore`, `home`, `grid-snap`, `orbit`, `pan`, `zoom`, `layout`, `shaded`, `shaded-edges`, `wireframe`, `xray`, `translucent`, `sliders` | The ribbon *View* tab, the floating viewport cluster, the header |
| `actions` | `confirm`, `dismiss`, `cancel`, `undo`, `redo`, `delete`, `add`, `remove`, `duplicate`, `rename`, `copy`, `paste`, `file-new`, `file-open`, `file-save`, `file-export`, `file-import`, `print`, `settings`, `search`, `visible`, `hidden`, `lock`, `unlock`, `expand`, `collapse`, `warning`, `error`, `info`, `help` | Menus, dialogs, strips and shortcut reference |
| `window` | `minimize`, `maximize`, `restore`, `close` | The window row of the header; drawn only when the system frame is off |

A theme has no other files: no sprites, no sheets, no per-scale variants. The UI rasterises one file
at the pixel size it draws, so a second size would only drift from the first.

`assets/icons/STYLE_GUIDE.md` is documentation, not an icon: the loader reads only
`assets/icons/<theme>/*.svg`.

## Naming

- lowercase ASCII `kebab-case`, extension `.svg`; the file stem is the icon id.
- no `icon-` prefix, no numeric prefix, no size suffix: the theme directory already says what the icon
  is for, and the UI scale already says how large it is drawn.
- one drawing per file, one file per id. If one drawing serves two themes, it is two files: a theme
  owns its icons and may change one without touching the other.
- a theme directory is named after the toolbar panel or the UI surface it serves; `actions` is the
  chrome theme of `STYLE_GUIDE.md` §19. Renaming a theme or an icon is an API change: the same commit
  updates `crates/ui/src/icons.rs` and the table above.

## Format

- SVG 1.1, one root `<svg>` with `xmlns="http://www.w3.org/2000/svg"`, `viewBox="0 0 24 24"`,
  `width="24"` and `height="24"`. The file is self-contained: no references to sibling files.
- a 24 × 24 unit drawing grid. The ink stays inside `3..21` on both axes and covers 20–45 % of that
  18 × 18 footprint, measured from the rasterised alpha (`STYLE_GUIDE.md` §7.4). The rasteriser
  follows the UI scale (16 pt inline, 20 pt in a toolbar, times `pixels_per_point`), so a shape
  thinner than 1.5 units disappears at a low scale.
- stroke geometry: `stroke="#fff"`, `stroke-width` between `1.5` and `2.6`, `stroke-linecap="round"`,
  `stroke-linejoin="round"`. The weights by role are in `STYLE_GUIDE.md` §7.3. A stroke-only shape
  keeps `fill="none"`.
- tone: a face takes `fill="#fff"` with `fill-opacity` `0.12–0.18` (passive) or `0.45–0.60` (result or
  changed geometry) and keeps its full-strength outline. The tone is alpha on the same white ink, so
  no theme needs a second file. `fill-opacity` is the only opacity attribute: `opacity` would dim the
  outline too. A dot, an arrowhead or a solid silhouette may use `fill="#fff"` with no stroke.
- one ink colour only, and it is white. The UI rasterises the master, multiplies it by the widget's
  foreground colour and uploads the result, so colour never carries meaning and a theme change needs
  no asset change. No other `#hex`, `class`, CSS variable or theme token inside the file.
- geometry only: `path`, `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `g` and their
  `transform`, with the paint attributes `fill`, `fill-opacity`, `stroke`, `stroke-width`,
  `stroke-linecap`, `stroke-linejoin`, and nothing else. No `<text>` (a glyph that must be a character
  belongs in the UI, not in an icon), no `<defs>`, `<clipPath>`, `<mask>`, `<style>`, `<image>`,
  `<foreignObject>`, `<script>`, font, filter, pattern or gradient — the loader is `resvg`, and the
  effects it can do either cost raster time or read wrong at button size.
- two decimals at most, no editor metadata namespaces, no `<title>`/`<desc>`: the button's tooltip is
  the accessible name, and the icon carries no prose.

## Placeholders

Every file is empty (zero bytes) until the surface that owns it is built. An empty file means "not
drawn yet": the icon button draws its text label instead, so a surface works while its artwork is
still arriving. Keep the file empty rather than half-drawn — a whitespace-only file is still a broken
drawing, not a placeholder.

To fill an icon, replace the whole file with a drawing that follows the format above. The path, the
theme and the file name stay unchanged.

## Ownership

The icons are drawn for FurniCraft and carry the project license, MIT OR Apache-2.0. Do not copy an
icon set from another product (`AGENTS.md` §4.6): a third-party icon keeps its license file next to
the asset and gets a row in `ATTRIBUTION.md` in the same change.
