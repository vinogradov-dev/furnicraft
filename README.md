# FurniCraft

Native, free and open-source 3D editor for parametric cabinet furniture: set dimensions, panels and hardware, get a 3D model — and, later, drawings and CNC programs.

Built with Rust, `wgpu` and `egui`: a native binary, with no Electron, no Tauri, no web UI and no JavaScript.

## Quick start

```sh
cargo run -p furnicraft     # opens the editor
cargo test --workspace
cargo xtask layers          # checks the crate dependency rules
```

## Where to read next

- [`PROJECT.md`](PROJECT.md) — the product, the domain vocabulary and the crate map.
- [`AGENTS.md`](AGENTS.md) — the rules every change follows.
- [`BRIEF.md`](BRIEF.md) — the owner's goals and constraints (in Russian).
- [`plan/STATUS.md`](plan/STATUS.md) — the current development state lives only here.

## License

MIT OR Apache-2.0, at your option: [`LICENSE-MIT`](LICENSE-MIT), [`LICENSE-APACHE`](LICENSE-APACHE).
