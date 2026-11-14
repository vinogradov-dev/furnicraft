# 0109. A niche is bounded by the faces the pointer sees

- Status: Accepted; supersedes the coverage clause of 0106
- Date: 2026-11-14

## Context

0106 made the free space a convex cell of wall planes and refused a cell whose wall did not cover a
whole face of it. The owner reports what that does to ordinary joinery: a case whose top covers only
part of the frame — an open front, a top that stops before the front edge — is refused with «the empty
space here is not one straight shape», and two turned boards that do not span the envelope are refused
for the same reason. A pointer that lands on a turned product can also answer «point at the empty space
inside the open product» while it is visibly over the piece.

The shell explains the second failure. It intersected the view ray with the **world-axis box around**
the product's envelope, and handed the niche that entry point. A product turned off the world axes has
a world box larger than the product, so the entry can lie outside the envelope, where the niche rightly
answers `Outside`. The same code ignored what the pointer was actually on: a ray that crossed an open
front and landed on the back wall was measured at the envelope's front face, not at the wall.

## Decision

**The measured point is the face the pointer points at.** The view ray enters the product's own authored
envelope, transformed through the product's placement rather than read as its world bounds. From a
micrometre inside that entry the query probes along the same ray; the first surface it meets gives the
face, and the measured point rests a micrometre on the free side of it. A ray that meets only the
envelope's far face keeps the entry point: the whole reserved space is the niche.

**A face's plane bounds the cell; its whole extent does not have to be covered.** The coverage test of
0106 is removed. A wall that stops halfway, a top that covers the back two thirds of a frame and a
board that does not reach the envelope's front are walls where they stand, and the void under and
beside them is fillable. The half-space of every met face is already what keeps a part from cutting
into it; a part that extends past a board's edge into open space is preferable to refusing the void.
`holds_body` stays: a surface standing inside the cell still makes the free space non-convex and is
refused.

**Shape always answers.** When the wall planes cannot all be met by one flat end face — two walls
leaning against each other, or a wall nearly level with the board — the shaped part takes the section
the whole slab leaves, with no end bevel, which is the same cut the largest fit reads. It is the shape
of the niche with a sliver at one wall instead of a refusal.

## Consequences

- An open front with a short top, a case of turned boards that stop at their own edges, and a product
  turned in the world all offer a niche again. The splayed and box cases answer exactly as before.
- A part may stick out past the visible edge of a board where the envelope continues; the niche is
  the intersection of the planes the pointer can see, not the exact connected free component. A slab
  cross-section that subtracts each surface's real footprint is the next stage if that matters.
- The half-wall and wedge refusals are gone. The two N9 tests that asserted them now assert the niche
  the face still bounds, and the shell's «walls lean against each other» and «narrows across the
  board's thickness» sentences are no longer reachable from a click.
- The tool stays armed after a click, and the next ghost is measured from the face under the pointer,
  which may now be the part just created. The ghost the click wrote is the one that was on screen
  before the write.
- `docs/domain/products.md` §7 states the pointer rule and the new fallback.

## Related

- [0103](0103-one-probe-serves-the-niche-the-distances-and-the-move.md) — the probe this rule reads.
- [0106](0106-a-niche-is-a-convex-cell-of-wall-planes.md) — the cell, whose coverage clause this
  supersedes.
- [0108](0108-a-shaped-fill-bevels-its-ends-onto-the-walls.md) — the end bevel and its refusals.
- [`docs/domain/products.md`](../domain/products.md) — the fill in prose.
