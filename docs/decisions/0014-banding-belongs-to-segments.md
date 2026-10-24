# 0014. Banding belongs to one contour segment

- Status: Accepted
- Date: 2026-10-24

## Context

Version 1 grouped the nominal outline into tangent-continuous paths and treated a path as one
banding unit: setting banding on any edge copied the spec to every edge of its path and clearing one
edge cleared the whole path. A radius therefore forced its two adjacent edges into one spec, and a
shape change could make a path partially banded and invalid.

The owner's model is segment-first. Every end of the panel is a contour segment with its own id:
four straight edges, plus a corner segment when a corner becomes a chamfer or a radius. Each segment
carries its own operations, including its own banding. A chamfer creates a new segment and shortens
the two adjacent edges without touching their bandings. A corner without a treatment is a point and
has no segment, so it has no banding either.

## Decision

A banding belongs to exactly one contour segment.

- Edge and corner banding share one ordered `band_order` list. An entry names an `EdgeId` or a corner
  `VertexId`; a segment carries at most one banding.
- Adding a banding appends its segment to the order. Editing the material, width or inclusion keeps
  its position. Removing the banding removes its entry.
- The card lists the banded segments. The add control offers only existing segments that are not yet
  banded; removing a banding returns its segment to the list. The order will later be reorderable by
  dragging; the last entry is drawn on top.
- A corner segment exists only while the corner has a chamfer or radius. Removing the treatment
  deletes the corner banding with the segment.
- The core contour and the band strips are built per segment: an included segment is offset inward
  by its own thickness, a non-included segment extends outward. A tangent path remains a description
  of the contour, not a banding unit.
- Schema 4 adds `band_order`. A schema-3 file migrates with its edge bands in their stored order,
  followed by its corner bands.

## Consequences

- The card no longer shows four fixed edge rows and four fixed corner rows; it shows what is banded.
- Existing schema-3 files load unchanged and gain an order.
- Straight-joint trimming uses the order: the later band extends over the earlier one and is cut at
  its outer face. Dragging to reorder is a later UI step.
- An included band on one tangent segment steps the core at the joint with its neighbour. That is
  the per-segment model; if the step proves wrong in practice, revisit it before changing the core
  offsets.
