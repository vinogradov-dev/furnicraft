# Scene document

Status: domain specification. This document describes the scene file: what it stores, what it does not store, how a scene is loaded, how it behaves when a material is missing and which diagnostics it shows. Panel geometry and catalogue structure are not duplicated here.

Related documents:

- [Panels and edge banding](panels.md) — panel, its dimensions, banding and commands.
- [Materials and catalogue](materials.md) — material records and the catalogue.

Related decisions: [0005](../decisions/0005-material-catalogue.md),
[0006](../decisions/0006-panel-nominal-and-resolved-geometry.md),
[0007](../decisions/0007-edge-banding-v1.md),
[0008](../decisions/0008-panel-diagnostics.md),
[0021](../decisions/0021-assemblies-are-components-groups-are-containers.md) and
[0022](../decisions/0022-closed-containers-pick-as-one-object.md).

## 1. Terms

- **Document** (scene) — a saved FurniCraft project: node tree, panels, their placements, machining and material references.
- **Catalogue** — the external material collection; it is not part of the scene file.
- **Fallback** — the last known value stored in the scene in case a material disappears from the catalogue.
- **Diagnostic** — a computed panel or scene state that is not stored as data.
- **Revision** — a monotonic document change counter.
- **Schema version** — the version of the scene file format.

## 2. What the scene file stores

- schema version;
- revision;
- id allocator;
- node tree (panels, assemblies and groups);
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

`material_id` is optional: `None` means no material was assigned, while `Some(id)` that the catalogue cannot resolve is a missing material. `last_known_*` values are always stored as a cache for resilience when the catalogue is missing, not user-editable parameters. When the material is found, the catalogue is the source of truth and the fallback is refreshed on load.

### 2.1. Nodes, panels, assemblies and groups

The node tree stores three kinds of node. A **panel** is an atomic leaf: it owns its machining,
banding and material references, is never shown with children, and cannot be ungrouped. An
**assembly** is a meaningful furniture or industrial component (a cabinet, a drawer box, a runner, a
hinge) and, from M6, carries a furniture type with type-specific properties and operations. A
**group** is an organisational container with no furniture type; it only groups, moves, copies and
deletes its children. A node id is unique across all three kinds and is never reused after a delete
or an undo.

A placement is always relative to the parent container. Grouping sibling nodes wraps them in a new
group with an identity placement, so their world transforms stay the same; ungrouping a group
composes each child with the removed group's placement for the same reason. An assembly is not
flattened by `node.ungroup`: dismantling a component is a separate operation with its own rules. The
inspector edits the translation in millimetres in the parent frame and the rotation as intrinsic XYZ
Euler angles in degrees; the document stores the unit quaternion, so editing one half never resets
the other. The viewport translate gizmo moves that same translation directly and the rotate gizmo
turns the node about its local axes and world origin; each drag previews the frame and one
`node.placement.set` is dispatched on release, so the drag is one undo entry. Move and Rotate are one
active gizmo at a time. The root group defines the world frame and cannot be deleted, moved, grouped
or ungrouped. See
[ADR 0020](../decisions/0020-the-translate-gizmo-previews-and-commits-one-placement.md),
[ADR 0021](../decisions/0021-assemblies-are-components-groups-are-containers.md),
[ADR 0022](../decisions/0022-closed-containers-pick-as-one-object.md) and
[ADR 0024](../decisions/0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md).

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
3. For each panel, read `material_id`. If it is `None`, use the fallback and mark `NoMaterial`; otherwise look it up in the catalogue.
4. If the material is found: panel thickness = the material's nominal thickness; refresh `last_known_thickness`.
5. If a present id is not found: thickness = `last_known_thickness`; mark `MissingMaterial` and the panel as a problem panel.
6. For each banding, look up its edge-banding material.
7. If found: profile thickness and width = catalogue values; refresh the fallback.
8. If not found: use the last known profile; mark the panel as a problem panel.
9. Recompute the core and overall size using the formulas in [Panels and edge banding](panels.md).
10. Compute machining diagnostics against the new core; do not reject the file for a fit problem.
11. Rebuild the panel geometry and group.
12. Build the scene and UI diagnostics.

