# AGENTS.md: guide for AI agents and contributors

FurniCraft is a free, open-source, native 3D editor for **cabinet furniture**, written in **Rust only**. The user sets dimensions, panels, materials and hardware; the editor builds a parametric 3D model and, later, drawings and CNC programs. The quality bar for interaction is Blender: smooth, native, predictable. Read `BRIEF.md` for the owner's goals and constraints, and `PROJECT.md` for the product and the crate map.

The application is a native binary: `wgpu` for graphics, `egui`/`eframe` for UI. **No Electron, Tauri, webview, JavaScript or TypeScript — never**, not as a dependency, build step or packaging target. The product name is always **FurniCraft** in user-facing text (`FurniCraft` in prose, `furnicraft-*` in machine names).

This file is the constitution. Where a task or a habit conflicts with it, this file wins: ask the owner instead of bending a rule.

## 1. Orientation

Opening a session is cheap by design: read the plan, not the codebase. Start with
`cargo xtask status` (or `plan/STATUS.md`, whose first line is the same state). Then:

1. `plan/STATUS.md` — the `STATE:` line and the `Next` section.
2. `plan/SESSION.md` — only when `state=doing`; it names the live step and the next command.
3. `plan/BACKLOG.md` — the task id in `STATE:` and the files its `files:` hint names. Read those
   files and their direct callers. **Do not scan the workspace**; if the hint is wrong, fix the hint.
4. Only then the documents the task itself needs.

`plan/debt.md` and `plan/questions.md` are reference files: search them when the task touches their
area, never read them at start. `plan/archive/done.md` is history; `git log --oneline` replaces it.

Answering the next step has its own procedure (§1.1); it never re-derives the state from the code.

| Read | Why |
|---|---|
| `PROJECT.md` | Vision, domain vocabulary, crate map, milestones |
| `plan/STATUS.md` | `STATE` line, where we are, index of the other plan files; rewritten after every task |
| `plan/BACKLOG.md` | One line per task: id, order, status, file hint |
| `plan/SESSION.md` | The live task while `state=doing`; template in `plan/SESSION.template.md` |
| `docs/decisions/` | Why the kernel, the units, the vendored crate and the panel frame are what they are |
| `BRIEF.md` | The owner's goals, constraints and preferences (Russian) |
| `crates/<name>/src/lib.rs` | Each crate's doc comment states its layer and job |
| `xtask/src/main.rs` | The layer map that `cargo xtask layers` enforces |

### 1.1 Answering the next step

