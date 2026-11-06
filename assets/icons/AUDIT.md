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
- `icon_button` / `tool_button` rasterise the master with `egui_extras::image::load_svg_bytes_with_size`
  at `pt × pixels_per_point`, tint the pixels, upload a `TextureHandle` cached per `(uri, px, colour)`
  in the context's temporary data, and paint it with `Painter::image`. `ui_scale.rs` therefore
  re-rasterises on a zoom change instead of stretching.
- Sizes are points: 20 pt for a toolbar tool, 16 pt for inline and menu use.
- A zero-byte file is a documented placeholder: the button draws its text label instead.

The pipeline this replaced was measured, because both of its assumptions were wrong:

1. `Image::tint` becomes the vertex colour of the mesh, and the egui shader computes
   `out = in.color * texture` (`egui-wgpu/src/egui.wgsl`). The masters were drawn `stroke="#000"`,
   so the product was always black: **the theme colour never reached a glyph.** On the dark theme
   that was `gray(140)` text on a `gray(50)` inactive button, about 1.4–1.5 : 1 — effectively
   invisible; on the light theme it was a harsh `0` against `gray(60)` text.
2. The only test of that path inspected the `RectShape.fill`, i.e. the colour handed to the painter,
   not the pixels that came out. That is why a green suite kept the defect.

Colour is not a scaling problem: the raster is produced at the drawn pixel size, so the master is
crisp at 75 % and 200 % alike.

## 2. What the set looks like, measured

Rendered with `rsvg-convert` at 64 px and reduced to the 24-unit grid; alpha above 16 counts as ink.
Coverage is alpha-weighted ink over the fixed 18 × 18 footprint (`STYLE_GUIDE.md` §7.4), so a passive
face counts as its tone rather than as a solid block. Drawn files only (18 of 51; the other 33 are
zero-byte placeholders). Measured after U28, 2026-11-06.

| icon | ink bbox (grid units) | size | coverage |
|---|---|---|---|
| bodies/extrude | 4.88..19.12 × 4.88..19.50 | 14.2 × 14.6 | 37.1 % |
| contour/arc | 3.00..21.00 × 4.50..20.62 | 18.0 × 16.1 | 22.8 % |
| contour/cancel | 4.12..19.88 × 4.12..19.88 | 15.8 × 15.8 | 31.1 % |
| contour/circle | 3.38..20.62 × 3.38..20.62 | 17.2 × 17.2 | 38.7 % |
| contour/close | 3.00..19.12 × 6.00..20.25 | 16.1 × 14.2 | 28.8 % |
| contour/line | 3.00..21.00 × 3.00..21.00 | 18.0 × 18.0 | 21.9 % |
| contour/polyline | 3.00..21.00 × 3.38..20.62 | 18.0 × 17.2 | 31.5 % |
| contour/rectangle | 3.38..20.62 × 4.50..19.50 | 17.2 × 15.0 | 35.0 % |
| primitives/cube | 4.50..19.50 × 3.75..20.25 | 15.0 × 16.5 | 36.6 % |
| primitives/cylinder | 5.25..18.75 × 4.12..20.25 | 13.5 × 16.1 | 30.2 % |
| primitives/sphere | 4.88..19.12 × 4.88..19.12 | 14.2 × 14.2 | 33.2 % |
| scene/assembly | 3.00..21.00 × 4.88..19.12 | 18.0 × 14.2 | 34.1 % |
| scene/camera | 3.38..19.88 × 6.75..18.00 | 16.5 × 11.2 | 33.8 % |
| scene/light | 3.00..21.00 × 3.00..21.00 | 18.0 × 18.0 | 33.1 % |
| scene/panel | 3.75..20.25 × 5.62..19.12 | 16.5 × 13.5 | 32.0 % |
| view/frame-all | 3.00..21.00 × 3.00..21.00 | 18.0 × 18.0 | 27.9 % |
| view/orthographic | 4.88..19.12 × 4.88..19.12 | 14.2 × 14.2 | 37.9 % |
| view/perspective | 3.38..20.62 × 3.38..20.62 | 17.2 × 17.2 | 39.0 % |

Optical size now sits at 13.5–18 units and coverage at 21.9–39.0 %, so the set reads as one family;
the stroke weights stay inside 1.5–2.6 units. The five volumetric icons share the one basis of
`STYLE_GUIDE.md` §9.1, and the UI test in `crates/ui/src/icons.rs` asserts the footprint and the
coverage band from the rasterised alpha.

The mass is what changed. Before U28 the same set was pure outline: 8.4–36.1 % coverage, with the
modelled objects drawn as wireframe while the neighbour set drew them as filled volumes with a grey
passive face. Detail did not make the difference at 16 px; tone did.

## 3. Three defects

- **D1 — the theme never reaches the icon.** See §1. **Fixed by U27**: the UI rasterises the master,
  bakes the theme colour into the uploaded pixels and guards the path with a pixel test. Owners:
  `crates/ui/src/icons.rs`.
- **D2 — no mass, only wireframe.** No file had a fill. **Fixed by U28**: one ink, tonal fills by
  alpha — a passive face at 12–18 %, the result at 45–60 % — so a theme change still needs no asset
  change. Owners: `assets/README.md`, `STYLE_GUIDE.md` §5.4/§7.3, all 18 drawn SVGs.
- **D3 — no canonical projection or footprint.** `primitives/cube`, `scene/panel`, `scene/assembly`
  and `bodies/extrude` each invented their own axonometry. **Fixed by U28**: one published basis
  (`STYLE_GUIDE.md` §9.1) and one 3..21 footprint with a 20–45 % coverage band. Owners:
  `STYLE_GUIDE.md` §9, the five volumetric SVGs, `crates/ui/src/icons.rs`.

## 4. Approved work

Step 0 shipped as **U27** and step 1 as **U28**. Steps 2–4 are recorded below and not approved.

### Step 0 — U27: rasterise in the UI (done)

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

### Step 1 — U28: mass and one projection (done)

- `fill-opacity` is allowed alongside the white ink; `opacity`, `defs`, `mask`, `clipPath`,
  gradients, filters and CSS stay forbidden.
- Language: passive face = fill 12–18 % + full-strength outline; **result or changed geometry** =
  fill 45–60 % + the heavy outline (2.2–2.6); service glyphs stay bare stroke or solid silhouette.
  Emphasis is still structural and still readable in one ink, so no per-theme asset exists.
- One axonometry is published in `STYLE_GUIDE.md` §9.1 (`x = 0.866 (u − v)`, `y = 0.5 (u + v) − w`,
  a face circle as a `0.577` ellipse); `cube`, `cylinder`, `panel`, `assembly` and `extrude` are
  drawn on it, and a plate stays a plate.
- All 18 drawn icons are redrawn inside the fixed 3..21 footprint at 20–45 % coverage, measured as
  alpha-weighted ink.
- The same change updated `assets/README.md`, `assets/icons/STYLE_GUIDE.md` (§5, §7.1, §7.3, §7.4,
  §9, §15), `.opencode/skills/draw-icon/SKILL.md` (§4, §6), `crates/ui/src/icons.rs` and the 18 SVGs.
- Acceptance: `cargo xtask ci` green; every drawn icon rasterises non-empty at 16 and 20 px and stays
  inside `3..21`; coverage inside the band, asserted from the rasterised alpha; the toolbar compared
  by eye in both themes (`AGENTS.md` §4.7).

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
