# Panels and edge banding

Status: domain specification. This document describes the domain model, not a particular screen. It fixes the terms, invariants, size rules, edge-banding behaviour and panel commands. Items marked "later" are outside the current scope.

Related documents:

- [Materials and catalogue](materials.md) — material records and the catalogue.
- [Scene document](document.md) — scene file, loading, storage and diagnostics.

## 1. Terms

- **Panel** — an atomic document object. It is always a group: the panel body plus edge banding. The group exists even when no banding is applied.
- **Panel body** (core, board) — the board after trimming the edges where banding is included in the size.
- **Edge banding** — a material profile applied to one edge of a panel. It is not a separate object in the document tree.
- **Edge** — a side face of a panel. A rectangular panel has four edges with stable `EdgeId` values 1–4.
- **Nominal size** — the length and width entered in the panel card. It is not always the finished overall size.
- **Core** — the actual length and width of the board after trimming for included edge banding.
- **Overall size** — the finished outer length and width including all edge banding.
- **Problem panel** — a diagnostic state, not document data. It appears, for example, when a catalogue material is missing; see [Scene document](document.md).

Panel material and edge-banding material are described in [Materials and catalogue](materials.md).

## 2. Panel as a group

1. In the document, `Panel` is a single entity. There are no separate "panel body" and "edge banding" entities; banding belongs to the panel and is addressed by `(panel_id, edge_id)`.
2. In the scene, a panel is a group of drawables: the body and the bandings. Every part of the group has the same `panel.id()`.
3. The group cannot be broken into body and banding. No command deletes, hides or transforms only the body or only a banding.
4. Selection, highlighting, deletion, hiding and movement apply to the whole panel. Picking returns `panel.id()`, never a mesh index or an individual banding.
5. The outliner may later show child rows "body" and "banding 1…4", but they are not separate editable objects.
6. Bandings are stored in application order. The order belongs to the panel.

## 3. Local coordinate system

- The world is Z-up. A panel has a local frame rigidly attached to its body.
- The origin is the minimum corner of the **core**.
- `+X` is length, `+Y` is width and `+Z` is thickness.
- The body occupies `x` from `0` to `core_length`, `y` from `0` to `core_width`, and `z` from `0` to the panel thickness.
- The large faces are `A` at `z = thickness` and `B` at `z = 0`; `+Z` points from `B` to `A`.
- Rectangle vertices are numbered 1–4 counter-clockwise when viewed from face `A`, starting at zero: 1 is `(0, 0)`, 2 is `(core_length, 0)`, 3 is `(core_length, core_width)`, 4 is `(0, core_width)`.
- Edge `i` runs from vertex `i` to vertex `i + 1`, wrapping at 4. For a rectangle: 1 is the bottom (`-Y`), 2 the right (`+X`), 3 the top (`+Y`), 4 the left (`-X`).
- Banding on the bottom and left sides may have negative coordinates. This is normal: the panel group is wider than the core, and bounds are computed from every part of the group.
- Machining is stored in core coordinates. The machine works on the board, so this is the primary system for holes and cutouts.
- Nominal size and overall size are derived values for the card, reports and UI. They do not have to coincide with `0..core`.

## 4. Panel dimensions

### 4.1. Panel card

When creating or editing a panel, the user sets:

- length (nominal);
- width (nominal);
- panel material.

Thickness is not editable: it comes from the panel material (see [Materials and catalogue](materials.md)). The card shows it as a read-only property. If the material is not found, the card shows `<no material id %d>` (or a similar marker) and the last known thickness with a warning; the loading behaviour is described in [Scene document](document.md).

### 4.2. Formulas

Each axis is handled separately. Let `t(e)` be the thickness of the banding on edge `e`, if present; otherwise 0. A banding has an `included_in_size` flag.

- `core_length = nominal_length - included(left) - included(right)`
- `core_width = nominal_width - included(bottom) - included(top)`
- `overall_length = core_length + t(left) + t(right)`
- `overall_width = core_width + t(bottom) + t(top)`

Equivalently:

- `overall_length = nominal_length + non_included(left) + non_included(right)`
- `overall_width = nominal_width + non_included(bottom) + non_included(top)`

Meaning:

- banding **included** in the size takes its thickness from the core; the overall size stays within the nominal size;
- banding **not included** in the size is added outside; the overall size becomes larger than the nominal size;
- panel thickness does not change: the banding is trimmed to the board thickness.

Constraints:

- `core_length > 0`, `core_width > 0`;
- banding thickness and width are positive;
- panel and banding dimensions do not exceed the supported `Length` limits.

### 4.3. Examples

Panel 720 × 500, banding 2 mm:

| Banding | Core | Overall |
|---|---|---|
| all four included in size | 716 × 496 | 720 × 500 |
| all four not included | 720 × 500 | 724 × 504 |
| left included, right not included (banding on X only) | 718 × 500 | 722 × 500 |
| left and right not included (banding on X only) | 720 × 500 | 724 × 500 |

