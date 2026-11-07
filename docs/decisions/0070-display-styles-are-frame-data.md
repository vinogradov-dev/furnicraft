# 0070. Display styles are frame data

- Status: Accepted
- Date: 2026-11-07

## Context

The viewport cluster and the Settings window offered five display styles — *shaded with edges*,
*shaded*, *wireframe*, *shaded with hidden edges* and *translucent* — but the renderer ignored the
choice and always drew shaded triangles with no edges. A second, separate `edges` checkbox existed
beside the style, so the two surfaces could disagree about whether edges were drawn.

Two constraints shape the implementation. `PolygonMode::Line` is unavailable on Metal, and wgpu
cannot widen a one-pixel line, so a wireframe has to be a line-list pass over derived edges (ADR
0059). And `egui`/`wgpu` separation means the decision must be expressible without a GPU, or a style
could only be tested by looking at a window.

## Decision

- `ShadingStyle` lives in `crates/scene` and is a field of `SceneFrame`, beside the camera, grid and
  lighting. It is frame data, never document data: a `.fcs` does not carry it (ADR 0029). The
  settings file stores the default and the cluster stores the live choice; both are view state.
- The style alone decides the passes. `ShadingStyle` promises `draws_faces`, `draws_edges`,
  `edges_through_faces` and `is_translucent`; the renderer turns those into `StylePasses` without a
  device, and a test asserts the mapping. The separate `edges` flag is retired.
- An edge list is derived data. `crates/mesh` computes one deduplicated line list per mesh, keyed by
  quantised vertex position so a flat-shaded mesh still yields one edge per geometric edge, and
  caches it in `NodeGeometry` beside the mesh under the same derivation inputs (ADR 0053). `Drawable`
  carries the shared `Arc<[u32]>`; the renderer transforms and flattens it, never searches.
- The edge pass is a `LineList` draw after the face pass. wgpu rejects `DepthBiasState` on a line
  topology, so the edge vertex shader nudges the clip-space depth toward the camera by a constant;
  that wins against the face the line lies on without z-fighting. *Shaded with hidden edges* uses
  the same geometry with `CompareFunction::Always`. *Wireframe* skips the face pass and draws the
  edge pass alone.
- *Translucent* uses an alpha-blended face pipeline with no depth write and a shader entry that caps
  opacity, so the blend reveals what lies behind. Objects are ordered back to front by their
  world-space bounds centre, per view, in a reusable index list. Edges are off in this style.
- A style change writes only the frame field: the renderer reads it at draw time, so no geometry is
  re-uploaded and no pipeline is recreated.

## Consequences

The style is testable as data, and `cargo test -p furnicraft-render` covers the pass mapping, the
translucent depth state, the hidden-edge comparison and the back-to-front order without a GPU.

Sorting whole cabinet panels by centre is honest for flat panels and wrong for interpenetrating
solids; a per-triangle sort would need a different pipeline and is not attempted. The edge list is
memory beside every mesh, roughly two indices per geometric edge, and it is rebuilt only when the
mesh is. A line is still one pixel wide: widening it needs a quad pass, which remains an open gap.
