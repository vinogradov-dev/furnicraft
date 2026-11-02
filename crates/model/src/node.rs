//! A child of a container: a nested container, a panel, a geometry object, a camera or a light.
//!
//! The enum is the only place that decides what a node kind is. Version-6 files used separate
//! `assembly` and `group` tags; this module accepts them on load and normalizes an assembly to a
//! component container.

use std::fmt;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::{CameraNode, Container, Id, LightNode, Object, Panel, Placement, Properties};

/// A child of a container.
///
/// A container is a group or a furniture component; a panel is an atomic leaf. Version-6 files
/// used separate `assembly` and `group` tags; [`Node`] accepts them on load and normalizes an
/// assembly to a component container.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Node {
    /// A group or furniture component with children.
    Container(Container),
    /// A panel.
    Panel(Panel),
    /// A free-form geometry object.
    Object(Object),
    /// An authored camera.
    Camera(CameraNode),
    /// An authored light.
    Light(LightNode),
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(NodeVisitor)
    }
}

/// Accepts current `container`/`panel` nodes and version-6 `assembly`/`group` nodes.
struct NodeVisitor;

impl<'de> Visitor<'de> for NodeVisitor {
    type Value = Node;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a node object")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        const FIELDS: &[&str] = &["container", "assembly", "group", "panel", "object", "camera", "light"];
        let mut container: Option<Container> = None;
        let mut panel: Option<Panel> = None;
        let mut object: Option<Object> = None;
        let mut camera: Option<CameraNode> = None;
        let mut light: Option<LightNode> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "container" | "assembly" | "group" => {
                    if container.is_some() {
                        return Err(de::Error::custom("a node must not have two container entries"));
                    }
                    let mut decoded = map.next_value::<Container>()?;
                    if key == "assembly" {
                        decoded.set_component_marker(true).map_err(de::Error::custom)?;
                    }
                    container = Some(decoded);
                }
                "panel" => {
                    if panel.is_some() {
                        return Err(de::Error::custom("a node must not have two panel entries"));
                    }
                    panel = Some(map.next_value()?);
                }
                "object" => {
                    if object.is_some() {
                        return Err(de::Error::custom("a node must not have two object entries"));
                    }
                    object = Some(map.next_value()?);
                }
                "camera" => {
                    if camera.is_some() {
                        return Err(de::Error::custom("a node must not have two camera entries"));
                    }
                    camera = Some(map.next_value()?);
                }
                "light" => {
                    if light.is_some() {
                        return Err(de::Error::custom("a node must not have two light entries"));
                    }
                    light = Some(map.next_value()?);
                }
                _ => return Err(de::Error::unknown_field(&key, FIELDS)),
            }
        }
        let present = usize::from(container.is_some())
            + usize::from(panel.is_some())
            + usize::from(object.is_some())
            + usize::from(camera.is_some())
            + usize::from(light.is_some());
        if present > 1 {
            return Err(de::Error::custom("a node must have exactly one kind"));
        }
        match (container, panel, object, camera, light) {
            (Some(container), None, None, None, None) => Ok(Self::Value::Container(container)),
            (None, Some(panel), None, None, None) => Ok(Self::Value::Panel(panel)),
            (None, None, Some(object), None, None) => Ok(Self::Value::Object(object)),
            (None, None, None, Some(camera), None) => Ok(Self::Value::Camera(camera)),
            (None, None, None, None, Some(light)) => Ok(Self::Value::Light(light)),
            _ => Err(de::Error::custom("a node must be a container, a panel, an object, a camera or a light")),
        }
    }
}

impl From<Container> for Node {
    fn from(container: Container) -> Self {
        Self::Container(container)
    }
}

impl From<Panel> for Node {
    fn from(panel: Panel) -> Self {
        Self::Panel(panel)
    }
}

impl From<Object> for Node {
    fn from(object: Object) -> Self {
        Self::Object(object)
    }
}

impl From<CameraNode> for Node {
    fn from(camera: CameraNode) -> Self {
        Self::Camera(camera)
    }
}

impl From<LightNode> for Node {
    fn from(light: LightNode) -> Self {
        Self::Light(light)
    }
}

impl Node {
    /// The stable node id, shared by every node kind.
    pub const fn id(&self) -> Id {
        match self {
            Self::Container(container) => container.id(),
            Self::Panel(panel) => panel.id(),
            Self::Object(object) => object.id(),
            Self::Camera(camera) => camera.id(),
            Self::Light(light) => light.id(),
        }
    }

    /// The user-visible node name.
    pub fn name(&self) -> &str {
        match self {
            Self::Container(container) => container.name(),
            Self::Panel(panel) => panel.name(),
            Self::Object(object) => object.name(),
            Self::Camera(camera) => camera.name(),
            Self::Light(light) => light.name(),
        }
    }

