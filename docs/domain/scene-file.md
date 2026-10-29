# Scene file

Status: design specification. This document defines the file that holds a scene: its envelope, its
versions, what it stores and what it deliberately omits, how it is written and read, and how it
relates to an order. Panel geometry, materials and the document model are described in their own
documents.

Related documents:

- [Scene document](document.md) — the document this file carries, its schema and migrations.
- [Geometry objects](geometry.md) — objects with a baked mesh or a feature history.
- [Materials and catalogue](materials.md) — the external catalogue the file does not store.

Related decisions: [0005](../decisions/0005-material-catalogue.md),
[0029](../decisions/0029-view-state-belongs-to-the-session.md),
[0031](../decisions/0031-external-resources-keep-their-own-system-of-record.md),
[0032](../decisions/0032-an-order-keeps-its-versions-as-folders.md),
[0036](../decisions/0036-cameras-and-lights-are-scene-objects.md) and
[0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md).

## 1. Terms

- **Scene** — everything the editor shows and saves: objects, camera objects, light objects, the
  active camera and scene settings. A scene does not know whether it is part of an order.
- **Scene file** — a saved scene, in the envelope described here.
- **Document** — the part of the scene that commands change, undo covers and `Revision` counts: the
  serialized `Project`.
- **Envelope** — the top-level framing that owns the format version and protects data it does not
  understand.
- **Format version** — the version of the envelope.
- **Schema** — the version of the document's own layout, carried inside the document.
- **Session** — runtime state of an open editor; it is not saved.

## 2. The file is an envelope around the document

A scene file is UTF-8 JSON with the extension `.fcs`:

```json
{
  "format": "furnicraft.scene",
  "format_version": 1,
  "document": {
    "schema": 11,
    "revision": 42,
    "next_id": 120,
    "root": {},
    "active_camera": 17
  }
}
```

- `format` is the constant `furnicraft.scene`. A file without it is not a scene file, and the reader
  says so instead of guessing.
- `format_version` versions the framing: the envelope keys, the document key and the top-level
  layout.
- `document` holds the document exactly as the model defines it. It is the only payload the file has;
  cameras and lights are nodes inside it, not a second section ([0036](../decisions/0036-cameras-and-lights-are-scene-objects.md)).
- The two versions are independent. A new geometry feature bumps `schema`; only a change to the
  framing bumps `format_version`.
- `furnicraft-project` implements this envelope and the atomic writer; check its goldens under
  `crates/project/tests/golden/` when a schema bump lands
  ([ADR 0038](../decisions/0038-the-scene-file-lives-in-its-own-crate.md)).

## 3. What the file stores

- the node tree: containers, panels, geometry objects, camera objects and light objects;
- each node's placement, stored pivot and typed properties, including its furniture type;
- panels: nominal size, material reference with `last_known_thickness`, banding with its fallbacks,
  machining;
- geometry objects: a baked mesh, or the ordered features of a constructive object
  ([0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md));
- camera and light parameters, and the active camera;
- scene settings: world ambient and, later, render settings;
- the document schema, revision and id allocator;
- the format version.

## 4. What the file never stores

- **anything about an order** — number, customer, version, status, prices, deadlines. The order
  program owns them ([0031](../decisions/0031-external-resources-keep-their-own-system-of-record.md),
  [0032](../decisions/0032-an-order-keeps-its-versions-as-folders.md)); a scene inside an order
  version folder is an ordinary scene file;
- the material catalogue ([0005](../decisions/0005-material-catalogue.md));
- derived geometry: the evaluated mesh of a constructive object, tessellation, bounds, caches;
- diagnostics and problem states;
- the undo and redo history;
- selection, hover, open containers, viewport navigation, grid, shading and projection;
- window layout, panel sizes, the list of recently opened files;
- credentials, endpoints, conversations and any assistant state.

## 5. Loading

1. Read the bytes and parse the envelope.
2. Reject a missing or different `format`, and a `format_version` newer than this build, with a
   readable error.
