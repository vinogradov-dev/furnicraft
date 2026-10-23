# 0012. In-house panel cutout union

- Status: Accepted
- Date: 2026-10-23

## Context

ADR 0009 stage 3 requires that all **through** cutouts on a panel form one removed region `K`, that the
core is `C \ K` and that the core stays one connected component with positive area. This is a small
2D region task: union of the cutouts and subtraction from the core outline `C`. ADR 0010 explicitly
leaves the choice of the 2D union/boolean layer to stage 3; ADR 0009 lists the candidates: an external
2D boolean crate, an extension of the kernel adapter, or an in-house module.

## Decision

`furnicraft-geom` gains an in-house `region` module: a small arrangement-based boolean layer that
performs the 2D union of the through cutouts and the subtraction `C \ K`. It operates on the same
`Contour` primitives as the rest of the contour model.

- A rectangle is the only cutout shape today, so the first version unions rectangles against the core
  rectangle (or a shaped core outline) exactly. Shaped cutout shapes arrive with later machining
  stages; the module is built so they extend it without changing callers.
- The result of `region::union` and `region::difference` is a set of closed regions, each described by
  an outer boundary and an optional list of hole boundaries. Coordinates stay `f64` millimetres at
  this geometry boundary, exactly like the existing `Contour` and `Profile` values.
- The implementation is owned by the domain: overlapping cutouts merge, a cutout crossing the outline
  becomes an open notch, and the "connected core" invariant is checked by counting the region
  components. No third-party geometry or boolean dependency is added.

## Consequences

- `model`, `param` and `engine` keep using `Contour`/`Profile`, and no new crate or dependency
  changes the crate graph.
- ADR 0010's postponed "2D union/boolean layer" question is resolved in the direction that record
  already prefers: our own analytic contours. A future curved-room extrusion still goes through the
  kernel port; the region module stops at the 2D contour layer.
- The boolean result is derived data owned by `geom`; `model` never stores it. Stage 4 banding on
  cutout edges reuses the same union layer.