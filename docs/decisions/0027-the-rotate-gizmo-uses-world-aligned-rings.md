# 0027. The rotate gizmo uses world-aligned rings

- Status: Accepted
- Date: 2026-10-27

## Context

[0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md) drew the rotate rings on the
selected node's local axes and post-multiplied the turn. Rings therefore turned with the object: a
cabinet already rotated about Z showed its "local Z" ring at an angle, the X and Y rings moved while
the object was turned, and the user could not read a turn as "90° about world Z". The inspector still
shows local intrinsic XYZ Euler angles, but direct manipulation needs a stable, legible frame.

A local-axis mode is a useful future tool, but it should be a mode, not the only behaviour.

## Decision

- The three rotate rings are aligned with the world X, Y and Z axes, centred on the selected node's
  stored manipulation pivot ([0026](0026-the-manipulation-pivot-is-stored-node-data.md)). They never
  follow the node's rotation or its parent's rotation.
- A drag intersects the pointer ray with the ring's world-axis plane, accumulates the angle
  continuously across the ±180° seam, and previews a world rotation of the panels about the pivot.
  The preview and the commit use the same world turn and the same pivot.
- On release the world turn `Δ_world` is converted into the parent frame and pre-multiplied onto the
  stored local quaternion. With `R_parent` the parent's world rotation, `q` the starting local
  rotation and `p` the local pivot:

  ```text
  Δ_parent = R_parent⁻¹ · Δ_world · R_parent
  q_new    = Δ_parent · q
  ```

- With `t` the starting local translation, the pivot stays fixed in the parent frame:

  ```text
  pivot_parent = t + q · p
  t_new        = pivot_parent + Δ_parent · (t − pivot_parent)
  ```

  `t_new` is rounded to whole micrometres. When `p` is zero the stored translation is preserved
  bit-for-bit.
- The result is one `node.placement.set`, so a drag remains one undo entry. Escape cancels; a zero or
  full turn dispatches nothing.
- A ring whose plane is nearly edge-on to the view is not drawn or grabbed: the pointer cannot define
  a stable angle on it. The root defines the world frame and gets no rings.
- Local-axis rings and snapping are deferred. If local rings arrive, they are an explicit mode that
  reuses the same commit with a different `Δ_world`.

## Consequences

- The rings stay put while the object turns, so a world-axis turn is readable and repeatable.
- Turning about a non-zero pivot adjusts the translation. Only a zero pivot leaves it untouched.
- The inspector keeps showing intrinsic XYZ Euler angles in the node's own frame; the gizmo writes
  the quaternion directly, so the displayed angles are a view of the result, not the input.
- A standard view exposes only the ring perpendicular to it; the user orbits slightly to reach the
  others, as before.
- The commit path is a pure function of the drag state and has unit tests without a GPU.

## Related

- [0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md) fixed the quaternion and Euler
  representation; its local-ring and post-multiply rule is superseded here.
- [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md) established the
  preview-and-commit shape; [0026](0026-the-manipulation-pivot-is-stored-node-data.md) defines the
  pivot.
- `apps/furnicraft/src/rotate_gizmo.rs`, `apps/furnicraft/src/gizmo_math.rs`.
