# 0028. The assistant is a client of the command registry

- Status: Accepted
- Date: 2026-10-27

## Context

FurniCraft is to gain an assistant — **Faber** — that follows the project in real time, answers in
chat or by voice, and edits the document the way a person does. It must run with a window, so the
user can watch it work, and headless, so it can process existing orders in the background. The
language model may be local or remote, and the feature is disabled by default because it needs a
model connection.

Two shapes were possible: give the assistant its own access to the document and the model, or make
it one more client of the surface the UI already uses. A private mutation path would bypass
`enabled` checks, typed parameters, undo and tests — exactly what `AGENTS.md` §4.1 exists to prevent.

## Decision

- **Every assistant action is a command, a query or a session view operation.** There is no private
  mutation path, no direct `Project` write, and no shell or filesystem tool by default. The tool
  registry is built from `CommandRegistry` plus read-only session queries; the UI, a future CLI, an
  MCP adapter and Faber dispatch the same ids.
- **The core is deterministic and model-agnostic.** `furnicraft-agent` holds the tool registry, the
  permission policy, the task runner, the scene digest and the ports. It has no HTTP client, no
  async runtime, no `egui` and no `wgpu`. A scripted model in tests must run a whole task without a
  network. The language model is an injected adapter behind a `ChatModel` port, exactly as
  `kernel-truck` sits behind `kernel`.
- **Commands expose their parameter schema.** The typed parameter struct is the single source of
  truth for validation, UI hints, documentation and the model's tool schema; the registry reports
  both. The schema crate is chosen when the registry grows the field, under the pure-Rust rule.
- **A task is one undo group.** History can group several commands into one user-visible step, so
  "build the wardrobe" is undone in one action. View-state operations are not undoable and never
  enter the document.
- **Disabled by default.** Without a configured endpoint and an explicit enable, the app makes no
  network call and Faber has no tools. The first enable starts read-only. Keys live in the OS keyring
  or a restricted configuration file, never in the document, the repository or a log.
- **The core does not see pixels in v1.** Faber reads the scene: the node tree, panels, materials,
  placements, machining, diagnostics and derived measures. An optional screenshot channel for a
  vision model is a separate decision and never a prerequisite for control.
- **The persona is Faber; the code stays neutral.** The crate and types are `furnicraft-agent` and
  `Agent*`, settings are `agent.*`. Renaming the persona must not touch code.
- **The app never listens.** A remote agent is reached through an outbound connection; the product
  contains no HTTP server (`AGENTS.md` §7).
- **Layers.** The assistant core sits above `engine` and below `ui`: the UI needs its state, the
  core needs the engine, and neither may reach upward. The model adapter and the UI share the next
  layer; the shell wires both. Crates, the layer map and `xtask` change in the same commit that
  creates them.

## Consequences

- The assistant cannot drift from the editor: any behaviour it has, the UI or CLI can have too, and
  the same tests cover both. A tool can be exercised without an LLM.
- New domain capability must become a command or a query before Faber can use it. That is a budget
  item on every feature, not a tax the assistant pays alone.
- Two pieces are prerequisites rather than details: command parameter schemas (so a model can call
  safely) and grouped history (so a task is one undo step).
- The language model, its transport and its cost stay outside the core; a failing provider degrades
  to "assistant unavailable" instead of breaking the editor.
- The chat panel needs agent state, which is why the UI layer moves above the agent crate; the
  shell remains the only place where both are wired.
- Voice in and out arrives later as optional ports; neither the editor nor the core depends on it.

## Related

- [0029](0029-view-state-belongs-to-the-session.md) moves the context the assistant reads into the
  session; [0030](0030-construction-knowledge-is-code.md) and
  [0031](0031-external-resources-keep-their-own-system-of-record.md) cover construction knowledge and
  other systems.
- `AGENTS.md` §4.1 (everything is a command), §4.11 (the assistant is a client).
- `crates/engine/src/commands.rs`, `crates/engine/src/session.rs`.