Loading must not fail because a material is missing. A whole-file load error is acceptable only when the scene cannot be read or recovered.

## 5. Missing material

### 5.1. Panel material

If the panel material is missing:

- the panel is not deleted or reset;
- `last_known_thickness` is used;
- the panel is drawn in a neutral colour;
- `NoMaterial` (no id assigned) is reported separately from `MissingMaterial(id)` (an id absent from the catalogue);
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
- `panel.material.set` rejects an unknown material id or a non-positive resulting core. If the new thickness makes machining invalid, the command is accepted, data is not deleted, and the panel becomes a problem panel with a warning.
- Changing an edge-banding material changes the profile and may change the core/overall size when the banding is included in the size. The nominal panel length and width are preserved. `panel.edge_banding.set` follows the same rule as a material change: unknown ids and a non-positive core are errors; machining that no longer fits is a diagnostic.
- `panel.resize` is stricter: it rejects a change that would leave existing machining outside the new core.
- Changing material properties in the catalogue under the same id is applied on the next scene load.

## 7. Diagnostics

Diagnostics are computed during loading/rebuild and are not stored as data:

- `NoMaterial`: no material id is assigned;
- `MissingMaterial(id)`: the assigned id is absent from the catalogue;
- `MissingBandingMaterial { edge, material }`;
- `MissingBandingProfile { edge }`: legacy data has no usable fallback;
- `NonPositiveCore`;
- `MachiningOutsideCore`;
- `BlindDepthNotLessThanThickness`.

A panel with an issue stays in the document, is drawn where possible and shows a warning in the card. A diagnostic does not change the document and does not enter the file.

The document itself validates identity, nominal dimensions, finite values, unique banding edges and fallback dimensions. Core positivity and machining fit are diagnostics, not load errors: a catalogue change must not make a saved scene unreadable.

## 8. Schema and migrations

- The scene file is versioned.
- New fields use `#[serde(default)]` so older files keep loading.
- Unknown required fields or unknown enum variants are a readable error, never a silent interpretation.
- Legacy files without a panel material load as panels with no material assigned:
  - the old length and width become the nominal size;
  - the old thickness is preserved as `last_known_thickness`;
  - `material = None` and the panel is marked as a problem panel;
  - the user sees a warning and can assign a material.
- The schema version is bumped; the migration preserves the old dimensions instead of attempting to infer a catalogue material.
- Version 3 moved panel machining to the nominal panel frame. Version 2 cutouts are migrated by adding the stored fallback core origin, so their physical position does not change.
- Version 4 gives banding one ordered list of segments. A version 3 file migrates with its edge bands in their stored order, followed by its corner bands.
- Version 5 splits assemblies from groups. A version 4 file stores an assembly at the root and only assemblies below it; the root loads as a group and every nested assembly keeps its kind.
- On save, fallback values are refreshed from the catalogue when the material is found.

## 9. Commands and undo

- Every document change goes through an `engine` command.
- A successful command is one undo entry; a failed command leaves the document unchanged.
- The undo snapshot is the document without catalogue, meshes, colours or diagnostics.
- The catalogue is immutable for the duration of a session. v1 has no hot reload: re-reading it builds a new session and clears the old undo history.

## 10. Tests

At minimum:

- the catalogue never appears in a serialised scene;
- a legacy file without a material loads with a warning instead of failing;
- missing panel material: old thickness, neutral colour, `<no material id %d>`, panel not lost;
- `NoMaterial` and `MissingMaterial(id)` are reported separately;
- restoring the material clears the problem;
- missing edge-banding material: fallback profile or no banding, panel remains a problem panel;
- changing material thickness on load changes panel thickness;
- changing panel or banding material rebuilds geometry;
- a material change that invalidates machining keeps the data and reports a diagnostic;
- a resize that invalidates machining is rejected without changing the document;
- undo/redo does not depend on the catalogue;
- loading the scene twice produces the same dimensions and diagnostics.

## 11. Open questions

1. Scene file format: JSON until a binary format appears?
2. How to synchronise the scene with a real database when the mock file is replaced by one.
3. Whether the scene should store a snapshot of the material name for the card when the catalogue is unavailable.
