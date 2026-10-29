# 0039. Point lights use watts and inverse-square falloff

- Status: Accepted
- Date: 2026-10-29

## Context

A light node stores a kind, a colour and an intensity ([0036](0036-cameras-and-lights-are-scene-objects.md)).
The first version only shaded the first sun light; a point light was saved and undoable but did not
affect the frame, so moving it or changing its intensity changed nothing. A lamp needs a distance
model, while a sun does not: it is directional and its brightness is independent of where its node
sits.

A furniture scene is authored at room scale and should eventually render a realistic picture.
Physical units and inverse-square falloff are the part of that goal that belongs in the document and
the lighting model, not in a later tone mapper. Blender was chosen as the reference because the owner
works with it: a point light is a power in watts, a sun is an irradiance in watts per square metre,
and both follow the inverse-square law.

## Decision

- A sun's intensity stays a multiplier and it has no distance falloff. The first sun in tree order
  supplies the key direction; the built-in key light remains the fallback when a scene has no sun.
- A point light's intensity is radiant power in **watts**. It radiates from its world position and
  the renderer applies `1 / (4 * pi * d^2)` falloff. There is no hard range cutoff.
- Point lights add to the key: up to `MAX_POINT_LIGHTS` (eight) in tree order light one frame, and
  further point lights are kept in the document but do not shade the viewport. The limit keeps the
  uniform fixed and the fragment loop bounded.
- The viewport applies a fixed exposure constant so a 1000 W point light at one metre shades a white
  surface at about half intensity. It is an exposure, not part of the light data: a scene exposure
  control can replace the constant later without rewriting a single light.
- Colour stays linear RGB in `[0, 1]`; power and colour are independent.

## Consequences

- A point light now behaves like a lamp: moving it away darkens its contribution, moving it closer
  brightens it, and its power is a number the owner can compare with Blender.
- Existing point lights were saved with an intensity that never had a visual effect. The same number
  is now read as watts, so a file written with `2.0` is a very dim 2 W lamp. No schema bump is needed
  because no field changed and no document ever depended on the old behaviour.
- A scene with more than eight point lights is lit by the first eight in tree order. A later task can
  add culling by distance, an importance order or a clustered light list.
- The shader still writes gamma-corrected linear colour and has no tone mapper. A proper view
  transform and a scene exposure belong to the render milestone; the fixed exposure is the seam
  where they will arrive.
- Shadows, spot lights, area lights and IES profiles remain out of scope until a light object needs
  them.

## Related

- [0036](0036-cameras-and-lights-are-scene-objects.md) — cameras and lights are scene objects.
- [`docs/domain/document.md`](../domain/document.md) — the light fields and their units.
- [`crates/render/src/scene.wgsl`](../../crates/render/src/scene.wgsl) — the falloff and fixed exposure.
