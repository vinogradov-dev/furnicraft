# 0087. The viewport's own colours follow the theme variant; material colours never do

- Status: Accepted
- Date: 2026-11-11

## Context

U38 gave the window two palettes (ADR 0063, 0069), but the viewport is not an `egui` surface: it is a
`wgpu` target, and its frame carried literal colours. On the light workspace the scene therefore kept
clearing to the dark chrome and the floor grid kept its light-on-dark lines, so the rendered frame
read as a dark hole in a light window, and nothing connected the palette to the frame when
`theme::install_with` changed the variant (X8).

A frame has two kinds of colour. A `Drawable` carries a **material** colour — the furniture's own;
that is data the user chose and it must survive any theme. Everything else is **ink**: the clear
colour, the grid lines, the edge pass and an unlit line chain — the viewport's presentation of space,
not a property of the piece.

## Decision

- `furnicraft_scene` owns the pairs: `SceneColors::DARK` and `SceneColors::LIGHT`, each holding the
  clear colour, a dark or light `Grid` (`Grid::DARK`, `Grid::LIGHT`) and the line ink. The values
  live beside the frame they paint, so the renderer needs no theme concept and `scene` keeps its
  layer: it does not know `egui`.
- `furnicraft_ui::theme::scene_colors(theme)` is the one bridge from the active `egui::Theme` to
  those values. The shell resolves it once per frame into `Shell::colors`, so a preference switch —
  including *System* following the OS while the window is open — reaches the viewport on the next
  frame without a document edit.
- `SceneFrame::apply_colors` writes the ink and never touches a `Drawable`. It also leaves the grid's
  switch, spacing, extent and major count alone: a variant is a colour, not a display setting.
- A `Polyline` may leave its colour to the frame (`Polyline::in_ink`, `color = None`); the renderer
  resolves it to `SceneFrame::line_ink` at upload. A drawn path is an annotation, so a theme switch
  repaints it without a rebuild and a chain that names its own colour (a selection highlight) keeps
  it.
- The edge pass keeps deriving its ink from the frame background's luminance, so a light background
  draws dark edges with no second switch.

## Consequences

A theme switch is display state: it repaints the uploaded frame and moves no revision, so no node is
re-evaluated and the undo history stays clean. The dark variant is byte-identical to what the editor
drew before.

The cost is one optional colour on `Polyline` and one constructor beside `Polyline::new`; a line that
wants a fixed colour still passes one. Screen-space overlays painted with `egui` (contour grips, the
drawing draft, size labels) are not part of a frame and keep their own colours —
`plan/debt.md` holds that leftover under `[gizmo]`.

Revisit this if a document ever stores its own line colour per path: then the theme ink becomes a
default only, and `Polyline::color` stops being an `Option` but a resolved value chosen at evaluation
time.
