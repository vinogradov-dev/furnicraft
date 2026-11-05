# 0056. Creation tools land at the scene root

- Status: Accepted
- Date: 2026-11-05

## Context

[0052](0052-a-drawing-commits-as-one-path-on-the-work-plane.md) chose a drawing's parent from the
selection: a selected container took the drawing as a child and a selected leaf contributed its
parent. The toolbar reused that rule for panels, assemblies, primitives, cameras and lights, so
selecting a group to inspect it silently redirected every new node into that group. Creation and
parenting are separate decisions, and the selection already moves to the new node after every
create, which makes a selection-derived rule feed on itself.

## Decision

- Every creation tool — a drawing (path), a panel, an assembly, a primitive, a camera and a light —
  lands at the scene root. The selection never chooses a parent.
- The editor has one function, `creation_target(project, armed) -> Id`; every creation action passes
  its result. It returns the armed container while that node still exists and the scene root
  otherwise. `ToolAction` carries no parent, so the toolbar strip cannot name a container.
- A child is created through an explicit entry: the user arms a container, and the armed node
  receives the next creations until the arm is cleared. The armed target is view state in the
  editor, never document data, and the root cannot be armed (U31).
- Extruding a closed path stays a sibling of that path: it derives a body from a drawing that
  already sits somewhere, so it is not a creation tool.

## Consequences

- Selecting a group no longer changes where a new node goes, and a toolbar action cannot smuggle a
  parent.
- The default is the top of the tree; a nested node costs one explicit arm, a top-level node costs
  nothing, and one arm can host several tools. The strip names the armed container and clears it
  with ✕; Escape and the container's removal clear it too.
- 0052's parent rule is superseded; its single-command drafting and work-plane placement stand.
