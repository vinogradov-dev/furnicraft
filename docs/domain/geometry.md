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
[0042](../decisions/0042-a-contour-panel-keeps-its-ring-and-edits-a-working-copy.md),
[0048](../decisions/0048-a-flat-drawing-is-a-path-of-segments.md) and
[0054](../decisions/0054-a-primitive-keeps-the-parameters-that-built-it.md).

## 1. Two authoring paths

| | Mesh object | Constructive object |
|---|---|---|
| Created from | a primitive, or a collapsed payload | a closed path plus features |
| Source of truth | the mesh itself | the path and the feature parameters |
| Edited by | moving vertices and faces | editing the path or a feature |
| Mesh editing | yes | no, until `object.collapse` |
| History | undo only | the feature list, saved in the file |
| Mesh in the file | yes | no, it is derived |

The two paths are deliberately separate ([0037](../decisions/0037-a-geometry-object-is-a-mesh-or-a-feature-history.md)).
A mesh has no shape history; a primitive keeps its parameters and a constructive object keeps its
path, so both have a source of truth that survives an edit. A constructive object has no direct
mesh editing, so its path stays the only way to change it, and extruding a drawing makes that
drawing the body rather than copying it ([0074](../decisions/0074-an-extrusion-owns-the-path-it-grew-from.md)).
A path object is a further payload: a drawing rather than a solid, described in section 6; a
primitive is described in section 5.

## 2. The object node

- A geometry object is a node with a name, a parent, a placement, a stored pivot and typed
  properties, exactly like a container.
- It is a **leaf** in v1: it has no children. Several parts are grouped by wrapping them in a
  container, which already exists.
- The node carries one **geometry payload**: `Mesh`, `Constructive`, `Primitive` or `Path`. There is
  no separate node kind per payload and no separate "solid" node.
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

- `path`: the closed drawing the body grew from, with its segment ids and its optional shape tag.
- `features`: an ordered list. The first feature is `Extrude`.
- `Extrude { height }` extrudes the body's own path in its local XY plane along `+Z` from `z = 0` by
  `height`. The path is stored once, at the body; the ring the extrusion sweeps is derived on read
  (`Constructive::contour`). A body that has a second extrusion feature repeats the same path with
  its own height.
- The constructor requires a closed path that passes `Path::to_contour`; an open chain is refused
  before the document changes. Extruding a path object converts that node into the body in place and
  keeps its id, name, placement, pivot and properties, so the drawing is not left behind as a
  separate object ([0074](../decisions/0074-an-extrusion-owns-the-path-it-grew-from.md)).
- A contour is a ring of directed **segments**, at least three. A segment is a straight line between
  two points or a circular arc with a centre, a positive radius, a start angle and a non-zero sweep
  of at most one turn. Every value is stored in document units: micrometres and millidegrees.
- Every segment carries a stable id, unique inside its ring and never reused. An edit keeps the id of
  every segment it does not touch, so a command names a segment instead of an index; a fresh ring
  numbers its segments from one and keeps a counter for the next id.
- The ring must be closed — each segment's end meets the next segment's start within the joint
  tolerance: one micrometre plus `radius × 8.73e-6` of rounding slack for every arc at the joint, so
  a derived arc endpoint inside the document's precision still closes — and must enclose a non-zero
  area. The constructor stores it counter-clockwise, so the face winding does not depend on how the
  user drew it ([0072](../decisions/0072-a-contour-joint-is-judged-with-the-drawing-tolerance.md)).
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
  is invalid, and a body that stores no path, or names its path twice, is a readable load error.
- A version 18 file stored the contour inside its extrusion feature; it loads into the body's path,
  and every later save writes the path once (schema 19).
- A fresh extrusion opens a **pull arrow** at the contour's bounds centre, on the top face at the
  current height and pointing along the body's local `+Z`. Dragging it or typing a height writes
  `object.extrude.height.set`; a released drag commits one command and leaves the pull open, while
  Enter or a bare click finishes it. The last committed height stays in session view state, so the
  next extrusion starts there (100 mm at first); Escape drops the pull without touching the
  document, leaving the height the last command gave the body.

## 5. Primitive payload

- `Primitive` is a shape and the parameters that build it, stored as
  `Geometry::Primitive(Primitive)` (schema 18). It is never a mesh: the mesh is derived on read
  ([0054](../decisions/0054-a-primitive-keeps-the-parameters-that-built-it.md)).
- The inner tag is `shape`, because the outer payload already carries `kind`. The variants are
  `cube { edge }`, `cylinder { radius, height, segments }` and `sphere { radius, segments, bands }`.
