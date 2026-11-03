# 0045. Machining is an ordered list of anchored operations

- Status: Accepted
- Date: 2026-11-03

## Context

A panel stored its machining as a vector of `Machining` variants, and the only variant was a cutout
that carried its own entry face. That shape was enough while the cutout was the only operation: one
id, one shape, one depth and one face, all in one record.

The next operations — grooves, drills, edge notches — do not share that field set. A groove runs
along an edge, a drill enters a face at a point, and both need their own parameters. Adding them as
more variants would force every consumer to match every variant, and the order of the vector would
still be an implementation detail rather than a user-visible order. A cutout stored as a nested
object also cannot grow an update command that replaces its anchor without rewriting the payload.

## Decision

Machining is an ordered list of `Machining` records at the panel level.

- Every record has a `MachiningId` from the document id space, so ids are unique across nodes and
  machining and participate in the same next-id scan.
- A record is `{ id, anchor, params }`. `MachiningAnchor` is tagged: `Face { side }` names the large
  face the operation enters from, `Edge { edge }` names one outline edge through `EdgeRef::Rect` or
  `EdgeRef::Ring`.
- `MachiningParams` is tagged by `kind`; `Cutout(Cutout)` is its only variant today. A cutout stores
  shape and depth only, and must be anchored to a face; an edge anchor on a cutout is a readable
  load error.
- The list order is the user's order. `panel.machining.add`, `panel.machining.update`,
  `panel.machining.remove` and `panel.machining.move` change it, each as one undo entry. A failed
  command leaves the revision and the id counter unchanged.
- Schema 16 writes the record form. The legacy `{"cutout": {id, shape, depth, entry}}` form loads
  through an untagged deserializer and becomes a face-anchored cutout record.

## Consequences

- The next operation adds a `MachiningParams` variant and an anchor kind; the record, the commands
  and the id rules do not change.
- The entry face is no longer a cutout field. Consumers read it from `machining.anchor().as_face()`,
  so an operation without a face anchor cannot silently stand in for a cutout.
- Machining ids share the document id space: `scan_tree` checks uniqueness and `next_id` covers them.
- The old `cutout.create` command is gone; `panel.machining.add` with a face anchor and cutout
  parameters is the general path. `Project::add_cutout` remains as a typed convenience for tests and
  callers that already know they are adding a cutout.
- The diagnostic fields name a `MachiningId`, not a cutout id, because the record is the unit the
  user can act on.

## Related

- [`crates/model/src/panel/machining.rs`](../../crates/model/src/panel/machining.rs) — records, anchors and the legacy wire form.
- [`crates/model/src/project/panel_cmds.rs`](../../crates/model/src/project/panel_cmds.rs) — model-level validation and mutation.
- [`crates/engine/src/panel_cmds.rs`](../../crates/engine/src/panel_cmds.rs) — the four commands.
- [`docs/domain/panels.md`](../domain/panels.md) — machining, commands and diagnostics.
- [0011](0011-panel-local-machining.md) — machining stays in the nominal panel frame.
- [0044](0044-one-outline-source-resolves-the-panel-core.md) — the region-based fit and core trace.
