# 0083. A panel is anchored at its base corner

- Status: Accepted
- Date: 2026-11-10

## Context

ADR 0082 made one rule for every creation: the content's anchor becomes the node's local zero, the
placement is the parent-frame position of that anchor, and the pivot starts at zero. For a closed
body the anchor is the bounds centre. A panel is different: it is a parametric entity, not a mesh.
The kernel draws a closed contour, extrudes the thickness along `+Z` and the document keeps the
nominal rectangle, the banding and the machining in the same panel-local frame. The centre of a
panel is not a point the user can name; its base corner is: the min-X/min-Y corner of its outline,
the corner every dimension and every machining coordinate is measured from.

`add_panel_contour` still subtracted the ring's minimum corner when it anchored the outline but left
the placement alone, so a panel made from a scene contour moved by that corner. The same choice also
decides how a new panel relates to the object whose contour it takes.

## Decision

- A panel is anchored with `BBoxBaseCorner`: its local zero is the min-X/min-Y corner of the
  outline, `pivot_local` is zero, and `translation` is the parent-frame position of that corner. The
  thickness grows from `z = 0`; the chosen corner is a `RectCorner` for a shape whose author names
  another one, and min-X/min-Y for a rectangle.
- `add_panel_contour` rebases the ring with `rebase_at_anchor(contour, placement,
  FrameAnchor::BBoxBaseCorner)`: the ring moves so the corner is zero and the placement gains the
  corner in the parent frame. No world point moves, and a ring that already starts at zero is
  unchanged apart from rounding.
- `panel.contour.create` places the panel exactly over its source: `inverse(parent_world) ∘
  source_world`, with the command's `placement` composed on top as a parent-frame offset. Identity
  means "exactly over the source" and the model then appends the ring's base corner in the source
  frame.
- Machining, banding and the nominal rectangle stay in the panel-local frame, whose origin is the
  base corner. A later outline edit re-anchors the frame and keeps the world shape (X30).

This is the owner's exception to "a closed body pivots at its bounds centre". It is not an
exception to "the local zero is the pivot": a panel's pivot is still zero, and because the local
zero is the base corner, the gizmo turns the panel about that corner.

## Consequences

A new rectangle panel is at its parent's origin with a zero pivot, and a panel made from a drawn
contour lands on that contour instead of moving by its corner. The panel frame answers the same
question as every other creation — which point of the content is the local zero — and the panel card
can read the corner back from the geometry instead of inventing a second handle.

The cost is that panel creation must know the corner in the parent frame, and that a contour panel
is not centred on its own bounds. A caller that has only a world frame converts it with
`Placement::inverse`; a caller that has a source object composes the source's world frame with the
target parent's inverse.
