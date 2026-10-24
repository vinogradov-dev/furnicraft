# 0015. Band joints extend and trim in application order

- Status: Accepted
- Date: 2026-10-24

## Context

Bands are built per contour segment. At a sharp joint, a band whose outer face sits closer to the
panel than its neighbour's leaves a step: a non-included edge band protrudes outward while an
included chamfer band stops on the nominal outline, so the chamfer band hangs short of the edge band.
The owner's rule is that the last band in the application order is visible on top.

## Decision

Band strips are built in `band_order`. At a sharp joint between a later band and an earlier band:

- If the later band's outward extent is smaller than the earlier band's, the later strip is extended
  along its own boundary lines to the earlier band's outer face and cut there. For an included band
  next to a non-included edge band this closes the hanging step.
- Overlaps are resolved by subtracting a later band from every earlier band. The subtraction uses a
  slightly taller tool (`0.001 mm` overshoot on both faces) so it does not depend on coplanar faces.
  If the kernel finds no solid, the earlier band is rebuilt untrimmed instead of dropping the build.
- Only straight line segments are extended and intersected for now. Arc and radius joints keep their
  butt joints until line-arc and arc-arc intersections are designed.
- The nominal segment endpoints still bound each strip; the extension is capped at `50 mm` so hostile
  shallow geometry cannot produce an unbounded sliver.

## Consequences

- The reported chamfer-plus-protruding-edge-band gap closes, and the later band is visible over the
  earlier one on straight joints.
- The core outline still connects per-segment offsets with straight connectors; the band extension is
  independent of that connector.
- Truck booleans on thin coplanar bands are fragile. The overshoot and the rebuild fallback keep the
  build alive, but the fallback leaves the earlier band untrimmed.
- Dragging to reorder bandings will reuse the same order and trimming path.
