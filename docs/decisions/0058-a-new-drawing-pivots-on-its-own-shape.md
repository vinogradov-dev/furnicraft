# 0058. A new drawing pivots on its own shape

- Status: Accepted
- Date: 2026-11-05

## Context
`object.path.create` and `object.extrude.create` stored every new object with `Pivot::ZERO`
(ADR 0026). A drawing is placed on a work plane away from the world origin, so both gizmos grabbed
it at an empty point in space: the rotate rings turned the shape around the origin instead of
around itself, and the translate gizmo drew its handles where the user had not clicked.

The pivot is stored data and does not follow the geometry afterwards, so the default has to be
chosen once, at creation, from the shape being created.

## Decision
A new flat figure stores the pivot its shape gives it:

- a closed path or contour — a drawn rectangle or circle, and the ring a body is extruded from —
  uses the centre of its local bounding box, the same centre `node.pivot.center` writes;
- an open path — a line, a polyline or an arc — uses the start of its first stored segment, so the
  point the user drew first is the handle; an arc's start is the stored, rounded curve start,
  never the raw pointer point;
- the pivot is computed in whole micrometres, with the midpoint summed in `i128` so two extents
  near the coordinate limits cannot overflow;
- a flat figure has no height, so the pivot lies on its plane (`z = 0`); the body extruded from a
  ring takes the ring's pivot on the base plane;
- a primitive keeps `Pivot::ZERO`, because its bounds are already centred on its origin.

`Project::add_object_with_pivot` is the only creation path that takes a pivot; `add_object`
delegates with `Pivot::ZERO` for every other object. A path or contour replacement does not move
the pivot (ADR 0026), and an older file that stores no pivot still loads with the zero it had.

## Consequences
The gizmos grab a new drawing where it was drawn, without a second "centre pivot" step. The cost is
one derived function per payload and a small asymmetry between a flat figure and a body: the body's
pivot is on its base plane, not at half its height, until the user centres it. If a body should be
authored around its volume centre, that is a separate decision.
