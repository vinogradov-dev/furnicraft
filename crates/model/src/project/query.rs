//! Read-only questions about a project: nodes, panels, origins, pivots, properties and queries.
//!
//! A query never mutates the document and never allocates more than the answer needs. The same
//! lookups feed the UI, reports and commands, so every caller agrees on what a node is.

use furnicraft_geom::DVec3;

use crate::project::Project;
use crate::tree::{find_container, find_node, find_panel_with_depth, find_parent, object_bounds_mm, panel_bounds};
use crate::*;

impl Project {
    /// Finds a container anywhere in the tree, including the root.
    pub fn container(&self, id: Id) -> Option<&Container> {
        find_container(&self.root, id, 1).map(|(_, container)| container)
    }

    /// Whether the id names a container or a panel.
    pub fn is_container(&self, id: Id) -> bool {
        self.container(id).is_some()
    }

    /// Whether the id names a container marked as a furniture component.
    pub fn is_component(&self, id: Id) -> bool {
        id != self.root.id() && self.container(id).is_some_and(Container::is_component)
    }

    /// Finds a panel anywhere in the tree.
    pub fn panel(&self, id: Id) -> Option<&Panel> {
        find_panel_with_depth(&self.root, id, 1).map(|(_, panel)| panel)
    }

    /// Finds a geometry object anywhere in the tree.
    pub fn object(&self, id: Id) -> Option<&Object> {
        self.node(id)?.as_object()
    }

    /// Finds an authored camera anywhere in the tree.
    pub fn camera(&self, id: Id) -> Option<&CameraNode> {
        self.node(id)?.as_camera()
    }

    /// Finds an authored light anywhere in the tree.
    pub fn light(&self, id: Id) -> Option<&LightNode> {
        self.node(id)?.as_light()
    }

    /// The camera the scene is rendered from, or `None` for the built-in camera.
    pub const fn active_camera(&self) -> Option<Id> {
        self.active_camera
    }

    /// Finds a panel or a nested container below the root.
    ///
    /// The root container is not a [`Node`]; use [`Project::container`] with the root id to reach
    /// it.
    pub fn node(&self, id: Id) -> Option<&Node> {
        find_node(&self.root, id)
    }

    /// The id of the container that directly owns `id`, or `None` for the root and unknown ids.
    pub fn parent_of(&self, id: Id) -> Option<Id> {
        find_parent(&self.root, id)
    }

    /// The given nodes that have no ancestor in the same list, in their original order.
    ///
    /// Unknown ids are skipped and a repeated id is kept once. The root counts as a top-most node;
    /// callers that cannot move it filter it out themselves. A multi-selection transform uses this
    /// so a moved container is not moved again through one of its descendants.
    pub fn top_most_nodes(&self, ids: &[Id]) -> Vec<Id> {
        let selected: HashSet<Id> = ids.iter().copied().filter(|id| self.contains_node(*id)).collect();
        let mut kept = HashSet::new();
        let mut top_most = Vec::new();
        for id in ids {
            if !selected.contains(id) || !kept.insert(*id) {
                continue;
            }
            let mut ancestor = self.parent_of(*id);
            let covered = loop {
                let Some(current) = ancestor else {
                    break false;
                };
                if selected.contains(&current) {
                    break true;
                }
                ancestor = self.parent_of(current);
            };
            if !covered {
                top_most.push(*id);
            }
        }
        top_most
    }

    /// Whether the id names the root or any node below it.
    ///
    /// Every node kind counts: a container, a panel, an object, a camera or a light. The root is
    /// included because it owns the document frame.
    pub fn contains_node(&self, id: Id) -> bool {
        id == self.root.id() || self.node(id).is_some()
    }

    /// The world placement of a node: every placement from the root down to it, composed.
    ///
    /// The root contributes its own placement, which is normally the identity. Returns `None` for
    /// an unknown id or a tree deeper than [`Project::MAX_ASSEMBLY_DEPTH`].
    pub fn world_placement(&self, id: Id) -> Option<Placement> {
        fn walk(node: &Node, id: Id, parent: Placement, depth: u32) -> Option<Placement> {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return None;
            }
            let world = Placement::compose(parent, node.placement()).ok()?;
            if node.id() == id {
                return Some(world);
            }
            let children = node.children()?;
            for child in children {
                if let Some(found) = walk(child, id, world, depth.saturating_add(1)) {
                    return Some(found);
                }
            }
            None
        }

