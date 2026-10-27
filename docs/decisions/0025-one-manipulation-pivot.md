# 0025. Move and Rotate share one pivot at the first corner of the local bounds

- Status: Superseded by 0026
- Date: 2026-10-27

## Context

The translate and rotate gizmos started with different pivots. Translate drew its handles at the
minimum corner of the descendant panels' **world** AABB for a container and at the local origin for
a panel; rotate always drew its rings at the node's world origin and changed only the quaternion.
For a panel the two points coincide, because a panel's frame starts at the minimum corner of its
nominal rectangle. For a container they diverge: the handles could sit far from the rings, and the
world AABB was recomputed after every edit, so the point jumped when the node turned.

Direct manipulation needs one point the user can predict. It must also stay attached to the object:
a point recomputed from a world-axis-aligned box is not a material point and moves relative to the
geometry as soon as the object rotates.

## Decision

- A node's **manipulation pivot** is the minimum corner of its local axis-aligned bounding box,
  expressed in the node's own frame:
  - a panel pivots at its local zero, the minimum corner of its nominal rectangle;
  - a container pivots at the minimum of the nominal panel boxes of its descendants, in the
    container's frame;
  - a container with no panels falls back to its own origin.
- The pivot is a material point: it moves and turns with the node. It is derived from the current
  tree by `Project::pivot_local(id)`; `Project::pivot_world(id)` applies the node's world placement.
- Both gizmos draw and act at `pivot_world`. The translate gizmo only changes the translation, so
  the pivot choice has no effect on its commit.
- The rotate gizmo keeps that point fixed. With `p` the local pivot, `t` and `q` the starting local
  translation and rotation, and `R = R_axis(θ)` the turn about one local axis:

  ```text
  pivot_parent = t + q · p
  delta_parent = q · R · q⁻¹
  t_new        = pivot_parent + delta_parent · (t − pivot_parent)
  q_new        = q · R
  ```

  The result is one `node.placement.set`, so the drag remains one undo entry. When `p` is zero, the
  translation is reused bit-for-bit; otherwise it is rounded to whole micrometres like a translate
  drag.
- The local bounds use each panel's nominal length × width × last known thickness. Shaped edges,
  banding overhangs and real material removed by machining do not enter the box yet.
- A per-node pivot override is a future convenience, not a stored document field now. Both gizmos
  already ask the model for one point, so such an override can replace the default without changing
  the drag or commit paths.

## Consequences

- Move and Rotate show one predictable point on every object, including a closed container.
- Rotating a container now adjusts its translation. This narrows the older rule that a rotation
  edit never touches the stored translation: that stays exact for a panel, whose pivot is zero.
- The pivot no longer jumps when a container is rotated, because it is derived in the container's
  frame rather than from a world-axis-aligned box.
- The preview and the commit use the same point: the ring centre is fixed during the drag and stays
  fixed after release.
- A fallback pivot can be far from the visible panels of an empty container, and a heavily shaped
  panel's pivot may sit outside its resolved outline. Both are acceptable until the pivot override
  and contour-aware bounds arrive.

## Related

- [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md) introduced the translate
  preview-and-commit shape and its original panel-origin/container-AABB pivot.
- [0022](0022-closed-containers-pick-as-one-object.md) chose the container pivot that this record
  replaces.
- [0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md) fixed the quaternion and Euler
  representation; its statement that rotation preserves the translation applies only at a zero
  pivot now.
- `crates/model` (`Project::pivot_local`, `Project::pivot_world`),
  `apps/furnicraft/src/translate_gizmo.rs`, `apps/furnicraft/src/rotate_gizmo.rs`.
