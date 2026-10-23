# 0010. Analytic line and arc contours

- Status: Accepted
- Date: 2026-10-23

## Context

ADR 0009 stage 2 needs offsets for chamfers, radii and signed bows. The kernel port accepts polygon
profiles, so the first rectangle pipeline has no way to keep an arc's shape while offsetting it. The
stage-2 spike had to choose between extending the kernel port with curved profiles, adding a 2D path
library, or implementing the small set of operations the panel model actually uses.

The spike evaluated the three paths:

- **Curved kernel profiles.** Exact, but it changes the port, the truck adapter and every kernel test;
  offsetting mixed band paths would still be our problem.
- **A 2D path library.** A new dependency and a general boolean/offset engine for a model whose
  contours are a short chain of lines and circular arcs. It would also hide the exact arc parameters
  that the banding-path resolver needs.
- **Our own analytic contours.** Lines and circular arcs have closed-form offsets. The model only
  needs one-sided offsets of a tangent-continuous path. The result can be sampled for the polygon-only
  kernel profile.

No 2D boolean operation is needed for stage 2: a band is the strip between a path and its offset, and
the core is one closed outer contour. Stage 3 still needs a union of through cutouts and will decide
that layer separately.

## Decision

`furnicraft-geom::contour` owns planar `Line`, `Arc`, `Segment` and `Contour` values. An offset moves a
path to the left of its travel direction by a signed distance; for a counter-clockwise closed contour
that is the material side. Lines translate along their left normal. A circular arc keeps its center
and start angle and changes radius by the signed distance, with the sign taken from the sweep
direction.

A contour is accepted only when consecutive endpoints meet within `1e-6` mm. An offset requires every
internal joint to be tangent continuous within `1e-6` radians; a sharp joint is rejected instead of
being mitered. Closed contours must bound a non-zero area and must not self-intersect. Sampling is
bounded by a chord tolerance and by hard caps so hostile input cannot allocate without limit.

Arcs are sampled to polylines for `furnicraft_kernel::Profile` until a future kernel port carries
curved profiles. The document keeps the arc parameters; the sampled points are derived data.

## Consequences

- No new dependency and no change to the kernel port for stage 2.
- Offsets of lines and arcs are exact before sampling; the error of a rendered or tessellated panel is
  bounded by the sampling tolerance.
- Sharp joints are not offset yet. The rectangle pipeline keeps its explicit corner construction, and
  a mixed core contour with sharp joints needs a join step when stage 2 is wired in.
- Stage 3 needs a 2D union for through cutouts; ADR 0012 chooses an in-house `geom::region` module for
  it, adding no dependency and no kernel-port change.
- The tangent and connection tolerances are defaults in `furnicraft-geom`; if the domain spec changes
  them, they change in one place.
