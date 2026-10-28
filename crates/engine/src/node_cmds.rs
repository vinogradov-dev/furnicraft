//! Commands that create, rename, place, copy, group and remove scene nodes, and edit their
//! properties.
//!
//! A panel is an atomic leaf: it owns its geometry and machining and is never split into child
//! nodes. A container is either an ordinary group or a furniture component; a component is marked
//! by a non-zero `furntype`, a `prototype.id` or `component = true`. These commands wrap siblings
//! into a new group and flatten an ordinary container back into its parent without moving any leaf
//! in world space. `node.ungroup` refuses a component; `node.component.convert_to_group` clears the
//! marker first.

use std::collections::HashSet;

use furnicraft_model::{Catalog, Id, Pivot, Placement, Project, ProjectError, PropertyChange, PropertyValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::session::Session;

/// Stable id of the component container creation command.
pub const ASSEMBLY_CREATE_ID: &str = "assembly.create";

/// User-facing label of the component container creation command.
pub const ASSEMBLY_CREATE_LABEL: &str = "Create assembly";

/// Parameters for `assembly.create`.
///
/// The command creates a container marked as a furniture component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyCreate {
    /// Id of the container that receives the new component.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// Placement relative to the parent container.
    pub placement: Placement,
}

/// The `assembly.create` command.
pub(crate) const ASSEMBLY_CREATE: CommandSpec =
    CommandSpec::new(ASSEMBLY_CREATE_ID, ASSEMBLY_CREATE_LABEL, assembly_create_enabled, assembly_create_run);

fn assembly_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<AssemblyCreate>(params, ASSEMBLY_CREATE_ID) else {
        return false;
    };
    valid_name(&params.name) && session.project().is_container(params.parent)
}

fn assembly_create_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<AssemblyCreate>(params, ASSEMBLY_CREATE_ID)?;
    validate_name(&params.name, ASSEMBLY_CREATE_ID)?;
    let id = project.add_component(params.parent, params.name, params.placement)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the node rename command.
pub const NODE_RENAME_ID: &str = "node.rename";

/// User-facing label of the node rename command.
pub const NODE_RENAME_LABEL: &str = "Rename";

/// Parameters for `node.rename`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeRename {
    /// Panel or container to rename.
    pub node: Id,
    /// New user-visible name; it must contain a non-whitespace character.
    pub name: String,
}

/// The `node.rename` command.
pub(crate) const NODE_RENAME: CommandSpec =
    CommandSpec::new(NODE_RENAME_ID, NODE_RENAME_LABEL, node_rename_enabled, node_rename_run);

fn node_rename_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodeRename>(params, NODE_RENAME_ID) else {
        return false;
    };
    valid_name(&params.name) && session.project().contains_node(params.node)
}

fn node_rename_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeRename>(params, NODE_RENAME_ID)?;
    validate_name(&params.name, NODE_RENAME_ID)?;
    if !project.contains_node(params.node) {
        return Err(ProjectError::NodeNotFound { id: params.node }.into());
    }
    project.rename_node(params.node, params.name)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the node deletion command.
pub const NODE_DELETE_ID: &str = "node.delete";

/// User-facing label of the node deletion command.
pub const NODE_DELETE_LABEL: &str = "Delete";

/// Parameters for `node.delete`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDelete {
    /// Panel or container to remove with all of its children.
    pub node: Id,
}

/// The `node.delete` command.
pub(crate) const NODE_DELETE: CommandSpec =
    CommandSpec::new(NODE_DELETE_ID, NODE_DELETE_LABEL, node_delete_enabled, node_delete_run);

fn node_delete_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodeDelete>(params, NODE_DELETE_ID) else {
        return false;
    };
    session.project().node(params.node).is_some()
}

fn node_delete_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeDelete>(params, NODE_DELETE_ID)?;
    project.remove_node(params.node)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the node duplication command.
pub const NODE_DUPLICATE_ID: &str = "node.duplicate";

/// User-facing label of the node duplication command.
pub const NODE_DUPLICATE_LABEL: &str = "Duplicate";

/// Parameters for `node.duplicate`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeDuplicate {
    /// Panel or container to copy deeply.
    pub node: Id,
}

/// The `node.duplicate` command.
pub(crate) const NODE_DUPLICATE: CommandSpec =
    CommandSpec::new(NODE_DUPLICATE_ID, NODE_DUPLICATE_LABEL, node_duplicate_enabled, node_duplicate_run);

