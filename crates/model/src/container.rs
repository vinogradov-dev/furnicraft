//! A node that holds children: an ordinary group or a furniture component.
//!
//! There is one container kind. A container is a component when a furniture type, a prototype
//! reference or an explicit marker says so; an unmarked container is a group and can be flattened.
//! The project root is a container too, with an immutable id and a zero pivot.

use serde::{Deserialize, Serialize};

use crate::{
    Id, Node, PivotLocal, Placement, Properties, PropertyError, PropertyValue, SYSTEM_COMPONENT, SYSTEM_FURNTYPE,
    SYSTEM_PROTOTYPE_ID, SYSTEM_PROTOTYPE_PARAMETERS, SYSTEM_PROTOTYPE_VERSION,
};

/// A node that can hold children: an ordinary group or a furniture component.
///
/// There is one container kind. "Assembly" is a word for a container marked as a component — a
/// cabinet, a drawer box, a facade or a hinge — by a non-zero `furntype`, a `prototype.id` or an
/// explicit `component = true`. An unmarked container is an ordinary group and can be flattened by
/// `node.ungroup`. The project root is a container.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    id: Id,
    name: String,
    placement: Placement,
    #[serde(default)]
    pivot_local: PivotLocal,
    children: Vec<Node>,
    #[serde(default)]
    properties: Properties,
}

impl Container {
    /// Creates an ordinary container. Global id uniqueness is checked by [`Project::validate`].
    ///
    /// The pivot starts at zero: a container is born at its own local zero, and a caller that wants
    /// the container grabbed at its content (a group) says so through [`crate::rebase_origin_at`],
    /// which also moves the placement. The root has its pivot forced back to zero by
    /// [`Project::new`].
    pub fn new(id: Id, name: impl Into<String>, placement: Placement, children: Vec<Node>) -> Self {
        Self {
            id,
            name: name.into(),
            placement,
            pivot_local: PivotLocal::ZERO,
            children,
            properties: Properties::new(),
        }
    }

    /// The stable container id.
    pub const fn id(&self) -> Id {
        self.id
    }

    /// The user-visible name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The placement relative to the parent frame.
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// The manipulation pivot in the container's own frame.
    pub const fn pivot_local(&self) -> PivotLocal {
        self.pivot_local
    }

    /// The panels and nested containers in this container.
    pub fn children(&self) -> &[Node] {
        &self.children
    }

    /// The typed properties of this container.
    pub const fn properties(&self) -> &Properties {
        &self.properties
    }

    /// The children for mutation inside the crate.
    pub(crate) fn children_mut(&mut self) -> &mut Vec<Node> {
        &mut self.children
    }

    /// Consumes the container, handing its children to the caller.
    pub(crate) fn into_children(self) -> Vec<Node> {
        self.children
    }

    /// Replaces the user-visible name; the caller has already rejected a blank one.
    pub(crate) fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Replaces the placement relative to the parent frame.
    pub(crate) fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    /// Replaces the stored manipulation pivot.
    pub(crate) fn set_pivot(&mut self, pivot_local: PivotLocal) {
        self.pivot_local = pivot_local;
    }

    /// Replaces the stable id when a subtree is copied.
    pub(crate) fn set_node_id(&mut self, id: Id) {
        self.id = id;
    }

    /// Whether the container is marked as a furniture component.
    ///
    /// A non-zero furniture type, a prototype reference or an explicit `component = true` all mark
    /// a component; an explicit `component = false` does not erase the other two markers.
    pub fn is_component(&self) -> bool {
        if let Some(PropertyValue::Bool { value: true }) = self.properties.get(SYSTEM_COMPONENT) {
            return true;
        }
        if let Some(PropertyValue::FurnType { value }) = self.properties.get(SYSTEM_FURNTYPE)
            && !value.is_unclassified()
        {
            return true;
        }
        self.properties.contains_key(SYSTEM_PROTOTYPE_ID)
    }

    /// Sets the explicit component marker through the validated property path.
    pub fn set_component_marker(&mut self, component: bool) -> Result<(), PropertyError> {
        self.properties.insert(SYSTEM_COMPONENT, PropertyValue::bool(component)).map(|_| ())
    }

    /// Clears the component marker, the prototype keys and the furniture type.
    pub fn clear_component_marker(&mut self) {
        self.properties.remove(SYSTEM_COMPONENT);
        self.properties.remove(SYSTEM_FURNTYPE);
        self.properties.remove(SYSTEM_PROTOTYPE_ID);
        self.properties.remove(SYSTEM_PROTOTYPE_VERSION);
        self.properties.remove(SYSTEM_PROTOTYPE_PARAMETERS);
    }

    /// Stores one validated property.
    pub fn set_property(&mut self, key: &str, value: PropertyValue) -> Result<(), PropertyError> {
        self.properties.insert(key, value).map(|_| ())
    }

    /// Removes one property; returns whether it was present.
    pub fn clear_property(&mut self, key: &str) -> bool {
        self.properties.remove(key).is_some()
    }
}
