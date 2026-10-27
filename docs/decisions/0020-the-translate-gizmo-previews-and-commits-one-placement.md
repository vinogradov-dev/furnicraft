# 0020. The translate gizmo previews and commits one placement

- Status: Accepted
- Date: 2026-10-25

## Context

The inspector can already move a node by typing local X/Y/Z, and every move is one
`node.placement.set` command. Direct manipulation in the viewport is the next step, but three
constraints shape it.

The document must not change during a drag: every document mutation goes through a command, and a
command is also the undo unit. A drag that writes a placement per pointer event would flood the undo
history and re-evaluate the whole scene on every frame. Second, the existing viewport uses a primary
drag for camera orbit, so the gizmo must claim the pointer without breaking orbit elsewhere. Third,
the camera can see a world axis nearly end-on, where its projected direction is unstable and a
pointer drag would translate the node by metres for a few pixels.

The scene frame is a renderer-agnostic description with one world transform per drawable. A
translation preview can therefore be a copy of the frame with shifted transforms, without touching
the document or the evaluated meshes.

## Decision

- The shell owns a screen-space translate gizmo in `apps/furnicraft/src/translate_gizmo.rs`. It
  projects three world-axis handles from the selected node's manipulation pivot, keeps them at a
  constant screen length, and paints them over the viewport image. It changes only shell state. The
  pivot is settled by [0025](0025-one-manipulation-pivot.md).
- An axis whose world direction is within about ten degrees of the view direction has no handle:
  its projected direction is unstable and the drag would be unusable.
- A press on a handle starts a drag. The drag parameter is the closest point between the pointer ray
  and the world axis line; the difference from the press parameter, projected onto the world axis,
  is the world-space delta. The camera is not orbited or zoomed while a drag is active.
- A drag never mutates the document. Each changed delta uploads a preview frame built with
  `SceneFrame::with_world_translation`, which shifts the panels under the selected node. The preview
  keeps the base frame and the document untouched.
- The release converts the world delta into the parent frame with the inverse parent world rotation,
  adds it to the starting local translation, rounds to whole micrometres and dispatches exactly one
  `node.placement.set`. A zero or sub-micrometre delta dispatches nothing. Escape cancels and
  restores the base upload.
- The rotation is preserved from the starting placement; the drag only changes translation.
- The root assembly has no placement, so it gets no gizmo. A press that starts a drag does not
  also select the model behind the handle.

## Consequences

- The inspector and the viewport share one command and one undo entry per move.
- The gizmo works without a GPU test: hit testing, axis projection, the preview key and the
  conversion to a parent-local placement are plain functions with unit tests.
- A preview upload rebuilds the drawable vertices for the whole frame on each pointer move. That is
  acceptable for the sample scene but not for a large cabinet; an incremental transform buffer per
  drawable is the next optimization if it shows.
- Handles sit at the manipulation pivot: for a panel its minimum corner, for a container the first
  corner of its local bounds ([0025](0025-one-manipulation-pivot.md)). The commit path does not
  depend on the pivot.
- The rotation representation is fixed by [0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md):
  the document keeps the quaternion, the inspector shows intrinsic XYZ Euler angles, and the rotate
  gizmo reuses this preview-and-commit shape.