fn node_duplicate_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodeDuplicate>(params, NODE_DUPLICATE_ID) else {
        return false;
    };
    session.project().node(params.node).is_some()
}

fn node_duplicate_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeDuplicate>(params, NODE_DUPLICATE_ID)?;
    let id = project.duplicate_node(params.node)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the grouping command.
pub const NODE_GROUP_ID: &str = "node.group";

/// User-facing label of the grouping command.
pub const NODE_GROUP_LABEL: &str = "Group";

/// Parameters for `node.group`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeGroup {
    /// Sibling nodes to wrap in a new group.
    pub nodes: Vec<Id>,
    /// User-visible name of the new group.
    pub name: String,
}

/// The `node.group` command.
pub(crate) const NODE_GROUP: CommandSpec =
    CommandSpec::new(NODE_GROUP_ID, NODE_GROUP_LABEL, node_group_enabled, node_group_run);

fn node_group_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodeGroup>(params, NODE_GROUP_ID) else {
        return false;
    };
    valid_name(&params.name) && session.project().validate_group(&params.nodes).is_ok()
}

fn node_group_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeGroup>(params, NODE_GROUP_ID)?;
    validate_name(&params.name, NODE_GROUP_ID)?;
    let id = project.group_nodes(&params.nodes, params.name)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the ungrouping command.
pub const NODE_UNGROUP_ID: &str = "node.ungroup";

/// User-facing label of the ungrouping command.
pub const NODE_UNGROUP_LABEL: &str = "Ungroup";

/// Parameters for `node.ungroup`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeUngroup {
    /// Ordinary container to flatten into its parent; a component is refused.
    pub node: Id,
}

/// The `node.ungroup` command.
pub(crate) const NODE_UNGROUP: CommandSpec =
    CommandSpec::new(NODE_UNGROUP_ID, NODE_UNGROUP_LABEL, node_ungroup_enabled, node_ungroup_run);

fn node_ungroup_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodeUngroup>(params, NODE_UNGROUP_ID) else {
        return false;
    };
    let project = session.project();
    params.node != project.root().id() && project.container(params.node).is_some_and(|c| !c.is_component())
}

fn node_ungroup_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeUngroup>(params, NODE_UNGROUP_ID)?;
    project.ungroup_group(params.node)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the node placement command.
pub const NODE_PLACEMENT_SET_ID: &str = "node.placement.set";

/// User-facing label of the node placement command.
pub const NODE_PLACEMENT_SET_LABEL: &str = "Place";

/// Parameters for `node.placement.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePlacementSet {
    /// Panel or container to place relative to its parent.
    pub node: Id,
    /// The new rigid placement.
    pub placement: Placement,
}

/// The `node.placement.set` command.
pub(crate) const NODE_PLACEMENT_SET: CommandSpec = CommandSpec::new(
    NODE_PLACEMENT_SET_ID,
    NODE_PLACEMENT_SET_LABEL,
    node_placement_set_enabled,
    node_placement_set_run,
);

fn node_placement_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePlacementSet>(params, NODE_PLACEMENT_SET_ID) else {
        return false;
    };
    session.project().node(params.node).is_some()
}

fn node_placement_set_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePlacementSet>(params, NODE_PLACEMENT_SET_ID)?;
    project.set_node_placement(params.node, params.placement)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the multi-node placement command.
pub const NODE_PLACEMENT_SET_MANY_ID: &str = "node.placement.set_many";

/// User-facing label of the multi-node placement command.
pub const NODE_PLACEMENT_SET_MANY_LABEL: &str = "Place selection";

/// One node and the placement it receives.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePlacement {
    /// Panel or container to place relative to its parent.
    pub node: Id,
    /// The new rigid placement.
    pub placement: Placement,
}

/// Parameters for `node.placement.set_many`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePlacementSetMany {
    /// The nodes to place; at least one, each listed once, none the root.
    pub placements: Vec<NodePlacement>,
}

/// The `node.placement.set_many` command.
///
/// The whole batch is one document change: one revision bump and one undo entry, so a gizmo drag
/// that moves a selection is undone in one step.
pub(crate) const NODE_PLACEMENT_SET_MANY: CommandSpec = CommandSpec::new(
    NODE_PLACEMENT_SET_MANY_ID,
    NODE_PLACEMENT_SET_MANY_LABEL,
    node_placement_set_many_enabled,
    node_placement_set_many_run,
);

fn node_placement_set_many_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePlacementSetMany>(params, NODE_PLACEMENT_SET_MANY_ID) else {
        return false;
    };
    valid_placement_batch(session.project(), &params.placements)
}

