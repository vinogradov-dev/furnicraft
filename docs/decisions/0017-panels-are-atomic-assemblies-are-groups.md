# 0017. Panels are atomic leaves, assemblies are editable groups

- Status: Accepted
- Date: 2026-10-25

## Context

The scene tree contains two kinds of nodes. A panel owns its geometry, banding and machining; an
assembly groups panels and nested assemblies. Editing needs both: a user creates a part, gives it a
coordinate, selects it and sometimes wraps several parts in a group or flattens that group again.
Treating both kinds as generic containers would let a command split a panel into its banding or
machining records, which is not a scene operation and would break the panel invariants.

## Decision

- A panel is an **atomic leaf**. It is never shown with children and can never be ungrouped. Its
  cutouts, banding and machining stay inside the panel record and move, copy and delete with it.
- An assembly is an editable group. It can be created empty, renamed, placed, duplicated, deleted,
  used as a grouping wrapper and flattened back into its parent.
- Grouping wraps two or more **sibling** nodes in a new assembly with an identity placement relative
  to the shared parent. The selected nodes keep their order and their placements, so no leaf moves
  in world space.
- Ungrouping replaces an assembly with its children and composes each child placement with the
  removed assembly's placement, so world transforms are preserved. Panels are rejected.
- The root assembly defines the world frame. It cannot be deleted, placed, grouped or ungrouped;
  renaming it is allowed.
- These mutations are commands (`assembly.create`, `assembly.rename`, `node.delete`,
  `node.duplicate`, `node.group`, `node.ungroup`, `node.placement.set`), so each is one undo entry
  and a failed command leaves the document unchanged. Selection is view state and never lives in the
  document.

## Consequences

- A cutting list sees panels only; grouping never changes what a panel is or what it owns.
- Grouping is deliberately sibling-only. Grouping nodes from different parents would need an
  implicit reparent and a new placement, which is a separate operation with its own undo rule.
- Deep duplication assigns fresh ids to the whole subtree and keeps the placement, so a copied
  assembly can be moved independently without touching the original.
- The scene tree shows the panel as a single row; there is no expansion into banding or machining.
  Those belong to the panel card and, later, the machining editor.
- Save and load remain outside the current editing milestone; until they land the scene lives only
  in the session.
