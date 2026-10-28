# PROJECT.md — FurniCraft

## Vision

FurniCraft is a native, free and open-source 3D editor for parametric cabinet furniture. The user describes a product the way a furniture maker thinks about it — overall dimensions, panels, materials, edge banding and hardware — and the program builds an accurate 3D model that later becomes drawings, cutting lists and CNC programs. It is built for the craft, not for the cloud: a fast Rust binary with `wgpu` graphics and `egui` UI, with interaction quality close to Blender, and no web technologies anywhere in the product.

## Scope

**In scope, in order of arrival:**

1. A document model for cabinet furniture: model space and placements, containers and components, panels, machined features, edge banding, materials, hardware, typed properties and furniture types.
2. Geometry objects beside the furniture parts: a mesh built from a primitive and edited directly, or a solid grown from a flat contour through a saved feature history and collapsed to a mesh when the user is done with it.
3. Classification and queries: every object carries a numeric furniture type and typed properties, so parts lists, reports and CNC rules are derived by query. Parametric construction arrives later as user scripts, not built-in product builders.
4. 3D viewport: camera views, orbit/pan/zoom, shading, selection and picking, a floor grid, dimensions.
5. Camera and light objects, and a versioned scene file that saves the whole scene. Orders and their versions stay with the order program, never in the file.
6. Editing: create and edit panels, assemblies, materials and hardware through commands, with undo/redo.
7. Output: cutting lists per material and sheet, panel drawings, and eventually CNC programs.
8. Assistance: an optional built-in assistant, Faber, that reads the project, dispatches the same
   commands a person uses, and can run headless jobs over existing orders.

**Not in scope:** web or browser builds, cloud storage, collaboration, real-time rendering for presentation, free-form organic modelling, and import of third-party CAD formats at this stage.

## Key data model

The model is intentionally described at the level of vocabulary and responsibilities. Exact fields, coordinate conventions and invariants are designed in M1 — see "Open model questions" below. Do not infer a schema from these descriptions.

### Document (`Project`)

The root of a saved scene: the node tree, material references and their fallbacks, the property and furniture-type data, the camera and light objects, document-level settings (default units, grain conventions) and a schema version. Serialized with `serde`; must keep older files loading. The first M1 skeleton stores the schema version, revision, next id and root container; catalogues and settings follow. The document is one section of the scene file; the envelope around it is described under "Scene file" below.

### `Id` and `Revision`

Stable identifiers (`Id`) let hardware, commands and the undo stack refer to a panel or a container without holding a pointer. A monotonically increasing `Revision` marks every document change; caches (meshes, picking structures, derived parts) are keyed by it. Both are technical plumbing, not user-visible entities. In v1 an `Id` is a non-zero `u64` serialized as a number, and a `Revision` is a `u64` starting at zero. Panel vertices and edges use stable ids of the same kind; their visible numbers are labels, not keys.

### `Container`, `Properties` and `FurnType`

The tree has one container kind beside the panel. A **container** is a named, placed node with
children and typed properties: a cabinet, a hinge of three parts, a facade holding a panel and its
hardware, an ordinary temporary group. The project root is a container and defines the world frame.
In v1 every tree node is unique and owns its children; shared definitions and instances are
deferred.

A container is a **component** (the older word, "assembly") when it carries a non-zero `furntype`, a
`prototype.id`, or an explicit `component = true`. `node.ungroup` refuses a component;
`node.component.convert_to_group` clears the marker so the container can be flattened like any other
group. Component identity is a property, not a node kind ([ADR 0034](docs/decisions/0034-nodes-carry-typed-properties.md)).

Every node — panel or container — carries a **property bag**: an ordered map from a dotted key to a
typed value (`Bool`, `Integer`, `Length`, `Angle`, `Text`, `Choice`, `Reference`, `FurnType`,
`Structured` JSON). Reset fields such as name, placement, pivot, panel size and material stay typed
fields and are not duplicated there. System keys (`component`, `furntype`, `prototype.id`,
`prototype.version`, `prototype.parameters`) are written only through validated commands; user keys
live in their own namespaces. Unknown user properties survive a load.

