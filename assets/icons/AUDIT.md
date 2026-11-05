# Icon pipeline audit

Status: evidence for [ADR 0055](../../docs/decisions/0055-the-ui-rasterises-icons-and-emphasis-is-tonal.md).
Not normative: the format is `README.md` and the meaning is `STYLE_GUIDE.md`. This file records what
the pipeline actually does today, the measurements behind the redesign, and what was deliberately
left out, so the question does not have to be re-explored.

Date: 2026-11-04. Written after studying a neighbouring CAD project's icon set **for behaviour and
look only** (`AGENTS.md` §4.6): no code, shader, asset or file format was copied, and the numbers
below are measurements of that project's public SVG sources, not extracted implementation.

## 1. The pipeline as it is

- `assets/icons/<theme>/<name>.svg` is embedded by `crates/ui/src/icons.rs` (`icons!` macro,
  `include_bytes!`), one module per theme, `ALL` per theme for audits.
- `icon_button` / `tool_button` wrap the icon in `egui::Image::from_bytes(uri, bytes)`,
  `.fit_to_exact_size(16 pt)`, `.tint(ui.visuals().text_color())`.
- `egui_extras`'s SVG loader rasterises with `resvg` at `points × pixels_per_point` and caches per
  `(uri, SizeHint)`; `ui_scale.rs` therefore re-rasterises on a zoom change instead of stretching.
- A zero-byte file is a documented placeholder: the button draws its text label instead.

Two consequences are worth stating plainly, because both were assumed rather than tested:

1. `Image::tint` becomes the vertex colour of the mesh, and the egui shader computes
   `out = in.color * texture` (`egui-wgpu/src/egui.wgsl`). The masters are drawn `stroke="#000"`,
   so the product is always black: **the theme colour never reaches a glyph.** On the dark theme
   that is `gray(140)` text on a `gray(50)` inactive button, about 1.4–1.5 : 1 — effectively
   invisible; on the light theme it is a harsh `0` against `gray(60)` text.
2. The only test of this path (`the_tint_is_the_theme_text_colour`) inspects the `RectShape.fill`,
   i.e. the colour handed to the painter, not the pixels that come out. That is why a green suite
   kept the defect.

Colour is not a scaling problem: the raster is produced at the drawn pixel size, so a multi-colour
master is crisp at 75 % and 200 % alike.

## 2. What the set looks like, measured

Rendered with `rsvg-convert` at 64 px and reduced to the 24-unit grid; alpha above 16 counts as ink.
Drawn files only (18 of 51; the other 33 are zero-byte placeholders).

| icon | ink bbox (grid units) | size | coverage |
|---|---|---|---|
| bodies/extrude | 1.1..22.1 × 6.4..20.2 | 21.0 × 13.9 | 26.0 % |
| contour/arc | 2.2..21.8 × 5.2..18.8 | 19.5 × 13.5 | 11.0 % |
| contour/cancel | 4.9..19.1 × 4.9..19.1 | 14.2 × 14.2 | 14.6 % |
| contour/circle | 3.4..20.6 × 3.4..20.6 | 17.2 × 17.2 | 18.8 % |
| contour/close | 3.4..18.8 × 6.4..19.5 | 15.4 × 13.1 | 15.6 % |
| contour/line | 3.4..20.6 × 3.4..20.6 | 17.2 × 17.2 | 8.4 % |
| contour/polyline | 3.4..20.6 × 4.5..19.5 | 17.2 × 15.0 | 13.2 % |
| contour/rectangle | 2.2..21.8 × 4.5..19.5 | 19.5 × 15.0 | 22.1 % |
| primitives/cube | 3.0..21.0 × 1.9..22.1 | 18.0 × 20.2 | 31.0 % |
| primitives/cylinder | 4.5..19.5 × 3.0..21.4 | 15.0 × 18.4 | 26.1 % |
| primitives/sphere | 3.4..20.6 × 3.4..20.6 | 17.2 × 17.2 | 27.7 % |
| scene/assembly | 1.9..22.1 × 3.8..20.2 | 20.2 × 16.5 | 26.1 % |
| scene/camera | 2.2..21.8 × 5.6..19.1 | 19.5 × 13.5 | 26.2 % |
| scene/light | 1.9..22.1 × 1.9..22.1 | 20.2 × 20.2 | 24.5 % |
| scene/panel | 1.5..20.6 × 4.5..18.4 | 19.1 × 13.9 | 22.7 % |
| view/frame-all | 2.2..21.8 × 2.2..21.8 | 19.5 × 19.5 | 23.0 % |
| view/orthographic | 2.2..21.8 × 4.1..19.9 | 19.5 × 15.8 | 36.1 % |
| view/perspective | 1.9..22.1 × 5.2..20.2 | 20.2 × 15.0 | 29.1 % |

Optical size wanders over 14.2…21.0 units and coverage over 8.4…36.1 %, so the set does not read as
one family. Stroke weights are 1.6…2.4 units, which is 1.07…1.6 px at 16 pt: on a 1× display the ink
breaks up, and only a 2× display looks clean.

The neighbour set, measured the same way, sits at 25–56 % coverage with a consistent footprint, and
its modelled objects are **filled volumes with a grey passive face**, not outlines. That contrast is
the whole difference: mass, not detail, is what makes it read at 16 px.

## 3. Three defects

- **D1 — the theme never reaches the icon.** See §1. Fix: rasterise in the UI and paint the texture
  in the theme's icon colour, with a pixel test. Owners: `crates/ui/src/icons.rs`.
