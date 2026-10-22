# Vendored code — rules

`vendor/truck-shapeops` is a copy of an upstream crate, not our code. Read
`vendor/truck-shapeops/PATCH.md` first: it states the one intended difference.

- Do not edit the Rust source to "fix" behaviour. If upstream is wrong, report it and work around it in
  `furnicraft-kernel-truck`.
- Any change to the copy must be recorded in `PATCH.md` (what and why) and, when the origin or the
  licence story changes, in `ATTRIBUTION.md` — in the same commit.
- Keep the upstream version, `LICENSE-APACHE` and the doc comments intact: the copy must stay diffable
  against the release it came from.
- When `truck` moves, update the copy, keep its `truck-base`, `truck-geometry`, `truck-geotrait`,
  `truck-topology` and `truck-meshalgo` versions consistent with the registry crates, and re-run
  `cargo xtask ci`.
- The copy exists for one reason: keeping C/C++ sources out of the dependency graph (`AGENTS.md` §2).
  Do not add features to it.
