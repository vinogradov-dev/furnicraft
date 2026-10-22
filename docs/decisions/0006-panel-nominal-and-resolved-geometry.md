# 0006. Panels store nominal size; geometry is resolved

- Status: Accepted
- Date: 2026-10-23

## Context

The user enters nominal length and width. Thickness comes from the panel material, and included edge
banding takes its thickness out of the board, so the core is smaller than the nominal size. Storing
one `PanelSize` with length, width and thickness would make the document depend on the catalogue and
would turn derived values into editable state. Machining, however, is authored on the board and must
stay in a stable frame.

## Decision

`Panel` stores `PanelNominalSize { length, width }`, an optional `MaterialId`, `last_known_thickness`,
`placement`, machining and ordered edge banding. It does not store core, overall or effective
thickness. `PanelGeometry` is derived from the panel, the catalogue and the banding:

- `core_length = nominal_length - included(left) - included(right)`;
- `core_width = nominal_width - included(bottom) - included(top)`;
- `overall_length = core_length + thickness(left) + thickness(right)`;
- `overall_width = core_width + thickness(bottom) + thickness(top)`;
- `thickness` is the catalogue material's nominal thickness, or `last_known_thickness` when the
  material is missing.

The panel-local frame stays the one from ADR 0004: origin at the minimum corner of the core, `+X`
length, `+Y` width, `+Z` thickness. `Placement` anchors the core zero; banding on the bottom and left
extends into negative coordinates. Assembly code accounts for that offset when it needs an overall
face. This keeps machining coordinates and the placement frame identical in v1.

The old `PanelSize` is not kept as a second source of truth: nominal size is stored, resolved geometry
is derived. A legacy panel without a material loads with its old thickness as `last_known_thickness`,
its old length/width as nominal size, `material = None`, and a `NoMaterial` diagnostic.

## Consequences

- Resizing changes nominal size; a material change can change core and overall size without rewriting
  the nominal size.
- `param` receives resolved geometry and never reads the catalogue; `engine` owns material resolution.
- Machining stays in core coordinates, so a resize does not move it.
- A panel placed by its core corner does not place its nominal or overall box at the same point:
  banding on the bottom and left shifts the outer box. M6 must derive that offset explicitly; changing
  the anchor later needs a new decision.
- The schema changes; legacy files need a migration that preserves old dimensions.
