//! Commands that add and edit the camera and light objects the user authors.

use crate::light::{valid_light_color, valid_light_intensity};
use crate::tree::{find_container_mut, find_node_mut};
use crate::*;

impl Project {
    /// Adds an authored camera to an existing container and bumps the revision once.
    ///
    /// The projection is validated by [`CameraNode::new`]; nothing changes when it is refused.
    pub fn add_camera(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
        projection: CameraProjection,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let camera = CameraNode::new(id, name, placement, projection)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Camera(camera));
        self.revision = next_revision;
        Ok(id)
    }

    /// Adds an authored light to an existing container and bumps the revision once.
    pub fn add_light(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
        kind: LightKind,
        color: [f32; 3],
        intensity: f32,
    ) -> Result<Id, ProjectError> {
        if !self.is_container(parent) {
            return Err(ProjectError::ContainerNotFound { id: parent });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let light = LightNode::new(id, name, placement, kind, color, intensity)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Light(light));
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces a camera's projection and bumps the revision once.
    ///
    /// The projection is validated before anything changes; an id that is not a camera is
    /// [`ProjectError::CameraNotFound`].
    pub fn set_camera_projection(&mut self, id: Id, projection: CameraProjection) -> Result<Revision, ProjectError> {
        if !projection.is_valid() {
            return Err(CameraError::InvalidProjection.into());
        }
        let next_revision = self.next_revision()?;
        let Some(camera) = find_node_mut(&mut self.root, id).and_then(Node::as_camera_mut) else {
            return Err(ProjectError::CameraNotFound { id });
        };
        camera.set_projection(projection)?;
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces a light's kind, colour and intensity and bumps the revision once.
    ///
    /// The values are validated before anything changes; an id that is not a light is
    /// [`ProjectError::LightNotFound`].
    pub fn set_light_params(
        &mut self,
        id: Id,
        kind: LightKind,
        color: [f32; 3],
        intensity: f32,
    ) -> Result<Revision, ProjectError> {
        if !valid_light_color(color) {
            return Err(LightError::InvalidColor.into());
        }
        if !valid_light_intensity(intensity) {
            return Err(LightError::InvalidIntensity.into());
        }
        let next_revision = self.next_revision()?;
        let Some(light) = find_node_mut(&mut self.root, id).and_then(Node::as_light_mut) else {
            return Err(ProjectError::LightNotFound { id });
        };
        light.set_params(kind, color, intensity)?;
        self.revision = next_revision;
        Ok(next_revision)
    }
}
