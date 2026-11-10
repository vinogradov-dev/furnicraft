# 0082. The local zero is the creation anchor

- Status: Accepted
- Date: 2026-11-10

## Context

Creation stored the figure as it was drawn: a line placed at (600, 400) kept its vertices there, the
placement stayed at zero and the pivot went to the shape's centre (ADR 0058). Three defects followed
from that one choice. The card reported a translation of zero for a node that was visibly away from
the origin, and no field could move that node to its parent's origin. A drawing made inside a moved or
rotated container landed somewhere else, because the shell handed the work plane's *world* placement to
a command that stores a placement *relative to the parent*. And every content kind — drawing,
primitive, panel, group — decided its own frame in its own command, so the same question had four
answers.

A frame has to answer one question: which point of the content is the node's local zero? Once that is
answered, the placement is the parent-frame position of that point and the pivot is zero.

## Decision

- A node's frame is chosen once, at creation. `rebase_at_anchor` in `crates/model/src/frame.rs` is the
  only operation: the content's anchor becomes the local zero, the content moves by `-anchor`, the
  rotation is kept, the translation becomes the parent-frame position of the anchor, and the pivot
  starts at `Pivot::ZERO`. No world point moves.
- Three strategies, one rule, one small adapter per content kind (`Anchored`):
  - `CreationPivot` — an open chain's first point, a closed ring's bounds centre;
  - `BBoxBaseCorner` — a corner of the bounds, min-X/min-Y by default and a named `RectCorner` on
    request;
  - `CenterOfBounds` — the centre of a collection of children's union bounds, the anchor of a group.
- The adapter answers two questions — where the strategy's anchor is in the content's own frame, and
  what the content looks like moved by an offset of that frame — so a flat kind can refuse a non-zero
  Z offset instead of silently dropping it.
- Anchors and offsets are `[Length; 3]`, integer micrometres. A collection's bounds are measured in
  millimetres by the existing tree query and rounded once, at that boundary.
- `Placement::inverse` is the missing half of `Placement::compose`: a shell that knows a world
  placement hands the command `inverse(parent_world) ∘ world_placement`, exactly the parent-relative
  placement the document stores.
- Creation only. A later edit, resize, move or turn never recomputes the frame, and the pivot keeps
  the value creation gave it (ADR 0026). A panel is the one exception to "a closed body is anchored at
  its bounds centre" — it is anchored at its base corner, recorded with the panel step.

## Consequences

The drawing, primitive, panel and group steps call one function, and
`crates/model/src/project/frame_contract.rs` checks the invariants as one contract instead of one per
kind (S6). A command that creates a node no longer decides where its zero is,
so a new content kind cannot invent a fourth answer.

The cost is that a shell must convert a world placement into the parent's frame before it dispatches
a creation. That conversion is `Placement::inverse`, applied at the one place the shell owns, and a
command that receives it can be tested without a window.