        let root_world = self.root.placement();
        if self.root.id() == id {
            return Some(root_world);
        }
        for child in self.root.children() {
            if let Some(found) = walk(child, id, root_world, 1) {
                return Some(found);
            }
        }
        None
    }

    /// The same world placement expressed in `parent`'s frame.
    ///
    /// `inverse(parent_world) ∘ world`. The root's world placement is normally the identity, so a
    /// placement converted through the root comes back unchanged. A caller that knows a work-plane
    /// frame only in world space converts it here before handing it to a command that stores a
    /// placement relative to the parent.
    ///
    /// Returns [`ProjectError::NodeNotFound`] when `parent` is not in the tree and
    /// [`ProjectError::PlacementCompose`] when the parent frame or the world placement is not a
    /// rigid transform.
    pub fn placement_in_parent(&self, parent: Id, world: Placement) -> Result<Placement, ProjectError> {
        let parent_world = self.world_placement(parent).ok_or(ProjectError::NodeNotFound { id: parent })?;
        let parent_inverse = parent_world.inverse().map_err(|_| ProjectError::PlacementCompose { id: parent })?;
        Placement::compose(parent_inverse, world).map_err(|_| ProjectError::PlacementCompose { id: parent })
    }

    /// The ids of the nodes that produce geometry at or below `id`, in tree order.
    ///
    /// A panel and an object both produce a drawable, so each yields itself; a container yields
    /// every geometry node in its subtree. Cameras and lights carry no geometry and are skipped.
    /// Returns `None` for an unknown id; the list is empty for a container with no geometry.
    pub fn geometry_ids_under(&self, id: Id) -> Option<Vec<Id>> {
        fn collect(children: &[Node], ids: &mut Vec<Id>, depth: u32) {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return;
            }
            for child in children {
                match child {
                    Node::Panel(panel) => ids.push(panel.id()),
                    Node::Object(object) => ids.push(object.id()),
                    Node::Container(container) => collect(container.children(), ids, depth.saturating_add(1)),
                    Node::Camera(_) | Node::Light(_) => {}
                }
            }
        }

        if self.panel(id).is_some() || self.object(id).is_some() {
            return Some(vec![id]);
        }
        let container = self.container(id)?;
        let mut ids = Vec::new();
        collect(container.children(), &mut ids, 0);
        Some(ids)
    }

    /// The parent-frame position of the node's local zero, the point its content is written around.
    ///
    /// The origin is implicit in [`Placement::translation`]: it is not a separately stored field.
    /// Moving a node changes this position; re-anchoring the content changes how the vertices are
    /// written around it (`rebase_origin_at`). The root defines the world frame and returns its own
    /// placement. Returns `None` for an unknown id.
    pub fn origin_local(&self, id: Id) -> Option<DVec3> {
        if id == self.root.id() {
            return Some(self.root.placement().translation());
        }
        Some(self.node(id)?.placement().translation())
    }

    /// The world-space position of the node's local zero, composed from the root.
    ///
    /// This is the world point the inspector labels *World origin*. Returns `None` for an unknown id.
    pub fn origin_world(&self, id: Id) -> Option<DVec3> {
        Some(self.world_placement(id)?.translation())
    }

    /// The stored pivot of a node, as an exact offset from its origin.
    ///
    /// A panel always returns zero: its pivot is derived from its base corner (ADR 0083). A
    /// container or an object may author a non-zero offset; a group's translation is the centre of
    /// its children's union and its offset starts at zero. The root is not selectable and always
    /// returns zero. Returns `None` for an unknown id.
    pub fn pivot_local(&self, id: Id) -> Option<PivotLocal> {
        if id == self.root.id() {
            return Some(self.root.pivot_local());
        }
        match self.node(id)? {
            Node::Panel(panel) => Some(panel.pivot_local()),
            Node::Container(container) => Some(container.pivot_local()),
            Node::Object(object) => Some(object.pivot_local()),
            Node::Camera(camera) => Some(camera.pivot_local()),
            Node::Light(light) => Some(light.pivot_local()),
        }
    }

    /// The stored pivot as millimetres from the origin, for gizmos and tests.
    pub fn pivot_local_mm(&self, id: Id) -> Option<DVec3> {
        Some(self.pivot_local(id)?.to_local())
    }

    /// The manipulation pivot in world space: [`Self::pivot_local`] through the node's world placement.
    ///
    /// Both the translate and the rotate gizmo draw and rotate around this point. Returns `None` for
    /// an unknown id.
    pub fn pivot_world(&self, id: Id) -> Option<DVec3> {
        let local = self.pivot_local_mm(id)?;
        Some(self.world_placement(id)?.transform_point(local))
    }

    /// The centre of the node's local axis-aligned bounding box, in its own frame.
    ///
    /// A panel's box is its nominal rectangle times its last known thickness. A container's box is
    /// the union of the nominal panel boxes below it, each transformed by its placement relative to
    /// the container. A container with no panels has no bounds and returns `None`.
    pub fn bounds_center_local(&self, id: Id) -> Option<DVec3> {
        if let Some(panel) = self.panel(id) {
            let size = panel.nominal_size();
            let half = DVec3::new(
                size.length().millimetres_f64(),
                size.width().millimetres_f64(),
                panel.last_known_thickness().millimetres_f64(),
            ) * 0.5;
            return Some(half);
        }
        if let Some(object) = self.object(id) {
            let (min, max) = object_bounds_mm(object.geometry())?;
            return Some((min + max) * 0.5);
        }
        let (_, container) = find_container(&self.root, id, 0)?;
        let (min, max) = panel_bounds(container.children(), Placement::IDENTITY, 0)?;
        Some((min + max) * 0.5)
    }

    /// The property bag of a node, including the root.
    pub fn properties(&self, id: Id) -> Option<&Properties> {
        if id == self.root.id() {
            return Some(self.root.properties());
        }
        match self.node(id)? {
            Node::Container(container) => Some(container.properties()),
            Node::Panel(panel) => Some(panel.properties()),
            Node::Object(object) => Some(object.properties()),
            Node::Camera(camera) => Some(camera.properties()),
            Node::Light(light) => Some(light.properties()),
        }
    }

    /// The class stored on a node; unclassified when the node has no class.
    pub fn class(&self, id: Id) -> Option<ClassCode> {
        match self.properties(id)?.get(SYSTEM_CLASS) {
            Some(PropertyValue::Class { value }) => Some(*value),
            Some(_) => None,
            None => Some(ClassCode::UNCLASSIFIED),
        }
    }

    /// The nearest product at or above `id`, or `None` when the node is outside every product.
    ///
    /// The answer is computed by a walk and never stored, so no reference can go stale (ADR 0097).
    /// A part two levels down answers with the product that owns it, and a product answers with
    /// itself; physical containment is not kinship, so a piece standing in another's niche is not
    /// part of it.
    pub fn product_of(&self, id: Id) -> Option<Id> {
        let mut current = Some(id);
        while let Some(id) = current {
            if self.container(id).is_some_and(Container::is_product) {
                return Some(id);
            }
            current = self.parent_of(id);
        }
        None
    }

    /// Every product in the tree, in tree order.
    pub fn products(&self) -> Vec<Id> {
        fn walk(children: &[Node], found: &mut Vec<Id>, depth: u32) {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return;
            }
            for child in children {
                if let Node::Container(container) = child {
                    if container.is_product() {
                        found.push(container.id());
                    }
                    walk(container.children(), found, depth.saturating_add(1));
                }
            }
        }
        let mut found = Vec::new();
        walk(self.root.children(), &mut found, 1);
        found
    }

    /// Selects the nodes that match a query, in tree order.
    pub fn query(&self, query: &NodeQuery) -> Vec<Id> {
        fn matches(properties: &Properties, kind: StructuralKind, query: &NodeQuery) -> bool {
            if let Some(wanted) = query.kind()
                && wanted != kind
            {
                return false;
            }
            if let Some(wanted) = query.class() {
                let found = match properties.get(SYSTEM_CLASS) {
                    Some(PropertyValue::Class { value }) => *value,
                    _ => ClassCode::UNCLASSIFIED,
                };
                if !found.is_ancestor_or_self(wanted) {
                    return false;
                }
            }
            if let Some(predicate) = query.property()
                && properties.get(predicate.key()) != Some(predicate.value())
            {
                return false;
            }
            true
        }
        fn walk(children: &[Node], query: &NodeQuery, ids: &mut Vec<Id>, depth: u32) {
            if depth > Project::MAX_ASSEMBLY_DEPTH {
                return;
            }
            for child in children {
                match child {
                    Node::Container(container) => {
                        if matches(container.properties(), StructuralKind::Container, query) {
                            ids.push(container.id());
                        }
                        walk(container.children(), query, ids, depth.saturating_add(1));
                    }
                    Node::Panel(panel) => {
                        if matches(panel.properties(), StructuralKind::Panel, query) {
                            ids.push(panel.id());
                        }
                    }
                    Node::Object(object) => {
                        if matches(object.properties(), StructuralKind::Object, query) {
                            ids.push(object.id());
                        }
                    }
                    Node::Camera(camera) => {
                        if matches(camera.properties(), StructuralKind::Camera, query) {
                            ids.push(camera.id());
                        }
                    }
                    Node::Light(light) => {
                        if matches(light.properties(), StructuralKind::Light, query) {
                            ids.push(light.id());
                        }
                    }
                }
            }
        }
        let mut ids = Vec::new();
        walk(self.root.children(), query, &mut ids, 1);
        ids
    }
}

