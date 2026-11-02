# Session

task: S2
goal: one domain-free solid plan in `furnicraft-param` builds every solid; the constructive extrusion and the panel cutout tool are its first users, with scene behaviour unchanged.

steps:
  [ ] inventory the body-building paths: `build_constructive_mesh`, `build_panel`, `build_cutout_tool`, banding extrudes; list the ops they share
  [ ] add `crates/param/src/solid.rs`: `SolidPlan`, `SolidOp::{Extrude, Boolean}` and `PlanBuilder`
  [ ] add `evaluate(plan, kernel) -> BodyId`; the plan carries a body and its attachments
  [ ] add `crates/param/src/constructive.rs`: the analytic single-extrusion backend that produces the document `Mesh`
  [ ] move `build_constructive_mesh` onto the plan and keep the collapse mesh identical
  [ ] move `build_cutout_tool` onto the plan; panel body, bands and cutouts keep their current output
  [ ] tests: rectangle volume object vs panel, cutout subtraction, kernel error, fast-path parity
  [ ] write the shape-layer ADR and link it from `docs/domain/geometry.md`
  [ ] run `cargo xtask ci`

now: create `crates/param/src/solid.rs` with the plan types and `evaluate`; add `mod solid;` to `crates/param/src/lib.rs`
blocked: nothing
run: cargo test -p furnicraft-param --lib
resume: write `SolidOp::Extrude`, `SolidPlan` and `evaluate` in `crates/param/src/solid.rs`, then compile the module
map: `plan/map.md` row "Constructive evaluation" — `crates/param/src/lib.rs` (`build_constructive_mesh`, `build_panel`, `build_cutout_tool`)
decisions: the plan is derived and never serialized; `param::solid` does not import `model`; the analytic single-extrusion backend stays for `object.collapse`; no new crate now
