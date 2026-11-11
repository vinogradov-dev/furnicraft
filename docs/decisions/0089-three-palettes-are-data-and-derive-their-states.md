# 0089. Three palettes are data: one ramp per variant, derived states, one accent override

- Status: Accepted
- Date: 2026-11-12

## Context

U38 gave the window a dark and a light `Palette` as two consts (ADR 0063, 0069), and Settings let the
user replace the accent. The owner approved three looks — P1 graphite·copper, P2 ink·azure, P4
slate·mint — each in both variants and chosen in Settings. Written out literally that is six palettes
of sixteen tokens, where hover, press, the hairline and the disabled ink are almost the same step away
from the base tokens in every one of them: a hundred values, six places to drift, and a token that
would cost six edits.

## Decision

- `theme::Ramp` holds the eleven tokens a designer actually chooses (chrome, surface, panel, control,
  floating, text, text_dim, accent, accent_fill, danger, warn). `ThemePreset::{dark, light}` are six
  `Ramp` consts, and `Ramp::palette(dark)` mixes the rest: hover and line one step towards the text,
  press sunk towards the chrome in the dark variant and towards the text in the light one, disabled
  folded into the panel. `mix` is integer arithmetic, so a palette never depends on the rounding of a
  float at start-up and the whole table stays a constant.
- The preset is settings data (`appearance.palette`, stored as a snake_case name). A name this build
  does not know is repaired to P1 and reported once in the status bar, not rejected: the rest of the
  file — the recent list, the viewport defaults, the scale — still describes the workspace the user
  left, and §4.5 asks for a report rather than a silent reinterpretation. A value of the wrong type is
  still a corrupt file.
- A file written before the presets has no `palette` key, and its accent was the only palette's, not an
  override: it opens on P1's own accent. `appearance.accent` is `Option<[u8; 3]>` from here on —
  `None` draws each variant's own accent, `Some` is the user's one colour for both.
- `Palette::with_accent` pushes an override away from the surface until it clears `ACCENT_CONTRAST`
  (3:1): a bright pick is kept exactly where it reads and is folded dark on paper, and the filled
  accent is derived from the result so text on it still reads.
- The colour tests measure WCAG contrast over every preset × variant instead of a brightness delta, so
  a new ramp cannot land below 4.5:1 text or 3:1 accent.

## Consequences

A preset costs eleven constants per variant; a new token costs one field on `Ramp` and one value per
ramp. The measured tables in `plan/notes/U71.md` remain the source for the values; the states are
reproduced within a few units of them.

The viewport interior keeps `SceneColors::DARK`/`LIGHT` (ADR 0087) and so does not take a preset's
tint: the presets paint the chrome around it, and the frame's own colours are listed in
`plan/debt.md`. The `ribbon` and `band` rows of the note are not on `Palette` yet — the ribbon draws
`surface` until U68 gives it its own band. Panel opacity became one `appearance.panel_opacity`
percentage (55–100, 92 by default) that the floating ribbon draws with; the side panels and the
clusters take that value with M2, when they float over the scene instead of docking beside it.