#[cfg(test)]
mod tests {
    use furnicraft_geom::DVec3;
    use furnicraft_units::{Degrees, Length};

    use crate::fixtures::*;
    use crate::*;

    #[test]
    fn placement_in_parent_undoes_the_parent_frame() {
        let mut project = sample_project();
        let root = project.root().id();
        let drawer = Id::new(3).expect("drawer id");
        let world = Placement::IDENTITY
            .with_translation(millimetres(30), millimetres(-10), millimetres(5))
            .expect("a world placement");

        assert_eq!(project.placement_in_parent(root, world), Ok(world), "the root frame is the identity");

        let turned = Placement::IDENTITY
            .with_translation(millimetres(100), Length::ZERO, Length::ZERO)
            .and_then(|placement| {
                placement.with_rotation_euler(EulerAngles {
                    z: Degrees::from_millidegrees(90_000),
                    ..EulerAngles::default()
                })
            })
            .expect("a turned parent");
        project.set_node_placement(drawer, turned).expect("turn the drawer");

        let local = project.placement_in_parent(drawer, world).expect("the world frame in the parent");
        // The parent is at (100, 0, 0) turned +90° about Z, so in its frame the world placement
        // lands at (−10, 70, 5) and the world X axis points along its −Y axis.
        assert!((local.translation() - DVec3::new(-10.0, 70.0, 5.0)).length() < 1e-9, "was {local:?}");
        assert!((local.transform_vector(DVec3::X) - DVec3::new(0.0, -1.0, 0.0)).length() < 1e-9, "was {local:?}");

        let parent_world = project.world_placement(drawer).expect("the parent's world frame");
        let rebuilt = Placement::compose(parent_world, local).expect("compose the frames back");
        assert!((rebuilt.translation() - world.translation()).length() < 1e-9, "was {rebuilt:?}");
        assert!(rebuilt.rotation().dot(world.rotation()).abs() >= 1.0 - 1e-12, "was {rebuilt:?}");

        let missing = Id::new(999).expect("id");
        assert_eq!(
            project.placement_in_parent(missing, world),
            Err(ProjectError::NodeNotFound { id: missing }),
            "an unknown parent has no frame"
        );
    }

