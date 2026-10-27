---
name: enforce-architecture
description: Audit FurniCraft's architectural boundaries: crate layering, forbidden UI/GPU dependencies, dependency hygiene and the checks that enforce them. Use when adding or moving code, adding a crate or dependency, reviewing a change, or before finishing a task that touches the workspace.
license: MIT
compatibility: opencode
metadata:
  audience: agents
  workflow: architecture
---

# Enforce FurniCraft's architecture

The crate map is not a suggestion: a crate may depend only on **lower** layers, the document never touches the GPU, and the engine never touches the UI. This skill is the procedure for checking those boundaries before a task is called done.

## When to use

- Adding, moving or deleting code that crosses a crate boundary.
- Adding a workspace crate or an external dependency.
- Reviewing a change, or finishing any task that touches `Cargo.toml`, `crates/`, `apps/` or `xtask/`.

## Source of truth

| Document | Contains |
|---|---|
| `AGENTS.md` §3 | The layer map, the dependency rules, the forbidden edges in prose |
| `PROJECT.md` › Crate map | Crate responsibilities and the dependency table |
| `xtask/src/main.rs` | The machine-readable crate rules (`CRATES`, `DEV_ONLY`) |

When the three disagree, fix all of them in one change. Never "fix" a violation by weakening the rule: change the code or propose a rule change to the owner.

## Automated gate

```sh
cargo xtask layers
```

What it verifies today:

1. Every workspace crate (except `xtask`) is registered in `CRATES`; an unregistered crate fails the check.
2. Normal dependencies point only to **strictly lower** layers. Dev- and build-dependencies are skipped by design.
3. Domain crates (`units`, `geom`, `model`, `param`, `mesh`, `scene`, `engine`) do not depend on `egui`, `eframe`, `egui-wgpu`, `wgpu` or `winit`.
4. `render` does not depend on `egui`, `eframe`, `egui-wgpu`, `winit` or `furnicraft-model`.
5. `ui` does not depend on `wgpu`, `egui-wgpu` or `winit`.
6. `furnicraft-testkit` appears only under `[dev-dependencies]` (`DEV_ONLY`).

What it does **not** verify (see "Known gaps"): file I/O inside `model`, document mutations performed by `ui`, window creation inside `render`, the general dev/build-dependency policy beyond `testkit`, and dependency licenses. Those need the manual checklist below.

## Manual checklist by crate

### `furnicraft-units` (L0) — lengths

- Allowed: `serde`.
- Forbidden: anything graphical, the document, floats as the canonical representation of a physical size.
- Check: public API exposes `Length` (integer micrometres), not `f32`/`f64` millimetres.

### `furnicraft-geom` (L0) — math primitives

- Allowed: `glam`, `serde`.
- Forbidden: `furnicraft-model`, `wgpu`, `egui`, window/event types.
- Check: `glam` types are re-exported here so the rest of the workspace never depends on `glam` directly.

### `furnicraft-model` (L1) — document data

- Allowed: `furnicraft-units`, `serde`.
- Forbidden: `geom`, `mesh`, `scene`, `render`, `engine`, `ui`, `wgpu`, `egui`, `winit`, file/network/process APIs (`std::fs`, `std::net`, `std::process`), threads.
- Check: the crate is data plus invariants; loading and saving belong to a future I/O layer, not here.

### `furnicraft-param` (L2) — parametric resolve

- Allowed: `furnicraft-model`, `furnicraft-units`.
- Forbidden: `mesh`, `geom`, `scene`, `render`, `engine`, `ui`, GPU/UI crates.
- Check: resolution returns parts and typed errors; it does not build triangles.

### `furnicraft-mesh` (L3) — triangle meshes

- Allowed: `furnicraft-model`, `furnicraft-geom`, `furnicraft-units`.
- Forbidden: `scene`, `render`, `engine`, `ui`, `wgpu`, `egui`.
- Check: produces `geom` meshes; owns no GPU buffers, pipelines or devices.

