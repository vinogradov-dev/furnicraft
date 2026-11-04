# 0049. The viewport toggles its own projection

- Status: Accepted
- Date: 2026-11-04

## Context

A perspective view makes parallel edges converge, so a carpenter cannot check a panel against a
measurement by eye. An orthographic view is the natural working view, and
[0019](0019-standard-views-from-a-corner-widget.md) already predicted that a later mode could change
the projection without changing the corner widget.

The projection is view state ([0029](0029-view-state-belongs-to-the-session.md)), but a camera node
stores its own projection as document data ([0036](0036-cameras-and-lights-are-scene-objects.md)).
A toggle that switched the view unconditionally would overwrite what the document owns. A toggle
that kept the camera distance unchanged would also make the scene jump: a perspective view shows a
different height at the target than the orthographic height the camera would be given.

## Decision

- `Camera::toggle_projection` switches a navigable camera between
  `Projection::Perspective { fov_y }` and `Projection::Orthographic { height }`.
- The visible height at the target is preserved. A perspective view becomes orthographic with
  `height = 2 * distance * tan(fov_y / 2)`, and an orthographic view becomes perspective with the
  default field of view and the distance that shows the same height.
- `Camera::zoom` scales the orthographic height together with the orbit distance. Zooming therefore
  works in both projections, and a toggle pair returns the same frame.
- The viewport strip shows an **Orthographic** or **Perspective** button with the `O` key. The button
  and the key do nothing while an authored camera is active: the strip disables the button, and a
  pinned camera ignores `toggle_projection` and `zoom` at the type level.
- The toggle changes no document data: it is not a command, it bumps no revision and it is not
  undoable. An authored camera's projection is changed only by `camera.projection.set`.

## Consequences

- A plan or elevation check no longer needs an authored camera; the view keeps its target and orbit.
- The strip reads the projection the viewport actually draws with, so an active authored camera shows
  its own projection and a disabled toggle instead of a control that lies.
- Toggling to perspective moves the eye along the view axis to keep the visible height. The target is
  unchanged, so the orbit pivot stays where the user left it.
- Top and bottom views still stop five degrees short of the poles ([0019](0019-standard-views-from-a-corner-widget.md)),
  so an orthographic top view is near-plan rather than an exact plan. The work plane decides which
  plane axis-aligned drawing uses.
