# 0102. A furniture entity is a surface; a frame is a shell

- Status: Accepted; amends the probe and collision clauses of 0099 and 0097
- Date: 2026-11-13

## Context

The owner's rule for the niche probe is one sentence: the ray stops at the first surface of a
furniture entity it meets and does not pass through it. That matches the niche definition he gave in
the same breath — the space bounded by the envelope's faces **and by panels and other furniture
products** — and it contradicts what the first version of the niche record said: that probes pass
through another product's empty interior and stop only at its parts.

The contradiction matters twice. It decides what a niche is when a piece stands inside another, and it
decides whether two products may share space at all. The move tool cannot answer the second question
differently from the probe, or the program would let a maker place a piece where the tool then refuses
to measure a niche.

## Decision

**Every furniture entity presents a surface.** A panel has faces, a hardware body has its own, and a
frame has six virtual faces. A probe stops at the first surface along its ray — whichever entity owns
it — and never passes through. This is what makes a niche bounded by "габариты каркаса и/или панели и
другие мебельные изделия" true in the program and not only in the sentence.

**A frame is a shell, not a solid.** Its envelope bounds the space the piece reserves; inside, the
space is empty until parts arrive. From that follow the two rules that were previously one and wrong:

- **bodies do not intersect bodies** — two panels, a panel and a hinge, two products' parts;
- **shells do not cross shells** — no product may be half inside another's envelope, because its walls
  would cut the reserved space;
- **a shell may contain another shell** — a wall cabinet in a run, a shelf unit in a niche, a piece
  under a worktop. Containment is legal; crossing is not.

**Movement obeys the same two rules.** The furniture move tool stops a body at contact with another
body and a shell at contact with another shell; it does not stop when one envelope merely sits inside
another. Dragging a product through the wall of a neighbouring envelope stops at the wall. This
replaces the earlier clause that collision considered parts only.

**A niche is measured against the same surfaces.** A product standing inside another's reserved space
therefore reduces the niche of the outer piece, exactly as a panel would: the ray stops at the inner
product's shell. Nothing is subtracted, stored or reserved — the next query simply sees a different
room.

## Consequences

- One concept — the furniture surface — answers three questions that used to have three answers: what
  bounds a niche, what stops a probe, and what stops a drag.
- The niche of a piece that holds another piece is smaller than its envelope; a report or a drawing
  that wants the free volume asks the same query and cannot disagree with the tool.
- A product is never "inside" another in the tree (0097); it is only inside its space, and the geometry
  of that space is derived. The parts list of the outer piece still does not collect the inner piece's
  parts, because the tree owns that answer.
- Two shells may touch face to face; the mover stops at contact rather than at a small gap, and the
  1 mm grid gives the maker the tiny clearance a real assembly needs.
- The niche query must be tested with three configurations: a shelf splitting a case, a piece inside
  another's space, and two pieces side by side with their shells touching.

## Related

- [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) — the envelope as a shell.
- [0099](0099-a-niche-is-the-space-a-frame-reserves.md) — the niche, whose probe and collision clauses
  this record corrects.
- [0100](0100-a-frame-is-created-opened-and-drawn.md) — the frame's drawing states.
- [`docs/domain/products.md`](../domain/products.md) — the specification.
