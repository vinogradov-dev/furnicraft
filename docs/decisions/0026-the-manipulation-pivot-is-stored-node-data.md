# 0026. The manipulation pivot is stored, editable node data

- Status: Accepted; supersedes 0025
- Date: 2026-10-27

## Context

[0025](0025-one-manipulation-pivot.md) gave Move and Rotate one shared point, but derived it from
the current tree on every read: the minimum corner of the local bounds. That is stable while the
node turns, yet it moves whenever the geometry changes. Resizing a panel, adding a child or removing
one silently moves the grab point; a user who placed the pivot on a hinge edge loses it after the
next edit. A derived point also has no way to express intent: a panel always pivots at its minimum
corner and an empty container at its origin.

Furniture editing needs a CAD-like pivot: one point for both gizmos, chosen once, surviving edits and
save/load, and editable when the default is wrong.

## Decision

- A node's **manipulation pivot** is stored document data: a `Pivot`, three exact micrometre
  `Length` values in the node's own frame. It is a field of `Panel`, `Assembly` and `Group`.
- The pivot moves and turns with the node because it is expressed in the node's frame. Changing the
  placement never changes the stored pivot; referencing it in world space applies the node's world
  placement.
- Defaults are materialized once, when the node is created:
  - a panel starts at its local zero, the minimum corner of its nominal rectangle;
  - a container created with children starts at the centre of the local axis-aligned box around the
    nominal panel boxes below it;
  - an empty container starts at its own origin and stays there until the user centres it;
  - the root group has a zero pivot and cannot be edited.
- "Local bounds" means the union of each descendant panel's nominal length × width × last known
  thickness, transformed by its placement relative to the container. Banding overhangs, contour
  shape and machining volume do not enter the box yet.
- Resizing a panel, adding or removing a child, grouping, ungrouping and duplicating never move a
  stored pivot. Duplicating copies it. Ungrouping discards the removed group with its pivot.
- `Project::bounds_center_local(id)` computes the centre that `node.pivot.center` writes;
  `node.pivot.set` writes explicit coordinates. Both are commands and each is one undo entry. The
  inspector shows X/Y/Z in millimetres and a Center button. A viewport pivot-pick tool is deferred.
- The root cannot be moved, so pivot commands reject it with `RootImmutable`.
- Schema 6 adds the field. Loading a version 5 or older file materializes the new defaults once:
  panels keep zero, non-empty containers get their bounds centre, empty containers and the root get
  zero.

## Consequences

- Both gizmos use one point that is stable across geometry edits and save/load, and the user can
  correct a default that does not fit the furniture.
- Containers are grabbed at their visual centre by default, while panels keep their local frame
  origin. This changes the old derived default from the minimum corner to the centre for containers.
- An empty assembly has no meaningful bounds, so it starts at its origin; the user presses Center
  after filling it.
- The pivot is a normal document edit: it participates in undo, dirty state and file comparison.
- The pivot can sit outside the visible material or inside a void; that is the user's choice and is
  not clamped.
- The pivot is not yet pickable in the viewport; the inspector is the only editor.

## Related

- [0025](0025-one-manipulation-pivot.md) introduced the shared pivot and is superseded here.
- [0027](0027-the-rotate-gizmo-uses-world-aligned-rings.md) uses this pivot for world-aligned rings.
- [0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md) keeps the rotation a quaternion.
- `crates/model` (`Pivot`, `Project::pivot`, `Project::set_node_pivot`,
  `Project::center_node_pivot`), `crates/engine/src/node_cmds.rs`,
  `crates/ui/src/pivot_card.rs`.
