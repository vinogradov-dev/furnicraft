# 0077. A pending arc is the closing segment

- Status: Accepted
- Date: 2026-11-08

## Context
The polyline tool can mix straight runs and arcs, so a figure often ends on an arc: the user draws
two or three straight sides, presses Tab into arc mode, places the arc's midpoint — and then has only
the first node left to hit. The *Close* button exists for exactly that moment, but it refused:
`DrawDraft::close` returned `ArcInProgress` while an arc waited for its end, because the closing
segment was always drawn as a straight line and an arc had no place in it. The first node never lit
up either, since `can_close` demanded `pending_arc.is_none()`. The user had to guess that one more
click on the first point would finish the arc *and* close the ring, and the chrome told them the
button beside it would not work.

A pending arc is not a half-drawn thing the close has to wait for. It already has everything the
closing segment needs: its start is the chain's stored end, its end is the chain's first start, and
its midpoint is the point the user placed. Refusing it was the bug, not a missing feature.

## Decision
A pending arc is the closing segment of a polyline.

- `close()` builds the closing segment from the chain's first and last stored segments. With an arc
  waiting, it is `arc_through(last.end(), midpoint, first.start())`; otherwise it stays the straight
  tie that runs from the stored end to the first start. The old equality guard stays: when the two
  ends already meet within the drawing joint tolerance, no segment is added at all (ADR 0072).
- `DrawDraft::closing_segment` is the one function behind both `close` and `to_path`, so the ring
  that was validated and the ring that is committed are the same chain.
- `can_close` is `tool.closes() && !closed && segments.len() >= 2`. Two segments are the whole
  requirement, with or without an arc: a straight close adds the third, and a pending arc *is* the
  third. The ribbon's *Close*, the status bar's *Close* and the first-node highlight all read this
  one predicate, through `draft_can_close`, so a closure the command accepts is never greyed out.
- The first node turns green (`CLOSE_COLOR`) whenever `closes_at` holds: the hovered point is the
  chain's first point and `can_close` is true. A waiting arc counts, so the node stays lit while the
  user looks at it.
- The pending arc keeps waiting on the closed draft, because `to_path` derives the closing segment
  from it on demand. `undo` reopens a closed draft before it takes the pending arc, so the arc that
  closed the ring is not lost; `push` refuses a closed draft before it touches a waiting arc, so the
  derived segment can never append a second one.
- `DrawDraftError::ArcInProgress` still refuses a *second* arc while one waits; it no longer refuses
  a close.

## Consequences
A polyline can be finished by the arc it is drawing, from either *Close* button or from a click on
the first node, and the first node tells the user that closure is available before they try. The
cost is one rule to keep in step: the count a close needs is two segments, not three, so every
surface that gates a close reads `can_close`/`draft_can_close` instead of comparing a point count.
A degenerate closing arc — a midpoint on the line between the ends — is refused by `close` and
leaves the draft open, editable and waiting, the same way an invalid straight close always did.