If banding is present on only one side, the formulas still work: the other side contributes zero.

### 4.4. Banding position relative to the core

Banding is always outside the core:

- on the bottom or left side, in negative coordinates, for example `y` from `-t` to `0`;
- on the top or right side, beyond the core, for example `y` from `core_width` to `core_width + t`.

The `included_in_size` flag does not change the position of the banding relative to the core. It only changes the core size: included banding makes the core smaller, non-included banding does not. Therefore:

- included left banding: the core starts `t` to the right of the nominal left boundary;
- non-included left banding: the core starts at the nominal left boundary;
- in both cases the physical banding lies to the left of the core.

## 5. Edge banding

### 5.1. Profile

Edge banding is a profile extruded along the edge path. The simplest profile is a rectangle:

- `thickness` — banding thickness in the panel plane, outward from the edge. It is the amount by which the core is trimmed when the banding is included in the size;
- `width` — banding width across the panel thickness;
- profile length — the edge length (later, the path length including radii).

### 5.2. Width and panel thickness

- If `width >= panel_thickness`, the banding is trimmed flush with the panel thickness. The full thickness is visible in 3D.
- If `width < panel_thickness`, the banding does not cover the whole edge. By default it is centred across the thickness; the visible coverage is shown in 3D.
- Banding width does not change panel thickness and does not extend beyond faces `A`/`B` after trimming.

### 5.3. Application order

- Bandings are stored in application order.
- Re-applying a banding to an edge makes it the last one.
- Removing a banding preserves the order of the others.
- Order matters at corners: the last applied banding is trimmed.

### 5.4. Corners

- **Right angle, 90°.** The last applied banding is trimmed so that it butts against the previously applied banding; its end face is visible. There is no miter in the first version.
- **Chamfer.** The last applied banding is trimmed along the chamfer; its end face is visible.
- **Radius.** Banding is continuous around the radius and the adjacent straight sections. Application order does not affect a radius. This is a later step: it requires a profile sweep along an arc; the port currently supports linear extrusion and booleans.
- **Single banded edge at a corner.** If the adjacent edge has no banding, the banding ends at the corner and is trimmed flush with the adjacent face. The exact end-face behaviour is to be clarified.
- **Edge banding milling.** Rounding the banding faces and corner is a later operation and is outside the current scope.

### 5.5. Inclusion in size

- `included_in_size = true`: the core is reduced by `thickness` on that edge; the banding fills the freed space; the overall size does not grow.
- `included_in_size = false`: the core is unchanged; the banding is added outside; the overall size grows by `thickness`.
- The flag belongs to the panel and the specific edge, not to the banding material.

## 6. Display and diagnostics

### 6.1. Colours

- Colours are not stored in the scene document.
- The base colour of a panel comes from the panel material. To distinguish panels that use the same material, a deterministic tint is applied from `panel.id()`, so the colour does not change when the scene is rebuilt.
- All bandings on one panel get one common colour, different from the body colour. For now this is a debug colour, not the colour of a specific banding material.
- If the panel material is missing, the panel is drawn in a neutral colour.
- Later, colour and texture will come from the material; textures are a separate rendering task.

### 6.2. Panel diagnostics

Panel issues are computed during loading/rebuild and are not stored as data:

- the panel material is missing;
- an edge-banding material is missing;
- the core is not positive;
- machining no longer fits the new core;
- the banding profile is unavailable and there is no fallback.

A panel with an issue stays in the document, is drawn where possible and shows a warning in the card. A whole-file load error is reserved for scenes that cannot be read or recovered; see [Scene document](document.md).

## 7. Panel commands

All changes are `engine` commands with stable ids and typed parameters. The UI never writes to the document directly.

- `panel.create` — parent, name, nominal length, nominal width, material, placement. Thickness is not passed.
- `panel.resize` — panel, nominal length, nominal width.
- `panel.material.set` — panel, material.
- `panel.edge_banding.set` — panel, edge, material, included_in_size. If the edge already has banding, it is replaced and becomes the last one.
- `panel.edge_banding.clear` — panel, edge.

Commands validate the material against the catalogue, the positivity of the core and the validity of machining. A successful command is one undo entry; a failed command leaves the document unchanged.

## 8. Panel tests

At minimum:

- the 720 × 500 size table for every inclusion combination;
- banding order and trimming of the last banding at a right angle;
- banding wider than the panel thickness is trimmed; banding narrower than the thickness is drawn partially;
- the panel group returns `panel.id()` for the body and every banding;
- panel commands and undo/redo;
- machining that no longer fits the core marks the panel as a problem panel without losing data.

Catalogue and scene-loading tests live in [Materials and catalogue](materials.md) and [Scene document](document.md).

## 9. Open questions

1. Exact banding trimming at a corner when the adjacent edge has no banding.
2. Machining policy when the material or banding changes: warning only, or command rejection.
3. Contour radii and chamfers: when they enter the model and when sweep is implemented.
4. Edge banding milling: rounding the faces and corner.
5. Whether the card should show core and overall size separately from the nominal size.
