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
[0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md),
[0040](../decisions/0040-a-constructive-contour-is-a-ring-of-segments.md),
[0041](../decisions/0041-every-solid-compiles-to-one-solid-plan.md),
[0042](../decisions/0042-a-contour-panel-keeps-its-ring-and-edits-a-working-copy.md) and
[0048](../decisions/0048-a-flat-drawing-is-a-path-of-segments.md).

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
direct mesh editing, so its contour stays the only way to change it. A path object is a third
payload: a drawing rather than a solid, described in section 5.

## 2. The object node

- A geometry object is a node with a name, a parent, a placement, a stored pivot and typed
  properties, exactly like a container.
- It is a **leaf** in v1: it has no children. Several parts are grouped by wrapping them in a
  container, which already exists.
- The node carries one **geometry payload**: `Mesh`, `Constructive` or `Path`. There is no separate
  node kind per payload and no separate "solid" node.
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
- The same ring may be the stored outline of a contour panel: the panel copies it, anchors its
  minimum corner at the panel zero and derives its nominal size from the bounds (see
  [Panels and edge banding](panels.md)).
- Later features — a boolean with another object, a chamfer, a fillet — are added to the same list.
  None of them may reference a face or an edge of the evaluated result
  ([0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)).
- The height is positive and capped by the document's size limit. An empty or negative feature list
  is invalid.

## 5. Path payload

- A path is an open or closed chain of directed **segments** in the object's local XY plane at
  `z = 0`. It uses the same segment value and the same stable ids as a ring, so a line or an arc
  drawn once keeps its shape and its identity when it becomes a contour
  ([0048](../decisions/0048-a-flat-drawing-is-a-path-of-segments.md)).
- The drawing tools build a path from shapes: a rectangle from two opposite corners, a circle as four
  quarter arcs whose joints sit exactly on the axes, and an arc through three points. A rectangle and
  a circle are closed; a three-point arc is one open segment. Degenerate input — a rectangle with no
  width or height, a non-positive radius, or three coincident or collinear points — is an error, not
  a flat shape.
- Chain rules only: at least one segment, the segment and coordinate caps, unique ids, each joint
  meeting within a micrometre, and a closed chain closing its last joint. Self-intersection and
  enclosed area are ring rules, so they run when the path becomes a ring, not while it is drawn.
- `Path::to_contour` is the only transition. It refuses an open chain, applies the ring rules and
  keeps every id, so the ring is cut from exactly what the user drew.
- A path is a drawing, not a solid: it has no mesh, no feature list and no evaluation. The viewport
  draws it as an unlit line chain and picks it within a few screen points
  ([0018](../decisions/0018-picking-ray-casts-the-scene.md)).
- The document stores the drawing as drawn: direction and orientation are kept, and a clockwise
  closed path becomes a counter-clockwise ring only in `to_contour`.
- An object payload is `Geometry::Path(Path)` (schema 17). It cannot be collapsed: there is no solid
  to bake.

## 6. Contour editing

A constructive contour and a contour panel outline are reshaped in one viewport mode. Its state is
view state, not document data ([0029](../decisions/0029-view-state-belongs-to-the-session.md)).

- The mode is entered with exactly one selected node that stores a ring: a contour panel outline or a
  constructive object. A rectangle panel, a mesh object and a multi-selection have nothing to edit.
  Mesh editing and contour editing are mutually exclusive.
- The editor keeps the stored ring as a **base** and a **draft** working copy. Every gesture rewrites
  only the draft, so the document, its revision and the undo history are untouched until the user
  applies. Leaving the mode without applying drops the draft.
- A corner handle sits at the start of every segment. Dragging it moves that corner in the node's
  local XY plane through its world placement; the two adjoining segments follow, and the rest of the
  ring is unchanged.
- Double-clicking a side splits it at the projected point. A split near a corner is refused so it
  cannot mint a segment a few micrometres long.
- An arc corner slides along its own circle: the drag point is projected onto the arc first, and the
  edit trims or extends the sweep. The centre and radius of a stored arc are never redefined; a point
  that misses the circle by more than a micrometre is rejected by the edit math.
- The model edit API is `move_corner`, `trim_segment` and `split_segment`. The viewport uses the first
  and the third; trimming along a supporting line or circle is available to a command but is not a
  gesture yet.
- Every edit keeps the id of each segment it does not touch. `move_corner` and `trim_segment` keep the
  edited segment's id; `split_segment` keeps the first part's id and mints the second part from the
  ring counter. A ring rebuilt this way never reuses an id, so a band or a selection keyed by a
  segment id survives the edit.
- Each intermediate draft must be a valid ring: closed, at least three segments, non-zero area,
  inside the coordinate limit and not self-intersecting. A gesture that would break the ring is
  rejected and leaves the last valid draft in place, so a corner simply stops at the boundary.
- **Apply** dispatches exactly one command — `panel.outline.set` for a panel,
  `object.contour.replace` for an object — and one undo entry is the whole edit. Enter applies and
  Escape cancels. A rejected apply keeps the draft so the user can fix it or cancel; a successful
  apply leaves the mode. An untouched draft dispatches no command and creates no empty undo entry.
