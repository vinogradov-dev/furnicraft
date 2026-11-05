# 0057. The camera reaches the poles

- Status: Accepted
- Date: 2026-11-05

## Context

[0019](0019-standard-views-from-a-corner-widget.md) clamped the orbit pitch five degrees short of
each pole so that the up vector stayed defined. A top or bottom view was therefore never
perpendicular to its plane: clicking the Z ball left the camera at 85 degrees, and the grid, the
work plane and a drawing on it all appeared skewed. The owner's answer is that the axis of a
standard view is perpendicular to the plane of the view.

The old basis derived `right` as `forward × Z`, which degenerates when the view direction is
parallel to `Z`, and the view matrix rebuilt `up` from `right × forward`, silently replacing the up
vector an authored camera pinned with `looked_from`.

## Decision

- `MIN_PITCH` is `-FRAC_PI_2` and `MAX_PITCH` is `FRAC_PI_2`: a top or bottom view looks straight
  down or up its plane. `orbit` and `look_from` clamp to those limits.
- `Camera::right()` is analytic in yaw, `(-sin yaw, cos yaw, 0)`: the exact limit of the old
  `forward × Z` that stays unit and horizontal at a pole. `up()` is `right × forward` with no
  fallback.
- `view_matrix` builds its basis from `right`, `up` and `forward` as the getters return them, so an
  authored camera's pinned up vector survives the matrix.
- At a pole, `look_from` keeps the current yaw, because the eye direction alone does not determine
  it; orbiting with a yaw delta still turns the view about the vertical axis.
- The work plane and the orientation widget recognise a pole by the pitch at its limit.

## Consequences

- The top and bottom views are true elevations: a drawing maps square to the grid, and picking and
  screen projection use the same perpendicular axis.
- The up vector at a pole follows the orbit yaw instead of a canonical world axis, so a top view
  entered from an oblique view is turned about Z. A later canonical snap can set yaw for the Z ball
  without touching the limits.
- 0019's pitch clamp is superseded; its widget, look direction and flip rule stand. The clamp notes
  in [0049](0049-the-viewport-toggles-its-own-projection.md) and
  [0050](0050-the-work-plane-follows-the-view.md) describe behaviour that no longer exists.
