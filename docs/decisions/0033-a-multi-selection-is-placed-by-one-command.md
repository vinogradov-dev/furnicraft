# 0033. A multi-selection is placed by one command

- Status: Accepted
- Date: 2026-10-27

## Context

The scene tree already toggles several nodes into one selection with Ctrl, Shift or Cmd, but the
gizmos only acted on the primary node: [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md)
and [0027](0027-the-rotate-gizmo-uses-world-aligned-rings.md) commit one `node.placement.set`. Moving
a cabinet and a loose shelf together therefore took two drags and two undo steps, and the second drag
started from a frame the first had already changed.

Two questions have to be answered before the gizmos can move a whole selection: which nodes actually
move when a container and one of its children are both selected, and which pivot a shared turn uses.

## Decision

- **One command for the batch.** A new command `node.placement.set_many` takes a non-empty list of
  `{node, placement}` pairs. `Project::set_node_placements` validates the whole list before changing
  anything — every node exists, none is the root, no node appears twice — then applies it and bumps
  the revision once. One dispatch is one revision and one undo entry.
- `node.placement.set` stays for single-node edits such as the inspector fields. The batch command is
  not a replacement: it exists so a drag of a selection is atomic.
- **Only top-most selected nodes move.** `Project::top_most_nodes` drops a node that has a selected
  ancestor, so selecting a cabinet and one of its panels moves the cabinet once instead of moving the
  panel twice (its own placement plus the cabinet's). Unknown ids and repeats are dropped, and the
  root never moves.
- **The gizmo anchors on the last movable node**, which is the primary selection whenever the primary
  can move. The anchor's stored pivot ([0026](0026-the-manipulation-pivot-is-stored-node-data.md)) is
  the world point the whole selection turns about, and it is the point the handles are drawn at.
- **Every node is committed in its own parent frame.** A translation is `R_parent⁻¹ · Δ_world`; a turn
  is `R_parent⁻¹ · Δ_world · R_parent` pre-multiplied onto the stored quaternion, and its centre is
  the common world pivot expressed in that parent. A selection whose nodes have different parents
  therefore moves rigidly in world space and still stores local placements.
- A translation is rounded to whole micrometres; the anchor's turn keeps its translation bit-for-bit
  when the centre equals it, and other nodes are rounded. A drag that produces no change dispatches
  nothing, and Escape still cancels.
- The preview is unchanged in kind: the shell still shifts or turns the panels of the moved nodes in
  the scene frame, never the document.
- Multi-selection remains view state. This decision adds no document field and no persisted state.

## Consequences

- A multi-node drag is one undo step, matching how a person thinks about "moving the cabinet".
- Selecting a container together with its contents is safe and does no double work.
- Turning a selection about the anchor pivot is exact for the anchor and approximate (micrometres)
  for the other nodes; the alternative, a per-node pivot, would tear the selection apart.
- The selection is not visible in the viewport yet: Ctrl+click picking is still deferred, so the
  multi-selection comes from the scene tree.
- Batch validation lives in the model and is reused by the command; a hostile or stale parameter list
  is rejected without a revision bump.
- Snapping, pivot-pick and a viewport marquee remain future tools; they act on the same command.

## Related

- [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md) and
  [0027](0027-the-rotate-gizmo-uses-world-aligned-rings.md) define the single-node math this extends.
- [0026](0026-the-manipulation-pivot-is-stored-node-data.md) defines the stored pivot.
- [0029](0029-view-state-belongs-to-the-session.md) keeps the selection out of the document.
- `crates/model/src/lib.rs`, `crates/engine/src/node_cmds.rs`,
  `apps/furnicraft/src/gizmo_math.rs`.
