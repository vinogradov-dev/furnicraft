# FurniCraft

Native, free and open-source 3D editor for parametric cabinet furniture: set dimensions, panels and hardware, get a 3D model — and, later, drawings and CNC programs.

Built with Rust, `wgpu` and `egui`. No Electron, no Tauri, no web UI: the desktop app is a native binary.

**Status:** M0 — workspace scaffold, no application logic yet.

```sh
cargo run -p furnicraft     # opens nothing yet (placeholder entry point)
cargo test --workspace
cargo xtask layers          # checks the crate dependency rules
```

Read [`PROJECT.md`](PROJECT.md) for the product and the crate map, [`AGENTS.md`](AGENTS.md) for the rules every change follows, and [`BRIEF.md`](BRIEF.md) for the owner's goals and constraints (in Russian).

License: MIT OR Apache-2.0 (license files are added before the first release).
