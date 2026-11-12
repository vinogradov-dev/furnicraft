# Order package

Status: domain specification. This document is the published contract between FurniCraft and the
programs that read an order: reports, drawings, cutting lists, CNC. It defines the package layout,
its tables and columns, the number rules, what the package resolves for the reader, and how the
contract is versioned.

Related decisions: [0095](../decisions/0095-the-order-package-is-a-published-snapshot.md),
[0094](../decisions/0094-a-class-registry-gives-the-machine-its-semantics.md),
[0005](../decisions/0005-material-catalogue.md).

## 1. What a package is

A one-way snapshot of one order, written to be read by software we do not control. It is **not** a
second document format: FurniCraft never reads a package back, and the scene file (`.fcs`) remains the
only import format. It is also **not** a dump of our internal structures — that is what makes reports
break when a program changes. A package is a contract: it promises a small, boring, stable set of
flat tables, and every change to them follows §7.

Two facts shape it:

- the document is not self-contained — a panel stores a material `id`, while names, thicknesses and
  banding profiles live in the catalogue — so the package **embeds the catalogue rows the order
  uses** and is self-contained;
- most of what a report needs is derivation (measured size with banding, area, banding length, the
  product a part belongs to), so the package **contains those numbers already resolved**. A reader
  that re-derives them will eventually disagree with the program.

## 2. Layout

```text
cabinet-2026-11-12.fcpkg/
  manifest.json      the entry point: format, version, units, tables, their columns and keys
  products.jsonl
  nodes.jsonl
  parts.jsonl
  banding.jsonl
  facades.jsonl
  fittings.jsonl
  machining.jsonl
  materials.jsonl
  classes.jsonl
  properties.jsonl
  parts.csv  …       convenience copies for spreadsheets; not the contract
```

`--format single` writes the same content as one JSON file with the tables embedded, for sending by
mail. Both forms carry the same manifest.

A table is **JSON Lines**: one record per line, one JSON object per record, UTF-8, LF, no BOM. That
choice removes every dialect question a CSV has — quoting, separators, decimal comma, encoding — and
any language reads it in a few lines. The CSV copies exist for people and spreadsheets; a test asserts
they carry exactly the columns their JSONL table declares.

## 3. Manifest

```json
{
  "format": "furnicraft.order",
  "version": 1,
  "app": "0.1.0",
  "exported_at": "2026-11-12T21:40:00Z",
  "document": { "file": "cabinet.fcs", "revision": 41, "hash": "3f9c…" },
  "units": { "length": "um", "angle": "mdeg", "area": "mm2", "rotation_scale": 1000000000 },
  "counts": { "products": 3, "parts": 57, "…": 0 },
  "tables": [
    {
      "name": "parts",
      "file": "parts.jsonl",
      "key": "node_id",
      "columns": [
        { "name": "node_id", "type": "integer" },
        { "name": "length_um", "type": "integer", "unit": "um" }
      ]
    }
  ]
}
```

The manifest describes its own tables, so a reader needs no document of ours to identify a column. A
column entry names its type (`integer`, `string`, `boolean`) and its unit where one applies.

## 4. Numbers

- Lengths: integer **micrometres**.
- Angles: integer **millidegrees**.
- Areas: integer **square millimetres**.
- Money (when a price exists): integer **minor units**, with the currency in the manifest.
- Rotations: four integer quaternion components scaled by `rotation_scale` (1e9), because a float in
  the contract would let a reader and the program disagree in the last bits. `units.length` and the
  scale are declared once in the manifest.
- No floats anywhere in the machine tables. Formatted values (`720.0`, `мм`) exist only in the CSV
  copies and in `name`/`label` columns meant for people.

## 5. Tables

Every table is flat, carries its keys and is ordered by its key, so two exports of one revision are
byte-identical.

### `products`

| Column | Meaning |
|---|---|
| `node_id` | the product node |
| `parent_id` | its parent, for a set of products inside a set |
| `name` | the user's name |
| `class` | the class code, e.g. `30.01.03` |
| `level` | `Product` |
| `width_um`, `height_um`, `depth_um` | the overall bounds of the product |
| `part_count` | nested parts that belong to it |
| `sheet_area_mm2` | total sheet area of those parts |
| `banding_um` | total banding length |
| `fitting_count` | fittings mounted in it |

### `nodes`

