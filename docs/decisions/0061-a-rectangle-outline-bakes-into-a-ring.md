# 0061. A rectangle outline bakes into a ring

- Status: Accepted
- Date: 2026-11-06

## Context
A rectangular panel stores its outline as parameters: a nominal size, corner radii and chamfers, and
edge bows (`PanelOutline::Rect`, `crates/model/src/panel/mod.rs`). A contour panel stores a ring of
segments with stable ids. The contour edit mode works on a ring only, so *Edit contour* is offered
only for a panel that already has one.

A ring panel cannot carry `EdgeBanding` or `CornerBanding`: the loader refuses
`PanelError::ContourWithBanding`, because those records name rectangle edges and corners that a ring
does not have. A conversion must therefore carry the banding across in the same change, or refuse.

## Decision
`panel.outline.to_contour` bakes a rectangle outline into the ring it draws, in one revision and one
undo entry. The model call is `Panel::to_contour_ring`, which returns a `BakedOutline`:

- the document ring, built from `Panel::nominal_contour` and quantized back to whole micrometres;
- one `RingBanding` per edge or corner band, under the stable id of the nominal segment the band
  covered, keeping material, inclusion and application order;
- the banded ring segments in that order.

`SegmentSource` already names the owner of every nominal segment, so the mapping needs no new
coordinate scheme. A corner band whose corner has no chamfer or radius has no segment to move to;
the conversion is refused with `CornerBandWithoutSegment` instead of dropping the record. A panel
that already stores a ring is refused with `AlreadyContour`.

The geometry ring and the document ring differ in tolerance: geometry joins segments within a
micrometre, while the document compares rounded endpoints exactly. `document_ring` converts every
segment and snaps each joint in the document units, rotating the ring to begin on a line when it has
one; the panel is then anchored at the ring's minimum corner, and the placement, pivot and machining
move with it (ADR 0009).

## Consequences
The conversion is one-way. The corner and bow parameters are gone after the bake, but undo restores
the rectangle with its features and banding. Edges and corners that carried a band still do, now as
ring segments. Arcs survive the conversion within the one-micrometre document resolution; a ring
made only of arcs that cannot be closed at that resolution is refused rather than silently altered.
The card warns that corner and bow parameters are lost, and the command is enabled only when the
conversion validates.
