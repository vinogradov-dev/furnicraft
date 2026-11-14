# 0105. A panel stance is the flat frame turned, never mirrored

- Status: Accepted
- Date: 2026-11-14

## Context

A panel is a rectangle in its own frame: the zero at one corner, `+X` along the length, `+Y` along the
width, `+Z` through the thickness, faces `A` at `z = thickness` and `B` at `z = 0`, vertices 1..n
counter-clockwise seen from `A` (ADR 0004, 0009). The frame says nothing about where that corner is
once the board stands up, so every tool that stands one decides it again — and the ids it writes do
not care where the viewer stands: banding, corner treatments, bows and machining all address vertices,
edges and segments, so two tools that stand a post differently write the same panel twice with
different meanings.

The fill tool showed the cost. It stands a post by turning the flat panel a quarter-turn about `+Y`,
which is the owner's post stance turned a half-turn about the same axis (the table below): the
length runs downward, the zero corner — which is the origin, a corner of the bottom face — lands on the
niche's ceiling, and edge 1 becomes the top edge. Nothing is visibly broken until a band is ordered for
"the top" and arrives on the bottom.

The owner described the behaviour he expects as one chain of quarter-turns through the three positions
a panel takes in a cabinet: the back wall, the post and the shelf. A chain fixes the answer: if the
shelf turned one way is the post and the wall turned the other way is the post, then a half-turn or a
reflection is not the post, and which corner is vertex 1 follows instead of being a habit. His
reference for the behaviour is K3-Mebel, read as behaviour and not as a specification: the shelf is
the base, the post is the shelf turned to the right about `Y`, and the wall is the post turned about
`Z`.

## Decision

**A stance is the flat panel frame turned about a world axis, never mirrored, never another stance
plus a half-turn.** The flat stance is the shelf; the post is the shelf turned `−90°` about `+Y`; the
wall is the post turned `−90°` about `+Z`. Read in the direction the positions chain in speech, the
wall turned `+90°` about `+Z` is the post, and the post turned `+90°` about `+Y` is the shelf. The
turns are right-hand rotations about the positive world axis, that is counter-clockwise seen from that
axis' own `+` side.

**The base position is the shelf, because that is what a panel is.** A panel is a flat contour in its
own `XY` plane with face `B` at `z = 0`, and its thickness is the material extruded along `+Z`; the
shelf is that contour unturned, the post is it turned a quarter-turn to the right about `+Y` (contour
in `YZ`, material extruded towards `−X`), and the wall is the post turned a quarter-turn to the right
about `+Z` (contour in `XZ`, material towards `+Y`).

| Stance | Local `+X` (length) | Local `+Y` (width) | Local `+Z` (thickness) | Face `A` points | Vertex 1 |
|---|---|---|---|---|---|
| shelf (полка) | world `+X` | world `+Y` | world `+Z`, up | up | the minimum `x` and `y`, on the bottom face |
| post (стойка) | world `+Z`, up | world `+Y` | world `−X` | `−X` | the bottom-rear corner |
| wall (стенка) | world `+Z`, up | world `+X` | world `+Y` | front, `+Y` | the corner at minimum `x` and `z` |

**Vertex 1 stays the origin and the base corner.** In all three stances it is the panel's zero, a
corner of the bottom face, so a placement's translation is the bottom corner of the board and never its
ceiling. This is the same rule the panel frame already had (ADR 0083, 0088); the stance is what makes
"the base corner" mean a definite point in space.

**The numbering follows the frame, not the view.** Vertices still run counter-clockwise seen from face
`A`. In the front view — looking along `−Y`, so screen-right is world `−X` (ADR 0023) — a wall reads 1
right-bottom, 2 right-top, 3 left-top, 4 left-bottom; a post reads 1 bottom-rear and 2 top-rear on the
edge farther from the front viewer, then 3 and 4 on the front edge; a shelf reads 1 right-far and 2
left-far on its rear edge. `A` never means "up" or "front": it is the face the stance points outward.

**The stance is a placement, not document data.** A tool that stands a part composes the stance's
rotation with the position it measured and passes one `panel.create`. Nothing records which stance the
panel was born in, a later gizmo turn is free, and a left and a right side panel are one stance at two
placements — nothing is turned to make the second hand, and the first corner is the node's zero in
both.

**The finished face follows the hand, and the hand is derived.** A post's face `A` points towards
`−X`, so the post on the maker's right — the smaller `x` in the product's own frame, because the front
view's screen-right is world `−X` — shows `A` outward and its left-hand twin shows `B`. The ids mean
the same edge on either hand; a statement about a finished face names the hand, and the hand is read
from where the board stands in its product rather than stored.

## Consequences

- The fill tool's post is created as the stance, so its origin moves from the niche's ceiling to its
  floor and its edge 1 is the bottom edge. The ghost, the distance fields and the tests follow the same
  placement; no document field changes, so no schema moves.
- A node sits on its base face, so a post hangs one thickness into `−X` from its zero corner: a tool
  that keeps the board out of the negative side puts the node at `x = thickness`, while a shelf and a
  wall need no offset. A tool that stands a part therefore has to know the stance's extrusion
  direction instead of assuming its node is the low corner of the board — which is exactly the
  assumption that made the old post hang from the ceiling.
- A fill may turn a board half a turn about its own length axis, and that is still the same stance:
  the turn keeps the length axis and the numbering and swaps the two large faces, which is all the
  shaped fill needs when a wall makes face `A` the wide one. It is a placement, recorded nowhere
  (ADR 0108).
- Both hands of a post are the same board, and one of them has its finished face on `A` and the other
  on `B`: the right post (smaller `x`) is `A` outward, the left post is `B` outward. A parts list, a
  drawing or a CAM program that says "finish `A`" must therefore name the hand, and can derive it
  from the placement. No second stance, no mirror and no extra quarter-turn exists for the other
  hand, so the two posts stay interchangeable in the document and are distinguished only by where
  they stand.
- A creation tool has one rule to satisfy instead of one decision to reinvent: a back panel, a facade
  or a drawer front that arrives later derives its placement from the same table.
- Banding, corner treatments and machining written by id mean the same thing on a shelf, a post and a
  wall, which is the only reason a parts list can be read by a person who never saw the screen.
- A panel turned on its own — any angle a maker chooses with the gizmo — is not in a stance, and
  nothing forces it to be. The stances are a creation convention, not a document invariant, so the
  file format and the commands stay as they are.
- Cost: a tool that genuinely needs another position must justify it against this table rather than
  pick a rotation that looks right, and the corner order of an existing scene changes where a tool
  moves to the stance (nothing is released, so no compatibility path is needed — ADR 0096).
- Revisit if a position appears that is none of the three. A door swung open is a wall turned about
  its hinge, not a fourth stance.

## Related

- [0004](0004-panel-local-frame.md) — the panel frame, faces and topology ids.
- [0023](0023-fasad-furniture-and-standard-views.md) — the front is `+Y`, and what the screen's right
  is.
- [0083](0083-a-panel-is-anchored-at-its-base-corner.md),
  [0088](0088-origin-and-pivot-are-different-points.md) — the origin is the base corner.
- [0103](0103-one-probe-serves-the-niche-the-distances-and-the-move.md) — the fill and its distances.
- [`docs/domain/panels.md`](../domain/panels.md) — §3.1, the stances in prose.