The contract is a template, not prose duplicated here: read
`state-contract/templates/AGENTS.section.md` before answering a request for the next step. In short:
`STATE` and `## Now` decide the answer, an interrupted session resumes at `now:` instead of starting
over, and a broken plan is reported rather than answered around.

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
crates/project         L2  scene file envelope: read, write, atomic save, preserved sections
crates/kernel-truck    L2  geometry kernel adapter implemented with truck
crates/kernel          L1  geometry kernel port: profiles, bodies, booleans, tessellation
crates/model           L1  document data: placements, panels, assemblies, hardware, materials
crates/units, geom     L0  lengths (micrometres), angles (millidegrees), vectors, frames, bounds, meshes
crates/testkit         dev-only test helpers (may depend on anything)
state-contract         tool  plan gate: parses and checks STATUS/BACKLOG/SESSION, reusable
xtask                  workspace checks; not part of the product
```

- **Layering is enforced** by `cargo xtask layers`. A crate may depend only on crates in **lower** layers, and the same table lists per-crate forbidden dependencies. A new crate must be registered in `xtask/src/main.rs` and in `PROJECT.md` in the same change.
- `model` holds serializable data and its invariants only: no `wgpu`, no `egui`, no window, no file I/O, no geometry kernels.
- `kernel` is our port: `model` and `param` never see a third-party kernel type. `kernel-truck` is the only crate allowed to depend on `truck`; the app constructs it and injects it into the session.
- `engine` must not depend on `wgpu` or `egui`: that is what keeps "everything is a command" honest.
- `render` must not depend on `egui`: the renderer is a library, the shell wires it into `egui_wgpu`. Symmetrically, `ui` must not depend on `wgpu`, and `testkit` is reachable only through `[dev-dependencies]`. `cargo xtask layers` enforces all three.
- `ui` never mutates the document directly: it dispatches commands and renders session state.
- `scene` is data about a frame (what to draw), `render` is the driver that draws it. The document stores placements; cameras and viewports belong to the session or the shell.

The assistant track (ADR 0028) adds `furnicraft-agent` at a new layer between `engine` and `ui`; `ui`, the shell and `testkit` move up one number in the same change. Do not create those crates or edit the layer map before the code lands.

Editing anything under `vendor/` follows `vendor/AGENTS.md`: the upstream source is not ours to change.

## 4. Golden rules

### 4.1 Everything is a command

Every user-visible change to the document is a command in `furnicraft-engine`: a stable string id, a label, parameters, an `enabled` check and a `run` that returns `Result`. The UI, and later a CLI/API, dispatch commands by id. Pure view state (camera, panel sizes, selection highlight) may live in the shell or the session, but anything that changes the document goes through a command — that is what makes undo/redo and future automation possible.

### 4.2 Data is data

The document is plain serializable data with stable ids. It does not know about the GPU, the UI, the undo stack or file formats. Rendering reads the document; it never owns it. If a value can be derived (a mesh, a bounding box, a parts list), derive it in a lower-layer function and cache it by revision instead of storing it in the model. The document stores placements and the node tree; cameras, viewports, grids, selection and per-node open state are view state, not document data.

### 4.3 Units are explicit

Physical sizes are `furnicraft_units::Length`: integer **micrometres**. No bare `f64`/`f32` millimetres, centimetres or "units" in `model`, `param`, `mesh` inputs or command parameters. Floating point exists only at the geometry and rendering boundary (`furnicraft-geom`, `furnicraft-kernel`, `furnicraft-kernel-truck`, `furnicraft-render`); `param` converts `Length` to kernel millimetres at that edge. Formatting for humans and parsing from text go through `furnicraft-units`.

Angles shown to the user are `furnicraft_units::Degrees`: integer **millidegrees**. A placement stores a unit quaternion as its only rotation; Euler angles are a presentation derived on read and never serialized. The inspector shows intrinsic XYZ Euler angles in degrees, and the rotate gizmo writes the quaternion directly (ADR 0024). Angle formatting and parsing go through `furnicraft-units`.

### 4.4 Never crash

A furniture project is the user's work; a crash must never be the answer to bad data.

- Non-test code never panics: no `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`. Return the crate's error type and propagate with `?`; use `ok_or(..)?`, `let .. else { return Err(..) }` and `unwrap_or*` where a fallback is truly correct.
- Input-derived numbers are hostile: use `get()` instead of `[i]`/`[a..b]` for indices that come from files, parameters or arithmetic on them; use `checked_*`/`saturating_*` for lengths and counts; guard division by zero and NaN/inf casts; cap allocations sized by input.
- Validate before allocating. A panel of 10 km or a 4-billion-facet mesh is a `Result::Err`, not an OOM.
- Bound recursion and nesting (group and assembly trees, future expressions) with depth limits or seen-sets.
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

`Panel`, `Assembly`, `Group`, `Hardware`, `Dimension` and `Material` are described in words in `PROJECT.md`. Their exact fields, coordinate conventions and invariants are designed in M1 (see "Open model questions" in `PROJECT.md`). Do not guess a schema while scaffolding, and do not extend it silently: propose the change, then implement it after approval.

### 4.10 Space and views

- The document stores model-space data: the node tree, placements, stored pivots, geometry features, materials, and the camera and light **objects** the user authored, together with the active camera. The built-in camera and light are defaults, not nodes. The document never stores a viewport, a selection, a hover highlight, a node's open state or the navigation of a viewport camera.
- A `Placement` is a rigid transform relative to the parent frame. World transforms are composed from the root; scale and shear are not placements.
- A placement stores a unit quaternion. The inspector presents it as intrinsic XYZ Euler angles in degrees (ADR 0024); the rotate gizmo writes the quaternion directly. Both gizmos act at one stored pivot expressed in the node's own frame (`Project::pivot_local` / `pivot_world`): a panel starts at its local zero, a container created with children starts at the centre of its local panel bounds, an empty container at its own origin, and the root is always zero and not editable. The pivot is document data edited by `node.pivot.set` / `node.pivot.center`; it never follows a resize or a reparent (ADR 0026). The rotate rings are world X/Y/Z aligned and do not turn with the node; a drag pre-multiplies the parent-frame turn onto the quaternion. A turn about a non-zero pivot also adjusts the translation so the pivot stays fixed; at a zero pivot the translation is untouched (ADR 0027). Move/Rotate mode is view state.
- View state — viewport navigation, projection, grid, shading, selection, hover, open containers — lives in the session or the shell (ADR 0029). Cameras and lights the user adds are scene objects with undo (ADR 0036). Named views are the only optional exception and are presentation metadata.
- A viewport is a UI adapter: it turns input into camera moves or commands and draws a scene. Picking returns document `Id`s, never mesh indices or pointers.
- A panel's local frame has its zero at the minimum corner; faces are `A` (at `z = thickness`) and `B` (at `z = 0`), vertices run counter-clockwise from the zero corner, and edge `i` joins vertex `i` to `i + 1`. Vertex and edge ids are stable and never reused; their numbers are labels, not keys. Machining is stored in panel-local coordinates; an edge reference is a topology reference, not a second coordinate system.
- Manipulation goes through a command; the UI never writes a placement directly.

### 4.11 The assistant is a client

Faber is the in-app assistant (ADR 0028). It is a client of the same surface as the UI, never a
second way into the document.

- Every assistant action is a command, a query or a session view operation. No private mutation path,
  no direct `Project` write, no shell or filesystem tool by default.
- The core (`furnicraft-agent`) has no network, no language-model SDK, no async runtime, no `egui`,
  no `wgpu`. The model is an injected adapter; a scripted model must run a whole task in tests.
- Disabled by default: no endpoint, no tools, no network call. Keys and conversations never enter the
  document, the repository or a log; the audit trail is a sidecar file.
- The assistant reads the scene as data — tree, panels, materials, placements, diagnostics, derived
  measures. A screenshot channel for a vision model needs its own decision.
- External systems (material catalogue, order registry) are reached through the API their own UI
  uses, never through their database. Irreversible external effects — ordering, issuing, sending —
  require explicit confirmation and an idempotency key.
- Any capability the assistant has must also be reachable from the UI or a CLI.

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
cargo xtask ci
```

