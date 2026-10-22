# PROJECT.md — FurniCraft

## Vision

FurniCraft is a native, free and open-source 3D editor for parametric cabinet furniture. The user describes a product the way a furniture maker thinks about it — overall dimensions, panels, materials, edge banding and hardware — and the program builds an accurate 3D model that later becomes drawings, cutting lists and CNC programs. It is built for the craft, not for the cloud: a fast Rust binary with `wgpu` graphics and `egui` UI, with interaction quality close to Blender, and no web technologies anywhere in the product.

## Scope

**In scope, in order of arrival:**

1. A document model for cabinet furniture: model space and placements, assemblies, panels, machined features, edge banding, materials, hardware, dimensions.
2. Parametric evaluation: named dimensions drive part sizes; the parts list is derived, not drawn by hand.
3. 3D viewport: camera views, orbit/pan/zoom, shading, selection and picking, a floor grid, dimensions.
4. Editing: create and edit panels, assemblies, materials and hardware through commands, with undo/redo.
5. Output: cutting lists per material and sheet, panel drawings, and eventually CNC programs.

**Not in scope:** web or browser builds, cloud storage, collaboration, real-time rendering for presentation, free-form organic modelling, and import of third-party CAD formats at this stage.

## Key data model

The model is intentionally described at the level of vocabulary and responsibilities. Exact fields, coordinate conventions and invariants are designed in M1 — see "Open model questions" below. Do not infer a schema from these descriptions.

### Document (`Project`)

The root of a saved file: the assembly tree, the material catalogue, the dimension set, document-level settings (default units, grain conventions) and a schema version. Serialized with `serde`; must keep older files loading. The first M1 skeleton stores the schema version, revision and root assembly; catalogues and settings follow.

### `Id` and `Revision`

Stable identifiers (`Id`) let hardware, commands and the undo stack refer to a panel or an assembly without holding a pointer. A monotonically increasing `Revision` marks every document change; caches (meshes, picking structures, derived parts) are keyed by it. Both are technical plumbing, not user-visible entities. In v1 an `Id` is a non-zero `u64` serialized as a number, and a `Revision` is a `u64` starting at zero. Panel vertices and edges use stable ids of the same kind; their visible numbers are labels, not keys.

### `Assembly`

A node in the product tree: it groups panels, hardware and nested assemblies, and owns the dimensions that describe it. An assembly is both "the product" and "the sub-assembly" depending on where it sits, so nesting is part of the model rather than a special case. In v1 every tree node is unique and owns its children; shared definitions and instances are deferred.

### `Placement`

A rigid transform relative to the parent frame: translation and rotation. Scale and shear are not part of a placement. Placements compose down the assembly tree to produce world transforms. The document stores placements; it never stores a camera or a viewport. In v1 the rotation is a unit quaternion; non-finite or non-unit values are rejected on load instead of being normalised silently.

### `Panel`

One flat part: length, width and thickness, the material it is cut from, its grain direction, the placement that puts it in its parent assembly, edge banding on each of its edges, and the list of machined features on it. Panels are the atoms of cutting lists, drawings and CNC output, so they carry their own manufacturing data. The first M1 panel record stores its id, name, size and placement; material, grain, edge banding and machining follow.

The panel's local frame has its origin at the **minimum corner** (smallest X and Y, `z = 0`), `+X` along the **length**, `+Y` along the **width**, and `+Z` along the **thickness**; the panel occupies `z` from `0` to its thickness. This convention is fixed for the geometry kernel and the panel features.

Panel dimensions are positive. The v1 validator accepts a length and width up to 10,000 mm and a thickness up to 1,000 mm; zero, negative and larger values are errors, not warnings.

The two large faces are **A** (at `z = thickness`) and **B** (at `z = 0`); `+Z` points from B to A. Viewed from A, outline vertices are numbered **1..n counter-clockwise**, starting at the panel zero: for a rectangle, corner 1 is `(0, 0)`, 2 is `(length, 0)`, 3 is `(length, width)`, 4 is `(0, width)`. Edge `i` runs from corner `i` to corner `i+1`, wrapping at `n`; for a rectangle, edge 1 is the bottom, 2 the right, 3 the top, 4 the left. Edge and vertex ids are stable and never reused; their numbers are human labels. The labels are local to the panel and do not depend on world orientation or on how the panel is used.

