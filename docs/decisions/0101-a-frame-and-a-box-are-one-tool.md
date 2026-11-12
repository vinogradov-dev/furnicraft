# 0101. A frame and a box are one tool: draw the base, pull the height

- Status: Accepted; refines 0097, amends the creation flow of 0100 and retires the `cube` primitive
- Date: 2026-11-13

## Context

The editor's only solid primitive is a cube with one size for all three edges, and the owner calls it
useless: nothing in cabinet making is a cube, and a box with three independent sizes is what a maker
actually draws. The product frame is the same shape seen from another side — three lengths — and the
owner wants both created and edited the same way, so one toolkit serves the reserved space and the
solid.

The gesture already exists next door: the extrusion tool draws a base and pulls a height from a
wireframe preview (ADR 0093, X19). The owner asks for exactly that for a box: draw the rectangle on a
plane, fix it, then pull the height.

## Decision

**One box, two owners.** A box is three lengths — width, depth, height — plus a local frame. Its two
owners are the product **envelope** (a document field on the container, 0097) and the **cuboid
primitive** (a geometry parameter set, 0054). The editing and creation code is shared; the storage is
not, because one is document structure and the other is geometry.

**`Cube` is deleted and `Cuboid` replaces it.** `PrimitiveKind::Cube` and its one `size` become
`Cuboid` with width, depth and height, each validated by the existing primitive bounds. This is a
schema change with no migration (0096) and it touches the ribbon cell, its icon, the message table,
the mesh builder, the object card, the scene tree, the fixtures and the golden file — so it is one
task, not a rename buried in another.

**Creation is one gesture, shared with the extrusion.** Activate the tool, draw the base rectangle in
the active work plane with the 1 mm step, fix it, then pull the height along the plane's normal with
the wireframe-prism preview. One click on the ✓ commits one command — `product.create` for a frame,
`object.primitive.create` for a cuboid — and one undo entry. The frame's card keeps its numeric path
(three fields and the type), so a maker can type the envelope instead of dragging it; the gesture and
the card reach the same command.

**Editing is three paths to the same field.**

| Gesture | Changes | Command |
|---|---|---|
| corner grip | two sizes | the owner's own command (`product.envelope.set` / `object.primitive.replace`) |
| face grip (highlighted, then dragged) | one size | the same |
| card fields | three numbers | the same |

Each gesture is one undo entry (the edit scope of ADR 0092). The grips are the grips the panel and the
contour already use; the frame shows them when it is selected, because the dashed lines are hidden in
the closed-with-parts state (0100) and editing the reserved space must stay possible.

**The box anchors at its base min corner.** The local zero is the corner of the base rectangle, as a
panel's zero is its base corner (0083), so the space a frame reserves is `[0..W] × [0..D] × [0..H]` in
its own frame and a report or a niche query needs no offset. Dragging a **max** face changes the size;
dragging a **min** face changes the size and translates the placement so the opposite face stays put in
world space — the same compensation the origin rebase and the contour re-anchor use (0082, 0088).
Cylinder and sphere keep their centre anchor until they are drawn by the same tool; the asymmetry is
recorded, not hidden.

**Constraints.** Grips move in 1 mm steps; typed numbers stay micrometres; a box is validated
(positive, bounded, finite) and a bad value is a typed error, never a clamp. The frame's envelope is
never derived from its children (0097) and a cuboid's mesh is never stored (0054).

## Consequences

- The ribbon's cube cell becomes the box tool, and the sample scene and the golden file lose their
  cubes. The old `PrimitiveKind::Cube` files from development builds stop loading, which 0096 allows.
- One box editor means one set of tests for two owners, and the second owner (the frame) proves the
  abstraction instead of a copy.
- The frame's creation now has two entries — the gesture and the card — and both dispatch
  `product.create`; nothing else may create a product.
- A future shape (a rounded box, a sloped envelope) plugs into the same grip and preview machinery only
  if it keeps the three-length base; a shape that does not is a new tool and a new decision.
- The extrusion and the box share their preview, their step and their commit shape, so a maker learns
  one gesture and uses it for a bar, a box and a piece of furniture.

## Related

- [0054](0054-a-primitive-keeps-the-parameters-that-built-it.md) — the primitive stores parameters.
- [0093](0093-a-preview-fills-a-session-overlay.md) — the preview the height pull and the face drag use.
- [0082](0082-the-local-zero-is-the-creation-anchor.md),
  [0083](0083-a-panel-is-anchored-at-its-base-corner.md) — the anchor and the world-still rule.
- [0096](0096-before-the-first-release-the-format-has-no-past.md) — why changing the primitive kind
  costs no migration.
- [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) — the envelope this edits.
- [`docs/domain/products.md`](../domain/products.md) — the product side of the shared box.
