# 0023. Fasad furniture and standard views

- Status: Accepted
- Date: 2026-10-25

## Context

The furniture's front face is the side that users typically design and view first. The original decisions (ADR 0019) established standard views from a projected corner widget, but the camera's starting orientation was yaw -π/4 (top-right view), which does not align the front face with the viewing direction.

The camera look_from convention uses a world direction to the eye, where +Y points from the camera towards the scene. To see the front face clearly, the camera should be positioned on the +Y side looking at the target, which requires yaw 2.8·π/8 (≈63°) and pitch 30°.

Keylight lighting should be consistent with the new camera orientation: the key light direction is currently (0.35, -0.55, 0.75) which points towards the top-left from the front. With yaw rotated towards +Y, the light should be (-0.35, 0.55, 0.75), pointing towards the top-right.

## Decision

- The front face of furniture is defined as the side with `+Y` normal.
- The standard camera front view is achieved by looking from the +Y side (yaw 2.8·π/8 ≈ 63°, pitch 30°).
- Camera::look_from(DVec3::Y) results in a view where right ≈ -X (left), up ≈ +Z, Y (front) towards the camera.
- The default camera is updated to yaw 2.8·π/8, pitch 30°, keeping other parameters unchanged.
- The key light direction is updated to (-0.35, 0.55, 0.75) so that when the front is visible, the light illuminates the front and left side.
- The corner orientation widget draws line connectors only for positive axes (+X, +Y, +Z), not for negative axes.
- The has_connector(direction) function returns true only for directions with a positive component (+X, +Y, +Z); negative directions and zero return false.

## Consequences

- The default scene load presents the furniture facade immediately, making it easier for users to start designing from the front.
- The key light matches the new camera orientation, keeping the front face well-lit and the left side slightly shadowed, similar to natural morning lighting.
- Line connectors on the orientation widget highlight the three visible axes (front, left, up) in a standard front view, improving visual clarity.
- All existing tests pass; new has_connector tests verify that positive axes have lines and negative axes and the zero vector do not.
- Documentation and ADR 0019 reference updated to reflect the new front convention and standard view behavior.

- The camera::look_from documentation mentions +Y as the front direction, -Y as the back, and provides examples for each.
- The orientation widget's line connector logic is consistent across all views.

## Related

- Superseded by ADR 0019 extension in docs/decisions/README.md
- Front convention added to PROJECT.md, §4.3 "Model space / Views"
- Camera doc comment in crates/scene/src/lib.rs updated with front direction notes.
