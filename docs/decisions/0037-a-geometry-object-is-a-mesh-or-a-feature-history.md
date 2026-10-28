# 0037. A geometry object is a baked mesh or a feature history

- Status: Accepted
- Date: 2026-10-28

## Context

Two authoring paths coexist in every CAD and mesh editor, and they have opposite edit semantics. A
primitive edited as a mesh has no shape history: moving a vertex is the edit, and undo is the only
record. A contour extruded by a feature is the opposite: the sketch and the feature parameters are
the source, the solid is derived, and its faces are not directly editable.

Furniture work needs both. A hardware bracket or a decorative part is modelled by pushing vertices; a
countertop is extruded from a contour and must stay editable through that contour after a dimension
changes.

One representation cannot serve both. A feature tree over a sculpted mesh forces every vertex move
into a feature; a bare mesh loses the contour. The tempting middle ground — keep the feature history
and also let the user edit the evaluated mesh — requires stable names for derived faces and edges
("persistent naming"). That problem has no general solution, and it would delay the first geometry
object by months.

## Decision

- A node that carries geometry holds exactly one of two payloads:
  - `Mesh` — positions and polygon faces; the mesh is the source of truth and is edited directly;
  - `Constructive` — an ordered list of features; the mesh is derived, never saved, and not editable.
- A mesh object is created from a primitive (a box first; cylinders and others later) and edited with
  mesh commands. It has no feature history: undo is its only history. Changing "the box size" as a
  parameter is not possible, by design.
- A constructive object is created from a closed flat contour and rebuilt from it. Editing the
  contour or a feature parameter rebuilds the derived mesh; the mesh itself stays read-only.
- `object.collapse` replaces a constructive object with its evaluated mesh in one command and one
  undo entry. It is the only way to make such an object editable as a mesh, and afterwards the
  history is gone. It is reversible only through undo.
- Features reference their inputs — a contour, source bodies, numeric parameters — never a derived
  face or edge of the result. Fillet, chamfer and similar operations on an arbitrary face produced by
  an earlier feature are out of scope until a persistent-naming design exists.
- Mesh element references are indices valid within one revision. A command that changes topology
  invalidates them, and a selection that refers to vanished elements is cleared. The selection lives
  in the session, not in the document.
- Feature parameters are document data and typed like every other value: lengths are `Length`, angles
  are `Degrees`. Floating point appears only at the kernel and render boundaries.

## Consequences

- Both authoring styles live in one editor, and the difference is visible: a constructive object shows
  its contour and features, a mesh object shows vertices and faces.
- The document carries two geometry kinds for good. A new node kind does not appear every time a
  modelling tool arrives.
- A primitive cannot be re-parameterised. A user who wants a parametrically sized box builds it from
  a contour; that is the deliberate price of avoiding persistent naming.
- Collapse is a one-way door, so the command must be undoable and the UI must warn.
- Booleans and fillets between constructive objects remain possible later because they act on inputs;
  features that must point at a face of a result wait for a naming design.

## Related

- [`docs/domain/geometry.md`](../domain/geometry.md) — the object, its payloads, evaluation and
  collapse.
- [0034](0034-nodes-carry-typed-properties.md) — every node, including a geometry object, carries
  typed properties.
- [0029](0029-view-state-belongs-to-the-session.md) — mesh selection is session state.
