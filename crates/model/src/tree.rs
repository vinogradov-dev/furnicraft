//! One traversal of the document tree: lookup, bounds, counting, removal and id reallocation.
//!
//! The tree is a container of nodes, each container holding more nodes. Every walker is bounded by
//! [`Project::MAX_ASSEMBLY_DEPTH`], so a hostile file cannot overflow the stack; a walk returns
//! `None` or an error rather than recursing forever. Ids are never reused while a document is open.

use furnicraft_geom::DVec3;

use crate::{Container, Geometry, Id, Node, Panel, Pivot, Placement, Project, ProjectError};

/// The node with `id` at or below `children`, with its depth.
///
/// This is the one walk every lookup shares: container, panel, node and parent searches differ only
/// in what they do with the result.
fn find_in(children: &[Node], id: Id, depth: u32) -> Option<(u32, &Node)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    for child in children {
        if child.id() == id {
            return Some((depth, child));
        }
        if let Node::Container(container) = child
            && let Some(found) = find_in(container.children(), id, depth.saturating_add(1))
        {
            return Some(found);
        }
    }
    None
}

/// The node with `id` at or below `children`, with its depth, for mutation.
fn find_in_mut(children: &mut [Node], id: Id, depth: u32) -> Option<(u32, &mut Node)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    for child in children {
        if child.id() == id {
            return Some((depth, child));
        }
        if let Node::Container(container) = child
            && let Some(found) = find_in_mut(container.children_mut(), id, depth.saturating_add(1))
        {
            return Some(found);
        }
    }
    None
}

/// Finds a container, including the root, and its depth.
pub(crate) fn find_container(root: &Container, id: Id, depth: u32) -> Option<(u32, &Container)> {
    if root.id() == id {
        return Some((depth, root));
    }
    match find_in(root.children(), id, depth.saturating_add(1)) {
        Some((depth, Node::Container(container))) => Some((depth, container)),
        _ => None,
    }
}

/// Finds a container's child list for mutation, including the root.
pub(crate) fn find_container_mut(root: &mut Container, id: Id, depth: u32) -> Option<(u32, &mut Vec<Node>)> {
    if root.id() == id {
        return Some((depth, root.children_mut()));
    }
    match find_in_mut(root.children_mut(), id, depth.saturating_add(1)) {
        Some((depth, Node::Container(container))) => Some((depth, container.children_mut())),
        _ => None,
    }
}

/// Finds a panel and its depth.
pub(crate) fn find_panel_with_depth(root: &Container, id: Id, depth: u32) -> Option<(u32, &Panel)> {
    match find_in(root.children(), id, depth) {
        Some((depth, Node::Panel(panel))) => Some((depth, panel)),
        _ => None,
    }
}

/// Finds a panel for mutation and its depth.
pub(crate) fn find_panel_mut(root: &mut Container, id: Id, depth: u32) -> Option<(u32, &mut Panel)> {
    match find_in_mut(root.children_mut(), id, depth) {
        Some((depth, Node::Panel(panel))) => Some((depth, panel)),
        _ => None,
    }
}

/// Finds any node below the root.
pub(crate) fn find_node(root: &Container, id: Id) -> Option<&Node> {
    find_in(root.children(), id, 1).map(|(_, node)| node)
}

/// Finds any node below the root for mutation.
pub(crate) fn find_node_mut(root: &mut Container, id: Id) -> Option<&mut Node> {
    find_in_mut(root.children_mut(), id, 1).map(|(_, node)| node)
}

/// Finds the container that owns `id`.
pub(crate) fn find_parent(root: &Container, id: Id) -> Option<Id> {
    fn walk(children: &[Node], id: Id, parent: Id, depth: u32) -> Option<Id> {
        if depth > Project::MAX_ASSEMBLY_DEPTH {
            return None;
        }
        for child in children {
            if child.id() == id {
                return Some(parent);
            }
            if let Some(children) = child.children()
                && let Some(found) = walk(children, id, child.id(), depth.saturating_add(1))
            {
                return Some(found);
            }
        }
        None
    }
    walk(root.children(), id, root.id(), 1)
}

