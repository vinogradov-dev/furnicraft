# AGENTS.md: guide for AI agents and contributors

FurniCraft is a free, open-source, native 3D editor for **cabinet furniture**, written in **Rust only**. The user sets dimensions, panels, materials and hardware; the editor builds a parametric 3D model and, later, drawings and CNC programs. The quality bar for interaction is Blender: smooth, native, predictable. Read `BRIEF.md` for the owner's goals and constraints, and `PROJECT.md` for the product and the crate map.

The application is a native binary: `wgpu` for graphics, `egui`/`eframe` for UI. **No Electron, Tauri, webview, JavaScript or TypeScript — never**, not as a dependency, build step or packaging target. The product name is always **FurniCraft** in user-facing text (`FurniCraft` in prose, `furnicraft-*` in machine names).

This file is the constitution. Where a task or a habit conflicts with it, this file wins: ask the owner instead of bending a rule.

## 1. Orientation

| Read | Why |
|---|---|
| `PROJECT.md` | Vision, domain vocabulary, crate map, milestones |
| `BRIEF.md` | The owner's goals, constraints and preferences (Russian) |
| `crates/<name>/src/lib.rs` | Each crate's doc comment states its layer and job |
| `xtask/src/main.rs` | The layer map that `cargo xtask layers` enforces |

## 2. Non-negotiable constraints

- **Rust only.** No JS/TS, no scripting runtime, no code generation into other languages.
- **Native.** Windows, Linux and macOS from one codebase. No webview shells, no browser as the primary target.
- **Free and accessible.** Dependencies must be free and permissively licensed: MIT, Apache-2.0, BSD-2-Clause/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0. GPL/AGPL or anything requiring an account, a paid tier or a foreign payment card is a stop-and-ask. Every new dependency needs one sentence of justification.
- **Pure Rust dependencies.** FurniCraft does not compile, vendor or ship C/C++/Objective-C code. A dependency that builds native code (`cc`, `cmake`, `bindgen`, or a `-sys` crate with bundled sources such as `lzma-sys`, `xz2` or `vtkio`) is forbidden. Bindings to libraries the operating system provides (X11, Wayland, Vulkan, Metal, fontconfig) are allowed only as platform integration, loaded through Rust FFI; they must not add native build steps to our build. `cargo xtask layers` rejects known C-vendoring crates.
- **Graphics:** `wgpu` (cross-platform, pure Rust). **UI:** `egui`/`eframe`. Do not add a second GUI toolkit or a second GPU abstraction.
- **Performance is native, not "good enough for a demo".** Keep the frame path free of document scans and allocations; rebuild only what changed.
- **Language:** code, comments, docs, commit messages — English. Discussion with the owner — Russian.

## 3. Crate map and layering

```text
apps/furnicraft        L8  composition root: window, input, egui <-> wgpu, kernel adapter
crates/ui              L7  egui panels and viewport, dispatches commands
crates/engine          L6  session, selection, view state, command registry, undo/redo
crates/render          L5  wgpu renderer
crates/scene           L4  renderer-agnostic frame description, camera, lights, grid
crates/mesh            L3  evaluation cache: revision -> solid and triangle mesh
crates/param           L2  document features -> kernel calls
crates/kernel-truck    L2  geometry kernel adapter implemented with truck
crates/kernel          L1  geometry kernel port: profiles, bodies, booleans, tessellation
crates/model           L1  document data: placements, panels, assemblies, hardware, materials
crates/units, geom     L0  lengths (micrometres), vectors, frames, bounds, meshes
crates/testkit         dev-only test helpers (may depend on anything)
xtask                  workspace checks; not part of the product
```

- **Layering is enforced** by `cargo xtask layers`. A crate may depend only on crates in **lower** layers, and the same table lists per-crate forbidden dependencies. A new crate must be registered in `xtask/src/main.rs` and in `PROJECT.md` in the same change.
- `model` holds serializable data and its invariants only: no `wgpu`, no `egui`, no window, no file I/O, no geometry kernels.
- `kernel` is our port: `model` and `param` never see a third-party kernel type. `kernel-truck` is the only crate allowed to depend on `truck`; the app constructs it and injects it into the session.
- `engine` must not depend on `wgpu` or `egui`: that is what keeps "everything is a command" honest.
- `render` must not depend on `egui`: the renderer is a library, the shell wires it into `egui_wgpu`. Symmetrically, `ui` must not depend on `wgpu`, and `testkit` is reachable only through `[dev-dependencies]`. `cargo xtask layers` enforces all three.
- `ui` never mutates the document directly: it dispatches commands and renders session state.
- `scene` is data about a frame (what to draw), `render` is the driver that draws it. The document stores placements; cameras and viewports belong to the session or the shell.