- The parameter set is the shape's own. A cube has one edge and always six faces; only the curved
  shapes have a face count, and only a sphere has latitude bands. There is no generic tessellation
  field shared by shapes that do not need it.
- Every dimension is validated on construction and on load: at least 2 µm and at most 10 m. A curved
  shape takes 3 to 256 sides; a sphere takes 2 to 128 latitude bands. A rejected parameter set is a
  readable error, never an empty shape.
- `object.primitive.create` takes the full extent — the cube's edge or the diameter of a cylinder or
  sphere — and builds the default tessellation: 32 sides and 16 bands. A cylinder's height equals the
  extent, so one number is one shape.
- The counts and the bounds are analytic (`vertex_count`, `face_count`, `bounds_micrometres`), so the
  scene tree, the pivot and the camera frame never evaluate a mesh. The bound of a sphere is the
  sphere, not its tessellated polygon.
- `object.primitive.replace` stores a new validated parameter set in one revision and one undo entry,
  keeping the id, name, placement, pivot and properties. The card shows the fields the shape has,
  in millimetres and whole numbers.
- Collapsing a primitive bakes the derived mesh and drops the parameters, exactly as a constructive
  object drops its feature list. A baked mesh and a path cannot be collapsed.

## 6. Path payload

- A path is an open or closed chain of directed **segments** in the object's local XY plane at
  `z = 0`. It uses the same segment value and the same stable ids as a ring, so a line or an arc
  drawn once keeps its shape and its identity when it becomes a contour
  ([0048](../decisions/0048-a-flat-drawing-is-a-path-of-segments.md)).
- The drawing tools build a path from shapes: a line from two points, a polyline from its clicks, an
  arc through three points or from a centre with a start and an end, a rectangle from two opposite
  corners, from its centre, or from a centre plus a side direction, a circle from its centre and a
  rim point or through three points, and a regular polygon from a centre, a rim point and `3..=128`
  sides. A rectangle, a circle and a polygon are closed; a line and an arc stay open. Degenerate
  input — a rectangle with no width or height, a non-positive radius, or three coincident, collinear
  or otherwise impossible points — is an error, not a flat shape.
- In a polyline Tab chooses whether the next segment is a straight line or an arc; the closing click
  uses the same kind and always starts at the stored end of the previous segment. Alt suppresses the
  draft-end and grid snaps for one click and never changes the gesture, while Shift still locks the
  point to an axis ([0076](../decisions/0076-a-drawing-tool-separates-the-segment-kind-from-the-snaps.md)).
  Escape or the right button commits the figure and Ctrl+Esc or the ribbon's Cancel discards it.
- A committed figure lands as one `object.path.create` and immediately enters its own contour edit
  mode, so the grips are on screen without a second selection.
- Chain rules only: at least one segment, the segment and coordinate caps, unique ids, each joint
  meeting within the joint tolerance (one micrometre plus the arc rounding slack), and a closed
  chain closing its last joint. Self-intersection and enclosed area are ring rules, so they run when
  the path becomes a ring, not while it is drawn.
- `Path::to_contour` is the only transition. It refuses an open chain, applies the ring rules and
  keeps every id, so the ring is cut from exactly what the user drew.
- A path is a drawing, not a solid: it has no mesh, no feature list and no evaluation. The viewport
  draws it as an unlit line chain and picks it within a few screen points
  ([0018](../decisions/0018-picking-ray-casts-the-scene.md)).
- Extruding a closed path turns that node into a constructive body that stores this same path, so the
  drawing is not copied and the body can be reshaped through the contour editor.
- The document stores the drawing as drawn: direction and orientation are kept, and a clockwise
  closed path becomes a counter-clockwise ring only in `to_contour`.
- An object payload is `Geometry::Path(Path)` (schema 17). It cannot be collapsed: there is no solid
  to bake.

## 7. Contour editing

A constructive contour, a contour panel outline, a path chain and a contour cutout ring are reshaped
in one viewport mode. Its state is view state, not document data
([0029](../decisions/0029-view-state-belongs-to-the-session.md)).

- The mode is entered with exactly one selected node that stores a ring or a chain: a contour panel
  outline, a constructive object, a path object or a contour cutout on the selected panel. A
  rectangle panel, a rectangle cutout, a mesh object and a multi-selection have nothing to edit.
  Mesh editing and contour editing are mutually exclusive.
- A path is a chain rather than a ring: its free start and end are handles of their own, closing adds
  the bridge segment, and a corner after an arc is the stored arc end. A cutout ring is nominal
  panel-local and the panel node frames it; applying it replaces the record's shape alone.