/// The local axis-aligned bounding box of the nominal panel boxes below `children`.
///
/// Returns `(min, max)` in the frame of the container that owns `children`, or `None` when the
/// subtree holds no panel or no finite corner. The walk starts from an identity parent, so each
/// child's own placement is relative to the frame being measured and no world lookup is needed per
/// panel.
pub(crate) fn panel_bounds(children: &[Node], parent: Placement, depth: u32) -> Option<(DVec3, DVec3)> {
    if depth > Project::MAX_ASSEMBLY_DEPTH {
        return None;
    }
    let mut bounds: Option<(DVec3, DVec3)> = None;
    let mut include = |point: DVec3| {
        if point.is_finite() {
            bounds = Some(match bounds {
                Some((min, max)) => (min.min(point), max.max(point)),
                None => (point, point),
            });
        }
    };
    for child in children {
        let Ok(relative) = Placement::compose(parent, child.placement()) else {
            continue;
        };
        match child {
            Node::Panel(panel) => {
                let size = panel.nominal_size();
                let length = size.length().millimetres_f64();
                let width = size.width().millimetres_f64();
                let thickness = panel.last_known_thickness().millimetres_f64();
                for x in [0.0, length] {
                    for y in [0.0, width] {
                        for z in [0.0, thickness] {
                            include(relative.transform_point(DVec3::new(x, y, z)));
                        }
                    }
                }
            }
            Node::Object(object) => {
                if let Some((min, max)) = object_bounds_mm(object.geometry()) {
                    for x in [min.x, max.x] {
                        for y in [min.y, max.y] {
                            for z in [min.z, max.z] {
                                include(relative.transform_point(DVec3::new(x, y, z)));
                            }
                        }
                    }
                }
            }
            Node::Container(container) => {
                if let Some((min, max)) = panel_bounds(container.children(), relative, depth.saturating_add(1)) {
                    include(min);
                    include(max);
                }
            }
            Node::Camera(_) | Node::Light(_) => {}
        }
    }
    bounds
}

/// The pivot a container saved before schema 6 was stored with: the centre of the local panel
/// bounds, or the origin.
///
/// Only the schema-5 migration writes this value now; a container created today is born at zero
/// and a group puts its content at zero through [`crate::rebase_at_anchor`]. No later pass
/// re-normalizes a stored pivot.
pub(crate) fn default_container_pivot(children: &[Node]) -> Pivot {
    let Some((min, max)) = panel_bounds(children, Placement::IDENTITY, 0) else {
        return Pivot::ZERO;
    };
    Pivot::from_local((min + max) * 0.5).unwrap_or(Pivot::ZERO)
}

/// The local bounding box of an object's geometry, in millimetres.
pub(crate) fn object_bounds_mm(geometry: &Geometry) -> Option<(DVec3, DVec3)> {
    match geometry {
        Geometry::Mesh(mesh) => {
            let (min, max) = mesh.bounds_micrometres();
            let to_point = |values: [i64; 3]| {
                DVec3::new(values[0] as f64 / 1000.0, values[1] as f64 / 1000.0, values[2] as f64 / 1000.0)
            };
            Some((to_point(min), to_point(max)))
        }
        Geometry::Constructive(constructive) => {
            let (min, max) = constructive.path().bounds_micrometres();
            let height = constructive.height()?.micrometres();
            Some((
                DVec3::new(min[0] as f64 / 1000.0, min[1] as f64 / 1000.0, 0.0),
                DVec3::new(max[0] as f64 / 1000.0, max[1] as f64 / 1000.0, height as f64 / 1000.0),
            ))
        }
        Geometry::Primitive(primitive) => {
            let (min, max) = primitive.bounds_micrometres();
            let to_point = |values: [i64; 3]| {
                DVec3::new(values[0] as f64 / 1000.0, values[1] as f64 / 1000.0, values[2] as f64 / 1000.0)
            };
            Some((to_point(min), to_point(max)))
        }
        Geometry::Path(path) => {
            let (min, max) = path.bounds_micrometres();
            Some((
                DVec3::new(min[0] as f64 / 1000.0, min[1] as f64 / 1000.0, 0.0),
                DVec3::new(max[0] as f64 / 1000.0, max[1] as f64 / 1000.0, 0.0),
            ))
        }
    }
}

