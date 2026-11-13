# 0104. A ghost of a missing node is shell paint, not the overlay

- Status: Accepted; narrows the scope of 0093
- Date: 2026-11-13

## Context

N6 draws the part a fill would create before the click that creates it. The task's shape said the
ghost travels through the session preview overlay, and the overlay is the right home for an operation
that previews a *node the document holds*: the extrusion pull replaces one node's geometry, and the
viewport re-derives that node through the evaluator (0093).

A part that does not exist yet has no id. The overlay is a map from node id to geometry, and the
frame builder visits the document's nodes, so an entry without a node is never drawn. Putting the
ghost into the document and undoing it on cancel is exactly what 0093 forbids: one undo entry per
pointer move, and a dirty file before the maker accepted anything.

## Decision

**A ghost of a node the document does not hold is shell paint.** The empty-space tool computes the
part's size, placement and class from the model's niche query (0103) and draws its wireframe and its
size callouts with the `egui` painter, the way a drawing draft and a contour grip are drawn. It
writes no document field, no revision and no overlay entry; the one command behind the click is the
only thing history sees.

**The overlay keeps its job.** 0093 still governs every preview of an existing node — the extrusion
pull today, a boolean later. The two are different pictures of different questions, and neither
replaces the other.

## Consequences

- A ghost is invisible to the renderer and to the evaluator, so it costs one painter pass and no
  rebuild; the ghost follows the pointer without touching the scene cache.
- A future preview that needs shaded geometry for a node that does not exist yet — a box creation
  pulling its height, for instance — has to give the overlay a key that is not a document id, and
  that is a new decision. Until then the wireframe is the honest answer, and the domain's own
  description of the niche already draws it that way.
- The ghost reads the same theme roles the other screen-space tools should: it takes the accent, the
  surface and the text from `theme`, not a fixed colour, so it survives the light variant (X8).

## Related

- [0093](0093-a-preview-fills-a-session-overlay.md) — the overlay this record narrows.
- [0103](0103-one-probe-serves-the-niche-the-distances-and-the-move.md) — the model answer the ghost
  draws.
- [0099](0099-a-niche-is-the-space-a-frame-reserves.md) — the niche the ghost fills.
