# 0024. Rotation is a quaternion shown as intrinsic XYZ Euler angles

- Status: Accepted; pivot and its translation rule narrowed by 0025
- Date: 2026-10-26

## Context

A placement stores a unit quaternion, which is the exact, composition-friendly representation of a
rigid rotation. Furniture is designed in angles, though: a door turned 90° about its hinge edge, a
panel tilted 15°, a carcass rotated about the vertical. The inspector must show numbers a
woodworker can read, and the viewport must offer a rotate gizmo without a second source of truth.

Three constraints shape the choice. First, an Euler triple is not unique: the same rotation has
several triples, and at gimbal lock one degree of freedom collapses. Second, a full turn is the
identity, so re-entering `360` must not create an undo entry. Third, the document must stay exact:
a translation edit must not perturb the stored quaternion, and a rotation edit about the node's
origin must not perturb the stored translation. A rotation about a non-zero pivot adjusts the
translation instead; [0025](0025-one-manipulation-pivot.md) settles that point.

## Decision

- The document keeps the unit quaternion as the only stored rotation. Euler angles are derived on
  read and are never serialized.
- The inspector shows intrinsic XYZ Euler angles in degrees: `R = Rx · Ry · Rz`, applied about the
  node's local axes in that order. Y is the middle axis, so gimbal lock is at Y = ±90°.
- Angles are integer millidegrees (`furnicraft_units::Degrees`), like lengths are integer
  micrometres. Text parses without floats and round-trips exactly through the displayed precision.
  The inspector wraps each angle into `(-180°, 180°]`.
- The rotate gizmo draws three rings on the selected node's local axes, centred on its manipulation
  pivot. A drag intersects the pointer ray with the ring plane, accumulates the angle continuously
  across the ±180° seam, and previews a world rotation of the panels about that pivot.
- On release the gizmo post-multiplies the local rotation: `R_new = R_start · R_axis(θ)`. This turns
  the node about its own axis. At the node origin the stored translation is kept bit-for-bit; at the
  non-zero pivot of a container the translation is adjusted so that point stays fixed
  ([0025](0025-one-manipulation-pivot.md)). The inspector's Euler fields edit the same quaternion;
  when only one half of the placement changed, the other half is kept exactly.
- Every edit dispatches exactly one `node.placement.set` on Apply or release. Escape cancels the
  drag. A zero turn, a full turn, or a triple that describes the same rotation as the stored one
  (`Placement::same_transform`) dispatches nothing.
- Move and Rotate are one active gizmo at a time. The mode switch is shell view state, not a command.
- A ring whose plane is nearly edge-on to the view is not drawn or grabbed: the pointer cannot define
  a stable angle on it. The root has no placement and gets no gizmo.

## Consequences

- Users type and read angles, while the document stays a unit quaternion that composes and
  interpolates cleanly.
- Euler is a lossy view: a quaternion set by the gizmo displays the nearest millidegree triple, and
  around Y = ±90° editing X or Z can move the other angle. The document is unaffected; the
  inspector is a presentation of it.
- The gizmo does not snap yet. Snapping, if added, quantizes the angle before the commit and remains
  one command.
- The pivot is the first corner of the node's local bounds, not its visual centre. A panel therefore
  turns about its minimum corner, which matches its local frame and keeps the translation exact; a
  container turns about the matching corner of its panels.
- The preview re-uploads the frame on each pointer move, the same cost as the translate gizmo; an
  incremental transform buffer is still the later optimization.
- Hiding edge-on rings means a standard view exposes only the ring perpendicular to it. The user
  orbits slightly to reach the other axes; showing thin, unstable ellipses was rejected.

## Related

- [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md) established the
  preview-and-commit shape this gizmo reuses; its open rotation note is resolved here.
- [0003](0003-length-is-integer-micrometres.md) is the same argument for lengths.
- `crates/model` (`EulerAngles`, `Placement::rotation_euler`), `crates/units` (`Degrees`),
  `crates/ui/src/placement_card.rs`, `apps/furnicraft/src/rotate_gizmo.rs`.