A **furniture type** is a six-digit code `GG.TT.VV` stored in `furntype`, with `000000` meaning
unclassified; `GG` is the group, `TT` the type and `VV` the variant of a strict tree. Codes are
stable and opaque, labels are registry data, and a query matches a prefix: `30` is every facade,
`20.01.01` is every shelf. Material, finish, dimensions and price are separate properties, never
encoded in the code. The registry lives in [`docs/domain/furntypes.md`](docs/domain/furntypes.md);
see [ADR 0035](docs/decisions/0035-furniture-types-form-a-numbered-tree.md).

### `Geometry` object

A named, placed node with free-form geometry, beside the panels and containers. It holds exactly one
payload: a **baked mesh** (positions and polygon faces, edited directly, undo is its only history) or a
**feature history** (an ordered list beginning with an extruded flat contour; the mesh is derived and
never saved). A constructive object rebuilds from its contour and is not editable as a mesh until
`object.collapse` bakes it, which is one undoable command and cannot be reversed after undo expires.
Features reference their inputs, never a derived face: fillet and chamfer on a generated face wait for
a persistent-naming design. A panel is not a geometry object; panel rules are manufacturing rules.
See [`docs/domain/geometry.md`](docs/domain/geometry.md) and
[ADR 0037](docs/decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md).

### `Placement`

A rigid transform relative to the parent frame: translation and rotation. Scale and shear are not part of a placement. Placements compose down the node tree to produce world transforms. The document stores placements and the camera and light objects; it never stores a viewport or its navigation. In v1 the rotation is a unit quaternion; non-finite or non-unit values are rejected on load instead of being normalised silently. The inspector presents that quaternion as intrinsic XYZ Euler angles in degrees, while the rotate gizmo writes the quaternion directly. See [ADR 0024](docs/decisions/0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md).

### Commands

Every document change is a command in `furnicraft-engine` with a stable id and typed parameters. `panel.create` takes the parent container id, a name, a `PanelNominalSize`, a `MaterialId` from the session catalogue and a `Placement`; it resolves the material thickness, allocates the panel id, bumps the revision once and returns the created id. `panel.rename` takes a panel id and a non-blank name and bumps the revision once. `panel.resize` takes a panel id and a new `PanelNominalSize` and bumps the revision once. `panel.material.set` takes a panel and a catalogue material and updates the material with its nominal thickness as the fallback. `panel.edge_banding.set` takes a panel, an edge, a catalogue banding material and the included-in-size flag; replacing an edge moves it to the end of the application order. `panel.edge_banding.clear` removes the banding on an edge. `cutout.create` takes a panel id, a rectangular `CutoutShape`, a `CutoutDepth` and an entry face, allocates the cutout id and bumps the revision once. The scene-tree commands operate on panels and containers: `assembly.create` adds an empty container marked as a component under a parent, `node.rename` renames any node including the root, `node.delete` removes a subtree, `node.duplicate` deep-copies one with fresh ids, `node.group` wraps two or more sibling nodes in a new group container, `node.ungroup` flattens a non-component container back into its parent, `node.component.convert_to_group` clears the component marker and prototype keys and then flattens the container, and `node.placement.set` moves a node relative to its parent. `node.property.set`, `node.property.clear` and their batch form edit typed properties as one revision each; setting `furntype` or `prototype.id` marks a container as a component. `node.placement.set_many` places a batch of nodes as one change: the selection's top-most nodes each get a placement, the revision bumps once and the whole drag is one undo entry ([ADR 0033](docs/decisions/0033-a-multi-selection-is-placed-by-one-command.md)). A panel is an atomic leaf: it is never split by grouping and never ungrouped. A component is not flattened by `node.ungroup`; `node.component.convert_to_group` clears the marker and prototype keys and flattens it, so the two operations stay explicit. Grouping keeps order and world transforms; the root container is immutable for delete, placement, grouping and ungrouping. The UI dispatches commands by id; `Session` snapshots the document before a command and keeps one undo entry per successful change. Undo and redo restore content but advance the revision and keep the id allocator, so caches stay keyed to a monotonic counter and ids are never reused.

### `Panel`

