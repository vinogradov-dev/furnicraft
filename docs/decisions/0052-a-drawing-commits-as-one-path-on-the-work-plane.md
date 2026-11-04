# 0052. A drawing commits as one path on the work plane

- Status: Accepted
- Date: 2026-11-04

## Context

The drawing tools build a `DrawDraft` in view state ([0048](0048-a-flat-drawing-is-a-path-of-segments.md)).
A finished draft has to become one document object, at a position and orientation that match what the
user saw, without a dragged-out, multi-command edit. A drawing also has to choose a parent, and the
tools differ in when they are done: a line finishes on its second point, a polyline only when the
user closes it or says so.

## Decision

- A `DrawDraft` commits through exactly one `object.path.create`, which is one undo entry and one
  revision step. The name follows the tool (`Line drawing`, `Circle drawing`, …).
- The path is stored in work-plane coordinates and the placement maps the object's local XY onto the
  plane: local X is the plane's `u`, local Y is its `v`, local Z is its normal. The plane passes
  through the world origin ([0050](0050-the-work-plane-follows-the-view.md)), so the translation is
  zero today; a plane with an origin would add it here and nowhere else.
- The parent is the selected container; otherwise the parent of the selected leaf; otherwise the
  root. The new object becomes the selection, so the next action applies to what was just drawn.
- A fixed tool (line, arc, rectangle, circle) commits automatically when its last point lands. A
  polyline commits when the user closes it, presses Enter, or clicks **Finish**; an open chain is a
  valid path and may be committed as drawn. **Close** closes and commits in one step.
- A refused path keeps the draft and the selection unchanged and reports the reason, so a
  degenerate rectangle or a collapsed arc costs no clicks to recover.
- The tool ends after the commit. Starting another drawing is one more tool click.

## Consequences

- The document sees no intermediate state: a half-drawn shape never reaches the revision counter or
  the undo history.
- A drawing made in a standard ortho view keeps its orientation when the view returns to perspective.
- A path drawn beside a selected panel is a sibling, not a child; selecting the frame first draws
  inside it. That is the same rule the scene tree uses for a new node.
- The auto-commit makes the last click feel like the end of the shape, which matches a CAD line
  tool; the polyline keeps an explicit end because its point count is open.
