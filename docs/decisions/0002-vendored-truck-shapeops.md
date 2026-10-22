# 0002. Vendored `truck-shapeops` without the VTK stack

- Status: Accepted
- Date: 2026-10-22

## Context

`truck-shapeops` 0.4.0 needs `truck-meshalgo` to tessellate boolean results. Upstream enables that
crate's default features, which pull `vtkio` and through it `xz2` → `lzma-sys`: C sources compiled by
our build. `AGENTS.md` §2 and §4.8 forbid that, and no feature of `truck-shapeops` turns it off.

## Decision

`vendor/truck-shapeops` keeps the upstream source unchanged, plus one `#[allow(dead_code)]` that the
narrowed feature set requires, and narrows a single dependency:

```toml
truck-meshalgo = { version = "0.4.0", default-features = false, features = ["analyzers", "tessellation"] }
```

The root `Cargo.toml` points `[patch.crates-io]` at the copy, `PATCH.md` records the difference and
`ATTRIBUTION.md` records the origin and the licence.

## Consequences

- The dependency graph stays pure Rust and `cargo xtask layers` passes its C-vendoring check.
- Updating `truck` means updating the copy by hand, and its `truck-base`, `truck-geometry`,
  `truck-geotrait`, `truck-topology` and `truck-meshalgo` versions must stay in step with the registry
  crates.
- Exit condition: when upstream lets a caller disable the VTK features, delete `vendor/`, remove the
  patch and mark this record superseded.
