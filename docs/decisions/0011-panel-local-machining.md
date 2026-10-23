# 0011. Machining uses the nominal panel frame

- Status: Accepted
- Date: 2026-10-23

## Context

Version 2 stored cutouts relative to the core origin. When bottom or left edge banding was included
in the nominal size, the core origin moved inward and every cutout moved with it. The owner's model
is different: the first corner of the panel is the zero of its local frame, the original nominal
size is the body envelope, and trimming an edge or including a band never moves that frame. A panel
whose bottom is trimmed by 50 mm hangs 50 mm above the floor; its local zero stays on the floor.

## Decision

Every panel-owned machining feature is stored in nominal panel-local coordinates. `(0, 0)` is the
nominal minimum corner, `+X` is length, `+Y` is width and `+Z` is thickness. The core origin,
included banding and future edge trimming are derived state and never translate machining.

The schema moves from version 2 to version 3. Loading a version 2 document adds the stored fallback
core origin to every cutout, preserving its physical position. Loading a version 3 document uses the
stored coordinates directly.

## Consequences

- Toggling `included_in_size` on a bottom or left band does not move a cutout.
- A cutout may end up under an included band or outside the trimmed core; that is a diagnostic, not
  a silent coordinate change. The stage-3 union and notch rules decide whether the opening is valid.
- The CAM boundary subtracts the resolved core origin when it needs board-local coordinates.
- Version 2 files keep their holes in the same place after loading and are saved as version 3.