- **D2 — no mass, only wireframe.** No file has a fill. Fix: one ink, tonal fills by alpha, so a
  theme change still needs no asset change. Owners: `assets/README.md`, `STYLE_GUIDE.md`, all 18
  drawn SVGs.
- **D3 — no canonical projection or footprint.** `primitives/cube`, `scene/panel`, `scene/assembly`
  and `bodies/extrude` each invent their own axonometry, with no shared projection and no enforced
  optical size. Owners: `STYLE_GUIDE.md` §9, four SVG files.

## 4. Approved work

Steps 0 and 1 are approved and queued as backlog tasks **U27** and **U28**. Steps 2–4 are recorded
below and not approved.

### Step 0 — U27: rasterise in the UI

- `crates/ui/src/icons.rs` stops using `Image::tint`: it calls
  `egui_extras::image::load_svg_bytes_with_size(asset, SizeHint::Size { width: px, height: px,
  maintain_aspect_ratio: false }, &Default::default())`, uploads a `TextureHandle` cached by
  `(uri, px)` in `ctx.data_mut` temp storage, and paints it with `Painter::image` and a tint colour.
  No new dependency: `resvg` already arrives through `egui_extras`'s `svg` feature.
- Glyph colour: `visuals.widgets.inactive.fg_stroke.color` (the documented button text colour) for
  the resting state, `hovered`/`active` for the others, and a plain alpha multiply when disabled —
  never a second colour path.
- Two sizes: 20 pt for toolbar tool buttons, 16 pt for inline/menu use.
- The masters switch from `#000` to `#fff`; geometry is untouched. `assets/README.md` says the ink is
  white and the button paints it.
- Test that fails on today's code: draw one icon under `Visuals::light()` and `Visuals::dark()`,
  assert the expected colour is present in the uploaded pixels, assert the alpha silhouette is
  byte-identical between the two, assert the colours differ, and assert the raster size is
  `round(pt × pixels_per_point)` at 1.0, 1.25 and 2.0.

### Step 1 — U28: mass and one projection

- Allow `fill-opacity` (and `opacity`) alongside the white ink; `defs`, `mask`, `clipPath`,
  gradients, filters and CSS stay forbidden.
- Language: passive face = fill 12–18 % + full-strength outline; **result or changed geometry** =
  fill 45–60 % + the heavy outline (2.2–2.6); service glyphs stay bare stroke or solid silhouette.
  Emphasis is still structural and still readable in one ink, so no per-theme asset exists.
- Publish one axonometry in `STYLE_GUIDE.md` (vertical axis vertical, both receding axes at 30°, a
  face circle as a `0.577` ellipse) and redraw `cube`, `cylinder`, `panel`, `assembly`, `extrude` on
  it. A plate stays a plate.
- Redraw all 18 drawn icons; target footprint 3…21 units, coverage 20–45 %.
- One change updates `assets/README.md`, `assets/icons/STYLE_GUIDE.md` (§5, §7.3, §9),
  `.opencode/skills/draw-icon/SKILL.md` (§4), `crates/ui/src/icons.rs`, the 18 SVGs and the tests.
- Acceptance: `cargo xtask ci` green; every drawn icon rasterises non-empty at 16 and 20 px and
  stays inside `3..21`; coverage inside the band (asserted from the rasterised alpha in a test); the
  toolbar compared by eye in both themes (`AGENTS.md` §4.7).

## 5. Deferred, not approved

- **Step 2 — one semantic accent.** A single theme accent for the "result" fill/stroke and for
  toggled-on view buttons. Needs literal substitution before rasterisation or a small role palette
  like the neighbour's, plus a cache key `(icon, theme, px)`. Amends ADR 0055/0047 from "one ink" to
  "one ink plus one redundant accent". Open question in `plan/questions.md`.
- **Step 3 — tooling.** A Rust `cargo xtask icons` subcommand: lint the contract (24 grid, allowed
  elements, weights by role, footprint, no zero-byte file in a shipped theme) and `--sheet <dir>` to
  write a 16/20/48 px contact sheet in both themes through the same rasteriser
  (`tiny-skia::Pixmap::encode_png`). Plus a runtime audit test: every `Icon` the UI asks for exists
  and every drawn icon rasterises non-empty. Plus an alias table if a real duplicate appears; the
  README's "a shared drawing is two files" rule is the open question.
- **Step 4 — coverage.** 33 zero-byte files: the whole `actions` theme (30) plus
  `primitives/cone`, `bodies/revolve` and `bodies/sweep`. Tracked in `plan/debt.md` until drawn.

## 6. Deliberately not copied

- The neighbour's 64-unit grid and 2.55/5.6 weights: those are sized for a 26–30 px ribbon; our 24
  grid at 20 pt has the same physical density and re-gridding adds nothing.
- Its 11 colour roles and 6 accent ramps: we have no workspaces or modes that need separate ramps.
- Its own 1000-line SVG parser, `defs`/`mask`/`clipPath` support and `GAP` eraser: our guide forbids
  those elements and `egui_extras` already rasterises what we need.
- Its review server with acceptance hashes, colour-vision variants and per-icon `small/` variants
  (8 files of 391): process scaled for another team and a set five times our size.

## 7. Reproducing the numbers

The audit used `rsvg-convert` and a throwaway Pillow script; neither belongs in the repository
(`STYLE_GUIDE.md` §15: automation is a Rust `xtask` subcommand, never a second toolchain). Step 3
replaces them with `cargo xtask icons --sheet`. Until then, any conversion tool that reports the
alpha bounding box is enough to re-derive §2.
