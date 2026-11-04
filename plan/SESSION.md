# Session

```text
task: U12
goal: Drawing knows the world plane its pixels land on: the six standard ortho views select XY, XZ or YZ, every other view uses world XY.

steps:
  [ ] `WorkPlane` with a right-handed basis and the world XY fallback
  [ ] `from_camera`: top and bottom by the clamped pitch, sides by an exact quarter-turn yaw
  [ ] `ray_point`, `world`, `local` and `pointer_point`
  [ ] tests for the six standard views, the fallback and the round trip
  [ ] the strip label and the closing commit

now: `apps/furnicraft/src/gizmo_math.rs`, adding `WorkPlane`
blocked: nothing
run: cargo test -p furnicraft
resume: open `apps/furnicraft/src/gizmo_math.rs` after `ray_plane` and add `WorkPlane`
map: the "View camera and projection" row names `Camera`; the work plane lives in `gizmo_math.rs`
decisions: the plane passes through the world origin; its u/v axes follow the screen, so a chain reads the way it was drawn; only an exact axis-aligned ortho view selects a side plane, and the top and bottom views are classified by the clamped pitch because they stop five degrees short of the pole; every other view uses world XY
```
