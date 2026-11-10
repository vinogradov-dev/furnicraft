//! Commands that change the tree: placement, naming, duplication, grouping and removal.

use furnicraft_geom::DVec3;

use crate::frame::children_center;
use crate::tree::{
    assign_new_ids, count_nodes, find_container, find_container_mut, find_node_mut, node_height, remove_child,
};
use crate::*;

impl Project {
    /// Moves a node's pivot within its own frame and bumps the revision once.
    ///
    /// The pivot is authored data: changing it never moves the node, and changing the geometry never
    /// moves it. The root defines the world frame and has no pivot to edit, and a panel's pivot is
    /// derived from its base corner (ADR 0083), so both are refused.
    pub fn set_node_pivot(&mut self, id: Id, pivot: Pivot) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        if self.panel(id).is_some() {
            return Err(ProjectError::PanelPivotIsDerived { panel: id });
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        match node {
            // A panel never reaches this arm: the check above refuses it before the revision moves.
            Node::Panel(_) => return Err(ProjectError::PanelPivotIsDerived { panel: id }),
            Node::Container(container) => container.set_pivot(pivot),
            Node::Object(object) => object.set_pivot(pivot),
            Node::Camera(camera) => camera.set_pivot(pivot),
            Node::Light(light) => light.set_pivot(pivot),
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Sets a node's pivot to the centre of its local bounds and bumps the revision once.
    ///
    /// A container with no panels has no bounds and is centred at its own origin. Returns
    /// [`ProjectError::RootImmutable`] for the root and [`ProjectError::PanelPivotIsDerived`] for a
    /// panel, whose pivot is always its base corner.
    pub fn center_node_pivot(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        if self.panel(id).is_some() {
            return Err(ProjectError::PanelPivotIsDerived { panel: id });
        }
        let center = self.bounds_center_local(id).unwrap_or(DVec3::ZERO);
        let pivot = Pivot::from_local(center).unwrap_or(Pivot::ZERO);
        self.set_node_pivot(id, pivot)
    }

    /// Checks that `nodes` can be wrapped in one new group.
    ///
    /// Grouping needs at least two distinct non-root nodes that share one parent container, and the
    /// new level must stay inside [`Project::MAX_ASSEMBLY_DEPTH`]; the name predates unified
    /// containers and is kept as a stable public name.
    pub fn validate_group(&self, nodes: &[Id]) -> Result<(), ProjectError> {
        self.group_parent(nodes).map(|_| ())
    }

    fn group_parent(&self, nodes: &[Id]) -> Result<(Id, u32), ProjectError> {
        if nodes.len() < 2 {
            return Err(ProjectError::TooFewNodes { minimum: 2, found: nodes.len() });
        }
        let mut seen = HashSet::new();
        let mut parent = None;
        for id in nodes {
            let id = *id;
            if !seen.insert(id.get()) {
                return Err(ProjectError::DuplicateNode { id });
            }
            if id == self.root.id() {
                return Err(ProjectError::RootImmutable { id });
            }
            if self.node(id).is_none() {
                return Err(ProjectError::NodeNotFound { id });
            }
            let node_parent = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
            match parent {
                Some(expected) if expected != node_parent => return Err(ProjectError::NodesNotSiblings),
                Some(_) => {}
                None => parent = Some(node_parent),
            }
        }
        let parent_id = parent.ok_or(ProjectError::TooFewNodes { minimum: 2, found: nodes.len() })?;
        let (parent_depth, _) =
            find_container(&self.root, parent_id, 1).ok_or(ProjectError::ContainerNotFound { id: parent_id })?;
        for id in nodes {
            let node = self.node(*id).ok_or(ProjectError::NodeNotFound { id: *id })?;
            let deepest = parent_depth
                .checked_add(node_height(node))
                .ok_or(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH })?;
            if deepest > Project::MAX_ASSEMBLY_DEPTH {
                return Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH });
            }
        }
        Ok((parent_id, parent_depth))
    }

    /// Adds an empty container marked as a furniture component and bumps the revision once.
    ///
    /// The parent may be the root or any nested container. Adding a child below
    /// [`Project::MAX_ASSEMBLY_DEPTH`] is rejected.
    pub fn add_component(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        let Some((depth, _)) = find_container(&self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        if depth >= Self::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Self::MAX_ASSEMBLY_DEPTH });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let mut container = Container::new(id, name, placement, Vec::new());
        container.set_component_marker(true)?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Container(container));
        self.revision = next_revision;
        Ok(id)
    }

    /// Renames any node, including the root container, and bumps the revision once.
    ///
    /// Callers validate that the name is not blank; the model keeps user-visible text verbatim.
    pub fn rename_node(&mut self, id: Id, name: impl Into<String>) -> Result<Revision, ProjectError> {
        let name = name.into();
        let next_revision = self.next_revision()?;
        if id == self.root.id() {
            self.root.set_name(name);
            self.revision = next_revision;
            return Ok(next_revision);
        }
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        node.set_name(name);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces a panel or container placement and bumps the revision once.
    ///
    /// The root container defines the world frame and cannot be moved. The placement stays relative to
    /// the parent container, so moving a container moves all of its leaves.
    pub fn set_node_placement(&mut self, id: Id, placement: Placement) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        node.set_placement(placement);
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Replaces several node placements at once and bumps the revision once.
    ///
    /// The whole batch is validated before anything changes: it must not be empty, every node must
    /// exist, none may be the root, and no node may appear twice. Listing a container together with
    /// one of its descendants is allowed here; a caller that moves a selection computes the
    /// placements and should first keep only [`Project::top_most_nodes`].
    pub fn set_node_placements(&mut self, placements: &[(Id, Placement)]) -> Result<Revision, ProjectError> {
        if placements.is_empty() {
            return Err(ProjectError::NoPlacements);
        }
        let mut seen = HashSet::new();
        for (id, _) in placements {
            if !seen.insert(*id) {
                return Err(ProjectError::DuplicateNode { id: *id });
            }
            if *id == self.root.id() {
                return Err(ProjectError::RootImmutable { id: *id });
            }
            if !self.contains_node(*id) {
                return Err(ProjectError::NodeNotFound { id: *id });
            }
        }
        let next_revision = self.next_revision()?;
        for (id, placement) in placements {
            let Some(node) = find_node_mut(&mut self.root, *id) else {
                return Err(ProjectError::NodeNotFound { id: *id });
            };
            node.set_placement(*placement);
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes a panel or a container, with its children, and bumps the revision once.
    ///
    /// The root cannot be removed. Removed ids are never reused.
    pub fn remove_node(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        if self.node(id).is_none() {
            return Err(ProjectError::NodeNotFound { id });
        }
        let next_revision = self.next_revision()?;
        if !remove_child(&mut self.root, id) {
            return Err(ProjectError::NodeNotFound { id });
        }
        // A deleted active camera, or a container that held it, returns the scene to the built-in
        // camera instead of leaving a dangling reference in the document.
        if self.active_camera.is_some_and(|camera| self.camera(camera).is_none()) {
            self.active_camera = None;
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Deep-copies a panel or a container under the same parent and returns the copy's id.
    ///
    /// Every copied node receives a fresh id from the document allocator. The copy keeps its kind,
    /// placement and geometry; only the copied root's name gets a `copy` suffix. The new node is
    /// inserted directly after the original.
    pub fn duplicate_node(&mut self, id: Id) -> Result<Id, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let original = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let parent_id = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
        let next_revision = self.next_revision()?;
        let count = count_nodes(&original);
        let ids = self.allocate_ids(count)?;
        let mut copy = original;
        let name = format!("{} copy", copy.name());
        copy.set_name(name);
        let new_id = assign_new_ids(&mut copy, &mut ids.into_iter())?;
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        let Some(index) = children.iter().position(|child| child.id() == id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        children.insert(index.saturating_add(1), copy);
        self.revision = next_revision;
        Ok(new_id)
    }

    /// Wraps sibling nodes in one new group and returns the new group id.
    ///
    /// All selected nodes stay in their existing order. The group is anchored at the centre of the
    /// selected children's union bounds: the new container's translation is that centre in the
    /// parent frame, every child placement moves by `-centre` and the pivot starts at zero, so no
    /// world transform changes while the card shows the point the group is grabbed at (ADR 0082).
    /// Children without bounds — only cameras and lights — have no centre, so such a group keeps the
    /// parent origin and its children's placements unchanged.
    pub fn group_nodes(&mut self, nodes: &[Id], name: impl Into<String>) -> Result<Id, ProjectError> {
        let (parent_id, _) = self.group_parent(nodes)?;
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let name = name.into();
        let selected: HashSet<u64> = nodes.iter().map(|id| id.get()).collect();
        let Some((_depth, parent)) = find_container(&self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        // The frame is resolved on a copy of the selection and the copy is what the group holds, so
        // a refused frame leaves the tree untouched and nothing fallible runs after the tree moves.
        let moved: Vec<Node> =
            parent.children().iter().filter(|child| selected.contains(&child.id().get())).cloned().collect();
        let (placement, moved) = match children_center(&moved) {
            Ok(_) => {
                let rebased = rebase_at_anchor(moved, Placement::IDENTITY, FrameAnchor::CenterOfBounds)
                    .map_err(|_| ProjectError::CreationFrame { id })?;
                (rebased.placement, rebased.content)
            }
            Err(FrameError::NoBounds) => (Placement::IDENTITY, moved),
            Err(_) => return Err(ProjectError::CreationFrame { id }),
        };
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        let mut rest = Vec::with_capacity(children.len());
        let mut insert_at = None;
        for child in children.drain(..) {
            if selected.contains(&child.id().get()) {
                if insert_at.is_none() {
                    insert_at = Some(rest.len());
                }
                continue;
            }
            rest.push(child);
        }
        let index = insert_at.unwrap_or(rest.len());
        rest.insert(index, Node::Container(Container::new(id, name, placement, moved)));
        *children = rest;
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces an ordinary container with its children and bumps the revision once.
    ///
    /// The children keep their world transforms: each child placement is composed with the removed
    /// container's placement. A container marked as a furniture component has its own identity and
    /// cannot be ungrouped by this command; use [`Project::convert_to_group`] first. Panels are
    /// atomic leaves and cannot be ungrouped. The root cannot be ungrouped.
    pub fn ungroup_group(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let node = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let Node::Container(container) = node else {
            return Err(ProjectError::NotAnOrdinaryContainer { id });
        };
        if container.is_component() {
            return Err(ProjectError::NotAComponent { id });
        }
        self.flatten_container(container)
    }

    /// Clears the component marker and prototype keys, then flattens the container.
    ///
    /// The container becomes an ordinary group and its children take its place in the parent, as
    /// with [`Project::ungroup_group`]. Nothing else about the children changes.
    pub fn convert_to_group(&mut self, id: Id) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        let node = self.node(id).ok_or(ProjectError::NodeNotFound { id })?.clone();
        let Node::Container(mut container) = node else {
            return Err(ProjectError::NotAnOrdinaryContainer { id });
        };
        container.clear_component_marker();
        self.flatten_container(container)
    }

    /// Moves a container's children into its parent and removes the container.
    fn flatten_container(&mut self, container: Container) -> Result<Revision, ProjectError> {
        let id = container.id();
        let parent_id = self.parent_of(id).ok_or(ProjectError::NodeNotFound { id })?;
        let next_revision = self.next_revision()?;
        let container_placement = container.placement();
        let mut children = Vec::with_capacity(container.children().len());
        for mut child in container.into_children() {
            let placement = Placement::compose(container_placement, child.placement())
                .map_err(|_| ProjectError::PlacementCompose { id })?;
            child.set_placement(placement);
            children.push(child);
        }
        let Some((_depth, parent)) = find_container_mut(&mut self.root, parent_id, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent_id });
        };
        let Some(index) = parent.iter().position(|child| child.id() == id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        parent.remove(index);
        parent.splice(index..index, children);
        self.revision = next_revision;
        Ok(next_revision)
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::tree::panel_bounds;
    use crate::*;

    use furnicraft_geom::{DQuat, DVec3};

    /// Asserts that two placements describe the same frame within the micrometre an anchor keeps.
    fn assert_same_frame(actual: Placement, expected: Placement, what: &str) {
        let gap = (actual.translation() - expected.translation()).length();
        assert!(
            gap < 1.1e-3,
            "{what}: translation was {:?}, expected {:?}",
            actual.translation(),
            expected.translation()
        );
        assert_eq!(actual.rotation(), expected.rotation(), "{what}: rotation changed");
    }

    #[test]
    fn nodes_can_be_renamed_and_placed() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let placement = Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::from_rotation_z(0.5)).expect("placement");

        assert_eq!(project.rename_node(panel, "Top").expect("rename panel").get(), 1);
        assert_eq!(project.rename_node(drawer, "Drawer box").expect("rename assembly").get(), 2);
        assert_eq!(project.rename_node(root, "Cabinet").expect("rename root").get(), 3);
        assert_eq!(project.panel(panel).expect("panel").name(), "Top");
        assert_eq!(project.container(drawer).expect("assembly").name(), "Drawer box");
        assert_eq!(project.root().name(), "Cabinet");

        assert_eq!(project.set_node_placement(panel, placement).expect("place panel").get(), 4);
        assert_eq!(project.set_node_placement(drawer, placement).expect("place assembly").get(), 5);
        assert_eq!(project.panel(panel).expect("panel").placement(), placement);
        assert_eq!(project.container(drawer).expect("assembly").placement(), placement);

        assert_eq!(project.set_node_placement(root, placement), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision().get(), 5);
    }

    #[test]
    fn placement_batches_validate_before_mutating_and_bump_once() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let missing = Id::new(99).expect("id");
        let moved = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("placement");
        let raised = Placement::new(DVec3::new(0.0, 0.0, 25.0), DQuat::IDENTITY).expect("placement");
        let revision = project.revision().get();

        assert_eq!(project.set_node_placements(&[]), Err(ProjectError::NoPlacements));
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (panel, raised)]),
            Err(ProjectError::DuplicateNode { id: panel })
        );
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (root, raised)]),
            Err(ProjectError::RootImmutable { id: root })
        );
        assert_eq!(
            project.set_node_placements(&[(panel, moved), (missing, raised)]),
            Err(ProjectError::NodeNotFound { id: missing })
        );
        assert_eq!(project.revision().get(), revision);
        assert_eq!(project.panel(panel).expect("panel").placement(), Placement::IDENTITY);
        assert_eq!(project.container(drawer).expect("assembly").placement(), Placement::IDENTITY);

        assert_eq!(
            project.set_node_placements(&[(panel, moved), (drawer, raised)]).expect("batch").get(),
            revision + 1
        );
        assert_eq!(project.panel(panel).expect("panel").placement(), moved);
        assert_eq!(project.container(drawer).expect("assembly").placement(), raised);
        assert_eq!(project.panel(front).expect("front").placement(), Placement::IDENTITY);
    }

    #[test]
    fn top_most_nodes_drop_descendants_repeats_and_unknown_ids() {
        let project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let missing = Id::new(99).expect("id");

        assert!(project.top_most_nodes(&[]).is_empty());
        assert_eq!(project.top_most_nodes(&[front, panel]), vec![front, panel]);
        assert_eq!(project.top_most_nodes(&[panel, drawer, front]), vec![panel, drawer]);
        assert_eq!(project.top_most_nodes(&[front, drawer]), vec![drawer]);
        assert_eq!(project.top_most_nodes(&[panel, panel, missing]), vec![panel]);
        assert_eq!(project.top_most_nodes(&[root, drawer, panel]), vec![root]);
    }

    #[test]
    fn removing_a_node_cascades_and_protects_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let next_id = project.next_id();

        assert_eq!(project.remove_node(drawer).expect("remove assembly").get(), 1);
        assert!(project.container(drawer).is_none());
        assert!(project.panel(front).is_none());
        assert!(project.panel(panel).is_some());
        assert_eq!(project.root().children().len(), 1);
        assert_eq!(project.next_id(), next_id);

        assert_eq!(project.remove_node(root), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(
            project.remove_node(Id::new(999).expect("id")),
            Err(ProjectError::NodeNotFound { id: Id::new(999).expect("id") })
        );
        assert_eq!(project.revision().get(), 1);
    }

    #[test]
    fn duplicating_a_node_remaps_every_id() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");

        let copy = project.duplicate_node(panel).expect("duplicate panel");
        assert_eq!(copy, Id::new(5).expect("id"));
        let copied_panel = project.panel(copy).expect("copied panel");
        assert_eq!(copied_panel.name(), "Shelf copy");
        assert_eq!(copied_panel.nominal_size(), project.panel(panel).expect("panel").nominal_size());
        assert_eq!(project.root().children().len(), 3);

        let copied_drawer = project.duplicate_node(drawer).expect("duplicate assembly");
        assert_eq!(copied_drawer, Id::new(6).expect("id"));
        let assembly = project.container(copied_drawer).expect("copied assembly");
        assert_eq!(assembly.name(), "Drawer copy");
        assert_eq!(assembly.children().len(), 1);
        let copied_front = assembly.children().first().expect("copied child").id();
        assert_ne!(copied_front, front);
        assert_eq!(project.node(copied_front).expect("child").name(), "Front");
        assert_eq!(project.parent_of(copied_front), Some(copied_drawer));
        project.validate().expect("ids stay unique");
        assert_eq!(project.root().id(), Id::new(1).expect("id"));
        assert_eq!(
            project.duplicate_node(project.root().id()),
            Err(ProjectError::RootImmutable { id: project.root().id() })
        );
    }

    #[test]
    fn grouping_siblings_preserves_order_and_world_placements() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let panel_placement = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("panel placement");
        let drawer_placement =
            Placement::new(DVec3::new(0.0, 20.0, 0.0), DQuat::from_rotation_z(0.25)).expect("drawer placement");
        let front_placement = Placement::new(DVec3::new(5.0, 0.0, 0.0), DQuat::IDENTITY).expect("front placement");
        project.set_node_placement(panel, panel_placement).expect("place panel");
        project.set_node_placement(drawer, drawer_placement).expect("place drawer");
        project.set_node_placement(front, front_placement).expect("place front");
        let before = project.world_placement(front).expect("front world before");
        let panel_world = project.world_placement(panel).expect("panel world before");
        let drawer_world = project.world_placement(drawer).expect("drawer world before");
        let siblings: Vec<Node> = project.root().children().to_vec();
        let (min, max) = panel_bounds(&siblings, Placement::IDENTITY, 0).expect("the union has bounds");
        let centre = (min + max) * 0.5;
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(project.group_nodes(&[panel, front], "Nested"), Err(ProjectError::NodesNotSiblings));
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);

        let group_id = project.group_nodes(&[panel, drawer], "Group").expect("group siblings");
        assert_eq!(group_id, Id::new(5).expect("id"));
        assert_eq!(project.root().children().len(), 1);
        assert_eq!(project.root().children().first().expect("group").id(), group_id);
        let group = project.container(group_id).expect("group");
        // The group's translation is the parent-frame centre of the children's union and its pivot
        // is zero, so the card names the point the group is grabbed at.
        assert_eq!(group.placement().rotation(), DQuat::IDENTITY);
        let translation = group.placement().translation();
        assert!((translation - centre).length() < 1.1e-3, "translation was {translation:?}, centre {centre:?}");
        assert_eq!(project.pivot_local(group_id), Some(DVec3::ZERO));
        let pivot_world = project.pivot_world(group_id).expect("world pivot");
        assert!((pivot_world - centre).length() < 1.1e-3, "world pivot was {pivot_world:?}");
        assert_eq!(group.children().len(), 2);
        assert_eq!(group.children()[0].id(), panel);
        assert_eq!(group.children()[1].id(), drawer);
        assert_eq!(project.parent_of(panel), Some(group_id));
        assert_eq!(project.parent_of(drawer), Some(group_id));
        // Every child moves by `-centre`, so its world transform and its rotation do not change.
        let moved_panel = project.panel(panel).expect("panel").placement();
        assert_eq!(moved_panel.rotation(), panel_placement.rotation());
        assert!(
            (moved_panel.translation() - (panel_placement.translation() - centre)).length() < 1.1e-3,
            "panel placement was {moved_panel:?}"
        );
        let moved_drawer = project.container(drawer).expect("drawer").placement();
        assert_eq!(moved_drawer.rotation(), drawer_placement.rotation());
        assert!(
            (moved_drawer.translation() - (drawer_placement.translation() - centre)).length() < 1.1e-3,
            "drawer placement was {moved_drawer:?}"
        );
        assert_same_frame(project.world_placement(panel).expect("panel world"), panel_world, "panel");
        assert_same_frame(project.world_placement(drawer).expect("drawer world"), drawer_world, "drawer");
        assert_same_frame(project.world_placement(front).expect("front world"), before, "front");
        // The children's union is centred on the group's own origin.
        let centre_after = project.bounds_center_local(group_id).expect("local centre");
        assert!(centre_after.length() < 1.1e-3, "local centre was {centre_after:?}");
        assert_eq!(project.revision().get(), revision.get() + 1);
    }

    #[test]
    fn grouping_rejects_duplicates_a_short_selection_and_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let next_id = project.next_id();
        let revision = project.revision();

        assert_eq!(project.group_nodes(&[panel], "Group"), Err(ProjectError::TooFewNodes { minimum: 2, found: 1 }));
        assert_eq!(project.group_nodes(&[panel, panel], "Group"), Err(ProjectError::DuplicateNode { id: panel }));
        assert_eq!(project.group_nodes(&[root, panel], "Group"), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.group_nodes(&[root, root], "Group"), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.next_id(), next_id);
        assert_eq!(project.revision(), revision);
    }

    #[test]
    fn a_group_inside_a_rotated_container_uses_the_parent_frame_centre() {
        let mut project = sample_project();
        let root = project.root().id();
        let frame_placement =
            Placement::new(DVec3::new(100.0, 50.0, 0.0), DQuat::from_rotation_z(0.4)).expect("frame placement");
        let frame = project.add_component(root, "Frame", frame_placement).expect("add a container");
        let first = project
            .add_panel(
                frame,
                "Side",
                sample_nominal(),
                None,
                sample_thickness(),
                Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("first placement"),
            )
            .expect("add a panel");
        let second = project
            .add_panel(
                frame,
                "Top",
                sample_nominal(),
                None,
                sample_thickness(),
                Placement::new(DVec3::new(0.0, 40.0, 12.0), DQuat::from_rotation_y(0.2)).expect("second placement"),
            )
            .expect("add a panel");
        let first_world = project.world_placement(first).expect("first world before");
        let second_world = project.world_placement(second).expect("second world before");
        let children: Vec<Node> = project.container(frame).expect("frame").children().to_vec();
        let (min, max) = panel_bounds(&children, Placement::IDENTITY, 0).expect("the union has bounds");
        let centre = (min + max) * 0.5;

        let group = project.group_nodes(&[first, second], "Pair").expect("group inside the frame");

        // The centre is in the parent (the rotated container's own) frame, so it is not the world
        // centre of the union.
        let container = project.container(group).expect("group");
        let translation = container.placement().translation();
        assert!((translation - centre).length() < 1.1e-3, "translation was {translation:?}, centre {centre:?}");
        assert_eq!(container.placement().rotation(), DQuat::IDENTITY);
        assert_eq!(project.parent_of(group), Some(frame));
        assert_eq!(project.pivot_local(group), Some(DVec3::ZERO));
        assert_same_frame(project.world_placement(first).expect("first world"), first_world, "first");
        assert_same_frame(project.world_placement(second).expect("second world"), second_world, "second");
        let group_world = project.pivot_world(group).expect("group world pivot");
        let expected_world = frame_placement.transform_point(centre);
        assert!((group_world - expected_world).length() < 1.1e-3, "pivot was {group_world:?}");
    }

    #[test]
    fn a_group_of_objects_and_panels_uses_the_same_union() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let object = project
            .add_object(
                root,
                "Brace",
                Placement::new(DVec3::new(1200.0, -30.0, 25.0), DQuat::from_rotation_z(0.1)).expect("placement"),
                Geometry::Mesh(box_mesh()),
            )
            .expect("add an object");
        project
            .set_node_placement(
                panel,
                Placement::new(DVec3::new(0.0, 0.0, 0.0), DQuat::from_rotation_x(0.15)).expect("panel placement"),
            )
            .expect("place panel");
        let panel_world = project.world_placement(panel).expect("panel world before");
        let object_world = project.world_placement(object).expect("object world before");
        let selection: Vec<Node> =
            [panel, object].iter().map(|id| project.node(*id).expect("selected node").clone()).collect();
        let (min, max) = panel_bounds(&selection, Placement::IDENTITY, 0).expect("the union has bounds");
        let centre = (min + max) * 0.5;

        let group = project.group_nodes(&[panel, object], "Mixed").expect("group a panel and an object");

        let translation = project.container(group).expect("group").placement().translation();
        assert!((translation - centre).length() < 1.1e-3, "translation was {translation:?}, centre {centre:?}");
        assert_eq!(project.pivot_local(group), Some(DVec3::ZERO));
        assert_same_frame(project.world_placement(panel).expect("panel world"), panel_world, "panel");
        assert_same_frame(project.world_placement(object).expect("object world"), object_world, "object");
        assert_eq!(project.panel(panel).expect("panel").placement().rotation(), DQuat::from_rotation_x(0.15));
    }

    #[test]
    fn a_group_without_bounds_keeps_the_parent_origin() {
        let mut project = sample_project();
        let root = project.root().id();
        let first = project.add_component(root, "First", Placement::IDENTITY).expect("add a container");
        let second_placement =
            Placement::new(DVec3::new(30.0, 0.0, 0.0), DQuat::from_rotation_z(0.25)).expect("placement");
        let second = project.add_component(root, "Second", second_placement).expect("add a container");

        let group = project.group_nodes(&[first, second], "Empty").expect("group two empty containers");

        // Children without geometry have no centre, so their placements and the parent origin stay.
        let container = project.container(group).expect("group");
        assert_eq!(container.placement(), Placement::IDENTITY);
        assert_eq!(project.pivot_local(group), Some(DVec3::ZERO));
        assert_eq!(project.container(first).expect("first").placement(), Placement::IDENTITY);
        assert_eq!(project.container(second).expect("second").placement(), second_placement);
        assert_eq!(container.children().len(), 2);
        assert_eq!(container.children()[0].id(), first);
        assert_eq!(container.children()[1].id(), second);
    }

    #[test]
    fn ungrouping_flattens_children_and_keeps_their_world_placements() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel id");
        let drawer = Id::new(3).expect("assembly id");
        let front = Id::new(4).expect("panel id");
        let panel_placement = Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::IDENTITY).expect("panel placement");
        let drawer_placement =
            Placement::new(DVec3::new(100.0, 0.0, 0.0), DQuat::from_rotation_z(0.5)).expect("drawer placement");
        let front_placement =
            Placement::new(DVec3::new(0.0, 5.0, 0.0), DQuat::from_rotation_x(0.25)).expect("front placement");
        project.set_node_placement(panel, panel_placement).expect("place panel");
        project.set_node_placement(drawer, drawer_placement).expect("place drawer");
        project.set_node_placement(front, front_placement).expect("place front");

        let group_id = project.group_nodes(&[panel, drawer], "Group").expect("group siblings");
        // Grouping moves the children into the group's frame; remember what it wrote.
        let grouped_panel = project.panel(panel).expect("panel").placement();
        let grouped_drawer = project.container(drawer).expect("drawer").placement();
        let group_placement =
            Placement::new(DVec3::new(50.0, 60.0, 70.0), DQuat::from_rotation_y(0.3)).expect("group placement");
        project.set_node_placement(group_id, group_placement).expect("place group");
        let revision = project.revision();

        assert_eq!(project.ungroup_group(group_id).expect("ungroup").get(), revision.get() + 1);
        assert!(project.container(group_id).is_none());
        assert_eq!(project.parent_of(panel), Some(root));
        assert_eq!(project.parent_of(drawer), Some(root));
        assert_eq!(project.parent_of(front), Some(drawer));
        assert_eq!(project.root().children().len(), 2);

        let expected_panel = Placement::compose(group_placement, grouped_panel).expect("panel world");
        assert_eq!(project.panel(panel).expect("panel").placement(), expected_panel);
        let expected_drawer = Placement::compose(group_placement, grouped_drawer).expect("drawer world");
        assert_eq!(project.container(drawer).expect("drawer").placement(), expected_drawer);
        let expected_front = Placement::compose(expected_drawer, front_placement).expect("front world");
        assert_eq!(project.world_placement(front).expect("front world"), expected_front);
        assert_eq!(project.panel(front).expect("front").placement(), front_placement);

        assert_eq!(project.ungroup_group(drawer), Err(ProjectError::NotAComponent { id: drawer }));
        assert_eq!(project.ungroup_group(panel), Err(ProjectError::NotAnOrdinaryContainer { id: panel }));
        assert_eq!(project.ungroup_group(root), Err(ProjectError::RootImmutable { id: root }));
    }

    #[test]
    fn a_panel_pivot_is_derived_and_both_pivot_commands_refuse_it() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel id");
        let revision = project.revision();
        let stored = project.panel(panel).expect("panel").placement();
        let pivot = Pivot::new(millimetres(1), millimetres(2), millimetres(3));

        assert_eq!(project.set_node_pivot(panel, pivot), Err(ProjectError::PanelPivotIsDerived { panel }));
        assert_eq!(project.center_node_pivot(panel), Err(ProjectError::PanelPivotIsDerived { panel }));
        assert_eq!(project.revision(), revision, "a refused pivot edit does not bump the revision");
        assert_eq!(project.pivot(panel), Some(Pivot::ZERO), "a panel's pivot stays at its base corner");
        assert_eq!(project.panel(panel).expect("panel").placement(), stored, "the refusal changes nothing");
    }
}