3. Migrate the document from its `schema` to the current one (section 7).
4. Validate the document: ids, placements, pivots, finite values, mesh and feature limits.
5. Resolve materials and banding against the catalogue; a missing material is a diagnostic, never a
   load failure ([document.md](document.md) §4–5).
6. Build the session: the document, its camera and light nodes, the built-in defaults for anything
   absent.
7. Build diagnostics and the scene.

Rules:

- a scene with no camera and no light loads normally and uses the built-in defaults;
- a camera or light node with an out-of-range parameter — a non-finite target, a zero field of view —
  loads with that parameter reset to the built-in default and a diagnostic; one bad lamp never makes
  the file unreadable;
- an unknown field inside `document` is a readable error, not a silent drop (`AGENTS.md` §4.5);
- unknown **top-level** sections are preserved verbatim and written back unchanged, so an older build
  does not destroy data a newer build added.

## 6. Saving

- Writing is atomic: the new content goes to `<name>.fcs.tmp` in the target directory, is flushed,
  and is renamed over `<name>.fcs`. A failure at any point leaves the previous file intact; the file
  is never truncated before the new content exists.
- Only the envelope and the document are written. Derived geometry is not recomputed into the file;
  `last_known_*` fallbacks are refreshed from the catalogue when a material resolves
  ([document.md](document.md) §8).
- Saving marks the session's saved content. The document is dirty when its user-visible content
  differs from that snapshot — the revision counter and the id allocator are ignored, so undoing back
  to the saved state clears the marker. Editing a viewport or a selection never makes it dirty.
- Save replaces the current file; Save As creates a new one and makes it current.
- Every failure — a missing directory, a permission, a full disk, a serialization error — reaches
  the user as a message with the path and the cause. No panic and no silent failure.

## 7. Versioning and migrations

- Bump `document.schema` when the document layout changes; bump `format_version` only when the
  framing changes.
- Every schema bump adds a migration from the previous version and a checked-in golden file that
  must keep loading. A bump that only adds a node kind or a payload variant still ships a golden
  file for the new version; the older goldens stay in the suite.
- A migration never guesses. A value that cannot be derived gets its documented default and a
  diagnostic; it is never inferred from geometry, a name or a catalogue entry.
- A file whose `schema` is newer than this build supports is a readable error — "saved by a newer
  version" — not a partial load.
- Unknown top-level sections survive a load and save cycle, so a newer build's extra data is not
  destroyed by an older one.

## 8. The scene and an order

- The order program manages the order: number, customer, status, and its versions as numbered
  folders ([0032](../decisions/0032-an-order-keeps-its-versions-as-folders.md)). It opens and saves
  scene files like any other program.
- A version folder contains one or more scene files plus the files produced for that version. The
  scene file itself has no order fields, so it can be copied, archived or opened without the order
  program.
- A standalone scene — a hardware model or a plain 3D object — is the same file with no camera and
  no light nodes. There is no second format and no scene "kind" field.
- Templates are application settings, not file data: "New scene" and "New hardware object" differ in
  the objects they create, and the saved result is an ordinary scene file.
- If a file ever needs to point back at its order, the link belongs to the order's index or a
  sidecar, never to the scene.

## 9. Tests

At minimum:

- every checked-in golden file loads, migrates and re-saves to the same document;
- a round trip of a scene with a container, a panel, a geometry object, a camera and a light;
- an unknown top-level section survives a load and save cycle;
- an unknown field inside `document` is a readable error;
- a newer `format_version` and a newer `schema` are readable errors;
- a saved file contains no catalogue, no derived mesh, no diagnostic and no undo history;
- a failed write leaves the previous file intact;
- a scene with no camera and no light loads and renders with the built-in defaults;
- saving and loading twice produces the same document and the same diagnostics.

## 10. Open questions

- Whether the file moves to a binary or compressed form once meshes make it large.
- Autosave and crash recovery: how often, where the copy lives, and how a recovered file is offered.
- Whether the envelope gains a `generator` field with the application version, for bug reports.
- Whether assets — textures and imported meshes — get a section with paths relative to the file, or
  stay outside the scene.