- A selection change, a deleted target or a target that no longer stores a ring ends the mode.
- The whole mode is a client of the commands: the draft never reaches the document by another path.

## 7. Evaluation

- `furnicraft-param` compiles a constructive object into a domain-free `SolidPlan`: a base
  extrusion followed by boolean steps. The plan is derived, never written to the file, and rebuilt
  when a feature changes ([0041](../decisions/0041-every-solid-compiles-to-one-solid-plan.md)).
- A plan with one extrusion is evaluated analytically, because a linear extrusion is exact as
  polygons; that mesh is also what `object.collapse` bakes. A plan with a boolean goes through the
  geometry kernel port. Both paths share one evaluator, so collapse cannot drift from the viewport.
- `furnicraft-mesh` owns the evaluation cache: one entry per node, keyed by the node id and the
  inputs the mesh was derived from, tagged with the revision that built it. A rebuild walks the tree
  once and reuses every node whose inputs are unchanged, so a move, a rename or an edit elsewhere
  reuses the mesh and only changes the world transform. The derived mesh is never written to the file.
- A derivation input is anything the geometry depends on: a panel's outline, nominal size, machining,
  banding and band order, its resolved thickness and the catalogue-resolved band profiles; an
  object's whole geometry payload. A catalogue material change reaches the cache through the resolved
  thickness and profile, so a material swap rebuilds the panels that use it.
- Editing the contour or a feature parameter bumps the revision once; the next frame re-evaluates only
  the nodes it touched. A pass that visits fewer nodes drops the entries of nodes that no longer
  exist.
- An evaluation failure — no kernel, a contour the kernel rejects, a boolean that yields no solid —
  keeps the object and its data and reports a diagnostic on the node. It is not a load error and it
  does not delete anything.
- `object.collapse` refuses to run while the current evaluation fails.

## 8. Collapse

- `object.collapse` replaces `Constructive` with the evaluated `Mesh` in one command and one undo
  entry.
- The id, name, placement, pivot and properties are unchanged; the feature list is deleted.
- Afterwards the object is a mesh object: mesh commands apply, and the contour is gone. Undo is the
  only way back, which is why the UI asks for confirmation.
- Collapse is never automatic: a rebuild happens on every edit, and a failing evaluation is a
  diagnostic to fix, not a reason to bake.

## 9. References and topology

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

## 10. Commands

| Command | Parameters | Effect |
|---|---|---|
| `object.primitive.create` | parent, name, primitive kind, size, placement | creates a mesh object, returns its id |
| `object.extrude.create` | parent, name, contour, height, placement | creates a constructive object with one `Extrude` |
| `object.contour.replace` | object, contour | replaces the contour and rebuilds |
| `object.path.create` | parent, name, path, placement | creates a path object, returns its id |
| `object.path.replace` | object, path | replaces the drawing and keeps the node |
| `object.collapse` | object | replaces the payload with the evaluated mesh |
| `mesh.vertices.move` | object, list of (index, local position) | moves vertices as one change |

A face move is a `mesh.vertices.move` over the face's vertices; it does not need its own command.
Every command bumps the revision once and becomes one undo entry.

## 11. Tests

At minimum:

- a box primitive has eight vertices and six faces, and its triangulation has twelve triangles;
- validation rejects an out-of-bounds face index, a degenerate face, a non-finite coordinate and a
  file above the vertex cap;
- an extruded contour has the volume of its area times its height;
- a contour with an arc loads, round-trips, and extrudes to the volume of its analytic area;
- an open ring, a ring that does not meet its neighbour and a ring that crosses itself are rejected
  on construction and on load;
- replacing the contour changes the mesh and keeps the id, name and placement;
- a path is created open or closed and keeps its segments and ids, and replacing it changes only the
  drawing in one revision;
- a blank name, an unknown parent or a replace on an object that stores no path is refused without
  mutating the document;
- collapse produces the same mesh as the evaluation, clears the features and is one undo entry;
- `mesh.vertices.move` on a constructive object is refused;
- one out-of-bounds vertex index rejects the whole move;
- a failing evaluation keeps the object, its data and its placement and reports a diagnostic;
- a constructive object's mesh never appears in a saved file;
- contour editing starts only on a single node that stores a ring, and mesh editing and contour
  editing exclude each other;
- moving a corner and splitting a side change only the draft, and applying is one command and one
  undo entry;
- a split keeps the first part's id and the new part takes the next id; untouched segments keep
  theirs;
- an arc corner slides on its circle, and a corner off the circle is rejected;
- a draft that would open or cross the ring leaves the last valid shape in place, and cancelling the
  mode changes nothing.

## 12. Open questions

- Arcs in a profile: the kernel profile is a polyline, so an arc is sampled; the document keeps the arc.
- Holes in a profile.
- Booleans between two objects, and what the operands look like in the tree.
- Fillet and chamfer: they need a face or an edge of the result, so they wait for a naming design.
- Multi-body objects versus one object per solid.
- Normals and shading: flat now; smooth shading and hard edges are undecided.
- Whether an object gets its own material and texture, or inherits from its container.
- Snapping to mesh vertices and edges while modelling.
