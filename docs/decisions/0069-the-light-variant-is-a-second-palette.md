# 0069. The light variant is a second palette

- Status: Accepted
- Date: 2026-11-07

## Context
ADR 0063 gave the editor one dark palette and applied it to both of `egui`'s variants, so the
operating system's light preference could not leave half a window on stock colours. Its consequence
already named the limit: "The light variant is the same palette, not a second design; a real light
theme is a later decision with its own file."

U41 puts a theme choice (dark, light, system) in the Settings window. Painting a dark palette when
the user picks light is no longer honest.

## Decision
`crates/ui/src/theme.rs` holds two `Palette` values, `DARK` and `LIGHT`, with the same roles and the
same spacing, rounding and type scale. Every chrome colour is a function of the active
`egui::Visuals` (`theme::surface(ui)`, `panel`, `text`, `text_dim`, `control`, `accent`, …), so a
widget call site draws correctly in both variants and a test need not share one global palette.

`theme::install_with(ctx, preference, accent)` installs both variants, folds the chosen accent into
each, and sets the `egui::ThemePreference`; the operating system only decides which installed variant
is active. The accent is data: a picked colour is folded towards the dark end for filled backgrounds
so white text still reads.

## Consequences
A second palette must pass the same contrast floor as the first, and a new colour role is added to
both. Any widget that still reaches for a palette constant instead of the active style draws the
dark value in light mode; the call sites were converted in the same change, and a new one is a review
item. The light variant is a visual design decision, not a document or session field: it lives in the
workspace file (ADR 0068) and never in the `.fcs`.