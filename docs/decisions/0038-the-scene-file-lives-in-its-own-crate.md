# 0038. The scene file lives in its own crate

- Status: Accepted
- Date: 2026-10-28

## Context

Saving and loading touch three things the document model must not do: the filesystem, JSON framing
and the preservation of data this build does not understand. `model` is serializable data with
invariants only; it has no file I/O. `engine` is the command layer and must stay free of window and
GPU types, but it is also not the only client that needs a file: a headless job and the assistant
open and save a scene without a session, and the shell owns the File menu.

## Decision

- The scene envelope and its atomic writer are a dedicated crate, `furnicraft-project`, at layer 2
  beside `param` and the kernel adapter.
- It depends on `model`, `serde`, `serde_json` and `thiserror` only. It never depends on `engine`,
  `ui`, `wgpu` or a window.
- It owns the format identity and framing version, the read/write path and the preservation of
  unknown top-level sections. The document owns its schema, migrations and validation.
- The shell decides when to save and what a dirty document means; the crate only turns a `Project`
  into bytes and back.

## Consequences

- A headless tool can load a scene without constructing a session or a window.
- The layer map gains a crate, and every new file-adjacent feature (autosave, recent files, the
  binary form) has one obvious home.
- The crate cannot enforce that the document omits order data, the catalogue or view state; the
  document model does that. The envelope can only serialize a `Project`.

## Related

- [`docs/domain/scene-file.md`](../domain/scene-file.md) — the contract this crate implements.
- [0031](0031-external-resources-keep-their-own-system-of-record.md) — order data stays outside
  the scene file.
- [0029](0029-view-state-belongs-to-the-session.md) — view state is never written.