### `furnicraft-scene` (L4) — frame description

- Allowed: `furnicraft-model`, `furnicraft-geom`, `furnicraft-mesh`.
- Forbidden: `wgpu`, `egui`, `eframe`, `winit`, `render`, `engine`, `ui`.
- Check: drawables, camera, lights and selection are plain data, testable without a GPU.

### `furnicraft-render` (L5) — wgpu renderer

- Allowed: `furnicraft-scene`, `furnicraft-geom`, `wgpu`, `bytemuck`.
- Forbidden: `egui`, `eframe`, `egui-wgpu`, `winit`, `furnicraft-engine`, `furnicraft-ui`, `furnicraft-model` (the scene already carries what to draw).
- Check: no window or event loop is created here; `egui_wgpu` integration lives in `apps/furnicraft`.
- Enforced by `cargo xtask layers` (`render` is outside the domain list but has its own forbidden set).

### `furnicraft-engine` (L6) — session and commands

- Allowed: `furnicraft-model`, `furnicraft-param`, `furnicraft-mesh`.
- Forbidden: `wgpu`, `egui`, `eframe`, `winit`, `furnicraft-scene`, `furnicraft-render`, `furnicraft-ui`.
- Check: every document mutation is a command (see the `create-command` skill); no rendering, no windows, no UI types in error messages.

### `furnicraft-ui` (L7) — egui shell

- Allowed: `egui`, `furnicraft-engine`, `furnicraft-model`.
- Forbidden: `wgpu`, `egui-wgpu`, `winit`, `furnicraft-render`, `furnicraft-scene` (the app passes what the UI needs).
- Check: no document mutation — no `&mut` on a model/session document, no `document_mut()`, no direct field assignment. Only `dispatch(command_id, params)` and reading session state.
- Enforced by `cargo xtask layers` for the GPU edge; the "no mutation" rule still needs review.

### `apps/furnicraft` (L7) — native shell

- Allowed: everything below; this is the composition root.
- Forbidden: domain rules, geometry algorithms, command implementations. The shell owns the window, input, the `egui_wgpu` paint callback and the event loop, and nothing else.
- Check: the only crate that may create a window or an `eframe` app.

### `furnicraft-testkit` (dev) — test helpers

- Allowed: anything in the workspace, for convenience.
- Forbidden: appearing in a normal `[dependencies]` table anywhere.
- Check: every reference is under `[dev-dependencies]`; enforced by `cargo xtask layers` (`DEV_ONLY`).

### `xtask` (tool)

- Allowed: enough to read `cargo metadata` (currently `serde_json`).
- Forbidden: being depended on by any product crate; containing product logic.

## Dependency hygiene

Before adding an external crate:

1. Justify it in one sentence in the change description; prefer the standard library and existing crates.
2. License must be MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0 or MPL-2.0. GPL/AGPL or anything needing an account or a paid tier: stop and ask the owner.
3. Add it to `[workspace.dependencies]` in the root `Cargo.toml`; crates reference it with `{ workspace = true }`.
4. Place it in the lowest crate that can use it. `wgpu` only in `render`, `egui` only in `ui` and the app, `serde` in L0–L6 as needed, `log` anywhere (its sink is installed by the app).
5. For a new workspace crate: register it in `CRATES` (`xtask/src/main.rs`) **and** in the `PROJECT.md` crate map in the same change.
6. No second GUI toolkit, no second GPU abstraction, no webview/Tauri/Electron, no JS runtime, no second async runtime.

## Fast audit

`cargo xtask layers` is the gate; the rest are spot checks for what the checker cannot see, and a quick way to locate a violation it reports.

