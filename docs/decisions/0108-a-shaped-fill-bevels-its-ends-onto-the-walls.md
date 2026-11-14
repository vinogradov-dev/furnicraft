# 0108. A shaped fill bevels its ends onto the walls, and turns the board when it must

- Status: Accepted
- Date: 2026-11-14

## Context

An end bevel is an angle about an outline edge and it only removes material: the stored outline is
the edge on face `B`, and face `A` is set back by `thickness * tan(angle)` (ADR 0107). A shaped fill
therefore has to answer two questions the section alone does not: which large face the outline
belongs to, and what the walls' angles are, in the board's own frame.

Take a wall with free-side unit normal `n` in the board's frame. Face `B` (`z = 0`) answers
`n_xy · xy >= offset`, face `A` (`z = t`) answers `n_xy · xy >= offset - n_z t`. So a wall with
`n_z < 0` is tighter at `A`: the `B` section is the wide one, a positive bevel sets `A` back into the
wall's plane exactly, and the cut-only board fits. A wall with `n_z > 0` is the mirror image: the
*wide* face is `A`, and no cut-only board can fill the void from a `B` outline.

Two cells defeat a single flat board whatever its ends do. Walls that lean opposite ways — one
narrowing the board at `A`, another widening it there — have no face that is the wide one. And a
wall nearer the board's own plane than to a side face (a ceiling that comes down at ten degrees, a
floor that rises) asks for an end face almost parallel to the large faces: that is a taper across
the board, which is a variable-thickness panel and not this feature.

## Decision

**Shape mode reads the section at the wide face, bevels every edge whose wall leans, and turns the
board when the wide face is the other side.**

- The plan reads the sign of every wall's `n_z` in the board's own frame. All non-zero signs
  negative: the board stands as it is and its outline is the section at `B`. All positive: the board
  is turned **half a turn about its own length axis**, which swaps `B` and `A` and swaps the
  board's `+Y` and `+Z`. That is a placement of the same stance, not a fourth stance: the length
  axis, the edge numbering and the panel's own frame are untouched (ADR 0105).
- Each outline edge gets the bevel of the wall it lies on, `beta = atan(|n_z| / |n_xy|)` in
  millidegrees, which is the angle whose setback `t * tan(beta)` is exactly the wall's own inset
  across the thickness. A wall perpendicular to the board (`n_z = 0`) gets nothing, and neither does
  an angle that rounds to zero millidegrees — a square end is the absence of a record, not a zero
  angle.
- A beveled board is **a contour with bevel records**, whatever its polygon looks like: the
  rectangle generator has no place for an end bevel, so the classifier of N10 only serves the
  square cases. A shape whose walls are all plumb is unchanged.
- **Two refusals, one sentence each.** Walls whose signs disagree are `WallsDisagree`: no face is
  the wide one, so no flat board meets them all. A wall whose bevel would pass a mitre — 45°, where
  the wall is nearer the board's plane than to a side — is `Wedge`: the end face it asks for is a
  taper across the large face, and a plane level with the board inside the slab is the same wedge
  with no edge at all. `Fit` remains the answer in both cases, because a smaller rectangle never
  has to meet a wall.
- **The plan carries the far face.** The bevels and the far ring are part of the plan, so the ghost
  draws the rim the finished panel ends at and the click writes the same records: `panel.create`
  gains an optional bevel list beside its outline, and one click is still one command and one undo
  entry.

## Consequences

A shelf or a post in a void bounded by boards turned in space now meets those boards with its end
face in their plane: the failure the owner described — a square end cutting in, or `Fit` leaving a
wedge of air — is gone, and no new stance or new node kind was needed. The cost is a booleans step
per beveled edge in the panel plan, which N11 already pays, and one more thing the plan answers.

The refusal is the price of the construction: half of the leaning cases turn the board, and the
other half — a cell whose walls lean against each other, or a ceiling nearly level with the board —
keep only `Fit`. Those voids are what a variable-thickness panel or a curved-section niche would
answer; neither exists yet.

## References

- ADR [0107](0107-an-end-bevel-is-an-angle-about-its-edge.md) — the record and the cutter this
  decision fills in.
- ADR [0105](0105-a-panel-stance-is-the-flat-frame-turned.md) — the stance the turn belongs to.
- ADR [0106](0106-a-niche-is-a-convex-cell-of-wall-planes.md) — the cell and its cut.
- [Filling a niche](../domain/products.md) — the maker's flow and the two modes.
