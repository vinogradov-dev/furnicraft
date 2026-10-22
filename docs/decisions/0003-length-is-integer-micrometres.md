# 0003. Lengths are integer micrometres

- Status: Accepted
- Date: 2026-10-22

## Context

Panel thickness (18 mm), gaps (0.5 mm), hardware offsets and sheet sizes live in the document, which is
serialized, compared and summed across an assembly tree. Binary floating point cannot represent 0.1 mm
exactly; the error accumulates into dimensions that change after a save and a load.

## Decision

`furnicraft_units::Length` is a signed `i64` count of micrometres, with checked arithmetic, a fallible
constructor from whole millimetres, and an explicit float conversion only at the geometry boundary.

## Consequences

- Document data is exact and round-trips through serde; equality and ordering are total.
- `i64` micrometres span ±9.2 × 10^12 m, far beyond furniture, so overflow is a validation error and
  not a design limit.
- Floating point exists only at the geometry and rendering boundary: `param` converts to millimetres
  for the kernel, `render` converts to scene units.
- Formatting for humans goes through `furnicraft-units`; parsing from text belongs there too, so no
  other crate formats or parses a length.
