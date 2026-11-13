# Scene document

Status: domain specification. This document describes the scene file: what it stores, what it does not store, how a scene is loaded, how it behaves when a material is missing and which diagnostics it shows. Panel geometry and catalogue structure are not duplicated here.

Related documents:

- [Scene file](scene-file.md) — the envelope that holds the document on disk.
- [Geometry objects](geometry.md) — objects with a baked mesh, a parametric primitive or a feature history.
- [Panels and edge banding](panels.md) — panel, its dimensions, banding and commands.
- [Materials and catalogue](materials.md) — material records and the catalogue.

Related decisions: [0005](../decisions/0005-material-catalogue.md),
[0006](../decisions/0006-panel-nominal-and-resolved-geometry.md),
[0007](../decisions/0007-edge-banding-v1.md),
[0008](../decisions/0008-panel-diagnostics.md),
[0022](../decisions/0022-closed-containers-pick-as-one-object.md),
[0034](../decisions/0034-nodes-carry-typed-properties.md),
[0035](../decisions/0035-furniture-types-form-a-numbered-tree.md),
[0036](../decisions/0036-cameras-and-lights-are-scene-objects.md) and
[0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md).

## 1. Terms

- **Document** (scene) — a saved FurniCraft project: node tree, panels, their placements, properties, machining and material references.
- **Catalogue** — the external material collection; it is not part of the scene file.
- **Fallback** — the last known value stored in the scene in case a material disappears from the catalogue.
- **Diagnostic** — a computed panel or scene state that is not stored as data.
- **Revision** — a monotonic document change counter.
- **Schema version** — the version of the scene file format.

## 2. What the scene file stores

- schema version;
- revision;
- id allocator;
- node tree (containers, panels, geometry objects, camera and light objects);
- each node's typed properties, including a furniture type;
- each node's pivot as an exact local offset from its origin; the origin itself is implicit in the placement;
- the active camera, and camera and light parameters;
- geometry payloads: a baked mesh, the parameters of a primitive, or the features of a constructive object;
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

### 2.1. Nodes, panels and containers

The node tree stores two kinds of node. A **panel** is an atomic leaf: it owns its machining, banding
and material references, is never shown with children, and cannot be ungrouped. A **container** is a
named, placed node with children and typed properties: a cabinet, a hinge of three parts, a facade
holding a panel and its hardware, or an ordinary temporary group. The project root is a container.
A node id is unique across both kinds and is never reused after a delete or an undo.

A container is a **component** when it carries a non-zero `furntype`, a `prototype.id` or an explicit
`component = true`; otherwise it is an ordinary group. `node.ungroup` flattens an ordinary container
into its parent and refuses a component. `node.component.convert_to_group` clears the component
marker and the prototype keys and then flattens the container into its parent. A **furniture type** is a
six-digit `GG.TT.VV` code stored in `furntype`; `000000` means unclassified. The registry and query
rules live in [Furniture types](furntypes.md).

Every node carries a property bag: an ordered map from a dotted key to a typed value (`Bool`,
`Integer`, `Length`, `Angle`, `Text`, `Choice`, `Reference`, `FurnType` or `Structured` JSON). Name,
placement, pivot_local, panel size and material stay typed fields and are not duplicated in the bag.
`component`, `furntype`, `prototype.id`, `prototype.version` and `prototype.parameters` are reserved
system keys; user properties live in their own namespaces. An unknown user property survives a load
unchanged. Arrays and nested attributes that belong to a prototype are stored as one `Structured`
JSON property and rendered in the UI as fields driven by the prototype schema.

A placement is always relative to the parent container. Grouping sibling nodes wraps them in a new
container anchored at the centre of their union bounds: the container's translation is that centre,
every child placement moves by `-centre` and the pivot stays at its local zero, so their world
transforms stay the same; ungrouping an ordinary container composes each child with the removed
container's placement for the same reason.
The inspector edits the origin's translation in millimetres in the parent frame and the rotation as
intrinsic XYZ Euler angles in degrees; the document stores the unit quaternion, so editing one half
never resets the other. The translation *is* the origin: it is not a second stored field. The
viewport translate gizmo moves that translation directly and the rotate gizmo turns the node about
world X, Y and Z axes; both use one stored `pivot_local`, an exact offset from the origin. A panel
starts with a zero offset, so it turns about its base corner; any container is born at its own
origin; a group puts the centre of its children's union into the translation while its pivot offset
stays zero. The root always has a zero pivot offset and cannot be edited. `node.pivot.set` and
`node.pivot.center` are the only ways to author the offset, and the inspector exposes X/Y/Z and
Center. `node.origin.rebase` re-anchors a node with an authored pivot and keeps both the world
vertices and `pivot_world`; `node.placement.world_zero` clears only the translation. Each drag
previews the frame and one `node.placement.set` is dispatched on release, so the drag is one undo
entry. Move and Rotate are one active gizmo at a time. The root container defines the world frame and
cannot be deleted, moved, grouped or ungrouped. See
[ADR 0020](../decisions/0020-the-translate-gizmo-previews-and-commits-one-placement.md),
[ADR 0022](../decisions/0022-closed-containers-pick-as-one-object.md),
[ADR 0024](../decisions/0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md),
[ADR 0026](../decisions/0026-the-manipulation-pivot-is-stored-node-data.md),
[ADR 0027](../decisions/0027-the-rotate-gizmo-uses-world-aligned-rings.md),
[ADR 0034](../decisions/0034-nodes-carry-typed-properties.md) and
[ADR 0088](../decisions/0088-origin-and-pivot-are-different-points.md).