- The editor keeps the stored ring as a **base** and a **draft** working copy. Every gesture rewrites
  only the draft, so the document, its revision and the undo history are untouched until the user
  applies. Leaving the mode without applying drops the draft.
- A corner handle sits at the start of every segment. Dragging it moves that corner in the node's
  local XY plane through its world placement; the two adjoining segments follow, and the rest of the
  ring is unchanged. The list of handles follows the figure the path was drawn as
  ([0075](../decisions/0075-a-grip-names-what-its-number-means.md)): a tagged circle adds its centre
  and four quadrants, a rectangle its side midpoints and centre, a polygon its radius point and
  centre, and a chain adds the centre and point-on-curve of each arc.
- A corner drag is **free** by default; the status bar switches it to **along the contour**. Then the
  segment nearest the pointer at the drag start becomes the support and the edit projects onto its
  line or circle for the whole drag. The target is resolved once, so crossing the junction does not
  switch segments half way.
- A tag constrains a free drag: a rectangle corner pins the opposite corner and keeps the right
  angles, a rectangle midpoint moves one side and leaves the opposite one, a circle quadrant scales
  the circle, and a polygon vertex keeps every side equal. A centre handle translates the figure
  without turning it or changing its size.
- A small numeric field follows the pointer while a grip is dragged. It is prefilled with the grip's
  current value and labelled with its dimension: a radius for a circle quadrant, an arc point or a
  hovered arc curve, an angle for an arc end, `W` or `H` for a rectangle side, the circumradius for a
  polygon radius point. Enter applies the same model setter the drag uses and keeps the edit mode;
  Escape restores the value the drag started from. A refused number leaves the field open and the
  draft untouched.
- Double-clicking a side splits it at the projected point. A split near a corner is refused so it
  cannot mint a segment a few micrometres long.
- A corner next to an arc is projected onto that arc's circle, so a drag trims or extends the sweep
  instead of redefining the curve. A free corner of a tagged figure takes the shape constraint
  instead. A point that misses the circle by more than a micrometre is rejected by the edit math.
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
- **Apply** dispatches exactly one command — `panel.outline.set` for a panel outline,
  `object.contour.replace` for the path inside a constructive body, `object.path.replace` for a path
  and `panel.machining.update` for a cutout ring — and one undo entry is the whole edit. A cutout
  update keeps the record's anchor and its `CutoutDepth`. Enter applies and Escape cancels. A rejected apply
  keeps the draft so the user can fix it or cancel; a successful apply leaves the mode. An untouched
  draft dispatches no command and creates no empty undo entry.
- A selection change, a deleted target or a target that no longer stores a ring or a chain ends the
  mode.
- The whole mode is a client of the commands: the draft never reaches the document by another path.

## 8. Evaluation

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
  object's whole geometry payload, which for a primitive is its shape and parameter set. A catalogue
  material change reaches the cache through the resolved thickness and profile, so a material swap
  rebuilds the panels that use it.
- A primitive is evaluated analytically in `furnicraft-mesh` (`evaluate_primitive`): the parameters
  are document data, the ring trigonometry runs at the geometry boundary and the result is rounded
  back to lengths. The viewport and `object.collapse` call that one function.
- Editing the contour, a feature parameter or a primitive parameter bumps the revision once; the next
  frame re-evaluates only the nodes it touched. A pass that visits fewer nodes drops the entries of
  nodes that no longer exist.
- An evaluation failure — no kernel, a contour the kernel rejects, a boolean that yields no solid —
  keeps the object and its data and reports a diagnostic on the node. It is not a load error and it
  does not delete anything.
- `object.collapse` refuses to run while the current evaluation fails.

## 9. Collapse

- `object.collapse` replaces a derived payload — `Constructive` or `Primitive` — with the evaluated
  `Mesh` in one command and one undo entry.
- The id, name, placement, pivot and properties are unchanged; the feature list or the parameters
  are deleted.
- Afterwards the object is a mesh object: mesh commands apply, and the contour is gone. Undo is the
  only way back, which is why the UI asks for confirmation.
- Collapse is never automatic: a rebuild happens on every edit, and a failing evaluation is a
  diagnostic to fix, not a reason to bake.

## 10. References and topology

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

## 11. Commands

