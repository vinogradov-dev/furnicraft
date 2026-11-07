# 0063. One theme owns the window's colour and rhythm

- Status: Accepted
- Date: 2026-11-06

## Context
Until U38 the editor used whatever `egui` ships. Every panel took the stock dark fill, every
separator was the stock widget stroke, every heading the stock 18 pt font, and the window had no
palette of its own: the viewport's background, the panel fill, the control fill and the text colour
were four unrelated decisions taken by a third-party default. The effect read as a prototype with a
3D view in it rather than as one application, and each new surface — a header, a ribbon, a status
bar — would have invented its own numbers.

`assets/icons/STYLE_GUIDE.md` §1 and §5.2 already state the rule the interface should follow:
technical, calm, dense, one accent used only for an action, a selection or a result, and colour
belonging to the interface theme rather than to an asset. Nothing in the code implemented it.

## Decision
One module, `crates/ui/src/theme.rs`, owns the window's presentation:

- a named palette (`CHROME`, `SURFACE`, `PANEL`, `CONTROL`, `CONTROL_HOVER`, `CONTROL_ACTIVE`, `TEXT`,
  `TEXT_DIM`, `TEXT_DISABLED`, `LINE`, `ACCENT`, `ACCENT_FILL`, `DANGER`, `WARN`) with no colour
  literal anywhere else in a widget;
- one spacing step of 4 pt and three row heights (`HEADER_HEIGHT`, `RIBBON_ROW_HEIGHT`,
  `STATUS_HEIGHT`) that are multiples of it;
- one rounding of 4 px, one type scale (13 pt headings, 12 pt body and buttons, 10.5 pt small), and
  one `Visuals`/`Widgets` map so a button, an icon button and a collapsing header share hover, press
  and disabled states;
- `install(ctx)` applies the same `Style` to both of `egui`'s variants, so the operating system's
  light preference cannot leave half a window in the stock palette.

`theme::divider` draws the short, quiet hairline between meaning groups; `ui.separator()` stays what
it is. A surface draws a frame from a named fill, never from a literal.

The theme stores nothing in the document and holds no widget state: it is presentation, rebuilt from
constants, and a colour change touches no revision, no command and no undo entry.

## Consequences
Every surface added after U38 — ribbon, status bar, settings, split views — reads the same names, so
a look change is one file. A test asserts the contrast floor of text on its surfaces, that chrome
stays neutral while only the accent is saturated, and that both `egui` variants receive the palette.

The palette is deliberately small: a new role needs a name, a contrast check and a reason, which
keeps a fourth grey from appearing because one panel looked slightly off. The light variant is the
same palette, not a second design; a real light theme is a later decision with its own file.
