# 0004. Panel-local frame, faces and topology ids

- Status: Accepted
- Date: 2026-10-22

## Context

Machining (holes, pockets, through cuts), edge banding and hardware anchors need coordinates that
survive a resize and a placement change. World coordinates survive neither: a panel that grows by
100 mm moves every feature measured from the far edge, and a placement edit moves all of them.

## Decision

A panel has its own frame with the zero at its minimum corner: X is length, Y is width, Z is thickness.
Faces are `A` at `z = thickness` and `B` at `z = 0`. Vertices run counter-clockwise from the zero
corner and edge `i` joins vertex `i` to `i + 1`. Vertex and edge ids are stable and never reused;
their numbers are labels, not keys. Machining is stored in panel-local coordinates; an edge reference
is a topology reference, not a second coordinate system.

## Consequences

- A resize moves the far edges, not the features anchored to the zero corner; a feature anchored to a
  named edge follows that edge.
- A placement edit changes no stored coordinate: placements are rigid transforms composed from the
  root.
- Picking and machining refer to `EdgeId` and vertex ids, so a mesh index never leaks into the
  document.
- `A` and `B` never mean "up" or "front": the labels travel with the panel.