| Command | Parameters | Effect |
|---|---|---|
| `object.primitive.create` | parent, name, primitive kind, size, placement | creates a parametric primitive with the default tessellation, returns its id |
| `object.primitive.replace` | object, primitive | replaces the parameters in one revision and keeps the node |
| `object.extrude.create` | parent, name, contour, height, placement | creates a constructive object with one `Extrude`; the contour becomes the body's path |
| `object.path.extrude` | object, height | turns a path node into the body that owns it, in place |
| `object.extrude.height.set` | object, height | replaces the first extrusion's height and keeps the path |
| `object.contour.replace` | object, path | replaces the body's path and rebuilds |
| `object.path.create` | parent, name, path, placement | creates a path object, returns its id |
| `object.path.replace` | object, path | replaces the drawing and keeps the node |
| `object.collapse` | object | replaces the payload with the evaluated mesh |
| `mesh.vertices.move` | object, list of (index, local position) | moves vertices as one change |

A face move is a `mesh.vertices.move` over the face's vertices; it does not need its own command.
Every command bumps the revision once and becomes one undo entry.

## 12. Tests

At minimum:

- a box primitive has eight vertices and six faces, and its triangulation has twelve triangles;
- a primitive stores its parameters and no mesh, round-trips through the file, and its counts and
  bounds follow the parameters; a rejected dimension or tessellation is a readable error;
- replacing the parameters is one revision and one undo entry and keeps the id, name and placement;
- a collapsed primitive bakes exactly the mesh the evaluator produces and drops the parameters;
- a primitive is reused by the cache until its parameters change;
- validation rejects an out-of-bounds face index, a degenerate face, a non-finite coordinate and a
  file above the vertex cap;
- an extruded contour has the volume of its area times its height;
- a contour with an arc loads, round-trips, and extrudes to the volume of its analytic area;
- an open ring, a ring that does not meet its neighbour and a ring that crosses itself are rejected
  on construction and on load;
- replacing the contour changes the mesh and keeps the id, name and placement;
- extruding a path replaces the node in place, leaves no sibling, is one undo entry and is refused on
  an open chain or a second time;
- replacing the body's path keeps its height, and setting the height keeps its path;
- a pull drag writes one height command, raises the body along its own axis, and the next extrusion
  starts at that height;
- every drawing tool builds its figure from its own clicks, and a committed figure opens its edit
  mode;
- Tab makes a polyline's next segment straight or an arc, and an arc that returns to the stored start
  closes the chain;
- a rejected intermediate preview keeps the last valid one and reports why, and the stored chain
  stays drawn;
- a version 18 extrusion contour loads as the body's path and a save writes no contour copy;
- a path is created open or closed and keeps its segments and ids, and replacing it changes only the
  drawing in one revision;
- a blank name, an unknown parent or a replace on an object that stores no path is refused without
  mutating the document;
- collapse produces the same mesh as the evaluation, clears the features or the parameters and is one
  undo entry;
- `mesh.vertices.move` on a constructive or primitive object is refused;
- one out-of-bounds vertex index rejects the whole move;
- a failing evaluation keeps the object, its data and its placement and reports a diagnostic;
- a constructive object's mesh never appears in a saved file, and neither does a primitive's;
- contour editing starts only on a single node that stores a ring or a chain, or on a contour cutout
  of the selected panel, and mesh editing and contour editing exclude each other;
- moving a corner and splitting a side change only the draft, and applying is one command and one
  undo entry;
- a cutout ring edit keeps its anchor and depth, and a ring outside the core is refused before the
  record changes;
- a split keeps the first part's id and the new part takes the next id; untouched segments keep
  theirs;
- an arc corner slides on its circle, and a corner off the circle is rejected;
- a circle, a rectangle, a polygon and an arc expose the grips their shape needs, and dragging one
  keeps the figure (a circle stays round, a rectangle keeps right angles, a polygon keeps its sides);
- a numeric field parses the grip's dimension and applies it, an angle for an arc end and a length
  for a radius or a side; Escape restores the drag-start value and a refused number changes nothing;
- free movement is the default, and an along-contour drag keeps the segment resolved at its start;
- a draft that would open or cross the ring leaves the last valid shape in place, and cancelling the
  mode changes nothing.

## 13. Open questions

- Arcs in a profile: the kernel profile is a polyline, so an arc is sampled; the document keeps the arc.
- Holes in a profile.
- Booleans between two objects, and what the operands look like in the tree.
- Fillet and chamfer: they need a face or an edge of the result, so they wait for a naming design.
- Multi-body objects versus one object per solid.
- Normals and shading: flat now; smooth shading and hard edges are undecided.
- Whether an object gets its own material and texture, or inherits from its container.
- Snapping to mesh vertices and edges while modelling.
