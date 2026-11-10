# Session

```text
task: S8
goal: shared test fixtures (millimetres, open_path, sample_project) live once, in furnicraft-testkit

steps:
  [x] model::testing holds millimetres, open_path and sample_project behind the `testing` feature;
      testkit re-exports the three (ADR 0084)
  [x] the 4 crate-local open_path shapes now build through the shared one
  [x] the 20 local millimetres copies are gone; model, param and mesh have the testkit dev-dependency
  [x] model's fixtures.rs keeps only the fixtures its own tests need
  [x] model, map.md, PROJECT.md and docs/decisions/0084 updated
  [ ] cargo xtask ci green, then the commit and the plan bookkeeping

now: running the gate before the one move commit
blocked: nothing
run: cargo xtask ci
resume: cargo xtask ci, then git commit -m "test: share the millimetres, open_path and sample_project fixtures"
map: plan/map.md row "Test fixtures"
decisions: model cannot dev-depend on testkit (dev-dependency cycle compiles the model twice); the
  three shared fixtures therefore live in model::testing behind a non-default `testing` feature and
  testkit re-exports them (ADR 0084)
```
