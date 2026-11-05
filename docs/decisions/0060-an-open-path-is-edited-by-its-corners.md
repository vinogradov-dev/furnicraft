# 0060. An open path is edited by its corners

- Status: Accepted
- Date: 2026-11-05

## Context
The contour edit math addresses a corner by the segment that starts on it
(`Contour::move_corner`, `crates/model/src/contour_edit.rs`). That is sound for a ring, which wraps:
the corner before the first segment is the end of the last one. A `Path` may be open, so its first
start and its last end have one segment each and no wrap; naming their corner by "the segment that
starts on it" has no answer for the last point, and walking the entries modulo their length would
weld the last segment to the first in the edit.

The drawing tools create open lines, polylines and arcs, and the edit mode has to move, split and
remove their segments without inventing a ring.

## Decision
A path corner is named explicitly:

```rust
pub enum PathCorner {
    Start(ContourSegmentId),
    End(ContourSegmentId),
}
```

`Start` is the point a segment begins on and `End` the point it finishes on. An interior corner is
addressed twice, once from each side; a closed chain's wrap corner is `Start` of the first segment or
`End` of the last, and both resolve to the same pair of neighbours. `Path` gains four operations that
share the ring helpers `with_corner`, `join_corner` and `split_document`:

- `move_corner_at` moves the shared corner through `join_corner`, or only the one segment at a free
  end; an arc keeps its centre and radius, a line takes the point exactly;
- `split_segment_at` keeps the first part's id and mints the second from the counter, like the ring;
- `remove_segment` drops the segment and joins its neighbours by moving the next segment's start to
  the previous segment's end, so everything before the removal stays put; removing a free first or
  last segment only shortens the chain, and a closed chain is re-closed around the join;
- `close` bridges the last end to the first start with one line and flips the flag, then asks the
  ring rules whether the result is a ring.

Every result is rebuilt from entries, so ids are never reused and the counter only grows. A path
edit keeps the open or closed flag; `close` is the one edit that changes it, and it goes through
`Path::rebuild_as`, which keeps the counter `Path::rebuild` keeps. `close` means "make this a ring",
so a bridge that crosses the chain, a chain that crosses itself and a chain that encloses no area are
refused with the ring's own error, and an already closed path is returned unchanged.

## Consequences
The path edit mode and its gizmo can address every corner of an open chain, including the last one,
without a second coordinate scheme or a synthetic closing segment. Removing a segment is a local
edit: the earlier part of the chain does not move, and an arc on either side keeps its circle; the
cost is a visible kink at the joined corner where the removed segment used to be. Closing requires a
valid ring rather than merely a connected chain, so a user who wants an open figure never calls it.
