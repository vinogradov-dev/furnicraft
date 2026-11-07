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
- An edge list is derived data. `crates/mesh` computes one edge list per mesh, keyed by quantised
  vertex position so a flat-shaded mesh still yields one edge per geometric edge, and caches it in
  `NodeGeometry` beside the mesh under the same derivation inputs (ADR 0053). `Drawable` carries the
  shared `Arc<[u32]>`; the renderer transforms and flattens it, never searches.
- That list holds **form edges only**: an edge used by one triangle is a boundary, and an edge shared
  by two triangles is kept only when the surface turns across it by more than about 2.6°. A quad's
  triangulation diagonal and every other coplanar seam are dropped, so a box shows twelve edges and a
  triangular body shows its nine, not the wireframe of its triangles. The comparison is on the
  absolute normal dot product, so a coplanar pair emitted with opposite winding counts as flat too.
  The full triangulation is a second list beside it, drawn only when the *show every triangle edge*
  switch is on, which is a diagnostic view state and never a style.
- The edge pass is a `LineList` draw after the face pass. wgpu rejects `DepthBiasState` on a line
  topology, so the bias lives on the **faces**: the opaque triangle pipeline pushes them away from the
  camera by a constant depth unit, and the unbias line then wins the comparison. A constant depth bias
  is distance-independent, which a clip-space nudge is not — a fixed NDC offset moved a surface one
  metre away by ninety millimetres and one five metres away by more than a metre, so far edges punched
  through near panels.
- *Shaded with hidden edges* ignores depth but not ownership: the opaque face pass stamps each pixel
  with the drawable's stencil slot, and a hidden edge is drawn only where that slot is empty (the
  background) or equal to the line's own drawable. An object's inside stays visible, and nothing
  behind the frontmost object shows through it. *Wireframe* skips the face pass and draws the
  visible-edge pass alone.
- The edge ink follows the frame background: light ink on the dark viewport, dark ink when a light
  theme brings a light clear colour, so the form stays readable in both palettes.
- *Translucent* uses an alpha-blended face pipeline with no depth write and a shader entry that caps
  opacity, so the blend reveals what lies behind. Objects are ordered back to front by their
  world-space bounds centre, per view, in a reusable index list. Edges are off in this style.
- A style change writes only the frame field: the renderer reads it at draw time, so no geometry is
  re-uploaded and no pipeline is recreated. Switching the *show every triangle edge* switch does
  re-upload the edge list, because it reads the other cached list.

## Consequences

The style is testable as data, and `cargo test -p furnicraft-render` covers the pass mapping, the
translucent depth state, the hidden-edge stencil gate, the face depth bias and the back-to-front order
without a GPU; `cargo test -p furnicraft-mesh` covers the form-edge classification.

Sorting whole cabinet panels by centre is honest for flat panels and wrong for interpenetrating
solids; a per-triangle sort would need a different pipeline and is not attempted. The edge list is
memory beside every mesh, roughly two indices per geometric edge, and it is rebuilt only when the
mesh is. A line is still one pixel wide: widening it needs a quad pass, which remains an open gap.

The crease threshold is a fixed angle, so a finely tessellated cylinder (5.6° per facet at 64
segments) fades its facet seams and a coarse one shows them. That is the honest answer for a
faceted body derived from a segment count, and a future "show smooth seams" switch can raise the
threshold without touching the renderer. The hidden-edge stencil budget is 255 slots, counted from
one; a scene past that reuses the low values, which can only let one object's inside show through
another at a shared pixel, never invent an occluded object into view.
