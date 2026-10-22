# Scene document

Status: domain specification. This document describes the scene file: what it stores, what it does not store, how a scene is loaded, how it behaves when a material is missing and which diagnostics it shows. Panel geometry and catalogue structure are not duplicated here.

Related documents:

- [Panels and edge banding](panels.md) — panel, its dimensions, banding and commands.
- [Materials and catalogue](materials.md) — material records and the catalogue.

## 1. Terms

- **Document** (scene) — a saved FurniCraft project: assembly tree, panels, their placements, machining and material references.
- **Catalogue** — the external material collection; it is not part of the scene file.
- **Fallback** — the last known value stored in the scene in case a material disappears from the catalogue.
- **Diagnostic** — a computed panel or scene state that is not stored as data.
- **Revision** — a monotonic document change counter.
- **Schema version** — the version of the scene file format.

## 2. What the scene file stores

- schema version;
- revision;
- id allocator;
- assembly tree;
- panels:
  - id;
  - name;
  - nominal length and width;
  - `material_id`;
  - `last_known_thickness`;
  - placement;
  - machining;
- panel edge banding:
  - `edge_id`;
  - `material_id`;
  - `included_in_size`;
  - application order;
  - `last_known_thickness`;
  - `last_known_width`.

`last_known_*` values are a cache for resilience when the catalogue is missing, not user-editable parameters. When the material is found, the catalogue is the source of truth and the fallback is refreshed on load.

## 3. What the scene file does not store

- the material catalogue;
- meshes and other derived geometry;
- colours, brushes and textures;
- diagnostics and "problem panel" states;
- camera, viewport, selection and other view state;
- panel thickness as a user-editable parameter; it comes from the material, and the file stores only a fallback.

## 4. Scene loading

1. Read the scene file.
2. Read the catalogue (see [Materials and catalogue](materials.md)).
3. For each panel, look up its panel material by `material_id`.
4. If the material is found: panel thickness = the material's nominal thickness; refresh `last_known_thickness`.
5. If the material is not found: thickness = `last_known_thickness`; mark the panel as a problem panel.
6. For each banding, look up its edge-banding material.
7. If found: profile thickness and width = catalogue values; refresh the fallback.
8. If not found: use the last known profile; mark the panel as a problem panel.
9. Recompute the core and overall size using the formulas in [Panels and edge banding](panels.md).
10. Validate machining against the new core.
11. Rebuild the panel geometry and group.
12. Build the scene and UI diagnostics.

Loading must not fail because a material is missing. A whole-file load error is acceptable only when the scene cannot be read or recovered.

## 5. Missing material

### 5.1. Panel material

If the panel material is missing:

- the panel is not deleted or reset;
- `last_known_thickness` is used;
- the panel is drawn in a neutral colour;
- the card shows `<no material id %d>` or a similar marker instead of the material;
- the panel status shows a warning;
- on the next load, if the material is present again, the panel recovers and the warning disappears.

### 5.2. Edge-banding material

If an edge-banding material is missing:

- the last known banding profile is used;
- the panel is marked as a problem panel;
- if there is no fallback profile (for example, legacy data), the banding is not drawn and the panel remains a problem panel.

### 5.3. What not to do

- Do not replace a missing material with a similar one.
- Do not search for a replacement by name.
- Do not delete the panel or the banding.
- Do not crash and do not lose user data.

## 6. Material changes

- Changing a panel material through a command changes `material_id`; thickness, geometry and banding are recomputed.
- If machining no longer fits the core after the recomputation, data is not deleted: the panel is marked as a problem panel and the user sees a warning.
- Changing an edge-banding material changes the profile and may change the core/overall size when the banding is included in the size. The nominal panel length and width are preserved.
- Changing material properties in the catalogue under the same id is applied on the next scene load.

## 7. Diagnostics

Diagnostics are computed during loading/rebuild and are not stored as data:

- the panel material is missing;
- an edge-banding material is missing;
- the core is not positive;
- machining no longer fits the new core;
- the banding profile is unavailable and there is no fallback.

A panel with an issue stays in the document, is drawn where possible and shows a warning in the card. A diagnostic does not change the document and does not enter the file.

## 8. Schema and migrations

- The scene file is versioned.
- New fields use `#[serde(default)]` so older files keep loading.
- Unknown required fields or unknown enum variants are a readable error, never a silent interpretation.
- Legacy files without a panel material load as panels with a missing material:
  - the old thickness is preserved as `last_known_thickness`;
  - the panel is marked as a problem panel;
  - the user sees a warning and can assign a material.
- On save, fallback values are refreshed from the catalogue when the material is found.

## 9. Commands and undo

- Every document change goes through an `engine` command.
- A successful command is one undo entry; a failed command leaves the document unchanged.
- The undo snapshot is the document without catalogue, meshes, colours or diagnostics.
- The catalogue is considered immutable for the duration of a session. If it is re-read, the scene is rebuilt; undo history for that reload has to be decided separately.

## 10. Tests

At minimum:

- the catalogue never appears in a serialised scene;
- a legacy file without a material loads with a warning instead of failing;
- missing panel material: old thickness, neutral colour, `<no material id %d>`, panel not lost;
- restoring the material clears the problem;
- missing edge-banding material: fallback profile or no banding, panel remains a problem panel;
- changing material thickness on load changes panel thickness;
- changing panel or banding material rebuilds geometry;
- machining that no longer fits the core produces a diagnostic instead of data loss;
- undo/redo does not depend on the catalogue;
- loading the scene twice produces the same dimensions and diagnostics.

## 11. Open questions

1. Scene file format: JSON until a binary format appears?
2. Whether to store `last_known_*` for banding always or only after a successful catalogue load.
3. Machining policy when the material or banding changes: warning only, or command rejection.
4. What to do when a banding has no fallback profile (legacy data): skip the banding or draw a placeholder.
5. Whether hot catalogue reload without restarting the scene is needed, and what happens to undo.
6. How to synchronise the scene with a real database when the mock file is replaced by one.
7. Whether the scene should store a snapshot of the material name for the card when the catalogue is unavailable.