One flat part: nominal length and width, the material it is cut from, its grain direction, the placement that puts it in its parent container, edge banding on each of its edges, and the list of machined features on it. Panels are the atoms of cutting lists, drawings and CNC output, so they carry their own manufacturing data. The document stores the nominal size, the material id with a fallback thickness, the banding with fallback profiles and the placement; core, overall size and effective thickness are resolved from the material and banding. Grain and hardware-generated holes follow.

The panel's local frame has its origin at the **minimum corner of the nominal rectangle** (`z = 0`), `+X` along the **length**, `+Y` along the **width**, and `+Z` along the **thickness**; the panel occupies `z` from `0` to its thickness. Included bottom and left banding move the core inward, while the nominal corner stays the placement anchor. This convention is fixed for the geometry kernel and the panel features.

Panel dimensions are positive. The v1 validator accepts a length and width up to 10,000 mm and a thickness up to 1,000 mm; zero, negative and larger values are errors, not warnings.

The two large faces are **A** (at `z = thickness`) and **B** (at `z = 0`); `+Z` points from B to A. Viewed from A, outline vertices are numbered **1..n counter-clockwise**, starting at the nominal zero: for a rectangle, corner 1 is `(0, 0)`, 2 is `(nominal length, 0)`, 3 is `(nominal length, nominal width)`, 4 is `(0, nominal width)`. Edge `i` runs from corner `i` to corner `i+1`, wrapping at `n`; for a rectangle, edge 1 is the bottom, 2 the right, 3 the top, 4 the left. Edge and vertex ids are stable and never reused; their numbers are human labels. The labels are local to the panel and do not depend on world orientation or on how the panel is used.

### `Machining` (inside `Panel`)

A manufacturing feature on a panel: a drilled hole (diameter, depth, entry face), a groove/dado, a cutout, a rebate or an angled end cut. Features are owned by the panel they belong to and addressed from outside as `(panel_id, machining_id)`. All machining is stored in **core-local coordinates relative to the core zero**, whether it belongs to the panel or is generated by hardware. The core origin is derived from included banding, so machining does not move when an included band changes. A panel can own cutouts directly, including blind cutouts that name their entry face A or B; holes required by hardware are generated by that hardware and are a separate source, not authored as panel-owned cutouts. The first cutout is a rectangle in core-local XY with a through or blind depth; cutouts must stay inside the core outline and must not overlap. Resizing a panel changes its dimensions but does not move or scale existing machining; a feature outside the new outline or a blind cutout deeper than the new thickness is a validation error. Hardware that needs an edge-relative position recomputes its holes in core-local coordinates.

### `EdgeBanding` (inside `Panel`)

The banding applied to one segment of the panel contour: material, thickness, inclusion in size, and width. The segment is referenced by its stable id: one of the four nominal edges or a chamfer/radius corner segment. Segments are banded in an application order, and a later band is cut against the earlier one's outer face on straight and arc joints (`docs/decisions/0014-banding-belongs-to-segments.md`, 0015, 0016); edge-banding milling remains later work.

### `Hardware`

A physical fitting: hinge, drawer runner, connector, shelf pin, handle, and so on. It has a type, a placement tied to one or more panels (a face and a position, or a reference to a machined feature such as a hole), and a quantity/orientation. Hardware must be able to generate the machining it requires, without duplicating hole definitions by hand.

### Queries and reports

Every object can be selected by structural kind, `FurnType` subtree and property predicates. The same query layer feeds the scene-tree filter, selection, reports and, later, CNC rules; a report is a saved query plus a column set and grouping, so a report and a machining program cannot disagree about which panels are shelves. Product dimensions (overall width, shelf pitch, door gap) are typed properties, not a separate dimension object: a prototype script reads them, and the panel sizes it derives stay materialized geometry in the document.

### `Material`

A catalogue entry: kind (chipboard, MDF, solid wood, plywood, HPL), nominal thickness, sheet size, decor/colour and grain availability. Panels reference materials by `MaterialId`; changing a material's thickness must not rewrite the panel's nominal size.

### Scene file

