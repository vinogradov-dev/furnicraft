# 0062. A large ring samples to fit its cap

- Status: Accepted
- Date: 2026-11-06

## Context
Every planar shape reaches the geometry kernel through one conversion: a document contour becomes an
analytic `Contour`, that becomes a `Region` whose rings stay exact, and the region is sampled into a
polygon-only `Profile` (`crates/param/src/profile.rs`). The chord tolerance was one fixed
`0.05 mm`.

The cap of a single extrusion is one document face, and that face is the sampled outer ring. The
document limits one face to `MAX_FACE_VERTICES = 1024` (`crates/model/src/mesh.rs`). A fixed chord
therefore ties the permitted shape to its radius: 350° of a 5 m radius is about 30 m of arc, which
samples to thousands of points, so a ring a user can legitimately draw was refused with "one
extrusion cap holds at most 1024". Before A6 that ring was stored and drew nothing; both outcomes are
wrong for a legal drawing (X2).

The count of a sampled ring grows with the number and size of its features, not with the drawing's
overall dimension. A chord chosen from the shape itself spends points where the shape needs them: a
small panel keeps the old tolerance exactly, and a large one gets a proportionally coarser chord that
is still invisible.

## Decision
The chord tolerance is the finer of a fixed floor and a fraction of the shape:

- `CONTOUR_SAMPLE_TOLERANCE_MM = 0.05` remains the floor, so nothing that already fit changes;
- `CONTOUR_TOLERANCE_FRACTION = 1e-4` of the region's bounding diagonal is the ceiling, so a chord
  never deviates by more than a tenth of a percent of the shape;
- `fit_tolerance` re-samples a ring that still overflows the cap, doubling the chord up to
  `MAX_TOLERANCE_RELAXATIONS = 6` times, and only then reports `ContourTooDetailed`.

The search is monotone: a ring that fits at the caller's chord is never re-sampled, and the first
chord that fits is the one used, not the coarsest. It lives inside `profile_of_region`, so bands,
panel cores, cutouts and constructive bodies all obey the same rule; `strip_points` applies it to the
two boundaries of a band strip, which share one chord so their arcs agree.

`object.extrude.create`, `object.contour.replace` and `object.collapse` keep validating through
`validate_extrusion_contour`. That call no longer refuses a large legal ring; it refuses only a shape
no chord within the budget can fit, which is a broken drawing rather than a big one.

## Consequences
A drawing is no longer refused for being large. The worst legal ring — 256 segments, all arcs — fits
within the budget; a hypothetical ring of thousands of arcs is still an error the user can read.

Resolution is now relative: a 10 m outline is sampled at about 1 mm instead of 0.05 mm. That deviation
is a ten-thousandth of the shape, far below the document's own micrometre grid at any furniture size
and below what a 4K viewport can show, so the visible result is unchanged.

The rule is one function with one test: the tolerance must grow with the shape, stay on its floor for
a panel, and converge for a ring that overflows. A future binary format or a larger face limit changes
`MAX_FACE_VERTICES` alone; this module reads it and follows.