### `Machining` (inside `Panel`)

A manufacturing feature on a panel: a drilled hole (diameter, depth, entry face), a groove/dado, a cutout, a rebate or an angled end cut. Features are owned by the panel they belong to and addressed from outside as `(panel_id, machining_id)`. All machining is stored in **panel-local coordinates relative to the panel zero**, whether it belongs to the panel or is generated by hardware. A blind hole or an edge hole names its entry face: A, B, or a stable edge id. Resizing a panel changes its dimensions but does not move or scale existing machining; a feature outside the new outline is a validation error. Hardware that needs an edge-relative position recomputes its holes in panel-local coordinates.

### `EdgeBanding` (inside `Panel`)

The banding applied to one edge: material, thickness, and any overhang/trim allowance. The edge is referenced by its stable id. Corners where two banded edges meet are a modelling question, not a detail of the renderer.

### `Hardware`

A physical fitting: hinge, drawer runner, connector, shelf pin, handle, and so on. It has a type, a placement tied to one or more panels (a face and a position, or a reference to a machined feature such as a hole), and a quantity/orientation. Hardware must be able to generate the machining it requires, without duplicating hole definitions by hand.

### `Dimension`

A named, typed value (a `Length`, a count, an angle or an enum choice) that describes an assembly: overall width, height and depth, board thickness, shelf count, drawer front gap. Dimensions are the parametric input; panel sizes and positions are computed from them by `furnicraft-param`. Whether dimensions may be expressions of other dimensions is an M1 decision.

### `Material`

A catalogue entry: kind (chipboard, MDF, solid wood, plywood, HPL), nominal thickness, sheet size, decor/colour and grain availability. Panels reference materials by `Id`; changing a material's thickness must not silently rewrite already-sized panels.

## Architectural principles

- **Data is separate from rendering.** `model` is serializable data; meshes and scene descriptions are derived in `mesh` and `scene`; only `render` touches `wgpu`. No GPU type ever appears in the document.
- **Space is data; views are state.** The document stores placements and the assembly tree. Cameras, viewports, grids, shading and selection are session or UI state.
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
- Every assembly and part has a local frame. A `Placement` is a rigid transform (translation and rotation; no scale or shear) relative to the parent frame.
- A node's world transform is the composition of placements from the root.
- Imported assets are normalised on import: Z-up, scale baked, origin at the mounting point. Placements stay rigid.
- A panel's local frame is fixed: origin at the minimum corner (smallest X and Y, `z = 0`), `+X` length, `+Y` width, `+Z` thickness; the panel occupies `z` from `0` to its thickness. Faces are A (`z = thickness`) and B (`z = 0`); vertices run 1..n counter-clockwise from the zero corner and edge `i` joins vertex `i` to `i+1`.

### Views

- The document stores placements and, optionally, named views (camera bookmarks).
- Camera, projection, grid, shading, selection and hover are session/view state; they are never document data.
- A viewport is a UI adapter: it owns a camera, turns input into camera moves or commands, and draws a scene. Several viewports may show the same document.
- Picking maps a screen position to a document `Id` (object, face, later edge); selection lives in the session.
- Manipulation (move/rotate, gizmos, snapping) is a command; the UI never writes a placement directly.
- Drawing views (projections, sections, details) are output, not interactive viewports, and arrive with the output milestone.

## Crate map

