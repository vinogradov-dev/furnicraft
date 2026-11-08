# 0071. A contact shadow is one transparent ground ellipse

- Status: Accepted
- Date: 2026-11-08

## Context

A cabinet built from flat panels floats in the floor grid: every panel meets the floor on a thin
edge, so nothing in the image says the object stands on something. A real shadow would need a
shadow map or a projected pass, which adds a depth target, a second camera and a frame cost that
can grow with the scene — exactly the cost the viewport must not pay for a visual cue.

The shadow is also not document data. It is a consequence of the scene and the view, like the grid,
the display style and the selection: a `.fcs` never carries it, and turning it off must not change
the model.

## Decision

- `ContactShadow` is frame data in `crates/scene`, beside `Grid`: a document `Id`, a centre on the
  ground plane and two half-axes. `ContactShadows` holds the switch, the peak strength and the
  gamma-space colour. Both are fields of `SceneFrame`, never of the document.
- One shadow is derived per root node whose **solid** subtree rests on the floor. The scene builder
  splits a subtree's bounds into everything it draws and the solid meshes only, so a flat drawing on
  the work plane — which lies exactly at `z = 0` — casts nothing. The lowest point must be within
  `FLOOR_TOLERANCE_MM` of the floor; a shelf mounted ten millimetres up does not qualify, and the
  committed placement decides, so a lifted object loses its shadow at the next evaluation.
- The shadow is an axis-aligned ellipse covering `SHADOW_FOOTPRINT_SCALE` (92 %) of the footprint
  half-extents around its centre, so it always stays inside the object's outline. It lies at
  `z = 0.01`, under the grid's `0.02`, so the lines draw over it and neither z-fights the floor.
  The bounds are those of the whole root object, so a cabinet is one soft blob instead of one dark
  blob per panel.
- The renderer draws the ellipses in one triangle pass: a fan whose centre vertex has full alpha and
  whose rim has none, so the fragment interpolates a radial falloff. The fragment entry is unlit and
  returns the uniform's shadow colour times the strength; it does not read the light. There is no
  texture, no shadow map and no additional render pass.
- The pass is alpha-blended, tests depth with `LessEqual`, writes no depth and owns no stencil slot.
  It draws after the opaque faces and before the grid, so an object hides the part of the shadow it
  covers and two overlapping footprints simply tint each other.
- The switch and the strength live in the display-settings popover as view state and are read from
  the frame at draw time; turning the pass off or moving the strength re-uploads nothing. The
  display-settings slider keeps the peak opacity, so zero strength draws nothing even with the
  switch on.
- A drag preview moves a matched shadow's centre with its object and keeps it on the ground plane;
  the ellipse stays world-axis aligned because it is a soft blob, not a silhouette.

## Consequences

A cabinet reads as standing on the floor for one triangle fan per object and no frame cost beyond a
few hundred vertices, and `cargo test -p furnicraft-scene` covers the derivation while
`cargo test -p furnicraft-render` covers the fan, the malformed input and the depth state without a
GPU. The shadow does not know about occlusion, self-shadowing, a directional light or the object's
real silhouette: an L-shaped cabinet gets one rectangle-wide ellipse, and a wall-mounted object with
a floor-touching part still casts the full footprint. Those are the costs of not having a shadow
map; a projected shadow map is a later decision with its own ADR, because it is the one feature here
that can cost a frame.

The shadow colour is black by default, which is honest on a dark viewport but nearly invisible on a
light theme until X8 repaints the frame background. Nested objects do not each cast a shadow; the
root object does, which is the behaviour a reader expects of a cabinet.