A saved scene is a UTF-8 JSON envelope with the extension `.fcs`: `format` (`furnicraft.scene`),
`format_version` for the framing, and one `document` payload that carries its own `schema`. The
envelope owns the versions, protects unknown top-level sections and lets the document layout change
without touching the framing. The file holds the scene — objects, camera and light objects, the active
camera, settings — and never the material catalogue, derived meshes, diagnostics, undo history,
selection, viewport state or any order data. Saving is atomic: a temporary file replaces the target
only after it is complete. See [`docs/domain/scene-file.md`](docs/domain/scene-file.md).

## Domain specifications

Detailed, normative descriptions live in `docs/domain/`:

- [`panels.md`](docs/domain/panels.md) — the panel as an atomic group, local frame, nominal/core/overall sizes, edge banding, order and corners.
- [`materials.md`](docs/domain/materials.md) — material records, the external catalogue and its future database structure.
- [`document.md`](docs/domain/document.md) — the document, its nodes, loading, fallbacks, diagnostics, schema and migrations.
- [`geometry.md`](docs/domain/geometry.md) — objects with a baked mesh or a feature history, evaluation and collapse.
- [`scene-file.md`](docs/domain/scene-file.md) — the envelope, its versions, what it stores and how it is written.
- [`furntypes.md`](docs/domain/furntypes.md) — the numeric furniture-type registry, encoding and query examples.

## Architectural principles

- **Data is separate from rendering.** `model` is serializable data; meshes and scene descriptions are derived in `mesh` and `scene`; only `render` touches `wgpu`. No GPU type ever appears in the document.
- **Space is data; views are state.** The document stores placements, the node tree, and the camera and light objects the user adds. Viewport navigation, grids, shading, selection and per-node open state are session or UI state.
- **The kernel is behind a port.** `model` and `param` use `furnicraft-kernel` types and traits; only `kernel-truck` knows the third-party B-rep engine.
- **Pure Rust dependencies.** No dependency may compile or vendor C/C++/Objective-C sources. Platform bindings that load an OS-provided library through Rust FFI are allowed; they must not add native build steps. `cargo xtask layers` checks the dependency graph for known C-vendoring crates.
- **Commands change data.** Every document mutation is a command in `engine` with a stable id and a `Result`; the UI dispatches commands and never writes to the model directly. This is what makes undo/redo, tests and future automation possible.
- **Derive, don't store.** Meshes, parts lists, bounds and caches are computed from the document and invalidated by `Revision`.
- **Units are explicit.** All physical sizes are `Length` in integer micrometres (`furnicraft-units`). Floating-point millimetres exist only at the geometry/rendering boundary (`furnicraft-geom`, `furnicraft-kernel`, `furnicraft-render`).
- **Stable ids, no pointers.** Cross-references use `Id`, so serialization, undo and clipboard operations stay sound.
- **Errors, not panics.** Every public operation returns a typed error; malformed input is reported to the user, never fatal.
- **Layers are enforced.** A crate may depend only on lower layers; `cargo xtask layers` checks the map below.

## Model space, placements and views

FurniCraft separates the model from the way it is viewed.

### Model space

- Right-handed, Z-up, millimetres for display; persisted lengths are `Length`.
- World origin at the floor (`z = 0`).
- Every container and part has a local frame. A `Placement` is a rigid transform (translation and rotation; no scale or shear) relative to the parent frame.
- A node's world transform is the composition of placements from the root.
- Imported assets are normalised on import: Z-up, scale baked, origin at the mounting point. Placements stay rigid.
- A panel's local frame is fixed: origin at the minimum corner of the nominal rectangle (`z = 0`), `+X` length, `+Y` width, `+Z` thickness; the panel occupies `z` from `0` to its thickness. Faces are A (`z = thickness`) and B (`z = 0`); vertices run 1..n counter-clockwise from the nominal zero and edge `i` joins vertex `i` to `i+1`. Included bottom and left banding move the core inward; machining is core-local.

### Views