The tree, for drawings and for anything a flat table does not answer:
`node_id`, `parent_id`, `name`, `class`, `level`, `path` (the names from the root, `/`-separated),
`x_um`, `y_um`, `z_um`, `qw`, `qx`, `qy`, `qz` (the local placement, rotation scaled as in §4).

### `parts`

One row per part that is cut, with every number a cutting list or a parts report needs:
`node_id`, `product_id` (the nearest `Product` ancestor — this is what makes a per-cabinet report a
`GROUP BY`), `class`, `material_id`, `thickness_um`, `length_um`, `width_um`,
`measured_length_um`, `measured_width_um` (with banding), `area_mm2`, `grain` (`none`, `length`,
`width`), `nesting` (`member` or `never`, from the class), `banding_total_um`.

### `banding`

One row per banded unit, because banding belongs to a contour segment and a contour part has more than
four edges: `node_id`, `unit` (edge index or segment id), `material_id`, `thickness_um`, `width_um`,
`included_in_size` (whether the banding was added to the nominal size).

### `facades`

`node_id`, `product_id`, `class`, `material_id`, `length_um`, `width_um`, `thickness_um`,
`supplier_group` (classes and materials that are ordered together), `glass` (boolean),
`hinges` (count and mounting pattern summary).

### `fittings`

`node_id` (the fitting, or the container that holds a multi-part fitting), `host_id` (the object it is
mounted on), `product_id`, `class`, `article` (empty until the catalogue carries articles), `qty`.

### `machining`

`node_id`, `op` (`hole`, `groove`, `cutout`, `cup_bore`, `pattern`, …), `face` (`A`, `B`),
`x_um`, `y_um`, `diameter_um`, `depth_um`, `params` (a JSON object for the operation-specific
fields, so a new operation does not need a new table).

### `materials`

The catalogue rows the order uses — this is what makes the package self-contained:
`material_id`, `name`, `kind`, `thickness_um`, `color`, `stock_group`. Banding profiles carry
`width_um` as well.

### `classes`

The registry rows the order uses: `code`, `parent`, `slug`, `label`, `supply`, `measure`, `nesting`,
`extent`, `level`, `opening`, `operations`, `retired`. A reader decides "worktop goes to linear
purchase and never to nesting" from this table, not from our digits.

### `properties`

The user's own typed properties in long form: `node_id`, `key`, `kind`, `value` (as text for scalars,
as JSON for structured values). A custom report reads this table instead of asking us for a column.

## 6. What the package resolves, and what it does not

**Resolved by us:** measured sizes with banding, areas, banding length per unit and in total, the
owning product of every part, the nesting flag, the purchase channel and measure unit from the class,
material names and thicknesses, machining records in panel-local coordinates, report grouping.

**Left to the reader:** nesting layout, drawings and dimensioning, CNC toolpaths and post-processing.
The package hands over the inputs and, when they exist, the declared rules (kerf, trim, sheet sizes)
in the manifest — never a chosen strategy.

**Not in version 1:** per-sheet cutting plans and prices, because the catalogue does not carry sheet
sizes or prices yet. They are additive columns when it does.

## 7. Versioning

- `version` is a major number. A breaking change — removing, renaming, splitting or re-uniting a
  column, or changing a unit — is a new major and a new decision record. Adding a column at the end is
  not a breaking change.
- A reader must ignore unknown columns and unknown tables; this is part of the contract, not advice.
- One golden package per major lives in the tree; a test asserts that a fixed revision produces
  byte-identical output, with `--no-timestamp` for the golden and a timestamp only in the manifest of
  a real export.
- The class registry and the catalogue are append-only, so a package always resolves its own codes
  without asking which version of our program wrote it.

## 8. Reading it

Any language: parse `manifest.json`, then read one JSON object per line from each `*.jsonl`. In
Python, for example (documentation only — no Python is shipped or executed by FurniCraft):

```python
import json, pathlib
root = pathlib.Path("cabinet-2026-11-12.fcpkg")
parts = [json.loads(line) for line in (root / "parts.jsonl").read_text(encoding="utf-8").splitlines()]
by_cabinet = {}
for part in parts:
    by_cabinet.setdefault(part["product_id"], []).append(part)
```

## 9. Out of scope

An importer, a scripting runtime inside the application (a second mutation path, forbidden by the
project rules), an embedded database (`SQLite` would drag C sources in), and any column that exists
only to serve one consumer's report.
