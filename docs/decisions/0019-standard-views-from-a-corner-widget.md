# 0019. Standard views come from a corner orientation widget

- Status: Accepted
- Date: 2026-10-25

## Context

The viewport needs orientation feedback and a fast way to reach the six standard views. A fixed 2D
diagram does not follow the camera, a three-dimensional gizmo at the origin competes with the model
and can be hidden behind it, and menu entries for every view are slow to use while modelling.

The camera orbits with yaw and pitch and clamps its pitch just short of the poles, so a top view is
always five degrees away from the vertical axis. That keeps the up vector defined and avoids a
gimbal flip, but it means an axis-aligned direction never matches the camera's eye exactly.

## Decision

- `Camera::look_from(direction)` takes the eye offset from the target, keeps the target and
  distance, and derives yaw and pitch. Pitch is clamped to the existing camera limits, so top and
  bottom views are five degrees off the pole.
- The floor grid gains a Z axis: `Grid::axis_z_color` and `Grid::axis_height_mm` describe one line
  from the origin cross upward. Its height is independent of the floor extent, so the pole does not
  grow with the grid.
- The shell draws a corner widget with six projected world-axis balls, positioned with the camera's
  right and up vectors and depth-sorted so the near balls cover the far ones. The widget lives in
  `apps/furnicraft/src/gizmo.rs`, not in `furnicraft-ui`: it reads the camera, which is shell view
  state, and changes no document data.
- A click returns an eye direction and the shell applies `Camera::look_from`. Clicking the side the
  camera already looks from flips to the opposite side. The clamped top and bottom pitches count as
  aligned, otherwise the centre ball would be dead in the two views where it is most useful.
- Snapping a view is not a command. It mutates only the camera, which the document never stores.

## Consequences

- The six standard views are one click away and are testable without a GPU: widget placement, the
  flip rule and the camera snap are plain functions.
- Top and bottom views stop at 85 degrees. The model stays upright and orbitable, at the cost of not
  being a true orthographic plan; a later orthographic mode can change the projection without
  changing the widget.
- The widget consumes clicks inside its hit boxes; the viewport selection handler ignores a click
  over the widget rectangle so a snap never selects the model behind it.
- The widget paints after the viewport image and is clipped to its own rounded rectangle. In a
  viewport too small for the widget plus margins it is not drawn, and the toolbar wraps instead of
  widening the viewport past its panel.