## 4. Golden rules

### 4.1 Everything is a command

Every user-visible change to the document is a command in `furnicraft-engine`: a stable string id, a label, parameters, an `enabled` check and a `run` that returns `Result`. The UI, and later a CLI/API, dispatch commands by id. Pure view state (camera, panel sizes, selection highlight) may live in the shell or the session, but anything that changes the document goes through a command — that is what makes undo/redo and future automation possible.

### 4.2 Data is data

The document is plain serializable data with stable ids. It does not know about the GPU, the UI, the undo stack or file formats. Rendering reads the document; it never owns it. If a value can be derived (a mesh, a bounding box, a parts list), derive it in a lower-layer function and cache it by revision instead of storing it in the model. The document stores placements and the assembly tree; cameras, viewports, grids and selection are view state, not document data.

### 4.3 Units are explicit

Physical sizes are `furnicraft_units::Length`: integer **micrometres**. No bare `f64`/`f32` millimetres, centimetres or "units" in `model`, `param`, `mesh` inputs or command parameters. Floating point exists only at the geometry and rendering boundary (`furnicraft-geom`, `furnicraft-kernel`, `furnicraft-kernel-truck`, `furnicraft-render`); `param` converts `Length` to kernel millimetres at that edge. Formatting for humans and parsing from text go through `furnicraft-units`.

### 4.4 Never crash

A furniture project is the user's work; a crash must never be the answer to bad data.

- Non-test code never panics: no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`. Return the crate's error type and propagate with `?`; use `ok_or(..)?`, `let .. else { return Err(..) }` and `unwrap_or*` where a fallback is truly correct.
- Input-derived numbers are hostile: use `get()` instead of `[i]`/`[a..b]` for indices that come from files, parameters or arithmetic on them; use `checked_*`/`saturating_*` for lengths and counts; guard division by zero and NaN/inf casts; cap allocations sized by input.
- Validate before allocating. A panel of 10 km or a 4-billion-facet mesh is a `Result::Err`, not an OOM.
- Bound recursion and nesting (assembly trees, future expressions) with depth limits or seen-sets.
- Every crate denies the panic lints at the crate root, and the workspace forbids `unsafe`. Do not add `unsafe` anywhere; if some platform API truly demands it, stop and ask the owner first.

### 4.5 No format assumptions

- Documents are versioned serde data. Keep older files loading: new fields get `#[serde(default)]`; unknown fields are preserved or explicitly rejected, never silently dropped.
- Never assume a material, thickness, unit or hardware type exists: all of these are data with defaults chosen by the document, not constants in code.
- Unknown enum variants from a newer file are an error the user can read, never a panic or a silent reinterpretation.

### 4.6 Clean room

We may study other furniture CAD programs for **behaviour and look only**. Never copy their code, resources, file formats, UI text or assets; never decompile or extract data from them. Implement from public knowledge, published standards and observation. Third-party assets must be permissively licensed, keep their license file next to them, and get a row in `ATTRIBUTION.md` in the same change (create the file with the first asset). Do not add assets at all until a feature needs them.

### 4.7 Tests are the gate

Every change comes with tests. Domain invariants, unit conversions, command behaviour and error paths are tested without a GPU or a window. A bug fix ships with a regression test that failed before the fix. UI changes are verified by looking at the result, not only by compiling.

### 4.8 Dependencies are a budget

Prefer the standard library and the crates we already have. A new crate must be justified, licensed permissively, maintained, and pinned in the workspace. No dependency may pull in a web runtime, a second GUI toolkit or a second async runtime.

FurniCraft is a Rust product: no dependency may compile or vendor C/C++/Objective-C sources. Check the build script and the feature graph before adding a crate; `*-sys` crates are a red flag unless they only bind an OS library. When a dependency forces a native feature we do not need, narrow its features or vendor a minimal patch with a note — do not accept the C build.

### 4.9 Do not invent the domain

`Panel`, `Assembly`, `Hardware`, `Dimension` and `Material` are described in words in `PROJECT.md`. Their exact fields, coordinate conventions and invariants are designed in M1 (see "Open model questions" in `PROJECT.md`). Do not guess a schema while scaffolding, and do not extend it silently: propose the change, then implement it after approval.

### 4.10 Space and views

