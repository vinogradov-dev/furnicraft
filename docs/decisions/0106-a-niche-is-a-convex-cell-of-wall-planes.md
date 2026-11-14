# 0106. A niche is a convex cell of wall planes

- Status: Accepted; refines 0099 and 0103
- Date: 2026-11-14

## Context

0099 promised a niche measured by probes, and 0103 gave it a query: four side rays along the product's
own axes cut with the section plane through the cursor's height, extruded from the two vertical probe
distances. That is exact only while every wall stands plumb and the floor and ceiling lie flat.

The owner reported the case it misses. A niche bounded by panels turned out of the vertical gives no
size and no shape: a back wall leaned 15 degrees answers `Unrepresentable` at every cursor height, a
ceiling leaned 10 degrees is measured at the cursor alone — so a shelf cut to it would drive into the
board behind — and a product turned 45 degrees is refused outright, although the section is already
taken in the product's own frame. A cabinet whose sides lean is ordinary joinery, not an edge case.

The query already has what it needs. A probe reports the **face** it stops at: the true plane of a
turned board, not the world box around it (0102). Free space is the intersection of the half-spaces
behind those planes, and an intersection of half-spaces is a convex cell.

## Decision

**The niche is the convex cell of the planes that bound it**, built in the product's own frame. It
starts from the envelope's six planes; each round probes from the point along every current face
normal and every current corner — the envelope's twenty-six axis, edge and corner directions in the
first round — clips the cell by each met face's plane, and repeats until a round adds no plane. The
rounds, the planes, the directions and the corners are capped, and hitting a cap refuses instead of
spinning.

**Two checks decide whether the free space is one cell.**

- Every bounding face must still be a surface over its whole extent: the coverage probe 0099 already
  ran at the void's floor and ceiling now runs at each corner of that face — moved just inside it —
  and at its middle. A board that stops halfway leaves the plane empty above its end and is refused.
- No other surface's true corner may lie strictly inside the cell. A void that holds a post or a shelf
  is not convex; the cell would swallow the obstacle, and a part cut to the cell would cut into it.
  Refusing is honest until the query can answer several cells.

**The query reports the cell, not the box that happens to be easy.** `Niche` carries its supporting
planes (unit free-side normal and offset) and its corners, in the product's frame; the box around them
stays for the callouts, and `NicheShape` says whether that box is the free space, whether one section
describes it from floor to ceiling, or whether the section changes along the height. `Niche::cut`
answers the polygon a board of a given thickness takes: the cell cut by the board's slab with every
plane's half-space evaluated at the slab's own two ends. The `Turned` refusal is deleted — a turned
product is measured in its own frame like any other.

**The fill keeps fitting boards.** A shelf takes the cut over its thickness as a rectangle or a
contour; a post takes the same cut in the panel's own board plane, which is the product's ZY against
the panel's local XY, so a post stands in a splayed case with the depth the slab really has. Box cases
answer exactly as before; a case whose walls are not plumb now answers too.

**A curved boundary is measured by the box around its body.** That box encloses the curve, so the cell
is conservative: a part fits with a gap and never cuts in. An exact curved section needs a surface
query that can meet an arc, and is not this decision.

## Consequences

- A leaned wall, a ceiling that comes down towards the back and a product turned off the world axes
  all answer: the cell is the truth, the box is only what the callouts print.
- A shelf under a sloped ceiling gets shallower where the ceiling comes down rather than being refused
  or cutting in — the part is measured at the face that could violate the wall.
- A post no longer refuses a splayed case; it takes the depth the void leaves at its own thickness.
- The cost is a walk of the trees per probe and more probes per measurement: forty-odd in the first
  round against six before. It is paid when the pointer moves half a millimetre, not per frame.
- A void with a notch, two boards meeting halfway, or a part standing inside it is refused with a
  sentence rather than approximated. Splitting a non-convex void into several cells is the next stage
  and needs a query that can return more than one region.
- A frame created over another piece's loose parts now refuses its own void, because that void really
  does hold them. The sample scene's fixture and the frame placement rule had to say so.

## Related

- [0099](0099-a-niche-is-the-space-a-frame-reserves.md) — the niche as the space a frame reserves.
- [0102](0102-a-furniture-entity-is-a-surface.md) — the true face of a turned board a probe meets.
- [0103](0103-one-probe-serves-the-niche-the-distances-and-the-move.md) — the probe this cell is built
  from.
- [0105](0105-a-panel-stance-is-the-flat-frame-turned.md) — the stance a post stands in, and the panel
  plane its cut is taken in.
- [`docs/domain/products.md`](../domain/products.md) — the niche and the fill in prose.
