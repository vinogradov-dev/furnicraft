# 0005. The material catalogue is external and session-scoped

- Status: Accepted
- Date: 2026-10-23

## Context

Panels and edge banding reference materials by a stable id. The catalogue is shared between scenes,
changes independently of a scene, and may later move from a mock JSON file to a database. Storing it
inside `Project` would make every scene carry a copy, make save/load ambiguous when the catalogue
changes, and couple document data to file I/O. The document still has to survive when a material
disappears.

## Decision

`furnicraft-model` defines `MaterialId`, `PanelMaterial`, `EdgeBandingMaterial` and `Catalog` as plain
serializable data. `MaterialId` is a non-zero `u64` newtype, separate from the document `Id` and not
allocated from `Project`'s id space; ids are stable within the catalogue and never reused.

The catalogue is not part of `Project`. The application reads it from the mock JSON file and hands an
immutable `Catalog` to the session; `engine` uses it for validation and resolution. `model` performs
no file I/O, and `param`/`mesh` never read the catalogue: `engine` resolves material values into the
geometry input.

Panels store `Option<MaterialId>`: `None` means no material was assigned, while `Some(id)` that the
catalogue cannot resolve is a missing material. Banding stores a required `MaterialId`. The document
keeps fallback thickness/width values and never deletes a panel or a band because a catalogue entry
disappeared.

For v1 the catalogue is immutable for the session. Re-reading it means constructing a new session and
rebuilding the scene; hot reload and its undo semantics are out of scope.

## Consequences

- No new crate: the catalogue types live in `model`, the loader in the shell.
- A scene file contains material ids and fallbacks, never the catalogue itself.
- Missing materials are diagnostics, not load failures.
- The mock file and a future database can implement the same lookup surface.
- A catalogue change is not undoable; it invalidates the session instead.
- `MaterialId` cannot be reused after deletion, because old scenes must not silently point at a new
  material with the same number.
