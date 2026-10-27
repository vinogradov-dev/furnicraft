# 0021. Assemblies are components, groups are containers

- Status: Superseded by 0034; supersedes 0017
- Date: 2026-10-25

## Context

ADR 0017 used one container kind, the assembly, both as "the product" and as an ad-hoc editing
wrapper. That was enough while every node was hand-built and untyped. With M6 approaching, an
assembly must become a furniture or industrial component — a cabinet, a drawer box, a runner, a
hinge — that carries a furniture type and type-specific properties and operations. Flattening such
a component with `node.ungroup` would erase what makes it a component.

Users still need disposable organisation: "these parts belong together for now", "hide this while I
work on the rest". That is a different concept and deserves a different node kind.

## Decision

- A **panel** stays an atomic leaf: it owns its geometry, banding and machining and is never
  ungrouped or split.
- An **assembly** is a meaningful furniture or industrial component. It owns its children, placement
  and, from M6, a furniture type with type-specific properties and operations. It is never flattened
  by `node.ungroup`; dismantling it is a separate operation with its own rules, added when furniture
  types land.
- A **group** is an organisational container for any nodes — panels, assemblies and nested groups,
  later points and contours. It has no furniture type and only the common operations: rename, move,
  duplicate, delete, group and ungroup.
- The project root is a `Group`. It defines the world frame and is immutable for delete, placement,
  grouping and ungrouping.
- `node.group` wraps siblings in a new **group**; `node.ungroup` accepts only a **group** and refuses
  an assembly or a panel with a typed error.
- `assembly.rename` becomes the generic `node.rename`, because groups and panels need renaming too.
  Command ids for operations shared by both container kinds use the `node.*` prefix.
- Groups and assemblies both accept newly created panels and assemblies through the common commands;
  only type-specific assembly operations are absent for now.
- Schema version 5 introduces the `group` node. A version 4 root assembly loads as a group root;
  nested assemblies keep their kind.

## Consequences

- Direct manipulation can treat a closed assembly as one movable object while leaving the component
  identity intact.
- The tree now shows two container labels; the inspector names the kind so the user can tell them
  apart.
- A future "dissolve assembly" is a real, separate design task rather than an overload of ungroup.
- Files saved by older builds load without data loss; files saved by this build declare version 5.
- A group carries no furniture semantics, so no later command may assume a container is an assembly.
