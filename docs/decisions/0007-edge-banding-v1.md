# 0007. Edge banding v1: ordered bands and butt joints

- Status: Accepted; per-path banding unit superseded by 0014
- Date: 2026-10-23

## Context

A panel is one document object, but its drawing is a group: the board and up to four bands. Banding
changes core and overall size, has its own material and profile, and the corner where two bands meet
must be unambiguous. Radii, chamfers and milling require profile sweeps or booleans that the first
version does not need.

## Decision

`Panel` stores a `Vec<EdgeBanding>` in application order, with at most one entry per `EdgeId`.
`EdgeBanding` stores the edge, a required banding `MaterialId`, `included_in_size`, and the last known
thickness and width. `panel.edge_banding.set` replaces an existing entry and moves it to the end;
`panel.edge_banding.clear` removes it and preserves the order of the rest.

Banding is a rectangular profile extruded along the edge. Its thickness is outward from the core; its
width is across the panel thickness. A width greater than or equal to the panel thickness is trimmed
flush; a smaller width is centred across the thickness. Banding is always outside the core: bottom
and left bands occupy negative coordinates.

For right-angle corners the later band covers the corner and is trimmed flush with the earlier band's
outer face. Concretely, when a band is applied, at each end that meets an already applied band it is
extended by that band's thickness along the edge; the end face is visible and flush. If the adjacent
edge has no earlier band, the band ends flush with the adjacent side face of the core. This rule makes
application order observable and avoids a mitre. A band that is replaced moves to the end, so it
becomes the one that covers both of its corners.

The geometry is built as separate solids/meshes for the body and every band; they are not unioned into
one body. All drawables of the group carry the same `panel.id()`.

Contour radii and chamfers, profile sweeps, overhang/trim allowance and edge-banding milling are not
part of v1.

## Consequences

- Volume and tessellation tests can check the body and each band independently.
- Banding order is data, not a renderer detail; saving and loading preserve it.
- A panel with no banding is still a group with an empty band list.
- The first implementation only supports straight rectangular edges and 90° corners.
- The corner rule is deterministic but differs from a mitre; the UI must show the visible end face.
