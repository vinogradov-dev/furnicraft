# Panels and edge banding

Status: domain specification. This document describes the domain model, not a particular screen. It fixes the terms, invariants, size rules, edge-banding behaviour and panel commands. Items marked "later" are outside the current scope.

Related documents:

- [Materials and catalogue](materials.md) — material records and the catalogue.
- [Scene document](document.md) — scene file, loading, storage and diagnostics.

Related decisions: [0006](../decisions/0006-panel-nominal-and-resolved-geometry.md),
[0007](../decisions/0007-edge-banding-v1.md),
[0008](../decisions/0008-panel-diagnostics.md) and
[0009](../decisions/0009-contour-first-panel-model.md).

## 1. Terms

- **Panel** — an atomic document object. It is always a group: the panel body plus edge banding. The group exists even when no banding is applied.
- **Panel body** (core, board) — the board after trimming the edges where banding is included in the size.
- **Edge banding** — a material profile applied to one edge of a panel. It is not a separate object in the document tree.
- **Edge** — a side face of a panel. A rectangular panel has four edges with stable `EdgeId` values 1–4.
- **Contour feature** — a corner radius or chamfer stored against a stable `VertexId`, or a signed bow stored against a stable `EdgeId`.
- **Bow** — a circular arc replacing a straight edge. A positive sagitta bows outward from the nominal rectangle; a negative sagitta bows inward.
- **Shaped panel** — a panel with at least one contour feature. Its nominal outline is the rectangle with those features applied.
- **Banding path** — a maximal tangent-continuous chain of nominal contour segments between sharp joints. One path is one banding unit.
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

- The world is Z-up. A panel has a local frame rigidly attached to its nominal outline.
- The origin is the minimum corner of the **nominal** rectangle.
- `Placement` anchors this nominal zero. Included bottom and left banding move the core inward, not
  the whole panel outward; non-included banding may extend outside the nominal rectangle, and bounds
  are computed from every part of the group.
- `+X` is length, `+Y` is width and `+Z` is thickness.
- The core occupies `x` from `core_x` to `core_x + core_length`, `y` from `core_y` to
  `core_y + core_width`, and `z` from `0` to the panel thickness. `core_x` and `core_y` are the
  included left and bottom banding thicknesses; they are zero when those edges are unbanded or not
  included.
- The large faces are `A` at `z = thickness` and `B` at `z = 0`; `+Z` points from `B` to `A`.
- Rectangle vertices are numbered 1–4 counter-clockwise when viewed from face `A`, starting at the
  nominal zero: 1 is `(0, 0)`, 2 is `(nominal_length, 0)`, 3 is `(nominal_length, nominal_width)`,
  4 is `(0, nominal_width)`. The core is inset from these edges when banding is included.
- Edge `i` runs from vertex `i` to vertex `i + 1`, wrapping at 4. For a rectangle: 1 is the bottom (`-Y`), 2 the right (`+X`), 3 the top (`+Y`), 4 the left (`-X`).
- A shaped panel keeps the same vertex and edge ids: a radius or chamfer belongs to its nominal corner vertex, a bow belongs to its nominal edge. The contour is counter-clockwise from face `A` and starts on edge 1 after vertex 1.
- The nominal outline stays anchored at the nominal zero corner. An outward bow may extend outside that rectangle; the panel bounds are the bounds of the finished contour, not always `0..nominal`.
- In core-local coordinates, banding on the bottom and left sides has negative coordinates. In the
  nominal frame an included band lies inside the nominal edge, for example `y` from `0` to `t` on the
  bottom; a non-included band lies outside it, for example `y` from `-t` to `0`.
- Machining is stored in nominal panel-local coordinates: the panel's first corner is the zero, and
  an included band or a trimmed end changes the core without moving the local frame. The machine
  works on the board, but its coordinates are derived from this fixed frame at the CAM boundary.
- Nominal size and overall size are derived values for the card, reports and UI. They do not have to coincide with `0..core`.

## 4. Panel dimensions

### 4.1. Panel card

When creating or editing a panel, the user sets:

- length (nominal);
- width (nominal);
- panel material.

Thickness is not editable: it comes from the panel material (see [Materials and catalogue](materials.md)). The card shows it as a read-only property. If the material is not found, the card shows `<no material id %d>` (or a similar marker) and the last known thickness with a warning; the loading behaviour is described in [Scene document](document.md).

The document stores nominal length and width only; core and overall are derived. A panel may have no material (`None`); that is different from a material id that is absent from the catalogue.

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

### 4.4. Banding position relative to the core and the nominal frame

Banding is always outside the core. In core-local coordinates:

- on the bottom or left side, in negative coordinates, for example `y` from `-t` to `0`;
- on the top or right side, beyond the core, for example `y` from `core_width` to `core_width + t`.

The `included_in_size` flag does not change the position of the banding relative to the core. It only changes the core origin and size:

