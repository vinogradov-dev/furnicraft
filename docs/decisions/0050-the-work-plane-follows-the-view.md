# 0050. The work plane follows the view

- Status: Accepted
- Date: 2026-11-04

## Context

A drawing tool turns a pointer position into a point in the scene, and a point needs a plane. A
perspective or axonometric view has no natural one; the floor, world XY, is the only plane that
always reads as "the ground". A standard front, side or top orthographic view is different: the user
looks straight at a world plane and expects the cursor to draw on it.

The six standard views come from the corner widget ([0019](0019-standard-views-from-a-corner-widget.md)).
The top and bottom views stop five degrees short of the pole, so a test on the view direction alone
would classify the most useful plan view as an arbitrary orthographic view. The plane's two axes
also matter: if they do not follow the screen, a shape drawn to the right and up would land on
negative coordinates or mirror in the document.

## Decision

- `WorkPlane` is view state in the shell (`apps/furnicraft/src/gizmo_math.rs`); the document stores
  none of it.
- A work plane passes through the world origin and is one of world **XY**, **XZ** or **YZ**. Its
  basis is right-handed (`u × v = normal`), the normal points from the plane towards the viewer, and
  `u` and `v` point right and up on screen in that view.
- Only an orthographic view selects a side plane. A top or bottom view is recognised by its clamped
  pitch, which is exactly `MAX_PITCH` or `MIN_PITCH`. A side view needs zero pitch and an exact
  quarter-turn yaw. Every other view — perspective, isometric, or a standard view orbited even a
  little — draws on world XY.
- A point maps by intersecting the pointer ray with the plane. Plane coordinates are the world
  point dotted with `u` and `v`; the origin is the world origin, so a 10 mm grid stays world-aligned
  while the camera pans.
- The viewport strip shows the active plane (`XY`, `XZ`, `YZ`) while the viewport owns its view; an
  authored camera hides it for the same reason it disables navigation and the projection toggle.

## Consequences

- Drawing is what-you-see-is-what-you-get in all six standard views: right and up on screen are
  positive `u` and `v`, and a committed path can keep those coordinates as its local frame.
- Snapping has a stable frame: the grid does not move when the target pans.
- Orbiting away from a standard view returns the plane to world XY. That is predictable, and it
  avoids a sticky plane with its own mode indicator; a tool that needs to keep a side plane must
  snap the view back first.
- The five-degree pole clamp makes a top view slightly oblique, so screen distances on the plane
  differ from world distances by about 0.4 %. The grid and the size readout must use plane
  coordinates, not screen pixels.
- A later panel-face work plane adds an origin and an arbitrary rotation to the same type; it does
  not introduce a second drawing plane.