    /// The placement relative to the parent container.
    pub const fn placement(&self) -> Placement {
        match self {
            Self::Container(container) => container.placement(),
            Self::Panel(panel) => panel.placement(),
            Self::Object(object) => object.placement(),
            Self::Camera(camera) => camera.placement(),
            Self::Light(light) => light.placement(),
        }
    }

    /// The children of a container, or `None` for a leaf.
    pub fn children(&self) -> Option<&[Node]> {
        match self {
            Self::Container(container) => Some(container.children()),
            Self::Panel(_) | Self::Object(_) | Self::Camera(_) | Self::Light(_) => None,
        }
    }

    pub(crate) fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Self::Container(container) => Some(container.children_mut()),
            Self::Panel(_) | Self::Object(_) | Self::Camera(_) | Self::Light(_) => None,
        }
    }

    /// The nested container, or `None` for a leaf.
    pub const fn as_container(&self) -> Option<&Container> {
        match self {
            Self::Container(container) => Some(container),
            Self::Panel(_) | Self::Object(_) | Self::Camera(_) | Self::Light(_) => None,
        }
    }

    /// The panel, or `None` for another node kind.
    pub const fn as_panel(&self) -> Option<&Panel> {
        match self {
            Self::Panel(panel) => Some(panel),
            Self::Container(_) | Self::Object(_) | Self::Camera(_) | Self::Light(_) => None,
        }
    }

    /// The geometry object, or `None` for another node kind.
    pub const fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(object) => Some(object),
            Self::Container(_) | Self::Panel(_) | Self::Camera(_) | Self::Light(_) => None,
        }
    }

    /// The geometry object for mutation, or `None` for another node kind.
    pub const fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(object) => Some(object),
            Self::Container(_) | Self::Panel(_) | Self::Camera(_) | Self::Light(_) => None,
        }
    }

    /// The authored camera, or `None` for another node kind.
    pub const fn as_camera(&self) -> Option<&CameraNode> {
        match self {
            Self::Camera(camera) => Some(camera),
            Self::Container(_) | Self::Panel(_) | Self::Object(_) | Self::Light(_) => None,
        }
    }

    /// The authored camera for mutation, or `None` for another node kind.
    pub const fn as_camera_mut(&mut self) -> Option<&mut CameraNode> {
        match self {
            Self::Camera(camera) => Some(camera),
            Self::Container(_) | Self::Panel(_) | Self::Object(_) | Self::Light(_) => None,
        }
    }

    /// The authored light for mutation, or `None` for another node kind.
    pub const fn as_light_mut(&mut self) -> Option<&mut LightNode> {
        match self {
            Self::Light(light) => Some(light),
            Self::Container(_) | Self::Panel(_) | Self::Object(_) | Self::Camera(_) => None,
        }
    }

    /// The authored light, or `None` for another node kind.
    pub const fn as_light(&self) -> Option<&LightNode> {
        match self {
            Self::Light(light) => Some(light),
            Self::Container(_) | Self::Panel(_) | Self::Object(_) | Self::Camera(_) => None,
        }
    }

    /// The typed properties of the node.
    pub const fn properties(&self) -> &Properties {
        match self {
            Self::Container(container) => container.properties(),
            Self::Panel(panel) => panel.properties(),
            Self::Object(object) => object.properties(),
            Self::Camera(camera) => camera.properties(),
            Self::Light(light) => light.properties(),
        }
    }

    pub(crate) fn set_name(&mut self, name: String) {
        match self {
            Self::Container(container) => container.set_name(name),
            Self::Panel(panel) => panel.set_name(name),
            Self::Object(object) => object.set_name(name),
            Self::Camera(camera) => camera.set_name(name),
            Self::Light(light) => light.set_name(name),
        }
    }

    pub(crate) fn set_placement(&mut self, placement: Placement) {
        match self {
            Self::Container(container) => container.set_placement(placement),
            Self::Panel(panel) => panel.set_placement(placement),
            Self::Object(object) => object.set_placement(placement),
            Self::Camera(camera) => camera.set_placement(placement),
            Self::Light(light) => light.set_placement(placement),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    #[test]
    fn contains_node_covers_every_node_kind() {
        let mut project = sample_project();
        let root = project.root().id();
        let camera =
            project.add_camera(root, "Camera", Placement::IDENTITY, CameraProjection::DEFAULT).expect("add camera");
        let light =
            project.add_light(root, "Key", Placement::IDENTITY, LightKind::Sun, [1.0; 3], 1.0).expect("add light");

        assert!(project.contains_node(root), "the root owns the document frame");
        assert!(project.contains_node(Id::new(2).expect("shelf")), "a panel");
        assert!(project.contains_node(Id::new(3).expect("drawer")), "a container");
        assert!(project.contains_node(camera), "a camera is a scene node");
        assert!(project.contains_node(light), "a light is a scene node");
        assert!(!project.contains_node(Id::new(999).expect("id")), "an unknown id is not a node");
    }
}
