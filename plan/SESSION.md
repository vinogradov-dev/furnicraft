# Session

task: S2
goal: one domain-free solid plan in `furnicraft-param` builds every solid; the constructive extrusion and the panel cutout tool are its first users, with scene behaviour unchanged.

steps:
  [x] inventory the body-building paths: `build_constructive_mesh`, `build_panel`, `build_cutout_tool`, banding extrudes; list the ops they share
  [x] add `crates/param/src/solid.rs`: `SolidPlan`, `SolidOp` as a base extrusion plus boolean steps, `PlanBuilder` and the backend trait
  [x] add `evaluate(plan, kernel) -> BodyId` and `evaluate_with(plan, backend)`; attachments stay out until D6
  [x] add `crates/param/src/constructive.rs`: the analytic single-extrusion backend that produces the document `Mesh`
  [x] move `build_constructive_mesh` onto the plan and keep the collapse mesh identical
  [x] move `build_cutout_tool`, the panel base and every band extrusion onto the plan; panel, bands and cutouts keep their current output
  [x] tests: rectangle volume object vs panel, cutout subtraction, kernel error, fast-path parity, nesting bound
  [x] write ADR 0041 and link it from `docs/domain/geometry.md`; update `plan/map.md`
  [x] run `cargo xtask ci`

now: commit the solid plan and close S2
blocked: nothing
run: cargo xtask ci
resume: cargo xtask ci, then commit the solid plan and close the task
map: `plan/map.md` row "Solid plan and evaluation" — `crates/param/src/solid.rs`, `crates/param/src/constructive.rs`
decisions: the plan is derived and never serialized; `param::solid` imports only `geom` and `kernel`; a plan is a base extrusion plus boolean steps, and a tool is itself a plan; the analytic backend answers exactly one world-aligned extrusion and refuses a boolean; attachments and the bend post-stage wait for D6; no new crate
