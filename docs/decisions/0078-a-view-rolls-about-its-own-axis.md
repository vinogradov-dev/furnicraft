# 0078. A view rolls about its own axis

- Status: Accepted
- Date: 2026-11-09

## Context

The corner orientation widget gains a ring that turns the camera about the axis it is looking
along (U58). Until now a camera had yaw, pitch and distance only: `right()` was analytic in yaw,
`up()` was `right × forward`, and both stayed horizontal or upright at every direction. The readout
derived a "roll" as `asin(right.z)`, which is zero by construction for an orbit camera, so the
number never told the truth once a real roll existed.

A roll could be applied as a post-multiplied rotation on the view matrix, but that would leave
`right()`, `up()`, picking rays, panning, the work plane and the widget reading the unrolled basis.
The image and the interaction would disagree about which way is screen right.

## Decision

- `Camera` gains `roll: f64` in radians, zero in `Default`. It is view state like yaw and pitch and
  never enters the document (ADR 0029).
- `right()` and `up()` build the unrolled basis `right = (-sin yaw, cos yaw, 0)` and
  `up = right × forward`, then turn both about `forward` by the roll:
  `right = right·cos r − up·sin r`, `up = right·sin r + up·cos r`. A positive roll turns the image
  counter-clockwise, and the invariant `up = right × forward` still holds.
- `roll()` returns the field (radians). `roll_to(r)` wraps it into `0..TAU` and ignores non-finite
  input; `roll_by(d)` adds and wraps. Both refuse a camera pinned to an authored `FixedBasis`, as
  `orbit` does.
- `look_from` and `look_from_oriented` set the roll back to zero on a valid direction: a standard
  view is upright. `looked_from` pins a basis and leaves the stored field alone, and the pinned
  basis ignores it. `orbit`, `pan_pixels`, `zoom`, `toggle_projection` and `with_projection` keep
  the roll.
- `Leaf::roll_to` and `Leaf::roll_by` clear the preset name: a rolled view is no longer the preset's
  canonical basis. Presets, Home, Frame all and the axis balls reset through `look_from_oriented`.
- The readout reads `signed_degrees(camera.roll())` instead of the old `right.z` derivation, so a
  270° roll reads as `-90.0°` in the same signed presentation as yaw.

## Consequences

- Every consumer of `right()` and `up()` — the view matrix, `ray_ndc`, `project_ndc`, `pan_pixels`,
  the work plane and the widget — follows the roll through the two getters. There is no second
  roll path to keep in sync.
- A rolled view's `right` and `up` are no longer horizontal. Code that assumed `right.z == 0` must
  use the getters; the tests pin the view matrix, the picking ray and panning under a roll.
- The work plane follows the camera basis (ADR 0050), so a figure drawn in a rolled view lies
  rotated on the plane. That is the honest consequence of "the plane follows the view", not a
  separate feature.
- At a pole the canonical up is the zero-roll case. The ring can roll a top or bottom view
  afterwards without moving its eye, target or forward direction; this extends ADR 0057 rather
  than superseding it.
- An authored camera keeps the basis the document pins; its roll is the node's placement, not a
  viewport gesture.