- included left banding: `core_x = t`, so the core starts `t` to the right of the nominal left boundary and the band occupies `x` from `0` to `t` in the nominal frame;
- non-included left banding: `core_x = 0`, so the core starts at the nominal left boundary and the band occupies `x` from `-t` to `0`;
- in both cases the physical banding lies to the left of the core.

The same rule applies on the bottom: included gives `core_y = t` and the band occupies `y` from `0` to `t`; non-included gives `core_y = 0` and the band occupies `y` from `-t` to `0`. Top and right banding do not move the core origin; included banding occupies the last `t` of the nominal side, non-included banding extends beyond it.

### 4.5. Shaped panels (stage 2b)

A shaped panel resolves its nominal contour `N` from straight edges, bows, corner radii and chamfers.
The core `C` is built from `N` by offsetting each banding path inward when its band is included and
joining consecutive paths at sharp joints with straight butt connectors. The outer envelope `O` is
`N` on included paths and the outward offset on non-included paths. The card reports core and overall
sizes from the bounds of `C` and `O`; a trim localized at one corner may therefore not change the
reported size, because the card shows the envelope rather than a per-edge minimum. The body is built
directly from `C`, so no stored core origin is needed for a shaped panel.

Edge banding is supported on shaped panels. A panel may keep its machining when its outline changes:
the cutout tools are subtracted from the shaped body in 3D. Adding new machining to a shaped panel
still waits for the contour-aware cutout union. Outline controls are never locked by banding or
machining: a shape change keeps every edge banding record, and a corner banding belongs to its corner
segment, so changing a chamfer to a radius keeps it while removing the treatment deletes it.

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

### 5.3. Segments and application order

- A banding belongs to one contour segment: an edge (ids 1..4) or a corner segment created by a
  chamfer or a radius. A segment carries at most one banding.
- Corner segments exist only while the corner has a treatment. Removing the treatment deletes the
  banding with the segment.
- Edge and corner bandings share one ordered list. Adding a banding appends its segment; editing the
  material, width or inclusion keeps its position; removing it preserves the order of the others.
- The card lists the banded segments and offers only existing unbanded segments in the add control.
  The order will later be reorderable; the last banding is drawn on top.
- A tangent path is contour grouping only. It no longer copies a banding to adjacent edges and a
  partially banded path is not an invalid state.
- Order matters at joints: the later band covers the earlier one and is cut at its outer face.

### 5.4. Corners

- **Right angle, 90°.** The later band covers the corner and is trimmed flush with the earlier band's outer face; its end face is visible. There is no mitre in the first version. At each end that meets an earlier band, the later band is extended along the edge by that band's thickness.
- **Chamfer.** The chamfer is its own segment with its own record. The adjacent edges keep their bandings; the chamfer strip meets them at butt joints.
- **Radius.** The radius is its own segment with its own record. If the adjacent segments share the
  same spec, the strips meet at the tangent points; they are not forced to share one record.
- **Stage 2b.** One strip is built per banded segment. An included segment offsets the core and a
  non-included segment extends the outer envelope. Machining on a shaped panel is subtracted from the
  shaped body in 3D; adding new machining still waits for the contour-aware cutout union.
- **Single banded edge at a corner.** If the adjacent segment has no later band, the strip ends flush with the adjacent side face of the core. Extending and trimming it into the joint is the next step.
- **Edge banding milling.** Rounding the banding faces and corner is a later operation and is outside the current scope.

### 5.5. Inclusion in size

- `included_in_size = true`: the core is reduced by `thickness` on that segment; a bottom or left straight band on a rectangular panel also moves the core origin inward; the banding fills the freed space; the overall size does not grow.
- `included_in_size = false`: the core is unchanged on that segment; the banding is added outside; the overall size grows by `thickness`.
- On a shaped panel the rule applies to the segment itself: an included segment is offset inward and its tangent neighbours are not moved. A straight connector joins the step in the core outline.
- The flag belongs to the segment's banding record, not to the banding material.
- Machining coordinates do not change when the flag changes: the machine works on the core, and the parametric layer applies the core origin when building the body and tools.

## 6. Display and diagnostics

### 6.1. Colours

- Colours are not stored in the scene document.
- The base colour of a panel comes from the panel material. To distinguish panels that use the same material, a deterministic tint is applied from `panel.id()`, so the colour does not change when the scene is rebuilt.
- All bandings on one panel get one common colour, different from the body colour. For now this is a debug colour, not the colour of a specific banding material.
- If the panel material is missing, the panel is drawn in a neutral colour.
- Later, colour and texture will come from the material; textures are a separate rendering task.

### 6.2. Panel diagnostics

Panel issues are computed during loading/rebuild and are not stored as data:

- no material is assigned to the panel (`None`);
- a material id is absent from the catalogue;
- an edge-banding material is missing;
- a corner-banding material is missing;
- the core is not positive;
- machining no longer fits the new core;
- the banding profile is unavailable and there is no fallback.

