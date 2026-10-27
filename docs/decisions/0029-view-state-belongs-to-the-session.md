# 0029. View state belongs to the session

- Status: Accepted
- Date: 2026-10-27

## Context

`PROJECT.md` already places selection and view state in `furnicraft-engine`'s session, but the code
keeps the selection and the open containers in `ui::Editor` and the camera in the app shell. That
works for one window with one person at the keyboard.

It stops working as soon as a second client exists. Faber has no `Editor`: an in-process assistant
would need a UI type, and a headless job has no UI at all. An external client also needs the same
context the user sees — what is selected, which level is open, where the camera points — or it will
answer about a scene the user is not looking at.

## Decision

- `Session` owns the view state the editor shares: the ordered selection with its primary id, the set
  of open containers, and the camera with its projection per viewport. The document stores none of
  it.
- View operations are session methods, not document commands: `selection.*`, `container.open/close`,
  `view.set`. They are not undoable and never bump the document revision.
- The UI renders and edits session state; it never keeps its own copy of the selection. The viewport
  and the scene tree observe the same selection, so they cannot disagree.
- Document changes still go through the command registry only. A view operation never writes to the
  document; a manipulation that changes a placement still ends in `node.placement.set`.
- The shell keeps only what is genuinely per-window presentation: panel sizes, fold state and the
  active transform mode.

## Consequences

- The assistant and any later client can read and set the context the user works in, using the same
  calls the UI uses.
- A headless job needs no `egui` at all; the rendering crate is not involved.
- `ui::Editor` shrinks to panels and dispatch, and the gizmos build their target from the session.
- Picking still returns document ids (§4.10); the session stores the result, not mesh references.
- An operation that changes both view state and the document must be split: the view part is not
  undone, the document part is. Hidden coupling between them is now a design error.
- The refactor touches `crates/ui/src/lib.rs`, the scene tree and both gizmos; it lands before the
  agent crate so the new client has something to read.

## Related

- [0028](0028-the-assistant-is-a-client-of-the-command-registry.md) needs this context.
- `AGENTS.md` §4.10 (space and views), §4.1 (everything is a command).
- `PROJECT.md` › Crate map, `furnicraft-engine` row.