- The document stores placements, each node's manipulation pivot, camera and light objects, and the active camera. A scene may have none of the latter.
- Viewport navigation, projection, grid, shading, selection, hover and open containers are session state; they are never document data. The built-in camera and key light are defaults, not objects. See [ADR 0029](docs/decisions/0029-view-state-belongs-to-the-session.md) and [ADR 0036](docs/decisions/0036-cameras-and-lights-are-scene-objects.md).
- A viewport is a UI adapter: it turns input into camera moves or commands and draws a scene; its camera is session view state. Several viewports may show the same document.
- Picking ray-casts the scene's world-space triangles and maps a screen position to a document `Id` (object, face, later edge); the session stores the resulting selection. See [ADR 0018](docs/decisions/0018-picking-ray-casts-the-scene.md) and [ADR 0029](docs/decisions/0029-view-state-belongs-to-the-session.md).
- The floor grid has X, Y and a short Z axis at the origin, and a corner orientation widget shows the six world directions. Clicking one snaps the camera to look from that side; clicking the current side flips to the opposite. Both are view state. See [ADR 0019](docs/decisions/0019-standard-views-from-a-corner-widget.md).
- Manipulation (move/rotate, gizmos, snapping) is a command; the UI never writes a placement directly. The viewport gizmos preview a moved frame during a drag and dispatch one `node.placement.set_many` on release, so a drag of the whole selection is one undo entry. Only the top-most selected nodes move, and a shared turn uses the primary node's stored pivot ([ADR 0033](docs/decisions/0033-a-multi-selection-is-placed-by-one-command.md)). The translate gizmo moves along a world axis; the rotate gizmo uses world-aligned X/Y/Z rings and turns the selection about one of those world axes. Both gizmos use one stored pivot in the node's own frame: a panel starts at its local zero, a container created with children at the centre of its local panel bounds, an empty container at its own origin, and the root is always zero. `node.pivot.set` and `node.pivot.center` move it explicitly; geometry edits do not. Only pure view state selects the active mode. See [ADR 0020](docs/decisions/0020-the-translate-gizmo-previews-and-commits-one-placement.md), [ADR 0024](docs/decisions/0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md), [ADR 0026](docs/decisions/0026-the-manipulation-pivot-is-stored-node-data.md) and [ADR 0027](docs/decisions/0027-the-rotate-gizmo-uses-world-aligned-rings.md).
- Drawing views (projections, sections, details) are output, not interactive viewports, and arrive with the output milestone.

## Assistant (Faber)

FurniCraft ships an assistant named **Faber** that follows the project in real time. It is disabled by
default because it needs a language-model connection; without one the editor behaves exactly as
before.

- Faber acts as a person does: every action is a command, a query or a session view operation,
  dispatched by the same ids the UI uses. It has no private mutation path (ADR 0028).
- It runs with a window, where the user watches it work, and headless, where it processes jobs
  without a viewport. The core is deterministic and model-agnostic; the language model is an injected
  adapter that may be local or remote. The application never listens for connections.
- The core does not receive pixels. It reads the scene as data: the node tree, panels, materials,
  placements, machining, diagnostics and derived measures. A vision channel is a separate, optional
  decision.
- Conversations, keys and the audit trail are not document data. Keys live in the OS keyring or a
  restricted configuration file; the audit trail is a sidecar.
- "Build a hinged wardrobe 600 × 2200 × 500" is one user prototype script with named parameters,
  not a prompt. The script builds panels and placements through commands, and the document keeps the
  materialized geometry, the furniture type, the prototype reference and its parameter record; the
  model only chooses the prototype and fills the parameters. The catalogue available to the model is
  generated from the prototype registry (ADR 0034).
- The material catalogue and the order registry are separate programs with their own databases and
  APIs. Faber uses the same API as their interfaces, never the database. An order is the project
  file, its records and the files produced for it; every version is a numbered folder holding the
  complete file set, and only one is sent to production (ADR 0032). External side effects such as
  ordering materials require a person's confirmation (ADR 0031).
- View state — selection, open containers and viewport navigation — belongs to the session, so every
  client shares the user's context (ADR 0029). Cameras and lights the user adds are scene objects
  (ADR 0036).

Open questions: what API the material and order programs expose locally and whether they ship with
FurniCraft; where the workshop profile lives; whether reference retrieval (catalogues, norms, past
jobs) is needed and in what form; voice input and output.

