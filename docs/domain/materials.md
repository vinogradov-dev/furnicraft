# Materials and catalogue

Status: domain specification. This document describes material records and catalogue rules. It will grow into the structure of the material database. It does not describe how the scene file stores material references; see [Scene document](document.md).

Related documents:

- [Panels and edge banding](panels.md) — panel, its dimensions, banding and commands.
- [Scene document](document.md) — scene file, loading, storage and diagnostics.

## 1. Terms

- **Material** — a catalogue record with a stable `MaterialId` and a set of properties.
- **Panel material** — the material a panel body is made from. It defines the nominal thickness first of all, as well as colour/decor and other properties.
- **Edge-banding material** — a ready-made banding material. It defines the banding profile: thickness, width, as well as colour/decor and other properties.
- **Catalogue** — an external collection of materials. Currently a mock file; later a database.
- **`MaterialId`** — a stable identifier of a catalogue record. Panels and bandings reference materials by it.

## 2. Common material model

A material has:

- a stable `MaterialId`;
- a name;
- a kind/category (for example chipboard, MDF, solid wood, ABS, PVC);
- colour/decor;
- additional properties.

The property set is extensible. In the first version these are typed `PanelMaterial` and `EdgeBandingMaterial` records; in a future database this may become a common material table plus a property table. A panel must not know which additional properties a material has.

Common rules:

- `MaterialId` is stable and is never reused;
- an id is unique within the catalogue;
- physical dimensions are `Length` (micrometres), never bare `f64`/`f32` millimetres;
- a material does not store references to panels; links go from the panel to the material;
- the catalogue is not part of the scene file.

## 3. Panel material

Required properties:

- `nominal_thickness` — the nominal thickness of the material. It defines panel thickness;
- name;
- colour/decor.

Possible future properties:

- sheet/board size;
- grain direction and availability;
- grade, coating, manufacturer, article number;
- machining and edge-banding restrictions;
- price, stock and similar commercial data.

A panel stores only a `MaterialId` and the last known thickness as a fallback. The user does not edit panel thickness: it comes from the material. When the material is found, the catalogue is the source of truth.

## 4. Edge-banding material

Required properties:

- `thickness` — the banding profile thickness in the panel plane, outward from the edge;
- `width` — the banding profile width across the panel thickness;
- name;
- colour/decor.

Possible future properties:

- base material (ABS, PVC, melamine, veneer and so on);
- adhesive layer;
- profile radii/chamfers;
- texture and grain direction;
- manufacturer, article number, roll length.

Banding thickness is not panel thickness. If `width` is greater than the panel thickness, the banding is trimmed flush during installation. If it is smaller, it covers the edge only partially. The geometry details are in [Panels and edge banding](panels.md).

## 5. Catalogue

1. The catalogue is not stored in the scene file.
2. The application reads the catalogue from a separate mock file at startup and/or when loading a scene.
3. The scene stores only stable `MaterialId` values and fallback values.
4. The catalogue is read-only for the duration of a session.
5. If the catalogue is re-read, the scene is rebuilt from scratch.
6. Changing a property under the same `MaterialId` intentionally affects panels on the next load: for example, changing `nominal_thickness` changes the thickness of every panel that uses that material.
7. Removing a `MaterialId` from the catalogue does not delete panels. The panel gets a "material missing" diagnostic and uses the last known thickness; see [Scene document](document.md).
8. The catalogue does not substitute a missing material with a similar one and does not look up a replacement by name.

### 5.1. Mock file format

The format is serialisable `Catalog` data: a list of panel materials and a list of edge-banding materials. The exact field names are settled during implementation; what matters is that ids are stable and dimensions are explicit.

Draft example:

```json
{
  "panel_materials": [
    {
      "id": 1,
      "name": "Chipboard 18 mm, oak",
      "kind": "chipboard",
      "nominal_thickness": "18mm",
      "color": "#b98a5a"
    }
  ],
  "edge_materials": [
    {
      "id": 101,
      "name": "ABS 2 mm, oak",
      "kind": "abs",
      "thickness": "2mm",
      "width": "22mm",
      "color": "#7a4f2a"
    }
  ]
}
```

### 5.2. Catalogue validation

- ids are non-zero and unique;
- names are not empty;
- `nominal_thickness` is positive;
- banding thickness and width are positive;
- dimensions do not exceed the supported `Length` limits;
- unknown required fields are a readable error, not a silent default.

## 6. Missing material

If a panel material is missing:

- the panel is not deleted or reset;
- the last known thickness is used;
- the panel is marked as a problem panel and drawn in a neutral colour;
- the card shows `<no material id %d>` or a similar marker instead of the material;
- if the material appears again on the next load, the panel recovers.

If an edge-banding material is missing:

- the last known banding profile is used;
- the panel is marked as a problem panel;
- if there is no fallback profile, the banding is not drawn and the panel remains a problem panel.

The exact loading order and fallback storage are in [Scene document](document.md).

## 7. Catalogue API

At minimum:

- `panel_material(id) -> Option<&PanelMaterial>`;
- `edge_material(id) -> Option<&EdgeBandingMaterial>`;
- a material list for the UI selector;
- uniqueness validation on load.

Commands and parametric evaluation must not know where the catalogue came from: a mock file or a future database.

## 8. Tests

At minimum:

- loading a catalogue with unique ids;
- rejection of a duplicate or zero id;
- rejection of non-positive thickness or width;
- lookup of a panel material and an edge-banding material;
- changing a material thickness under the same id changes panel thickness on rebuild;
- removing a material produces a diagnostic instead of a crash;
- the catalogue never appears in a serialised scene.

## 9. Open questions for the future database

1. One material table with common properties, or separate tables for panels and banding.
2. How to store an extensible property set: typed columns, a JSON field or a property–value table.
3. Material versioning: edit a record under the same id or create a new version.
4. Whether `MaterialId` may be reused after deletion. The proposal is no.
5. Units and dimension precision in the database.
6. Localisation of names and descriptions.
7. Textures: file path, atlas, hash, embedded resources.
8. Sheets, nesting, grain direction and stock accounting.
9. Catalogue import/export and merging with an existing catalogue.
10. Who edits the catalogue: a separate database editor only, or also the application.