A panel with an issue stays in the document, is drawn where possible and shows a warning in the card. A whole-file load error is reserved for scenes that cannot be read or recovered; see [Scene document](document.md).

## 7. Panel commands

All changes are `engine` commands with stable ids and typed parameters. The UI never writes to the document directly.

- `panel.create` — parent, name, nominal length, nominal width, material, placement. Thickness is not passed.
- `panel.rename` — panel, a non-blank name.
- `panel.resize` — panel, nominal length, nominal width.
- `panel.material.set` — panel, material.
- `panel.edge_banding.set` — panel, edge, material, included_in_size. The command applies to that edge segment only. A new banding is appended to the order; editing an existing one keeps its position.
- `panel.edge_banding.clear` — panel, edge. Removes that edge's banding.
- `panel.corner_banding.set` — panel, corner `VertexId`, material, included_in_size. The corner must currently have a chamfer or radius segment. A new banding is appended; editing keeps its position.
- `panel.corner_banding.clear` — panel, corner `VertexId`. Removes the corner segment's banding.
- `panel.corner.set` — panel, corner `VertexId`, and a feature: `Radius { radius }` or `Chamfer { incoming, outgoing }`. The two chamfer setbacks are measured along the incoming and outgoing edges and may differ.
- `panel.corner.clear` — panel, corner `VertexId`. The corner becomes a point; its corner banding is removed with the segment.
- `panel.bow.set` — panel, edge `EdgeId`, signed sagitta. Positive bows outward; negative inward. Zero is rejected; use `panel.bow.clear`.
- `panel.bow.clear` — panel, edge `EdgeId`.

Commands reject unknown ids, unknown catalogue materials, a non-positive resulting core, a resize that would leave a through cutout entirely outside the core or split the remaining core, and a corner feature that no longer fits. A shape command is never rejected by banding or machining: edge records stay on their edges, while a corner banding belongs to its corner segment and is deleted when the corner becomes a point. A valid `panel.material.set` or `panel.edge_banding.set` that only makes machining invalid is accepted: the data stays and the panel becomes a problem panel. Loading a scene with a non-positive core, a cutout outside the core, or a core split by cutouts preserves the panel as a problem panel; those conditions are diagnostics, not load errors. Adding a cutout to a shaped panel is rejected until the shaped-core cutout union lands; shaping a panel that already has machining is accepted and the cutout tools are subtracted from the shaped body. A successful command is one undo entry; a failed command leaves the document unchanged.

## 8. Panel tests

At minimum:

- the 720 × 500 size table for every inclusion combination;
- an included bottom or left band moves the core inward and leaves the nominal envelope unchanged;
- banding order and corner coverage by the later band at a right angle;
- banding wider than the panel thickness is trimmed; banding narrower than the thickness is drawn partially;
- the panel group returns `panel.id()` for the body and every banding;
- panel commands and undo/redo;
- banding one edge leaves the neighbouring segments of its tangent path unbanded;
- a new banding is appended, a parameter edit keeps its position, and clearing it returns the segment to the add picker;
- corner banding survives a chamfer-to-radius change and is removed when the corner becomes a point;
- a shaped panel with an included band trims the core and a non-included band extends the outer envelope;
- a shape change keeps every edge banding record and every machining feature;
- a shaped panel with machining builds and subtracts the cutout volume;
- machining that no longer fits the core marks the panel as a problem panel without losing data;
- overlapping through cutouts merge and a cutout crossing the boundary opens a notch;
- a cutout that would split the core into disconnected pieces or remove it is rejected, and a loaded file with such a core is diagnosed;
- machining keeps its core-local position when an included band moves the core origin.

Catalogue and scene-loading tests live in [Materials and catalogue](materials.md) and [Scene document](document.md).

## 9. Open questions

1. At a sharp joint between two included paths with different thicknesses, the core and each band use butt connectors; the current model leaves the resulting step as-is. Confirm whether a later-band cover rule should replace it.
2. Stage 3: the in-house `geom::region` rectangle boolean implements the through-cutout union, boundary notches, the one-component core invariant and closed `Region` rings with holes for rectangular panels (ADR 0012). Shaped cores are next.
3. Stage 4: banding on cutout edges uses the same tangent-path and inclusion rules, but the path representation for inner loops is not designed yet.
4. Edge banding milling: rounding the faces and corner.
5. Whether the card should show core and overall size separately from the nominal size.

The target contour rules — tangent-continuous paths, signed bow, union of through cutouts and the
no-division invariant — are recorded in ADR 0009. Corner features, bows, tangent paths, chamfer corner
banding and shaped-panel band strips are implemented; the rectangular cutout union and its `Region`
ring tracing are implemented, and shaped-core rings plus cutout banding are staged next.
