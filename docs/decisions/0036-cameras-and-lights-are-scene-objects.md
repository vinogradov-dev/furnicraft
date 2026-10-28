# 0036. Cameras and lights are scene objects

- Status: Accepted
- Date: 2026-10-28

## Context

A scene file must hold what every 3D editor holds: the objects plus the cameras and lights their user
authored. A second camera is how a drawing view or a presentation view is framed; a light is how a
scene is lit for a render. Dropping them on save would make the file incomplete.

ADR 0029 placed "the camera" in the session so that every client shares the user's view. That rule
was written when the only camera was the one orbiting the model. It is right for navigation and wrong
for authored objects: a user who adds a camera expects it to survive a reload and to be undoable like
any other object.

## Decision

- `Camera` and `Light` are node kinds in the scene tree, siblings of containers, panels and geometry
  objects. They carry a name, a parent, a placement, typed properties, selection and undo like any
  node.
- A camera node stores its projection parameters and field of view. A light node stores its kind,
  colour and intensity. `Project::active_camera` references the camera the scene is rendered from; it
  is document data.
- The built-in camera and the built-in key light are defaults, not nodes. They cannot be selected,
  renamed or deleted. The viewport uses the active camera when one is set and the built-in camera
  otherwise; the renderer uses the built-in light when the scene has no light node.
- A scene with no camera and no light is valid and normal: a furniture part or a hardware model needs
  neither.
- Navigation of the viewport camera — orbit, pan, zoom, projection toggle — remains session state
  ([0029](0029-view-state-belongs-to-the-session.md)), as do selection, hover and open containers.
- Undo covers camera and light edits. The undo history is never saved, and the file never stores a
  viewport.

## Consequences

- The document schema grows two node kinds. A version 7 file loads with no camera and no light and
  falls back to the built-in defaults.
- Picking must treat a camera or a light as a selectable object without a shaded mesh: the viewport
  draws a gizmo icon and hit-tests its bounds.
- Deleting the active camera clears `active_camera`; the viewport returns to the built-in camera
  instead of failing.
- An authored camera is not a viewport: two viewports may look through it or not, and switching a
  viewport away changes nothing in the document.
- No separate "presentation" section in the file is needed: the scene document already holds
  everything the file keeps.
- Revisit if named views return as bookmarks: they would reference camera nodes instead of storing a
  second copy of the camera parameters.

## Related

- [0029](0029-view-state-belongs-to-the-session.md) — view state stays in the session; this record
  carves camera and light **objects** out of that rule.
- [0022](0022-closed-containers-pick-as-one-object.md) — picking returns document ids; camera and
  light nodes join the pickable set.
- [`docs/domain/scene-file.md`](../domain/scene-file.md) — what the file stores.
- [`docs/domain/document.md`](../domain/document.md) — the document model.