`cargo xtask ci` runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `cargo xtask layers` and `cargo xtask plan`, in that order, stopping at the
first failure. Run the steps individually while iterating (see below); run `cargo fmt --all` to fix
formatting, the gate only checks it.

`cargo xtask plan` fails when the plan files contradict each other: `STATE` naming a task the backlog
does not have, has finished, or keeps outside `## Now`; `state=doing` without exactly one `[~]` task
in `## Now` and a `plan/SESSION.md` carrying `task:`, `goal:`, `now:` and `run:`; `state=idle` with a
`[~]` task or a leftover session note; a duplicated task id; a `[x]` task outside `## Done`, or a
`## Done` entry that is not `[x]`; a finished task without a commit hash that exists; a task line
without a `**bold**` id; a task without a `files:` hint, or a hint naming a path that does not exist;
a `base` that is not an ancestor of `HEAD`; a `STATUS.md` or `SESSION.md` over its line budget.
`HEAD` is a legal `base` while the plan edit itself is still uncommitted.

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
- [ ] The plan is updated (see 6.1) and `cargo xtask plan` passes.
- [ ] The completed operation is committed; that commit is a working checkpoint.

While iterating, run less: `cargo test -p <crate> --lib --tests` and `cargo clippy -p <crate> --all-targets`. The gate above is the final check.

### 6.1 Commit each completed operation

Commit after every coherent, fully verified unit of work — not at the end of a session and not only when the owner asks. A commit is the rollback point: checking out any such commit must produce a working result, not a half-finished step.

- A "completed operation" is a change that is self-contained and passes the gates above. If a task has independent steps, commit each step separately.
- Do not mix unrelated changes in one commit, and do not leave finished work uncommitted while starting the next operation.
- A scaffold that does not build or pass tests is not a completed operation; keep it with the first working step.
- Stage explicit paths and review `git diff --cached` before committing. Never use `git add -A` blindly.
- Commit messages and bodies are in English, in the project's Conventional Commits style.

The commit closes the operation; the plan files close it too, in the same change:

1. Mark the task `[x]` in `plan/BACKLOG.md` with the short hash of the commit that delivers it. When a
   task needs several commits, mark it on the last one.
2. Rewrite `plan/STATUS.md`: a new `STATE:` line (`task` = the next task, `updated` = today, `base` =
   the commit whose tree the plan describes — `HEAD` when the plan edit is its own commit, the parent
   when the edit rides in the commit it describes) and a new `Next` paragraph. Never append to it.
3. Delete `plan/SESSION.md`; set `state=idle`.
4. Move a newly discovered gap to `plan/debt.md` and an owner decision to `plan/questions.md`. Neither
   file grows unless something new is found; do not re-copy entries that are already there.

Starting a task is the mirror image: copy `plan/SESSION.template.md` to `plan/SESSION.md`, fill it in,
set `state=doing` and `[~]` on that one task, and update `SESSION.md` after every meaningful step and
after every green gate run — a session that dies must not lose more than the current sub-step.

### 6.2 Branch workflow

New tasks are developed on the `dev` branch; `main` holds only verified, merge-ready work. Work directly on `dev` and commit each completed operation there (section 6.1).

- When a task is finished and `cargo xtask ci` is green on `dev`, merge `dev` into `main` with `--no-ff`, so the merge point is explicit and the history shows when work landed.
- Do not commit directly to `main`; it moves forward only through these merges.
- A hot-fix follows the same path: fix on `dev`, run the gate, merge.

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
- `README.md` — the product and how to build it. Development status lives only in `plan/STATUS.md`; README must not repeat it. If they disagree, `STATUS.md` wins and README is wrong.
- `AGENTS.md` — this file: the rules.
- `BRIEF.md` — the owner's original brief; do not edit it to match the code.
- `ATTRIBUTION.md` — vendored and bundled third-party components, their licenses and modifications.
- `docs/decisions/` — architecture decision records: one short file per decision, with the reason and the consequences.
- `plan/STATUS.md` — the `STATE` line, where the project stands and what comes next; rewritten at the end of every task, never appended to.
- `plan/BACKLOG.md` — one line per task with a stable id, its order and a `files:` hint; the single list of what is done, in progress and next.
- `plan/SESSION.md` — the live task note; exists only while `state=doing`, template in `plan/SESSION.template.md`.
- `cargo xtask dashboard` prints the state with the file each section comes from.
- `plan/debt.md` — accepted gaps, tagged by area; append-only, an entry leaves when the fix lands.
- `plan/questions.md` — decisions waiting for the owner.
- `plan/archive/done.md` — the history the old status file carried; `git log --oneline` is the live record.
