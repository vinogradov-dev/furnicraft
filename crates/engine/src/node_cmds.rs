//! Commands that create, rename, place, copy, group and remove scene nodes.
//!
//! A panel is an atomic leaf: it owns its geometry and machining and is never split into child
//! nodes. An assembly is an editable group; these commands wrap siblings into a new assembly and
//! flatten an assembly back into its parent without moving any leaf in world space.

use furnicraft_model::{Catalog, Id, Placement, Project, ProjectError};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::session::Session;

/// Stable id of the assembly creation command.
pub const ASSEMBLY_CREATE_ID: &str = "assembly.create";

/// User-facing label of the assembly creation command.
pub const ASSEMBLY_CREATE_LABEL: &str = "Create assembly";

/// Parameters for `assembly.create`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyCreate {
    /// Id of the assembly that receives the new group.
    pub parent: Id,
    /// User-visible assembly name; it must contain a non-whitespace character.
    pub name: String,
    /// Placement relative to the parent assembly.
    pub placement: Placement,
}

/// The `assembly.create` command.
pub(crate) const ASSEMBLY_CREATE: CommandSpec =
    CommandSpec::new(ASSEMBLY_CREATE_ID, ASSEMBLY_CREATE_LABEL, assembly_create_enabled, assembly_create_run);

fn assembly_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<AssemblyCreate>(params, ASSEMBLY_CREATE_ID) else {
        return false;
    };
    valid_name(&params.name) && session.project().assembly(params.parent).is_some()
}

fn assembly_create_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<AssemblyCreate>(params, ASSEMBLY_CREATE_ID)?;
    validate_name(&params.name, ASSEMBLY_CREATE_ID)?;
    let id = project.add_assembly(params.parent, params.name, params.placement)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the assembly rename command.
pub const ASSEMBLY_RENAME_ID: &str = "assembly.rename";

/// User-facing label of the assembly rename command.
pub const ASSEMBLY_RENAME_LABEL: &str = "Rename assembly";

/// Parameters for `assembly.rename`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssemblyRename {
    /// Id of the assembly to rename.
    pub assembly: Id,
    /// New user-visible name; it must contain a non-whitespace character.
    pub name: String,
}

/// The `assembly.rename` command.
pub(crate) const ASSEMBLY_RENAME: CommandSpec =
    CommandSpec::new(ASSEMBLY_RENAME_ID, ASSEMBLY_RENAME_LABEL, assembly_rename_enabled, assembly_rename_run);

fn assembly_rename_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<AssemblyRename>(params, ASSEMBLY_RENAME_ID) else {
        return false;
    };
    valid_name(&params.name) && session.project().assembly(params.assembly).is_some()
}

fn assembly_rename_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<AssemblyRename>(params, ASSEMBLY_RENAME_ID)?;
    validate_name(&params.name, ASSEMBLY_RENAME_ID)?;
    if project.assembly(params.assembly).is_none() {
        return Err(ProjectError::AssemblyNotFound { id: params.assembly }.into());
    }
    project.rename_node(params.assembly, params.name)?;
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
    /// Panel or nested assembly to remove with all of its children.
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
    /// Panel or nested assembly to copy deeply.
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
    /// Sibling panels and/or nested assemblies to wrap in a new assembly.
    pub nodes: Vec<Id>,
    /// User-visible name of the new assembly.
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
    /// Assembly to flatten into its parent.
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
    project.assembly(params.node).is_some() && params.node != project.root().id()
}

fn node_ungroup_run(project: &mut Project, _catalog: &Catalog, params: &Value) -> Result<CommandOutcome, CommandError> {
    let params = parse::<NodeUngroup>(params, NODE_UNGROUP_ID)?;
    project.ungroup_assembly(params.node)?;
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
    /// Panel or nested assembly to place relative to its parent.
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

fn parse<T: serde::de::DeserializeOwned>(params: &Value, command: &'static str) -> Result<T, CommandError> {
    serde_json::from_value(params.clone())
        .map_err(|error| CommandError::InvalidParams { command, message: error.to_string() })
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
}

fn validate_name(name: &str, command: &'static str) -> Result<(), CommandError> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(CommandError::InvalidParams { command, message: "name must not be empty".to_owned() })
    }
}