    #[test]
    fn a_furniture_type_marks_a_container_as_a_component() {
        let mut project = sample_project();
        let root = project.root().id();
        let group = project.group_nodes(&[Id::new(2).expect("id"), Id::new(3).expect("id")], "G").expect("group");
        assert!(!project.is_component(group));

        project.set_property(group, SYSTEM_CLASS, PropertyValue::class(class("30.11"))).expect("class");
        assert!(project.is_component(group));
        project.set_property(group, SYSTEM_COMPONENT, PropertyValue::bool(false)).expect("false marker");
        assert!(project.is_component(group), "an explicit false does not erase the furniture type");
        assert_eq!(project.class(group), Some(class("30.11")));
        assert_eq!(project.class(root), Some(ClassCode::UNCLASSIFIED));

        assert_eq!(project.ungroup_group(group), Err(ProjectError::NotAComponent { id: group }));

        project.convert_to_group(group).expect("convert to group");
        assert!(project.container(group).is_none());
        assert!(project.properties(Id::new(2).expect("id")).is_some());
    }

    #[test]
    fn queries_select_by_kind_furniture_type_and_property() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel");
        let front = Id::new(4).expect("front");
        let drawer = Id::new(3).expect("drawer");
        project.set_property(drawer, SYSTEM_CLASS, PropertyValue::class(class("30.11"))).expect("class");
        project.set_property(panel, "role", PropertyValue::text("shelf")).expect("role");

        assert_eq!(project.query(&NodeQuery::any()), vec![panel, drawer, front]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Container)), vec![drawer]);
        assert_eq!(project.query(&NodeQuery::any().with_kind(StructuralKind::Panel)), vec![panel, front]);
        assert_eq!(
            project.query(&NodeQuery::any().with_class_under(class("30"))),
            vec![drawer],
            "a query on a group matches every type below it"
        );
        assert_eq!(project.query(&NodeQuery::any().with_class_under(class("30.11"))), vec![drawer]);
        assert!(project.query(&NodeQuery::any().with_class_under(class("40"))).is_empty());
        assert_eq!(
            project.query(&NodeQuery::any().with_class_under(ClassCode::UNCLASSIFIED)),
            vec![panel, front],
            "unclassified is a sentinel, not the root of the type tree"
        );
        assert_eq!(project.query(&NodeQuery::any().with_property("role", PropertyValue::text("shelf"))), vec![panel]);
        assert!(
            project
                .query(
                    &NodeQuery::any()
                        .with_kind(StructuralKind::Panel)
                        .with_property("role", PropertyValue::text("shelf"))
                )
                .len()
                == 1
        );
    }
}
