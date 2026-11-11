# 0026. The manipulation pivot is stored, editable node data

- Status: Accepted; supersedes 0025; refined by 0088
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

## Amendment: a container is born at zero (2026-11-10)

The creation default changed with [0082](0082-the-local-zero-is-the-creation-anchor.md): a container
is born with a zero pivot in its own frame, and a caller that wants it grabbed at its content puts
the centre into the **translation** instead. `group_nodes` is that caller — it anchors a new group at
the centre of its children's union bounds, moves every child by `-centre` and leaves the pivot at
zero, so the inspector's translation names the same point the gizmo turns about and a nested group
composes without inventing a second handle. A container added empty, duplicated or loaded keeps the
pivot it was given; `node.pivot.center` is still how the user centres one explicitly. The bullets
above about "a container created with children" describe version 6 behaviour and the schema-5
migration, which remains the only writer of such a centre.

The shape-derived pivot of [0058](0058-a-new-drawing-pivots-on-its-own-shape.md) is gone for the same
reason: a drawing now passes `Pivot::ZERO` and puts the point the user drew first (or the ring's
bounds centre) into the translation, so the pivot is never a second copy of information the frame
already carries. The contract tested in `crates/model/src/project/frame_contract.rs` covers every
creation path.

## Related

- [0025](0025-one-manipulation-pivot.md) introduced the shared pivot and is superseded here.
- [0027](0027-the-rotate-gizmo-uses-world-aligned-rings.md) uses this pivot for world-aligned rings.
- [0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md) keeps the rotation a quaternion.
- `crates/model` (`Pivot`, `Project::pivot`, `Project::set_node_pivot`,
  `Project::center_node_pivot`), `crates/engine/src/node_cmds.rs`,
  `crates/ui/src/pivot_card.rs`.

## Amendment: origin and pivot are separate (2026-11-11)

[0088](0088-origin-and-pivot-are-different-points.md) settles the vocabulary. The stored value is
`PivotLocal`, an exact offset from the node's origin; the serialized field is `pivot_local` from
schema 20 (a schema 19 `pivot` is read through a serde alias). The origin is implicit in
`Placement::translation`; it is not a second field. A creation writes the anchor into the
translation and leaves the pivot at zero. A later move or turn never changes `pivot_local`, and a
general `rebase_origin` carries the pivot with `pivot_local_new = pivot_local_old - anchor` so
`pivot_world` stays fixed. A panel remains the exception: its pivot is derived and always zero.
The old schema-6 materialized container centre stays a legacy `pivot_local`; the migration does not
re-anchor it.
