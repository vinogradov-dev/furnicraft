# 0095. The order package is a published snapshot, not the internal database

- Status: Accepted
- Date: 2026-11-12

## Context

Reports, drawings and CNC programs will be written by people who are not us, in whatever language
they prefer — Python being the likely first. K3 solved that by exporting its own database: workable
while the schema held still, and a trap afterwards, because every report then depended on the internal
tables and broke whenever the program changed them. A report is a promise to the workshop; it cannot
be coupled to our internal representation.

Two facts about our own data force the shape of the answer. The document is not self-contained: a
panel stores a material `id` and a fallback thickness, while names, thicknesses and banding profiles
live in the session catalogue (`crates/model/src/catalog.rs`). And most of what a report needs is
derivation, not data: the measured size with banding, the area, the banding length per side, the
product a part belongs to. A consumer that re-derives those numbers will eventually disagree with
the program — the worst class of error on a shop floor.

## Decision

**The export is a one-way, versioned snapshot: an order package.** The scene file (`.fcs`) stays the
only import format; there is no package reader in FurniCraft. A package is a directory
`<name>.fcpkg/` with `manifest.json` as its entry point, or a single JSON file with the same content
embedded.

**Tables are JSON Lines, one record per line.** A line is a JSON object, so no quoting dialect,
encoding or decimal-comma rule can differ between readers; any language reads it in a few lines.
CSV copies are generated from the same column table for spreadsheets and people, and a test asserts
the two agree.

**The tables are flat and relational.** `products`, `nodes`, `parts`, `facades`, `fittings`,
`machining`, `materials`, `classes`, `properties`; each row carries its keys, so a consumer loads
them into pandas, a spreadsheet or any database it owns, and groups by `product_id` for a per-cabinet
report. We ship a portable dataset, not a database engine.

**Numbers are integers with declared units.** Lengths in micrometres, angles in millidegrees, areas
in square millimetres, money in minor units when it arrives. No float crosses the contract. The unit
names live in `manifest.json`; formatted `mm` and human spans appear only in the CSV convenience
copies, never in the machine tables.

**The package is self-contained.** It embeds the catalogue rows the order actually uses — the
materials and banding profiles — and the class rows it references, so a reader resolves a thickness,
a banding profile and a set of rules without our catalogue file. The scene file keeps ids only, by
design.

**Derivation belongs to us.** Measured size with banding, area, banding length per side and in total,
the owning `product_id`, the nesting flag, the purchase channel: all computed by the exporter with
the same accessors the program uses, never by the consumer. The consumer is free to disagree with a
rule, but then it re-derives deliberately, not by accident.

**The boundary is explicit.** Nesting, drawings and CNC programs stay with the consumer. The package
hands it the inputs — part sizes, grain, material, banding, machining records — and the declared
rules (kerf, trim) when they exist. We do not smuggle a nesting strategy into a data format.

**The contract is versioned like an API.** `version` is major; within a major, columns are only
appended, never renamed, removed or re-united, and readers ignore unknown fields. One golden package
per major, and a test that one revision produces byte-identical output; the timestamp lives only in
the manifest and `--no-timestamp` drops it so a golden file stays stable.

**No scripting runtime, no embedded database.** A Python or Lua interpreter inside the product is
forbidden by the project rules and would be a second mutation path; SQLite would drag C sources in.
External readers read files. Anything we want in-app later is ours to implement in Rust.

## Consequences

- A report written today keeps working across our releases inside one major version, and the release
  that breaks it says so in the major number.
- The class registry (0094) becomes part of the published contract: a reader branches on
  `supply`/`measure`/`nesting`, not on our codes, so a worktop lands in linear purchase and never in
  nesting in any report that honours the table.
- Because the catalogue is external today, two different catalogues produce two different packages
  from one scene. The snapshot makes the package reproducible; whether the `.fcs` itself should embed
  its materials is a separate decision, not taken here.
- Sheet sizes and prices are not in the catalogue yet, so the first package version carries areas and
  stock groups, not per-sheet plans or money. The nesting and purchase views grow when the catalogue
  does.
- The exporter must be reachable without a window, which also serves the headless job the assistant
  milestone needs.
- Every column is a maintenance promise: adding one is additive and cheap, removing one is a major
  bump and a new ADR.

## Related

- [0094](0094-a-class-registry-gives-the-machine-its-semantics.md) — traits travel with the package.
- [0005](0005-material-catalogue.md) — the catalogue is external and session-scoped, which is why the
  package embeds its rows.
- [`docs/domain/order-export.md`](../domain/order-export.md) — the table and column contract.
