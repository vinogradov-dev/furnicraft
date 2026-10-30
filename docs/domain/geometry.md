# Geometry objects

Status: design specification. This document defines the generic geometry object: its two payloads, how
a mesh is evaluated, how the user moves between the payloads and which references survive an edit.
Panels are a separate, manufacturing-specific node kind and are described in
[Panels and edge banding](panels.md).

Related documents:

- [Scene file](scene-file.md) — how the object is saved.
- [Scene document](document.md) — the node tree the object lives in.

Related decisions: [0026](../decisions/0026-the-manipulation-pivot-is-stored-node-data.md),
[0029](../decisions/0029-view-state-belongs-to-the-session.md),
[0034](../decisions/0034-nodes-carry-typed-properties.md),
[0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md) and
[0040](../decisions/0040-a-constructive-contour-is-a-ring-of-segments.md).

## 1. Two authoring paths

| | Mesh object | Constructive object |
|---|---|---|
| Created from | a primitive (box first) | a closed flat contour plus features |
| Source of truth | the mesh itself | the contour and the feature parameters |
| Edited by | moving vertices and faces | editing the contour or a feature |
| Mesh editing | yes | no, until `object.collapse` |
| History | undo only | the feature list, saved in the file |
| Mesh in the file | yes | no, it is derived |

The two paths are deliberately separate ([0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)).
A mesh has no shape history, so a primitive cannot be re-parameterised; a constructive object has no
direct mesh editing, so its contour stays the only way to change it.

## 2. The object node

- A geometry object is a node with a name, a parent, a placement, a stored pivot and typed
  properties, exactly like a container.
- It is a **leaf** in v1: it has no children. Several parts are grouped by wrapping them in a
  container, which already exists.
- The node carries one **geometry payload**: `Mesh` or `Constructive`. There is no separate node
  kind per payload and no separate "solid" node.
- A **panel is not an object**. A panel stores nominal size, material, banding and machining and
  obeys the panel rules; a geometry object is free-form. A board drawn as an object does not become a
  cutting-list part. The duplication is accepted because furniture rules are not generic mesh rules.
- Camera and light nodes are their own kinds ([0036](../decisions/0036-cameras-and-lights-are-scene-objects.md)).

## 3. Mesh payload

- `positions`: local coordinates as `Length` triples, in the object's own frame.
- `faces`: polygons, each a list of at least three vertex indices, counter-clockwise when seen from
  outside the solid.
- The mesh is triangulated only for rendering; the document keeps polygons so editing a box does not
  turn it into triangles.
- Validation:
  - every index is inside the position array;
  - a face has at least three distinct indices and no repeated index;
  - coordinates are finite and inside the document's size limit;
  - vertex and face counts are capped, and a file above the cap is a readable error, not an
    allocation.
- Unused vertices are allowed: deleting a face leaves its points behind until they are removed, and
  they render nothing.
- v1 has no smoothing groups, no UVs and no per-object material: an object is shaded with the default
  surface colour and the selection tint.

## 4. Constructive payload

- `features`: an ordered list. The first feature is `Extrude`.
- `Extrude { contour, height }` takes a closed ring in the object's local XY plane and extrudes it
  along `+Z` from `z = 0` by `height`.
- A contour is a ring of directed **segments**, at least three. A segment is a straight line between
  two points or a circular arc with a centre, a positive radius, a start angle and a non-zero sweep
  of at most one turn. Every value is stored in document units: micrometres and millidegrees.
- Every segment carries a stable id, unique inside its ring and never reused. An edit keeps the id of
  every segment it does not touch, so a command names a segment instead of an index; a fresh ring
  numbers its segments from one and keeps a counter for the next id.
- The ring must be closed — each segment's end is the next segment's start, rounded to the
  micrometre — and must enclose a non-zero area. The constructor stores it counter-clockwise, so the
  face winding does not depend on how the user drew it.
- A contour that touches or crosses itself is rejected; the test runs on a chord polyline sampled
  from the curve, so an arc that crosses a line or another arc is caught as well. Arcs and lines may
  meet at any angle; a tangent-continuous joint is not required.
- A shaped ring has no holes yet and its arcs are sampled into the kernel profile within a chord
  tolerance. A version 10 file stored a plain point list; it loads as a ring of lines.
- Later features — a boolean with another object, a chamfer, a fillet — are added to the same list.
  None of them may reference a face or an edge of the evaluated result
  ([0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)).