```sh
# 1. Layers, registration, per-crate forbidden deps and dev-only crates.
cargo xtask layers

# 2. The engine must not see renderer, scene or UI crates.
cargo tree -p furnicraft-engine -e normal | grep -E 'furnicraft-(render|scene|ui)|wgpu|egui'

# 3. The renderer must stay egui-free (no window, no UI types).
cargo tree -p furnicraft-render -e normal | grep -E 'egui|eframe|winit'

# 4. The UI must stay GPU-free.
cargo tree -p furnicraft-ui -e normal | grep -E '\bwgpu\b|egui-wgpu'

# 5. No UI/GPU imports in the domain-side sources.
grep -rnE '\b(wgpu|egui|eframe|winit)::' crates/units/src crates/geom/src crates/model/src crates/param/src crates/mesh/src crates/scene/src crates/engine/src

# 6. The document model does not touch the filesystem, network or processes.
grep -rnE 'std::fs|std::net|std::process' crates/model/src

# 7. testkit is dev-only: every hit must be a [dev-dependencies] entry.
grep -rn 'furnicraft-testkit' --include=Cargo.toml .
```

Reading the results: for commands 2–6 an **empty** result is a pass; any line is a violation to investigate. In command 7, hits in the root `Cargo.toml` (`[workspace.dependencies]`) and in `crates/testkit/Cargo.toml` itself are normal; every other hit must be a `[dev-dependencies]` entry. `cargo tree -e normal` excludes dev- and build-dependencies, so a dev-only helper will not produce false positives there.

Also inspect the change itself, because no command catches these:

- `ui`: search for `document_mut`, `&mut self.document`, assignments to model fields, `session.document.` writes. There must be none.
- `render`: search for `winit`, `Window`, `Surface::create`, `eframe`, `egui`. There must be none.
- `apps/furnicraft`: does it contain geometry or command logic? It must not.
- `model`: does it import anything from a higher layer, even behind a feature flag? It must not.

## Review checklist

- [ ] New crates are in `CRATES`, `PROJECT.md` and have a `//!` doc comment naming their layer.
- [ ] Every normal dependency points to a strictly lower layer.
- [ ] Domain crates are free of `egui`/`wgpu`/window types.
- [ ] `render` has no `egui`; `ui` has no `wgpu`; `engine` has neither.
- [ ] `ui` does not mutate the document; it dispatches commands by id.
- [ ] `testkit` appears only under `[dev-dependencies]`.
- [ ] The app is a composition root, with no domain logic.
- [ ] New external dependencies have a one-line justification and a permissive license.
- [ ] `PROJECT.md`, `AGENTS.md` and `xtask/src/main.rs` agree with the code.
- [ ] `cargo xtask layers` is green.

## If a violation is found

Do not silence it. Pick the smallest honest fix:

1. **Move the type down.** If a lower crate needs a value from above, define the value (or a trait over data) in the lower crate and let the upper crate implement/convert it.
2. **Invert the dependency.** Pass data in, return data out: the renderer takes a scene, it does not ask the engine for one.
3. **Move the code up.** Rendering code that needs `egui` belongs in the app, not in `render`.
4. **Remove the dependency.** Often a type alias, a `Vec` and a small struct replace a crate; ask whether it is really needed.
5. If none of these is right, the rule may be wrong: bring the case to the owner instead of adding `#[allow]`, a feature flag or a private exception.

## Known gaps

The checker covers layering, per-crate forbidden dependencies and `testkit`'s dev-only policy. Still manual:

- General dev- and build-dependency policy beyond `testkit` (they are skipped by design: they do not ship).
- File/network/process access in `model`, document mutation in `ui`, window creation in `render`.
- Licenses and justification of dependencies.
- Source-level imports that `cargo metadata` cannot see (re-exports, feature-gated code).

Next improvements: a license allowlist check over the dependency graph, and an optional source scan that strips comments before looking for forbidden imports. When such a check lands, update this skill and the `AGENTS.md` checklist in the same change.

## Project status

The workspace is at M5: the document, the kernel adapter, the renderer and the editor exist, so the
boundaries are real rather than vacuous. Run `cargo xtask layers` in every task that touches manifests
or crate structure, and the manual audit whenever a change crosses a layer.
