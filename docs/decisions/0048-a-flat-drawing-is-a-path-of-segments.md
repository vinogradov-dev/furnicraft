# 0048. A flat drawing is a path of segments

- Status: Accepted
- Date: 2026-11-04

## Context

The viewport has to store what the user draws before it is a solid. A `Contour` cannot be that
value: it is a ring, so it refuses an open chain, a chain that crosses itself and a chain that
encloses no area, and those are exactly the states a drawing passes through. Flattening the drawing
into a point list would lose the arcs, which a cabinet drawing needs, and the stable segment ids
that banding and the contour editor already rely on
([0040](0040-a-constructive-contour-is-a-ring-of-segments.md)).

A path also needs a payload of its own: `Mesh` is baked and editable but has no history, and
`Constructive` needs a ring and a feature ([0037](0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)).

## Decision

- `Path` is a chain of directed line and arc segments with the same `ContourSegment` value and the
  same stable ids as a ring, plus a `closed` flag. It is store data in the model crate.
- A path validates only chain rules: at least one segment, the segment and coordinate caps, unique
  ids, joints meeting within a micrometre, and a closed chain closing its last joint. It never
  validates self-intersection or enclosed area: those are ring rules.
- `Path::to_contour` is the only transition to a ring. It refuses an open chain, applies the ring
  rules and keeps every segment id, so the ring is cut from the same segments the user drew.
- `Geometry::Path(Path)` is the third payload of a geometry object (schema 17). It has no mesh, no
  feature list and no evaluation; it is drawn as an unlit line chain.
- `object.path.create` and `object.path.replace` carry the whole `Path` value and are one undo entry
  each, like the contour commands. They share their `enabled` check with `run`.
- A path object cannot be collapsed: there is no solid to bake, and `object.extrude.create` takes the
  contour the user derives from it, not the path itself.

## Consequences

- The drawing tools write a `Path` and the extrusion asks for a ring, so an unfinished or
  self-crossing drawing stays editable instead of being rejected at the first click.
- A closed path can still fail `to_contour`; the error is readable and leaves the drawing intact.
- The document gains a payload with no derived geometry, so `furnicraft-mesh` and the renderer learn
  nothing new beyond the line primitive.
- A later sketch feature — a fillet on a path corner, a constraint — extends the path value, not a
  second drawing type.
