---
name: create-command
description: Add a new user-visible command to FurniCraft end to end: stable id, typed params, engine mutation, undo/redo, UI dispatch by id, tests, docs and the final gate. Use when adding, renaming or changing any editor action that mutates the document.
license: MIT
compatibility: opencode
metadata:
  audience: agents
  workflow: engine
---

# Add a command to FurniCraft

Every user-visible change to the document is a command (AGENTS.md §4.1). This skill is the checklist for adding one without breaking layering, undo or the "never crash" rule.

## What counts as a command

- A command changes the **document**: panels, assemblies, machining, hardware, materials, dimensions.
- Camera, selection highlight, open panels, tool mode and other view state are **not** commands. They live in the session or the shell.
- If the command needs a document field that does not exist yet, stop: the schema is designed in M1 (`PROJECT.md` › Open model questions). Propose the field and get approval before writing code (AGENTS.md §4.9).
- Pure helper functions (mesh building, dimension resolution) are not commands either; commands call them.

## Before you start

1. Read `AGENTS.md` §4 (golden rules) and the crate map in §3; read the relevant part of `PROJECT.md`.
2. If the change touches more than one file, present a plan and wait for approval (AGENTS.md §8).
3. Find the crate the command belongs to: commands live in `furnicraft-engine` (L5). They may use `model`, `param` and `mesh`, and must not use `wgpu` or `egui`.
4. Check that the command is not already there under another id; reuse beats duplication.

## Steps

### 1. Specify the command before writing code

Write down:

- **id** — `<area>.<verb>`, lowercase, stable forever: `panel.create`, `panel.resize`, `dimension.set`, `hardware.place`. Never reuse an id for a different meaning; rename by adding a new id and retiring the old one.
- **label** — one user-facing sentence in English, shown in menus and undo history.
- **params** — a typed struct, serializable, with every field documented.
- **result** — what the caller gets back (created id, affected panels, nothing).
- **undo semantics** — what the inverse operation is.

### 2. Put it in its own module

- One area per file: `crates/engine/src/<area>_cmds.rs` exposing `pub fn specs() -> Vec<CommandSpec>`.
- Register the module's specs in the engine's command list; that registration is a small, surgical edit — re-read the shared file before touching it.
- Do not grow a shared file with unrelated commands.

### 3. Define the parameters

- Physical sizes are `furnicraft_units::Length` (integer micrometres). Never `f32`/`f64` millimetres, never unit-less numbers (AGENTS.md §4.3).
- Use enums and newtypes instead of string magic and booleans with unclear meaning.
- Optional fields get `#[serde(default)]` so saved macros, tests and older callers keep working.
- Validate ranges (positive thickness, non-empty name, panel within sane limits) and cap anything that sizes an allocation. A 10 km panel or 4 billion facets is an `Err`, not an OOM (AGENTS.md §4.4).

### 4. Implement `enabled` and `run`

- `enabled` is a pure check: document exists, target id exists, the operation makes sense. It must not mutate or allocate anything large.
- `run` returns the crate's `Result` type. It is total: **any** params value and **any** document state produce `Ok` or a readable `Err`, never a panic.
- Validate before acting: look up ids with `get()`, check bounds before indexing or dividing, use `checked_*`/`saturating_*` for counts and lengths.
- Bad input gets an actionable error ("panel `…` does not exist"), not a silent no-op.

### 5. Mutate through the session, once

- The command performs one logical change: mutate the document, bump `Revision` exactly once, mark the session dirty.
- Capture the data needed for the inverse **before** mutating and push exactly one undo entry per command. Undo must restore the previous state bit-for-bit, including ids.
- Do not touch derived caches (meshes, parts lists, picking structures) by hand: they are keyed by `Revision` (AGENTS.md §4.2).
- No command mutates the document from a background thread; all edits go through the session.

### 6. Wire the UI by id only

- Toolbar buttons, menu items and shortcuts dispatch the command by its exact id. Handlers contain no business logic and never write to the document themselves (AGENTS.md §3, §7).
- Enabled/disabled state comes from `enabled`; the visible label comes from the spec. No duplicated strings.
- If the command needs a dialog, the dialog collects params and dispatches once on confirm — it does not mutate while the user types.

### 7. Add tests

At minimum, in the engine crate:

- **success** — the command changes exactly the intended data and nothing else;
- **not found / wrong type / missing params** — returns `Err`, does not panic;
- **out of range** — negative or absurd sizes, indices past the end, empty names → `Err`;
- **undo/redo** — one undo restores the prior document byte-equivalently; redo re-applies;
- **serde round-trip** — params survive serialization if they are persisted.

Tests must not need a GPU or a window. Use `furnicraft-testkit` fixtures rather than hand-built documents where possible.

### 8. Update the documentation

- `PROJECT.md` — if the command changes the product surface or the crate map.
- Command reference — when `docs/commands.md` exists, add a row: id, label, params, since-version.
- `AGENTS.md` — only if the command introduces a new rule, not for every command.

### 9. Run the final gate

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask layers
```

`cargo xtask layers` must stay green: the engine may not gain `wgpu`/`egui` dependencies.

## Checklist

- [ ] Id is namespaced, lowercase and stable; not reused.
- [ ] Params are typed, lengths are `Length`, optional fields have defaults.
- [ ] `enabled` is pure; `run` returns `Err` for every hostile input.
- [ ] One document mutation, one `Revision` bump, one undo entry.
- [ ] UI dispatches by id; no logic in the handler.
- [ ] Tests cover success, invalid input, undo/redo and serde.
- [ ] Docs updated where the product surface changed.
- [ ] `fmt`, `clippy -D warnings`, `test`, `xtask layers` are green.

## Anti-patterns

- Mutating the document from an egui event handler or skipping the command because "it is only one field".
- `unwrap()`/`expect()`/panic on params, missing ids or lock poisoning.
- Float millimetres in params or in the model.
- One undo entry per changed field, or an inverse that loses ids.
- A command that also renders, opens a window or queries the GPU.
- Reusing an id for new behaviour, or silently changing an existing command's params.
- Adding a dependency to the engine to do something the standard library or `param`/`mesh` already do.

## Project status

The workspace is at M0: `crates/engine` is an empty scaffold and the command registry does not exist yet. The first command creates `crates/engine/src/commands.rs` (`CommandSpec`, registry, dispatch) following this skill, and is the reference implementation for the rest. If the real API ends up different from this description, update this skill and `AGENTS.md` in the same change — the skill must describe the code that exists, not an aspiration.
