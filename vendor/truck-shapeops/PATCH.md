# Vendored `truck-shapeops` 0.4.0

Source: <https://github.com/ricosjp/truck>, crate `truck-shapeops` 0.4.0,
licensed under Apache-2.0.

FurniCraft vendors this crate for one reason: upstream depends on `truck-meshalgo`
with default features, which enables `vtkio` and its C compression backend
(`xz2` → `lzma-sys`). FurniCraft does not compile C or C++ code in its
dependencies. This copy only changes the `truck-meshalgo` dependency to:

```toml
truck-meshalgo = { version = "0.4.0", default-features = false, features = ["analyzers", "tessellation"] }
```

The only source change is an `#[allow(dead_code)]` on the internal `Alternative`
enum, whose module is private; without the VTK feature set rustc warns about it.
When upstream exposes a way to disable the VTK features from `truck-shapeops`,
drop this vendored copy and return to the registry crate.
