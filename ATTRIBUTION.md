# Third-party attributions

FurniCraft uses permissively licensed dependencies. This file records components that are
vendored, bundled or otherwise shipped with the product; ordinary crates.io dependencies keep
their licenses in the Cargo registry and are not repeated here.

| Component | Where | License | Copyright | Notes |
|---|---|---|---|---|
| `truck` (modeling, geometry, topology, meshalgo, polymesh, base, geotrait) | Cargo dependencies | Apache-2.0 | RICOS Co., Ltd. and contributors | Geometry kernel; only `furnicraft-kernel-truck` may use it. |
| `truck-shapeops` 0.4.0 | `vendor/truck-shapeops` | Apache-2.0 | RICOS Co., Ltd. and contributors | Vendored from <https://github.com/ricosjp/truck>. Rust source unchanged; Cargo.toml narrows `truck-meshalgo` features to `analyzers` and `tessellation` to exclude the VTK/C compression stack. See `PATCH.md`. |
