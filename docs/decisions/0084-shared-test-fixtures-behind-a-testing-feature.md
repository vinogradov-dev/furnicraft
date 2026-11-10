# 0084. Shared test fixtures live in `furnicraft-model` behind a `testing` feature

- Status: Accepted
- Date: 2026-11-10

## Context

Every crate's tests wrote their own `fn millimetres`, and the fixture helpers `millimetres`,
`open_path` and `sample_project` were copied into 26 test modules across eight crates. The obvious
home is `furnicraft-testkit`, which exists for exactly this and already depends on `furnicraft-model`.
For seven crates that works. For `furnicraft-model` it cannot: the crate's own tests are unit tests
inside the library target, so a `[dev-dependencies]` entry on `furnicraft-testkit` closes a cycle
(`model` → dev `testkit` → `model`). Cargo compiles the library twice for that graph, and every
function `testkit` returns — `Project`, `Path` — comes back as a type from the *other* compilation,
so `Err(ProjectError::…)` from the fixture no longer matches the model's own `ProjectError`.

`millimetres` alone survives the cycle because `furnicraft-units` sits outside it. `sample_project`
and `open_path` do not, and they are the two fixtures that must not be duplicated: a sample project
that drifts between model tests and engine tests is exactly the defect C7 forbids.

## Decision

- The three shared fixtures are defined once, in `crates/model/src/testing.rs`: `millimetres`,
  `open_path(points)`, `sample_project`.
- They are exposed under the non-default feature `testing`
  (`#[cfg(any(test, feature = "testing"))] pub mod testing;`), so the model's own tests reach them
  without a dependency edge at all, and `furnicraft-testkit` reaches them by enabling the feature.
- `furnicraft-testkit` re-exports the three and keeps the fixtures only it needs
  (`sample_catalog`, `sample_cutout_rect`, …). Product crates keep writing `furnicraft_testkit::…`.
- A shape a single crate's tests need — a 100 × 60 mm chain, a 600 × 400 mm one — stays local and is
  built *through* `open_path`, so the rule for building one has one definition while the shape keeps
  its name at the call site.
- `model`, `param` and `mesh` gain `furnicraft-testkit` as a dev-dependency; `model` does not, because
  it cannot.

## Consequences

The feature is off in a release build, so nothing of `testing` ships: `cargo build` sees the model
without it, `cargo test` sees it. The cost is one feature and one public module in the document
crate, which a reader may mistake for product API; the module header says it is not, and a release
build cannot name it. If Cargo ever stops duplicating a crate in a dev-dependency cycle, the module
can move into `furnicraft-testkit` unchanged.