| Layer | Crate | Responsibility | Depends on |
|---|---|---|---|
| L0 | `furnicraft-units` | `Length` in micrometres, thickness helpers, parsing and formatting in mm | serde |
| L0 | `furnicraft-geom` | `glam` re-exports, `Frame`, indexed `MeshData`; bounds and planes as features need them | glam, serde |
| L1 | `furnicraft-kernel` | Geometry kernel **port**: `Profile`, opaque `BodyId`, extrusion (linear sweep), booleans, tessellation, measures. No third-party kernel types | geom, thiserror |
| L1 | `furnicraft-model` | Document data: `Project`, `Assembly`, `Placement`, `Panel`, `Material`, `Hardware`, `Dimension`, `Id`, `Revision`; invariants and typed errors | units, geom, serde |
| L2 | `furnicraft-kernel-truck` | Geometry kernel **adapter** implemented with `truck` (Apache-2.0). The only crate allowed to depend on `truck` | kernel, geom, truck-modeling, truck-shapeops, truck-meshalgo |
| L2 | `furnicraft-param` | Compiles document features into kernel operations: dimensions, placements, panel machining | model, kernel, geom, units |
| L3 | `furnicraft-mesh` | Evaluation cache: `(Revision, PartId)` to solid and triangle mesh; tessellation policy | model, param, kernel, geom, units |
| L4 | `furnicraft-scene` | Renderer-agnostic frame description: drawables, world transforms, materials, selection highlight, camera, lights, grid | model, mesh, geom |
| L5 | `furnicraft-render` | `wgpu` renderer: pipelines, buffers, depth, id-buffer picking, camera. No `egui`, no window creation | scene, geom, wgpu, bytemuck |
| L6 | `furnicraft-engine` | `Session`: document, selection, view state (viewports/cameras), undo/redo, dirty state; command registry | model, param, mesh, kernel |
| L7 | `furnicraft-ui` | `egui` panels and viewport widgets: outliner, inspector, input, gizmos; dispatches commands | engine, model, egui |
| L8 | `apps/furnicraft` | Composition root: `eframe` window, input, `egui` ↔ `wgpu` paint callback, constructs `TruckKernel` | ui, engine, render, scene, model, kernel, kernel-truck, eframe |
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
| M1 | Model space and document skeleton | Z-up and floor origin, `Placement`, panel local frame (corner; X length, Y width, Z thickness), A/B faces and stable counter-clockwise corner/edge ids, assembly tree, `Id`, `Project`, serde round-trip, transform composition tests |
| M2 | Geometry kernel boundary | `kernel` port and `kernel-truck` adapter; extrusion (linear sweep), boolean, tessellation, measure, panic guard; spike "panel + groove + chamfer" with analytic volumes; curved sweeps arrive with panel machining |
| M3 | Viewport | `wgpu` window via the app shell: shaded primitives, orbit/pan/zoom, floor grid, resize handling; visual check |
| M4 | Panel geometry | Panel outline, thickness, through cuts, pockets, holes, chamfers/roundovers, shaped-cutter sweeps and edge banding, with volume and tessellation tests |
| M5 | Editing | `egui` shell, command registry, undo/redo, picking/selection, outliner and inspector, move/rotate |
| M6 | Parametric assemblies | Dimensions drive panel sizes and placements; parts list derived; validation errors surfaced in the UI |
| M7 | Hardware and joinery | Hardware catalogue, anchor points, generated holes, collision warnings |
| M8 | Output | Cutting lists per material and sheet, panel drawings, CNC export |

## Open model questions (M1)

These are deliberately unresolved. Do not pick an answer inside an unrelated task; design them together.

- **View state.** What, if anything, is persisted in the document (only named views?) and what lives in the session?
- **Selection.** In v1, can the user select objects and faces only, or edges too? How are faces identified after a rebuild?
- **Hardware anchoring.** Does hardware attach to a panel face and a coordinate, to an existing machined feature (a hole), or to an edge? How are multi-panel fittings (hinges, runners) expressed? Generated holes themselves are always stored in panel-local coordinates.
- **Derived versus edited.** Which panels are fully derived from dimensions, which are hand-placed, and what happens when a dimension changes under a hand-edited panel?
- **Reusable assemblies.** v1 uses unique tree nodes. Whether a later version adds shared definitions and instances is still open.
- **Edge banding.** How are corners between two banded edges represented, and how does banding thickness affect nominal panel size (finished versus cut size)?
- **Grain.** How is grain direction stored and how does it constrain rotation, cutting and edge banding?
- **Dimensions.** Literal values only, or expressions depending on other dimensions? If expressions, when are they evaluated and how are cycles reported?
- **Invariants.** Zero or negative thickness, machining outside panel bounds, panel larger than the sheet, overlapping holes: which are errors, which are warnings?
- **Schema.** Versioning strategy and migrations for the document format.