- The document stores model-space data: the assembly tree, placements, geometry features and materials. It never stores a camera, a viewport, a selection or a hover highlight.
- A `Placement` is a rigid transform relative to the parent frame. World transforms are composed from the root; scale and shear are not placements.
- View state (camera, projection, grid, shading, selection, hover) lives in the session or the shell. Named views are the only optional exception and are presentation metadata.
- A viewport is a UI adapter: it turns input into camera moves or commands and draws a scene. Picking returns document `Id`s, never mesh indices or pointers.
- A panel's local frame has its zero at the minimum corner; faces are `A` (at `z = thickness`) and `B` (at `z = 0`), vertices run counter-clockwise from the zero corner, and edge `i` joins vertex `i` to `i + 1`. Vertex and edge ids are stable and never reused; their numbers are labels, not keys. Machining is stored in panel-local coordinates; an edge reference is a topology reference, not a second coordinate system.
- Manipulation goes through a command; the UI never writes a placement directly.

## 5. Stack

| Area | Choice |
|---|---|
| Language | Rust 2024 edition, MSRV 1.95 |
| UI | `egui` + `eframe` (native, `wgpu` backend) |
| Graphics | `wgpu` 30 (`egui-wgpu` 0.37 is pinned to the same major version) |
| Geometry kernel | `truck` (Apache-2.0), behind the `furnicraft-kernel` port; only `furnicraft-kernel-truck` may use it |
| Math | `glam`, re-exported through `furnicraft-geom` |
| Serialization | `serde` (+ `serde_json` until a binary format exists) |
| Errors | `thiserror` per crate |
| Logging | `log` (the shell installs the sink) |
| Tests | built-in test harness; `furnicraft-testkit` for fixtures |

`render` and `egui-wgpu` must agree on the `wgpu` major version, otherwise device/queue types do not match and the paint callback cannot be wired. Bump them together.

## 6. Before you finish a task

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask layers
```

Then check by hand:

- [ ] New behaviour has tests; a fixed bug has a regression test.
- [ ] No new `unwrap`/`expect`/panic path (tests in the same file may use them).
- [ ] No floating-point physical sizes outside the geometry/render boundary.
- [ ] No document mutation outside a command.
- [ ] Camera, selection and viewport state did not leak into the document.
- [ ] New crate, module or dependency is reflected in `PROJECT.md` and the layer map.
- [ ] New dependencies are pure Rust: no C/C++ sources, no `cc`/`cmake` build of native code, no C-vendoring `-sys` crate; `cargo xtask layers` passes the C-vendoring check.
- [ ] Public items have doc comments; a crate's `//!` comment still describes what it does.
- [ ] Keep the tree building at every step: a half-finished workspace blocks everyone.

While iterating, run less: `cargo test -p <crate> --lib --tests` and `cargo clippy -p <crate> --all-targets`. The four commands above are the final gate.

## 7. Anti-patterns

- Business logic inside an egui event handler; writing to the document without a command.
- Storing a camera, viewport, selection or hover highlight in the document.
- Picking that returns mesh indices or pointers instead of document `Id`s.
- Reaching around the geometry port: `truck` types in `model`, `param`, `engine` or `ui`.
- Scale or shear in a placement; placements are rigid.
- `f32`/`f64` millimetres travelling through the domain; unit conversions scattered across crates.
- Deriving meshes every frame, or storing render data in the document.
- `unwrap()` on file data, user input, GPU results or locks; "it cannot be None here" assumptions.
- Adding Tauri, Electron, a web server, a JS runtime, a second GUI toolkit or a second GPU API.
- Adding a dependency that compiles or vendors C/C++/Objective-C code just to save some Rust work.
- Inventing document fields, file formats or CNC code "for later".
- Copying code, shaders or assets from proprietary CAD or from unrelated projects.
- Silent data loss on load/save (dropping unknown fields, ignoring malformed values).
- Adding a dependency to save ten lines of safe code.

## 8. Working with the owner

- **Plan before build.** Any task larger than one file starts with a plan; the owner is the technical director and approves it.
- **Explain non-obvious decisions in one sentence** (why this crate, this representation, this boundary). Do not re-explain variables and functions.
- **Ask when unsure** — a wrong guess about the domain costs more than a question.
- **Keep it simple and incremental**: the smallest thing that works, then the next step. No speculative abstractions.
- **Artifacts in English, discussion in Russian.**

## 9. Where things are tracked

- `PROJECT.md` — product, domain vocabulary, crate map, milestones, open model questions.
- `AGENTS.md` — this file: the rules.
- `BRIEF.md` — the owner's original brief; do not edit it to match the code.
- `ATTRIBUTION.md` — vendored and bundled third-party components, their licenses and modifications.
- `docs/` — created when the first long-lived document (architecture decisions, file format, UI guidelines) is needed; not before.
