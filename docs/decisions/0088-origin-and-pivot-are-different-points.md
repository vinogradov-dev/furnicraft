# 0088. Origin and pivot are different points

- Status: Accepted; refines 0026 and 0082, and settles the panel exception in 0083
- Date: 2026-11-11

## Context

The word *pivot* had been used for two different things. The local zero of a node — the point its
vertices are written around and the point `Placement::translation` names — was called a creation
pivot in `FrameAnchor::CreationPivot`, `Path::creation_pivot` and `Project::add_object_with_pivot`.
The point both gizmos act around is a stored local offset, also called a pivot. The creation API
therefore passed the origin through a `Pivot` value that was always zero, and the panel card showed
the base corner as a read-only *local pivot*. The data model was already split for objects,
containers, cameras and lights; the vocabulary and the panel rule were not.

Without a written rule a later `rebase_origin` could silently move a pivot that the user had
authored, and a reader could not tell which field a caller meant.

## Decision

**Origin** is the start of the geometry's local coordinate system. It is implicit in
[`Placement`]: `origin_local = translation` and `origin_world = world_placement.translation`.
It is not a second stored field and not an editable handle. Vertices are written relative to it.
Its local definition changes only by a re-anchor; a move changes its world position without
changing the vertex numbers.

**Pivot** is the point a manipulation uses — rotate, insert, joint, alignment. It is stored as
`PivotLocal`, an exact micrometre offset from the origin. `pivot_world = origin_world +
rotation_world * pivot_local`. A zero offset means the pivot coincides with the origin.

The two formulas that every path must satisfy are:

```text
vertex_world[i] = translation + rotation_local * vertex_local[i]
pivot_world     = translation + rotation_local * pivot_local
```

### Operations

- `rebase_origin(strategy)` re-anchors the content. With `a` the chosen anchor in the old local
  frame: `vertex_local_new = vertex_local_old - a`, `translation_new = translation_old +
  rotation_old * a`, and `pivot_local_new = pivot_local_old - a`. The world vertices and
  `pivot_world` do not move. The subtraction is the same operation in a rotated parent; written in
  world terms it is `pivot_local_new = R_world^-1 * (pivot_world - origin_world_new)`, where
  `R_world` is the node's composed world rotation. A creation is the special case where the old
  pivot is zero.
- `set_pivot(local)` changes only `pivot_local`. The origin, `translation` and every vertex stay
  put; only `pivot_world` follows.
- `center_pivot()` writes the centre of the node's local bounds into `pivot_local`. It changes no
  origin and no vertex.
- `placement.world_zero` clears `translation` in the parent frame. `pivot_local` and every vertex
  stay put; the world origin and the world pivot move with the node.
- A move or a turn edits the placement. It never changes `pivot_local` and never rewrites a
  vertex.

The strategies are the ones `FrameAnchor` already names: `CreationOrigin` (an open chain's first
point, a closed ring's bounds centre), `BBoxBaseCorner` (a named bounds corner, min-X/min-Y by
default) and `CenterOfBounds` (a group's children union).

### Per-type rules

- **Path, Contour, Constructive, Mesh, Primitive.** The creation anchor is
  `CreationOrigin` for a path or contour; a primitive's origin is its own centre; a mesh is written
  by its caller. The pivot starts at zero and `set_pivot`/`center_pivot` may author it. A later
  `rebase_origin` shifts a path, contour, constructive or baked mesh; a primitive refuses it,
  because its parameters define a shape centred on its own origin and there is no free vertex
  offset to rewrite.
- **Panel.** The origin is the base corner of the outline. `pivot_local` is always zero, so the
  gizmo turns the panel about the same point the dimensions are measured from. `set_pivot` and
  `center_pivot` are refused with `PanelPivotIsDerived`. An outline edit re-anchors at the new
  base corner and compensates `translation`, machining and banding so every world point stays put;
  it sets `pivot_local = 0` again. This is the owner's choice for v1: a panel is a parametric
  entity, not a free mesh, and a separate hinge pivot is a later feature with its own field and
  migration.
- **Container.** A group's origin is the centre of its children's union, written into the
  translation by `group_nodes`; its children move by `-centre`, and its pivot starts at zero. An
  empty component is placed at the given placement with a zero pivot. A container may author a
  pivot later; a rebase carries it with `pivot_local_new = pivot_local_old - anchor`.
- **Camera, Light.** The placement is the insertion point, the origin, and the pivot starts at
  zero. Their pivot may be authored like any other node's.

### Serialization and legacy files

Origin is not serialized: `Placement::translation` is the only copy. The pivot field is
`pivot_local` from schema 20; a schema 19 or older file's `pivot` is read through a serde alias and
means the same offset. A panel with a non-zero stored pivot remains a readable load error, because
its pivot is derived. Containers saved before schema 6 keep the materialized bounds-centre pivot as
a legacy `pivot_local`; the migration does not re-anchor them, so no world point moves. Normalizing
such a container is an explicit future command, not a load-time rewrite.

## Consequences

- The inspector has two independent rows: *Origin (local to parent)* with a read-only *World
  origin*, and *Pivot (local offset)* with a read-only *Pivot (world)*. A panel shows
  *Pivot follows origin (offset 0)* instead of an editable pivot.
- A creation never invents a second handle: it writes the anchor into `translation` and a zero
  pivot. Only a later `set_pivot`/`center_pivot` authors a different pivot.
- `rebase_origin` is safe on an authored pivot because it preserves `pivot_world`; the panel's
  outline edit is the one operation that deliberately keeps its pivot at zero.
- The cost is one more named operation (`rebase_origin`) and a schema bump for a rename. There is
  no duplicate origin field to keep in sync.

## Related

- [0026](0026-the-manipulation-pivot-is-stored-node-data.md) stored the pivot and is refined here.
- [0082](0082-the-local-zero-is-the-creation-anchor.md) made the creation anchor the local zero
  and is the origin half of this record.
- [0083](0083-a-panel-is-anchored-at-its-base-corner.md) is the panel exception, confirmed here.
- [0027](0027-the-rotate-gizmo-uses-world-aligned-rings.md) and
  [0033](0033-a-multi-selection-is-placed-by-one-command.md) use `pivot_world`.
- `crates/model/src/frame.rs`, `crates/model/src/placement.rs`,
  `crates/model/src/project/{node_cmds,query,wire}.rs`,
  `crates/engine/src/node_cmds.rs`, `crates/ui/src/{placement_card,pivot_card,coords}.rs`.