The assistant adds `furnicraft-agent` between `engine` and `ui`, a model adapter and resource
adapters at the composition boundary, and a project store for save and load. They enter the crate map
only when the crates exist.

## Crate map

| Layer | Crate | Responsibility | Depends on |
|---|---|---|---|
| L0 | `furnicraft-units` | `Length` in micrometres and `Degrees` in millidegrees, thickness helpers, parsing and formatting in mm and degrees | serde |
| L0 | `furnicraft-geom` | `glam` re-exports, `Frame`, indexed `MeshData`; bounds and planes as features need them | glam, serde |
| L1 | `furnicraft-kernel` | Geometry kernel **port**: `Profile`, opaque `BodyId`, extrusion (linear sweep), booleans, tessellation, measures. No third-party kernel types | geom, thiserror |
| L1 | `furnicraft-model` | Document data: `Project`, `Container`, `Object`, `Geometry`, `Properties`, `FurnType`, `Placement`, `Panel`, `Material`, `Hardware`, `Id`, `Revision`; invariants and typed errors | units, geom, serde |
| L2 | `furnicraft-kernel-truck` | Geometry kernel **adapter** implemented with `truck` (Apache-2.0). The only crate allowed to depend on `truck` | kernel, geom, truck-modeling, truck-shapeops, truck-meshalgo |
| L2 | `furnicraft-param` | Compiles document features into kernel operations: panel features, placements, panel machining | model, kernel, geom, units |
| L2 | `furnicraft-project` | Scene file envelope: format identity and version, read, atomic write, preserved unknown sections | model, serde, serde_json, thiserror |
| L3 | `furnicraft-mesh` | Evaluation cache: `(Revision, PartId)` to solid and triangle mesh; tessellation policy | model, param, kernel, geom, units |
| L4 | `furnicraft-scene` | Renderer-agnostic frame description: drawables, world transforms, materials, selection highlight, camera, lights, grid | model, mesh, geom |
| L5 | `furnicraft-render` | `wgpu` renderer: pipelines, buffers, depth, camera. No `egui`, no window creation | scene, geom, wgpu, bytemuck |
| L6 | `furnicraft-engine` | `Session`: document, catalogue, derived panel diagnostics, selection, view state (viewports/cameras), undo/redo, dirty state; command registry | model, param, mesh, units |
| L7 | `furnicraft-ui` | `egui` panels and viewport widgets: scene tree, inspector, input, gizmos; dispatches commands | engine, model, egui |
| L8 | `apps/furnicraft` | Composition root: `eframe` window, input, `egui` ↔ `wgpu` paint callback, constructs `TruckKernel` and evaluates the sample document | ui, engine, render, scene, model, param, mesh, kernel, kernel-truck, geom, units, eframe |
| dev | `furnicraft-testkit` | Fixtures and helpers for tests | model, units |
| tool | `xtask` | Workspace checks (`cargo xtask layers`) | serde_json |

```text
apps/furnicraft        composition root (window, input, egui <-> wgpu, TruckKernel)
└── ui                 egui panels, viewport, commands by id
    └── engine         session: selection, view state, undo/redo
        ├── mesh       evaluation cache: revision -> mesh
        ├── param      document features -> kernel calls
        ├── model      document data
        └── kernel     geometry port (no third-party types)
            └── geom   math primitives

kernel-truck (adapter) ── kernel + truck   (only place truck is allowed)
render (wgpu) ────────── scene (what to draw) ── mesh, geom, model
```

A crate may depend only on crates in lower layers. `kernel-truck` is the only crate that may depend on `truck`; `model`, `param`, `engine` and `ui` see only the `kernel` port. `engine` never sees `wgpu` or `egui`; `ui` never writes to the document.

`truck-shapeops` is vendored in `vendor/truck-shapeops` with one Cargo.toml change: its `truck-meshalgo` dependency is narrowed to `analyzers` and `tessellation`, so the VTK I/O stack and its C compression backend are not part of FurniCraft. The Rust source has only a lint allowance; see `vendor/truck-shapeops/PATCH.md` and `ATTRIBUTION.md`.

## Milestones

