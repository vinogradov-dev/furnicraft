# 0043. Region booleans trace the boundary of the set

- Status: Accepted
- Date: 2026-11-03

## Context

ADR 0012 chose an in-house 2D boolean layer for the panel cutout union, but only implemented it for
rectangles: `subtract_rects` decomposes the core and the cutouts into disjoint rectangles and
`regions_from_rects` traces their edges back into rings. That layer cannot describe a curved cutout.
The contour model (ADR 0010) keeps lines and arcs exact, and a contour panel (ADR 0042) is a ring, so
the core trace has to answer the same question — does this material stay one positive component? —
for arbitrary rings.

The obvious alternative is to sample every ring into a polygon and use a polygon boolean. It would be
much less code, but it would move the area, the bounds and the containment of a result behind a
sampling tolerance, which the rest of the contour model deliberately avoids. A second alternative, a
third-party boolean crate, adds a dependency for a task the crate graph already owns.

## Decision

`geom::boolean` gains exact booleans over `Region` sets: `union_regions`, `intersect_regions` and
`subtract_regions`, while the ring model itself stays in `geom::ring`. Rings stay lines and arcs from
input to output.

- Every input edge is oriented with its own material on the left: an outer ring counter-clockwise, a
  hole clockwise. All edges of both operands are collected into one arrangement.
- Each edge is split at every point where it meets another edge — a crossing, a tangency or a joint
  of the other edge that falls inside it — and the split points are welded into shared vertices. That
  is what makes a T-junction and a coincident overlap one connected arrangement.
- A piece belongs to the result boundary exactly when the operation's membership differs on its two
  sides, probed at a fixed small offset from the piece's middle. The piece is oriented so the result
  material is on its left. Coincident edges, touching edges and edges covered on both sides need no
  case of their own: a side test decides all three, and two operands that contribute the same boundary
  are deduplicated.
- The kept pieces are chained into closed walks at the shared vertices, taking at each vertex the
  first edge clockwise from the reversal of the incoming one. A walk that visits a vertex twice is
  split into simple rings, so material that touches only at a corner stays two components.
- A ring with positive area is an outer boundary, a negative one is a hole, and a hole belongs to the
  smallest outer ring that contains it. That nesting is what makes an island inside a removed hole a
  region of its own.
- Consecutive pieces on one supporting line or circle are joined, so a split point another operand's
  joint needed does not survive as a collinear vertex.
- The work is bounded: more segment pairs than the module accepts is a typed error, never an
  unbounded loop.

## Consequences

- The through-cutout union and the core subtraction run on the same exact representation as the
  outline, so a rectangle, a ring, a notch and a curved hole are one code path.
- Area, bounds and containment of a result read lines and arcs; only sampling at the kernel boundary
  is lossy, exactly as before.
- A boolean result ring closes on a joint tolerance rather than the strict geometry one, because two
  pieces meeting at a welded vertex can miss each other by the welding tolerance. That bound is far
  below the micrometre a document coordinate carries.
- The rectangle helpers stay: the rectangle decomposition is still the simplest exact answer for
  rectangle-only callers and the existing tests keep it honest.
- The side probe assumes no other boundary passes within its offset of a piece's middle. A feature
  narrower than that offset is outside the document's resolution, so the assumption is recorded
  rather than guarded.
- The core is still the nominal rectangle for a rectangle panel and the bounds rectangle for a shaped
  or contour one; one outline source is the next step, and it can now hand a `Region` to this layer.

## Related

- [`crates/geom/src/boolean.rs`](../../crates/geom/src/boolean.rs) — the general boolean;
  [`crates/geom/src/ring.rs`](../../crates/geom/src/ring.rs) — `Region` and its rings;
  [`crates/geom/src/rect.rs`](../../crates/geom/src/rect.rs) — the exact orthogonal one.
- [`docs/domain/panels.md`](../domain/panels.md) — machining and the core invariant.
- [0010](0010-analytic-line-arc-contours.md) — why rings stay analytic.
- [0012](0012-in-panel-cutout-union.md) — the in-house boolean decision this completes.
- [0042](0042-a-contour-panel-keeps-its-ring-and-edits-a-working-copy.md) — the ring as an outline.
