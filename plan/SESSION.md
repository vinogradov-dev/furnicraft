task: S11
goal: `geom/region.rs` holds only the general boolean (`boolean.rs`), the ring model lives in `ring.rs`

steps:
  [x] green baseline: `cargo xtask ci` before the move
  [ ] `Region`, `RegionSamples`, `RingKind`, `RegionError`, the area/containment/validation helpers
      and `loop_to_contour` move to `crates/geom/src/ring.rs`
  [ ] the rest becomes `crates/geom/src/boolean.rs` with its method, reference and refusals in the header
  [ ] `lib.rs` keeps every public path; `rect.rs` imports from `ring`
  [ ] `plan/map.md` row for planar regions names both files; ADR 0043 link updated

now: the ring/boolean split of `crates/geom/src/region.rs` (1126 production lines)
blocked: nothing
run: cargo test -p furnicraft-geom
resume: `sed -n 26,415p crates/geom/src/region.rs` is the ring body, `sed -n 421,1124p` the boolean
map: `plan/map.md` row 30, planar regions and booleans
decisions: `ANGLE_EPSILON`, `POINT_EPSILON`, `contour_area`, `ring_contains`, `loop_to_contour` and
  `turn_angle` stay in `ring.rs` as `pub(crate)`; `boolean.rs` imports them, so each constant keeps
  one definition (C7)
