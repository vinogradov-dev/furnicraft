# 0074. An extrusion owns the path it grew from

- Status: Accepted
- Date: 2026-11-08

## Context
A constructive object stored a closed `Contour` inside its `Extrude` feature: the ring was the
source of truth and the feature list hung off it. The drawing tools produce a `Path`, not a ring, so
extruding a drawing had to convert it with `Path::to_contour`, hand the ring to a second object and
leave the drawing in the tree beside the new body. The user then had two nodes to keep in step, and
the drawing was no longer the drawing the body came from — it was a copy of it.

The ring and the path differ in more than a wrapper: a path is open or closed, carries the shape tag
of ADR 0073 and keeps the ids a grip or a band refers to. Copying the ring into the feature threw
the tag and the chain semantics away and created a second thing to edit.

## Decision
The constructive payload is the path plus the feature history:

```rust
pub struct Constructive {
    path: Path,
    features: Vec<Feature>,
}

pub enum Feature {
    Extrude(Extrude),
}

pub struct Extrude {
    height: Length,
}
```

- `Constructive::new(path, features)` requires a closed path that passes `Path::to_contour`, so an
  open chain is refused before the document changes. The ring is **derived on read** by
  `Constructive::contour`; it is never stored twice.
- `object.path.extrude { object, height }` converts a `Path` node into the body **in place**: the
  node keeps its id, name, placement, pivot and properties, and no sibling is created. A second
  extrusion of the same node is `ProjectError::NotAPathObject`, because the node is a body now.
- `object.contour.replace { object, path }` stores a new closed path in the body and keeps every
  feature, including the height. The mesh cache keys on the whole `Constructive`, so a changed path
  rebuilds the body and an unchanged one is reused.
- `object.extrude.height.set { object, height }` replaces the first extrusion's height and leaves
  the path untouched. This is the command behind the pull arrow (ADR 0075).
- The document stores a body's path **once**, with its shape tag and its segment ids. A
  split or a reshape of that path edits the body; it is not a copy that can drift.
- Schema 19 carries the shape. A version 18 file stored `extrude.contour`; `ConstructiveWire` reads
  it and moves the contour into `constructive.path`. A file that names the path twice, or carries
  two legacy contours, is a readable error rather than a silent pick. No released build wrote schema
  19, so the migration rides on the same version as the shape tag.

## Consequences
"Absorb the contour" becomes literal: the body is the drawing, and the selection, the scene tree and
undo all point at one node. The model loses the ability to store a ring that is not a path; a panel
still owns a `Contour`, because a panel outline is manufacturing geometry, not a drawing. The cost is
a derived ring on every read of a contour — cheap, and already what the mesh build does. A future
feature that needs the ring as its input takes it from `Constructive::contour`, never from a stored
copy.