### 2.2. Geometry, camera and light nodes

Beside containers and panels the tree holds **geometry objects** and **camera** and **light**
objects. A geometry object carries one payload — a baked mesh, a parametric primitive, a flat path or
an ordered feature history; they are never mixed, and only a mesh object accepts mesh editing
([Geometry objects](geometry.md),
[ADR 0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)).

A camera or a light is an ordinary node with a name, a placement, properties, selection and undo; the
document also stores which camera is active. The built-in camera and the built-in key light are
defaults, not nodes: they cannot be selected or deleted and are used when the scene has none
([ADR 0036](../decisions/0036-cameras-and-lights-are-scene-objects.md)). A scene with no camera and
no light is valid.

A camera looks down its local `-Z` with `+Y` up, and its projection is either a perspective vertical
field of view or an orthographic visible height. A sun light emits along its local `-Z`; a point
light radiates from its origin. The frame is lit by the first sun light in tree order, whose world
rotation gives the direction and whose colour is scaled by its intensity; the intensity is a
multiplier with no distance falloff, because a sun is directional. Every point light adds its world
position, colour and power, at most eight in tree order; its intensity is radiant power in watts and
the renderer applies inverse-square falloff with no hard range cutoff
([ADR 0039](../decisions/0039-point-lights-use-watts-and-inverse-square-falloff.md)). A scene
without a sun keeps the built-in key light, and point lights add to that key. While an active camera
is set the viewport draws through it, and navigation and the projection toggle are disabled, because
both would change a camera the document owns. Clearing the active camera returns the viewport to the
built-in, navigable camera, whose projection the viewport toggles between perspective and
orthographic ([ADR 0049](../decisions/0049-the-viewport-toggles-its-own-projection.md)).

## 3. What the scene file does not store

- the material catalogue;
- meshes and other derived geometry; a constructive object stores its features, and the evaluated
  mesh is rebuilt on load;
- colours, brushes and textures;
- diagnostics and "problem panel" states;
- viewport navigation, selection, hover, open containers, grid, shading, projection and the work
  plane — they belong to the session, not to the document
  ([ADR 0029](../decisions/0029-view-state-belongs-to-the-session.md));
- the undo and redo history;
- anything about an order: number, customer, version or status; the order program owns them
  ([Scene file](scene-file.md) §4);
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
- `InvalidOutline`: the outline or its edge banding does not form a valid contour ring;
- `MachiningOutsideCore`;
- `BlindDepthNotLessThanThickness`.

A panel with an issue stays in the document, is drawn where possible and shows a warning in the card. A diagnostic does not change the document and does not enter the file.

The document itself validates identity, nominal dimensions, finite values, unique banding edges and fallback dimensions. Core positivity and machining fit are diagnostics, not load errors: a catalogue change must not make a saved scene unreadable.

## 8. Schema and migrations

**Before `1.0` the document has no past**
([0096](../decisions/0096-before-the-first-release-the-format-has-no-past.md)). The current layout is
the only one this build reads: any other `schema` number is one readable error naming both numbers.
A schema change bumps the number and deletes the old loader in the same commit — no migration pass,
no alias for a previous field name, no older golden file. The first release flips the rule; a
recorded decision restores compatibility for released users only.

- A field that is optional in the *current* layout uses `#[serde(default)]`. A default that exists
  only so a previous layout can load is a defect; the tree has none left.
- The loader converts nothing. A field an earlier layout stored — the panel `size`, a container
  `assembly` or `group` tag, a `pivot` named `pivot_local`, a tagged `cutout` record, a contour
  `points` list — is an unknown field and a readable error.
- Unknown required fields or unknown enum variants are a readable error, never a silent
  interpretation.
- The version-by-version history of pre-release schemas is not kept in this document; `git log` and
  the decisions hold it.
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

1. Binary or compressed form of the scene file, once meshes make it large; the JSON envelope is
   described in [Scene file](scene-file.md).
2. How to synchronise the scene with a real database when the mock file is replaced by one.
3. Whether the scene should store a snapshot of the material name for the card when the catalogue is unavailable.
