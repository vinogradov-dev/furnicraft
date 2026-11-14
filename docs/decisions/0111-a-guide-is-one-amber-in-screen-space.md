# 0111. A guide is one amber, drawn in screen space

- Status: Accepted
- Date: 2026-11-14

## Context

The window so far has two kinds of colour: the chrome's ramp, which is mixed from one palette per
variant and preset (`theme.rs`, ADR 0063), and the scene's own inks, which are constants per variant
(`SceneColors`). Geometry the *person* authoring had no colour of its own: the selection is cyan
(ADR 0059), the accent is the action, the warning is a sentence in a panel, and the open
product's envelope picked its amber by hand.

U88's work cursor and U89's corner brackets are the first things FurniCraft draws to **help** rather
than to show furniture, and more will follow: a work plane, a snap mark, a measurement. Without a
rule each one invents a colour and a thickness, and a guide either disappears into the light theme or
reads as an action, a refusal or a selection. A colour literal in a feature module is exactly what
`cargo xtask ui-rules` refuses (AGENTS §6, U80), so the rule has to live in `theme.rs` before the
guides do.

## Decision

**One ink, warm amber, derived rather than chosen.** `theme::helper()` is the guide ink, and
`Palette::helper()` is the same value for a palette: the palette's warning amber lifted a quarter of
the way off the surface — towards white on a dark palette, towards black on a light one, the
direction `Palette::readable` already pushes an accent. Because it is derived from the two tokens that
are identical in every preset of a variant, the ink is the same in all three presets: it is *chrome
data* on the `ui` side and *frame data* on the scene side, and `scene::SceneColors::helper` carries the
same value so a guide the renderer draws is the same amber as one `egui` paints. A test in `ui` pins
the two together and pins the contrast.

**Amber, and never an action.** The ink has at least 3:1 against the surface, the panel and the
viewport background of both variants of all three presets, and it is never equal to the accent, the
filled accent, the danger ink or the scene's selection cyan. Warm amber is the one hue the chrome
ramp leaves free for "the program is helping you": the accent carries the action, red the refusal,
cyan the selection.

**Three weights, one rule, in points.** `HELPER_CROSSHAIR_STROKE` is 1 pt: a line across the whole
viewport is long enough to read at the quietest weight. `HELPER_STROKE` is 1.5 pt: a bracket at rest,
a cursor outline. `HELPER_OPEN_STROKE` is 2 pt: the bracket of the container that is open, because
opening a level is what decides the next click. Widths are points, never a world size.

**A guide is screen space and never document data.** It is computed from view state — the open
container's bounds, the pointer, the camera — and written to neither the document nor the scene file.
The same rule keeps the ink out of the saved file: `SceneColors` is display state, like the grid.

## Consequences

A new guide takes `theme::helper()` and one of the three widths and invents nothing; a change of ink
or weight is one edit that every guide inherits. The cost is a token that is not a designer's pick
but a formula: the amber cannot be tuned per preset, and that is the point — the scene carries one
constant, so a guide cannot be preset-aware without a second bridge from chrome to frame. If a guide
ever needs a second ink (a locked level, a refused snap), that role is recorded here as another
derivation, not as a literal where it is drawn.

## References

- `crates/ui/src/theme.rs` — the ink, the three widths and the contrast test.
- `crates/scene/src/lib.rs` — `SceneColors::helper`, the frame's copy of the same ink.
- ADR [0063](0063-one-theme-owns-the-window.md) — the theme owns the window's colour and metrics.
- ADR [0089](0089-three-palettes-are-data-and-derive-their-states.md) — a palette is data and derives
  its states, which is what lets this ink be derived too.
- ADR [0059](0059-a-selected-line-is-a-screen-space-overlay.md) — the selection cyan a guide must not
  borrow.
- Plan: U83 (the ink and the widths), U88 (the work cursor), U89 (the open level's brackets).