| Milestone | Goal | Done when |
|---|---|---|
| M0 | Foundation | Workspace builds, documents exist, `cargo xtask layers` passes, no application logic |
| M1 | Model space and document skeleton | Z-up and floor origin, `Placement`, panel local frame (corner; X length, Y width, Z thickness), A/B faces and stable counter-clockwise corner/edge ids, node tree, `Id`, `Project`, serde round-trip, transform composition tests |
| M2 | Geometry kernel boundary | `kernel` port and `kernel-truck` adapter; extrusion (linear sweep), boolean, tessellation, measure, panic guard; spike "panel + groove + chamfer" with analytic volumes; curved sweeps arrive with panel machining |
| M3 | Viewport | `wgpu` window via the app shell: shaded primitives, orbit/pan/zoom, floor grid, resize handling; visual check |
| M4 | Panel geometry | Panel outline, thickness, rectangular through and blind cutouts with an entry face, holes generated by hardware, chamfers/roundovers, shaped-cutter sweeps and edge banding, with volume and tessellation tests |
| M5 | Editing | `egui` shell, command registry, undo/redo, scene tree with atomic panels, components and groups, create/place/select, closed-container viewport picking with an open mode, Blender-like axes and orientation widget, move/rotate, multi-node placement as one undo entry |
| M6 | Properties and classification | Every node carries typed properties; `Assembly`/`Group` become one container with a component marker; a numeric `FurnType` registry and prefix queries select objects by type; reports derive from queries |
| M7 | Objects and the scene file | Geometry objects with a baked mesh and with a feature history, mesh editing gated to the mesh payload, `object.collapse`, camera and light objects, and a versioned `.fcs` scene file with a migration and a golden file per schema |
| M8 | Hardware and joinery | Hardware catalogue, anchor points, generated holes, collision warnings |
| M9 | Output | Cutting lists per material and sheet, panel drawings, CNC export |
| M10 | Assistant | Faber runs as a client of the command registry: disabled by default, read-only first, one undo step per task, a whole task runnable by a scripted model without a network, and a headless job over a saved project |

## Open model questions (M1)

These are deliberately unresolved. Do not pick an answer inside an unrelated task; design them together.

- **Named views.** Cameras are document objects (ADR 0036); whether named views return as bookmarks over them, or are dropped, is open.
- **Selection.** In v1, can the user select objects and faces only, or edges too? How are faces identified after a rebuild? Mesh element references are indices valid within one revision (ADR 0037).
- **Hardware anchoring.** Does hardware attach to a panel face and a coordinate, to an existing machined feature (a hole), or to an edge? How are multi-panel fittings (hinges, runners) expressed? Generated holes themselves are always stored in core-local coordinates.
- **Derived versus edited.** Which panels are produced by a prototype script, which are hand-placed, and what happens when a parameter changes under a hand-edited panel?
- **Reusable assemblies.** v1 uses unique tree nodes. Whether a later version adds shared definitions and instances is still open.
- **Grain.** How is grain direction stored and how does it constrain rotation, cutting and edge banding?
- **Dimensions.** Product parameters are typed properties read by a user prototype script. Whether a property can be an expression of other properties is deferred with the script runtime.
- **Property schema.** User properties are free-form until a schema/catalogue exists; decide whether the catalogue is per project or per workshop, and how a script declares the properties it writes.
- **User scripts.** Product prototypes are scripts, not built-in builders. The sandbox, the language, the unit boundary and the failure policy (missing script/DB is a warning, never a load failure) are undecided.
- **Invariants.** Panel-owned cutouts outside the core and overlapping cutouts are command errors in v1; loading a scene keeps machining that no longer fits as a problem panel instead of rejecting the file. ADR 0009 replaces these v1 rules in stage 3 with a union of through cutouts, boundary notches and a connected-core invariant; the union layer is the in-house `geom::region` module (ADR 0012). Panel larger than the sheet and overlapping hardware-generated holes remain open.
- **Persistent naming.** Features reference their inputs only (ADR 0037); fillet and chamfer on a face generated by an earlier feature wait for a naming design.
- **Scene file.** The envelope and its versions are specified in [`docs/domain/scene-file.md`](docs/domain/scene-file.md); the binary form, autosave and asset storage remain open.