fn node_placement_set_many_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePlacementSetMany>(params, NODE_PLACEMENT_SET_MANY_ID)?;
    let placements: Vec<(Id, Placement)> =
        params.placements.iter().map(|entry| (entry.node, entry.placement)).collect();
    project.set_node_placements(&placements)?;
    Ok(CommandOutcome::changed())
}

fn valid_placement_batch(project: &Project, placements: &[NodePlacement]) -> bool {
    if placements.is_empty() {
        return false;
    }
    let root = project.root().id();
    let mut seen = HashSet::new();
    placements.iter().all(|entry| seen.insert(entry.node) && entry.node != root && project.contains_node(entry.node))
}

/// Stable id of the pivot edit command.
pub const NODE_PIVOT_SET_ID: &str = "node.pivot.set";

/// User-facing label of the pivot edit command.
pub const NODE_PIVOT_SET_LABEL: &str = "Set pivot";

/// Parameters for `node.pivot.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePivotSet {
    /// Panel or container whose pivot moves.
    pub node: Id,
    /// The new pivot in the node's own frame, in exact micrometres.
    pub pivot: Pivot,
}

/// The `node.pivot.set` command.
pub(crate) const NODE_PIVOT_SET: CommandSpec =
    CommandSpec::new(NODE_PIVOT_SET_ID, NODE_PIVOT_SET_LABEL, node_pivot_set_enabled, node_pivot_set_run);

fn node_pivot_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePivotSet>(params, NODE_PIVOT_SET_ID) else {
        return false;
    };
    session.project().node(params.node).is_some()
}

fn node_pivot_set_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePivotSet>(params, NODE_PIVOT_SET_ID)?;
    project.set_node_pivot(params.node, params.pivot)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the pivot centring command.
pub const NODE_PIVOT_CENTER_ID: &str = "node.pivot.center";

/// User-facing label of the pivot centring command.
pub const NODE_PIVOT_CENTER_LABEL: &str = "Center pivot";

/// Parameters for `node.pivot.center`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePivotCenter {
    /// Panel or container whose pivot moves to its local bounds centre.
    pub node: Id,
}

/// The `node.pivot.center` command.
pub(crate) const NODE_PIVOT_CENTER: CommandSpec =
    CommandSpec::new(NODE_PIVOT_CENTER_ID, NODE_PIVOT_CENTER_LABEL, node_pivot_center_enabled, node_pivot_center_run);

fn node_pivot_center_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePivotCenter>(params, NODE_PIVOT_CENTER_ID) else {
        return false;
    };
    session.project().node(params.node).is_some()
}

fn node_pivot_center_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePivotCenter>(params, NODE_PIVOT_CENTER_ID)?;
    project.center_node_pivot(params.node)?;
    Ok(CommandOutcome::changed())
}

/// Stable id of the property edit command.
pub const NODE_PROPERTY_SET_ID: &str = "node.property.set";

/// User-facing label of the property edit command.
pub const NODE_PROPERTY_SET_LABEL: &str = "Set property";

/// Stable id of the multi-node property edit command.
pub const NODE_PROPERTY_SET_MANY_ID: &str = "node.property.set_many";

/// User-facing label of the multi-node property edit command.
pub const NODE_PROPERTY_SET_MANY_LABEL: &str = "Set properties";

/// Stable id of the property removal command.
pub const NODE_PROPERTY_CLEAR_ID: &str = "node.property.clear";

/// User-facing label of the property removal command.
pub const NODE_PROPERTY_CLEAR_LABEL: &str = "Clear property";

/// Stable id of the component-to-group conversion command.
pub const NODE_COMPONENT_CONVERT_TO_GROUP_ID: &str = "node.component.convert_to_group";

/// User-facing label of the component-to-group conversion command.
pub const NODE_COMPONENT_CONVERT_TO_GROUP_LABEL: &str = "Convert to group";

/// Parameters for `node.property.set`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePropertySet {
    /// Panel or container that receives the property.
    pub node: Id,
    /// Property key; a reserved system key requires its documented value type.
    pub key: String,
    /// The typed value to store.
    pub value: PropertyValue,
}

/// The `node.property.set` command.
pub(crate) const NODE_PROPERTY_SET: CommandSpec =
    CommandSpec::new(NODE_PROPERTY_SET_ID, NODE_PROPERTY_SET_LABEL, property_set_enabled, property_set_run);