- The height is positive and capped by the document's size limit. An empty or negative feature list
  is invalid.

## 5. Evaluation

- A constructive object's mesh is evaluated by `furnicraft-param` through the geometry kernel port and
  cached by `(Revision, Id)` in `furnicraft-mesh`. It is never written to the file.
- Editing the contour or a feature parameter bumps the revision once, and the next frame rebuilds the
  mesh.
- An evaluation failure — no kernel, a contour the kernel rejects, a boolean that yields no solid —
  keeps the object and its data and reports a diagnostic on the node. It is not a load error and it
  does not delete anything.
- `object.collapse` refuses to run while the current evaluation fails.

## 6. Collapse

- `object.collapse` replaces `Constructive` with the evaluated `Mesh` in one command and one undo
  entry.
- The id, name, placement, pivot and properties are unchanged; the feature list is deleted.
- Afterwards the object is a mesh object: mesh commands apply, and the contour is gone. Undo is the
  only way back, which is why the UI asks for confirmation.
- Collapse is never automatic: a rebuild happens on every edit, and a failing evaluation is a
  diagnostic to fix, not a reason to bake.

## 7. References and topology

- The document stores the mesh; the session stores the selection.
- Vertex editing is editor view state: it names the object and the selected vertex indices, so
  entering or leaving the mode needs no undo entry. Vertices are picked in screen space with a fixed
  point radius and painted as markers over the viewport; a click outside every vertex clears the
  vertex selection, Shift or Ctrl adds or removes one.
- A selected vertex set is dragged with the same three world-axis handles as the object gizmo, at the
  median of its vertices. The drag shows a moved copy of the mesh and a moved set of markers; the
  release dispatches one `mesh.vertices.move` with absolute local positions, and Escape cancels the
  drag by restoring the document. The world shift is rotated by the inverse of the object's placement
  and rounded to micrometres, so the preview and the committed positions agree.
- A mesh element is referenced by its index, valid only for the revision it was read in. A command
  that changes topology invalidates the selection, and the session clears it.
- There are no stable names for vertices, edges or faces beyond a revision, and no persistent naming
  for derived geometry. A feature refers to its inputs only.
- `mesh.vertices.move` validates every index and rejects the whole command if one is out of bounds; it
  never skips a bad entry silently.

## 8. Commands

| Command | Parameters | Effect |
|---|---|---|
| `object.primitive.create` | parent, name, primitive kind, size, placement | creates a mesh object, returns its id |
| `object.extrude.create` | parent, name, contour, height, placement | creates a constructive object with one `Extrude` |
| `object.contour.replace` | object, contour | replaces the contour and rebuilds |
| `object.collapse` | object | replaces the payload with the evaluated mesh |
| `mesh.vertices.move` | object, list of (index, local position) | moves vertices as one change |

A face move is a `mesh.vertices.move` over the face's vertices; it does not need its own command.
Every command bumps the revision once and becomes one undo entry.

## 9. Tests

At minimum:

- a box primitive has eight vertices and six faces, and its triangulation has twelve triangles;
- validation rejects an out-of-bounds face index, a degenerate face, a non-finite coordinate and a
  file above the vertex cap;
- an extruded contour has the volume of its area times its height;
- a contour with an arc loads, round-trips, and extrudes to the volume of its analytic area;
- an open ring, a ring that does not meet its neighbour and a ring that crosses itself are rejected
  on construction and on load;
- replacing the contour changes the mesh and keeps the id, name and placement;
- collapse produces the same mesh as the evaluation, clears the features and is one undo entry;
- `mesh.vertices.move` on a constructive object is refused;
- one out-of-bounds vertex index rejects the whole move;
- a failing evaluation keeps the object, its data and its placement and reports a diagnostic;
- a constructive object's mesh never appears in a saved file.

## 10. Open questions

- Arcs in a profile: the kernel profile is a polyline, so an arc is sampled; the document keeps the arc.
- Holes in a profile.
- Booleans between two objects, and what the operands look like in the tree.
- Fillet and chamfer: they need a face or an edge of the result, so they wait for a naming design.
- Multi-body objects versus one object per solid.
- Normals and shading: flat now; smooth shading and hard edges are undecided.
- Whether an object gets its own material and texture, or inherits from its container.
- Snapping to mesh vertices and edges while modelling.