pub(crate) fn node_height(node: &Node) -> u32 {
    match node {
        Node::Panel(_) | Node::Object(_) | Node::Camera(_) | Node::Light(_) => 1,
        Node::Container(container) => container.children().iter().map(node_height).max().unwrap_or(0).saturating_add(1),
    }
}

pub(crate) fn count_nodes(node: &Node) -> u64 {
    match node {
        Node::Panel(_) | Node::Object(_) | Node::Camera(_) | Node::Light(_) => 1,
        Node::Container(container) => {
            1u64.saturating_add(container.children().iter().map(count_nodes).fold(0, u64::saturating_add))
        }
    }
}

pub(crate) fn remove_child(root: &mut Container, id: Id) -> bool {
    remove_child_in(root.children_mut(), id)
}

pub(crate) fn remove_child_in(children: &mut Vec<Node>, id: Id) -> bool {
    if let Some(index) = children.iter().position(|child| child.id() == id) {
        children.remove(index);
        return true;
    }
    for child in children {
        if let Some(nested) = child.children_mut()
            && remove_child_in(nested, id)
        {
            return true;
        }
    }
    false
}

pub(crate) fn assign_new_ids(node: &mut Node, ids: &mut impl Iterator<Item = Id>) -> Result<Id, ProjectError> {
    let id = ids.next().ok_or(ProjectError::IdSpaceExhausted)?;
    match node {
        Node::Panel(panel) => panel.set_node_id(id),
        Node::Object(object) => object.set_node_id(id),
        Node::Camera(camera) => camera.set_node_id(id),
        Node::Light(light) => light.set_node_id(id),
        Node::Container(container) => {
            container.set_node_id(id);
            for child in container.children_mut() {
                assign_new_ids(child, ids)?;
            }
        }
    }
    Ok(id)
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    use furnicraft_geom::{DQuat, DVec3};

    #[test]
    fn world_placement_composes_the_chain_and_lists_geometry() {
        let mut project = sample_project();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");
        let shelf = Id::new(2).expect("shelf id");
        let root = project.root().id();

        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 0.0, 0.0), DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2))
                    .expect("drawer placement"),
            )
            .expect("place drawer");
        project
            .set_node_placement(
                front,
                Placement::new(DVec3::new(1.0, 0.0, 0.0), DQuat::IDENTITY).expect("front placement"),
            )
            .expect("place front");

        let world = project.world_placement(front).expect("front world placement");
        let origin = world.transform_point(DVec3::ZERO);
        assert!((origin - DVec3::new(10.0, 1.0, 0.0)).length() < 1.0e-12, "origin was {origin:?}");
        let world = project.world_placement(drawer).expect("drawer world placement");
        assert!((world.transform_point(DVec3::ZERO) - DVec3::new(10.0, 0.0, 0.0)).length() < 1.0e-12);
        assert_eq!(project.world_placement(root), Some(Placement::IDENTITY));
        assert_eq!(project.world_placement(Id::new(999).expect("id")), None);

        let cube = project
            .add_object(
                drawer,
                "Bracket",
                Placement::IDENTITY,
                Geometry::Mesh(Mesh::cube(Length::from_millimetres(20).expect("size")).expect("cube")),
            )
            .expect("add a geometry object");

        assert_eq!(project.geometry_ids_under(root), Some(vec![shelf, front, cube]));
        assert_eq!(project.geometry_ids_under(drawer), Some(vec![front, cube]));
        assert_eq!(project.geometry_ids_under(shelf), Some(vec![shelf]));
        assert_eq!(project.geometry_ids_under(cube), Some(vec![cube]));
        assert_eq!(project.geometry_ids_under(Id::new(999).expect("id")), None);
    }

    #[test]
    fn default_pivots_are_stored_and_keep_their_meaning() {
        let mut project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let drawer = Id::new(3).expect("drawer id");
        let root = project.root().id();

        // A panel starts at its local zero, a container is born at its own origin — a group puts
        // the centre of its children into the translation instead — and the root is always zero.
        assert_eq!(project.pivot(shelf), Some(Pivot::ZERO));
        assert_eq!(project.pivot(drawer), Some(Pivot::ZERO));
        assert_eq!(project.pivot(root), Some(Pivot::ZERO));

        project
            .set_node_placement(
                shelf,
                Placement::new(DVec3::new(100.0, 200.0, 300.0), DQuat::IDENTITY).expect("shelf placement"),
            )
            .expect("place shelf");
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("drawer placement"),
            )
            .expect("place drawer");

        // Placing a node changes only its world pivot, never the stored local one.
        assert_eq!(project.pivot_local(shelf), Some(DVec3::ZERO));
        assert_eq!(project.pivot_world(shelf), Some(DVec3::new(100.0, 200.0, 300.0)));
        assert_eq!(project.pivot_local(drawer), Some(DVec3::ZERO));
        assert_eq!(project.pivot_world(drawer), Some(DVec3::new(10.0, 20.0, 30.0)));
        assert_eq!(project.pivot_local(root), Some(DVec3::ZERO));
        assert_eq!(project.pivot_world(root), Some(DVec3::ZERO));

        let empty_placement = Placement::new(DVec3::new(-5.0, 7.0, 9.0), DQuat::IDENTITY).expect("empty placement");
        let empty = project.add_component(root, "Empty", empty_placement).expect("add empty assembly");
        assert_eq!(project.pivot_local(empty), Some(DVec3::ZERO));
        assert_eq!(project.pivot_world(empty), Some(DVec3::new(-5.0, 7.0, 9.0)));
        assert_eq!(project.pivot_local(Id::new(999).expect("id")), None);
        assert_eq!(project.pivot_world(Id::new(999).expect("id")), None);
    }

    #[test]
    fn a_container_pivot_is_local_and_turns_with_the_container() {
        let mut project = sample_project();
        let drawer = Id::new(3).expect("drawer id");
        project.center_node_pivot(drawer).expect("author a pivot at the children's centre");
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("drawer placement"),
            )
            .expect("place drawer");
        assert_eq!(project.pivot_local(drawer), Some(DVec3::new(350.0, 250.0, 9.0)));

        let turn = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2);
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), turn).expect("turned drawer placement"),
            )
            .expect("turn drawer");

        let local = project.pivot_local(drawer).expect("local pivot");
        assert!(
            (local - DVec3::new(350.0, 250.0, 9.0)).length() < 1.0e-12,
            "the local pivot must not jump, was {local:?}"
        );
        let world = project.pivot_world(drawer).expect("world pivot");
        assert!((world - DVec3::new(-240.0, 370.0, 39.0)).length() < 1.0e-12, "world pivot was {world:?}");
    }

    #[test]
    fn pivots_are_authored_once_and_survive_later_edits() {
        let mut project = sample_project();
        let root = project.root().id();
        let drawer = Id::new(3).expect("drawer id");
        let front = Id::new(4).expect("front id");

        // Centring uses the panel's own local box.
        project.center_node_pivot(front).expect("centre front");
        assert_eq!(project.pivot_local(front), Some(DVec3::new(350.0, 250.0, 9.0)));

        // Centring a container uses the union of its panels, including their child placements.
        project
            .set_node_placement(
                front,
                Placement::new(DVec3::new(1.0, 2.0, 3.0), DQuat::IDENTITY).expect("front placement"),
            )
            .expect("place front");
        project.center_node_pivot(drawer).expect("centre drawer");
        assert_eq!(project.pivot_local(drawer), Some(DVec3::new(351.0, 252.0, 12.0)));

        // Moving or resizing geometry does not move the stored pivot.
        let pivot = project.pivot(drawer);
        project
            .set_node_placement(
                drawer,
                Placement::new(DVec3::new(10.0, 20.0, 30.0), DQuat::IDENTITY).expect("drawer placement"),
            )
            .expect("place drawer");
        project
            .set_panel_nominal_size(front, PanelNominalSize::new(millimetres(900), millimetres(600)).expect("size"))
            .expect("resize front");
        assert_eq!(project.pivot(drawer), pivot);

        // An explicit zero is a real value, and the root has no editable pivot.
        project.set_node_pivot(drawer, Pivot::ZERO).expect("clear the pivot");
        assert_eq!(project.pivot_local(drawer), Some(DVec3::ZERO));
        assert!(matches!(project.set_node_pivot(root, Pivot::ZERO), Err(ProjectError::RootImmutable { .. })));
        assert!(matches!(project.center_node_pivot(root), Err(ProjectError::RootImmutable { .. })));
    }
}