fn property_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePropertySet>(params, NODE_PROPERTY_SET_ID) else {
        return false;
    };
    let project = session.project();
    params.node != project.root().id() && project.contains_node(params.node)
}

fn property_set_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePropertySet>(params, NODE_PROPERTY_SET_ID)?;
    project.set_property(params.node, &params.key, params.value)?;
    Ok(CommandOutcome::changed())
}

/// One property change in a batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeProperty {
    /// Panel or container that receives the value.
    pub node: Id,
    /// Property key.
    pub key: String,
    /// The typed value to store.
    pub value: PropertyValue,
}

/// Parameters for `node.property.set_many`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePropertySetMany {
    /// The changes; at least one, each node and key listed once, none the root.
    pub properties: Vec<NodeProperty>,
}

/// The `node.property.set_many` command.
///
/// The whole batch is one document change: one revision bump and one undo entry, so editing a
/// selection's properties is undone in one step.
pub(crate) const NODE_PROPERTY_SET_MANY: CommandSpec = CommandSpec::new(
    NODE_PROPERTY_SET_MANY_ID,
    NODE_PROPERTY_SET_MANY_LABEL,
    property_set_many_enabled,
    property_set_many_run,
);

fn property_set_many_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePropertySetMany>(params, NODE_PROPERTY_SET_MANY_ID) else {
        return false;
    };
    let project = session.project();
    let root = project.root().id();
    let mut seen = HashSet::new();
    !params.properties.is_empty()
        && params.properties.iter().all(|entry| {
            seen.insert((entry.node, entry.key.as_str())) && entry.node != root && project.contains_node(entry.node)
        })
}

fn property_set_many_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePropertySetMany>(params, NODE_PROPERTY_SET_MANY_ID)?;
    let changes: Vec<PropertyChange> = params
        .properties
        .into_iter()
        .map(|entry| PropertyChange { node: entry.node, key: entry.key, value: entry.value })
        .collect();
    project.set_properties(&changes)?;
    Ok(CommandOutcome::changed())
}

/// Parameters for `node.property.clear`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodePropertyClear {
    /// Panel or container whose property is removed.
    pub node: Id,
    /// Property key to remove.
    pub key: String,
}

/// The `node.property.clear` command.
pub(crate) const NODE_PROPERTY_CLEAR: CommandSpec =
    CommandSpec::new(NODE_PROPERTY_CLEAR_ID, NODE_PROPERTY_CLEAR_LABEL, property_clear_enabled, property_clear_run);

fn property_clear_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodePropertyClear>(params, NODE_PROPERTY_CLEAR_ID) else {
        return false;
    };
    let project = session.project();
    params.node != project.root().id()
        && project.properties(params.node).is_some_and(|properties| properties.contains_key(&params.key))
}

fn property_clear_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodePropertyClear>(params, NODE_PROPERTY_CLEAR_ID)?;
    project.clear_property(params.node, &params.key)?;
    Ok(CommandOutcome::changed())
}

/// Parameters for `node.component.convert_to_group`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeComponentConvertToGroup {
    /// Component container to demote and flatten.
    pub node: Id,
}

/// The `node.component.convert_to_group` command.
pub(crate) const NODE_COMPONENT_CONVERT_TO_GROUP: CommandSpec = CommandSpec::new(
    NODE_COMPONENT_CONVERT_TO_GROUP_ID,
    NODE_COMPONENT_CONVERT_TO_GROUP_LABEL,
    component_convert_to_group_enabled,
    component_convert_to_group_run,
);

fn component_convert_to_group_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<NodeComponentConvertToGroup>(params, NODE_COMPONENT_CONVERT_TO_GROUP_ID) else {
        return false;
    };
    let project = session.project();
    params.node != project.root().id() && project.is_component(params.node)
}

fn component_convert_to_group_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeComponentConvertToGroup>(params, NODE_COMPONENT_CONVERT_TO_GROUP_ID)?;
    project.convert_to_group(params.node)?;
    Ok(CommandOutcome::changed())
}

fn parse<T: serde::de::DeserializeOwned>(params: &Value, command: &'static str) -> Result<T, CommandError> {
    serde_json::from_value(params.clone())
        .map_err(|error| CommandError::InvalidParams { command, message: error.to_string() })
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
}

pub(crate) fn validate_name(name: &str, command: &'static str) -> Result<(), CommandError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(CommandError::InvalidParams { command, message: "name must not be empty".to_owned() })
    }
}
