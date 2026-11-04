# 0054. A primitive keeps the parameters that built it

- Status: Accepted
- Date: 2026-11-04

## Context

`object.primitive.create` built a cube, a cylinder or a sphere by baking its polygon mesh into the
document ([0037](0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)). The mesh was the only
copy of the shape, so a sphere was a fixed 32 × 16 tessellation from the moment it was created: a
wrong radius could only be fixed by deleting the object, and a coarse sphere could only be smoothed
by rebuilding it. The tessellation constants lived in `Mesh::cylinder` and `Mesh::sphere`, which made
"more sides" a code change rather than a user edit.

A constructive object already solves the same problem for extrusions: it stores the contour and the
height and derives the mesh. A primitive is the same idea with a different set of parameters.

## Decision

- `Geometry::Primitive(Primitive)` is a fourth payload beside `Mesh`, `Constructive` and `Path`
  (schema 18). It is **data**: a shape and its parameters, never a mesh.
- `Primitive` is an internally tagged enum with `shape` as the tag — `Geometry` already uses `kind`
  at the outer level. Its variants are `Cube { edge }`, `Cylinder { radius, height, segments }` and
  `Sphere { radius, segments, bands }`. Every variant validates on construction and on load:
  `2 µm ..= 10 m` for a dimension, `3 ..= 256` sides and `2 ..= 128` latitude bands.
- The parameter set is the shape's own: a cube has one edge, and only the curved shapes have a face
  count. A cube therefore always has six faces; "sides" is a cylinder parameter and "bands" a sphere
  one. The three parameters are not unified into one generic tessellation field.
- The mesh is derived, never stored and never stored beside the parameters. `furnicraft-mesh` owns
  the one definition (`evaluate_primitive`), which the viewport draws through `EvaluationCache` and
  `object.collapse` bakes, so a collapsed primitive keeps exactly the shape the user saw.
- The bounds are analytic (`Primitive::bounds_micrometres`), so the tree, the pivot and the camera
  frame do not need an evaluated mesh. A sphere's bound is the sphere, not the tessellated polygon.
- `object.primitive.replace` stores a new parameter set in one revision and one undo entry, keeping
  the id, name, placement, pivot and properties. The parameters arrive already validated, so an
  invalid set is rejected by the payload constructor before the command runs.
- `object.collapse` now bakes either a constructive or a primitive payload; a baked mesh or a path
  is still refused.
- `object.primitive.create` takes the full extent — the cube's edge or the diameter of a cylinder or
  sphere — and builds the default tessellation (`DEFAULT_SEGMENTS`, `DEFAULT_BANDS`). Height equals
  that extent, so one number is still one shape.
- The card shows the parameters as millimetre and whole-number text fields and applies them with one
  command. Only the fields the shape has are shown; a field that belongs to another shape is never
  invented.
- Schema 18 keeps older files loading unchanged: a version 17 file has no primitive payload, and its
  baked meshes stay baked. A collapsed primitive saves as a mesh, so the file does not grow a
  parameter for a shape the user can no longer edit.

## Consequences

- A sphere is edited by its radius, a coarse shape is smoothed by raising its sides, and both are one
  undo entry instead of a delete and a redraw.
- The mesh is now paid for on every frame the cache misses rather than once at creation. On a miss the
  trig runs once per node and the result is cached by the parameters, so an unchanged primitive
  rebuilds nothing (ADR 0053).
- Two code paths produce polygon faces for a primitive: the analytic bounds and the mesher. A test per
  shape pins the counts (`vertex_count`, `face_count`) to the built mesh so they cannot drift.
- A primitive with many sides still writes a small file; the file holds no vertices for it.
- Collapse is still irreversible without undo: the parameters are gone once baked. That is the same
  deal a constructive object makes with its contour, and the card says so before the bake.
- The `MAX_FACE_VERTICES` cap applies to a curved primitive's cap: `MAX_SEGMENTS` is 256, well inside
  it. The primitive cannot reproduce the X2 cap problem, where a sampled contour ring can.
