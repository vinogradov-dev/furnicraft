# 0022. A closed container picks as one object

- Status: Accepted
- Date: 2026-10-25

## Context

Viewport picking ray-casts the scene and returns the panel under the pointer. Selecting a panel
directly gives precise panel editing, but it breaks Blender-like manipulation of an assembly: drag
a cabinet and its panels separate, because the click selected one board rather than the cabinet.
The user needs both behaviours without a modal tool switching the meaning of every click.

## Decision

- A viewport pick returns the **closed container** that owns the hit panel. Walk up from the hit
  panel while its parent is closed; select the child of the nearest open container. If every
  ancestor is open, the panel itself is selected.
- Openness is **view state**: a per-node flag held by the editor, never written to the document. It
  survives until the node is removed or the session ends.
- **Double-click** in the viewport enters one level: it selects the child of the picked node that
  contains the hit panel and marks the picked node open. **Escape** exits one level: it closes the
  current node and selects its parent.
- A click in the scene tree always selects the exact node; the tree is the explicit path to a panel
  or a nested container regardless of the viewport's open state.
- Every panel below the selected node is highlighted, so a selected container reads as one object.
- The manipulation pivot is the minimum corner of the node's local bounds: the panel's local origin,
  the minimum of the descendant panel boxes in a container's frame, or the node origin for an empty
  container. Both gizmos use it; see [0025](0025-one-manipulation-pivot.md).

## Consequences

- Dragging a cabinet moves the whole cabinet; entering it once exposes the drawer and double-clicking
  again exposes a board.
- Open state is intentionally not saved: reopening a project starts from a clean, closed tree.
- Picking must resolve the selection against the current open set, so the rule lives in the editor
  (view state) rather than in the document or the renderer.
- Highlighting passes a list of panel ids to the scene frame; the document still stores only
  placements and materials.
