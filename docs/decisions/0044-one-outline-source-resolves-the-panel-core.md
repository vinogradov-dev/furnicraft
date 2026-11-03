# 0044. One outline source resolves the panel core

- Status: Accepted
- Date: 2026-11-03

## Context

A panel reached its core in two different ways. A rectangle panel computed the core from the nominal
size and the included band thicknesses, and machining was checked against that rectangle. A shaped
or contour panel built its core ring from per-segment offsets, but the machining rules still read the
bounds rectangle of that ring: a through ring had to stay strictly inside it, and a shaped panel
rejected new cutouts until a "shaped-core union" existed.

ADR 0043 made the exact 2D boolean available for arbitrary rings. Keeping two descriptions of the
core after that means every later stage — the body, the cutout fit, the core trace, diagnostics —
has to ask which outline a panel was created with, and the answers drift apart. The rectangle and the
ring also had different notions of where the core starts: a rectangle carried an included-band origin,
a shaped panel always reported zero.

## Decision

The outline is the one source of the panel shape, and it resolves to one `geom::Region`.

- `PanelOutline::Rect` owns its corner features and bowed edges; they are the parameters of the
  rectangle contour generator, not separate panel fields. `PanelOutline::Contour` owns the stored
  ring. Both generate a nominal contour and then the core and overall rings through the same offset
  path.
- `PanelGeometry` stores the exact `core: Region` and `overall: Region` in the panel-local frame,
  plus the sizes derived from their bounds. `core_x` and `core_y` are the bounds origin; a rectangle
  with included low banding reports a positive origin, and a shaped panel may report a negative one
  when an outward bow carries material past the nominal corner.
- The panel body is the core region extruded at the world frame. There is no rectangle body branch.
- A cutout is one `Region` whether it is a rectangle or a ring. The through-fit rule is "shares
  positive area with the core", the blind rule is "lies inside the core, edge contact allowed", and
  the core trace subtracts the exact union from the core region. A through cutout may therefore touch
  or cross the core boundary and open a notch; the one-component trace decides whether the panel
  survives.
- A too-wide included band folds the core ring on itself. That is reported as an invalid contour, not
  as a non-positive rectangle, because the ring is the thing that no longer exists.

Schema 15 moves `corner_features` and `edge_bows` from the panel into the rectangle outline. A
version 14 panel loads its old top-level fields into the outline and writes them back inside it.

## Consequences

- Machining code, diagnostics and the parametric body no longer branch on the outline kind; one
  region-based rule set covers rectangle, shaped and contour panels.
- `PanelGeometry` is no longer `Copy`: a region owns two contours. Callers borrow it.
- A shaped panel may report a negative core origin. The card shows core and overall sizes from the
  region bounds, so the signed origin is not user-visible.
- The rectangle constructor `PanelGeometry::new` remains for callers that already know the two boxes
  and keeps its non-negative-origin contract; the document goes through `from_regions`.
- A contour cutout that crosses the core edge is now legal and opens a notch, where it used to be
  refused. A blind ring may still touch the edge but not cross it.
- The invalid-ring diagnostic (`InvalidOutline`) replaces the non-positive-core report for a band that
  destroys the core ring.

## Related

- [`crates/model/src/panel/geometry.rs`](../../crates/model/src/panel/geometry.rs) — `PanelGeometry`.
- [`crates/model/src/panel/contour.rs`](../../crates/model/src/panel/contour.rs) — the core and outer rings.
- [`crates/model/src/panel/machining.rs`](../../crates/model/src/panel/machining.rs) — the core trace.
- [`docs/domain/panels.md`](../domain/panels.md) — panel shape, machining and diagnostics.
- [0043](0043-region-booleans-trace-the-boundary-of-the-set.md) — the exact booleans this uses.
- [0042](0042-a-contour-panel-keeps-its-ring-and-edits-a-working-copy.md) — the ring as an outline.
