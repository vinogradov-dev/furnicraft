//! The serialized envelope of a project.
//!
//! Loading goes through a wire struct, checks the schema version and the tree, and only then yields
//! a [`Project`]. Unknown top-level fields are rejected; the reserved property keys are validated on
//! the same pass. Exactly one schema is accepted — the current one — so a file from any other build
//! is a readable error and never a silent conversion (ADR 0096).

use serde::Deserialize;

use crate::project::Project;
use crate::tree::find_node;
use crate::*;

use super::{next_id_after, scan_tree};

/// Serialized form of [`Project`]; validation happens in its `TryFrom` implementation.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectWire {
    schema_version: u32,
    revision: Revision,
    #[serde(default)]
    next_id: Option<Id>,
    root: Container,
    #[serde(default)]
    active_camera: Option<Id>,
}

impl TryFrom<ProjectWire> for Project {
    type Error = ProjectError;

    fn try_from(wire: ProjectWire) -> Result<Self, Self::Error> {
        if wire.schema_version != Project::CURRENT_SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedSchemaVersion {
                found: wire.schema_version,
                supported: Project::CURRENT_SCHEMA_VERSION,
            });
        }
        // The root defines the world frame and has no pivot to edit, so a stored offset is a
        // contradiction and not something to normalize away.
        let root = wire.root;
        if root.pivot_local() != PivotLocal::ZERO {
            return Err(ProjectError::RootPivotNotZero { found: root.pivot_local() });
        }
        let max_id = scan_tree(&root)?;
        let next_id = match wire.next_id {
            Some(next_id) if next_id.get() <= max_id => {
                return Err(ProjectError::NextIdNotAfterMax { next: next_id, max: max_id });
            }
            Some(next_id) => next_id,
            None => next_id_after(max_id)?,
        };
        let active_camera = match wire.active_camera {
            Some(id) if find_node(&root, id).is_some_and(|node| node.as_camera().is_some()) => Some(id),
            Some(id) => return Err(ProjectError::CameraNotFound { id }),
            None => None,
        };
        Ok(Self {
            schema_version: Project::CURRENT_SCHEMA_VERSION,
            revision: wire.revision,
            next_id,
            root,
            active_camera,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    #[test]
    fn reserved_property_keys_are_validated_on_load() {
        let mut wire = serde_json::to_value(sample_project()).expect("serialize project");
        wire["root"]["properties"] = serde_json::json!({"component": {"kind": "text", "value": "yes"}});
        let error = serde_json::from_value::<Project>(wire).expect_err("invalid reserved property");
        assert!(error.to_string().contains("must be a boolean"), "error was {error}");
    }

    #[test]
    fn projects_reject_any_other_schema_and_unknown_fields() {
        let project = sample_project();
        let text = serde_json::to_string(&project).expect("serialize project");
        let current = format!("\"schema_version\":{}", Project::CURRENT_SCHEMA_VERSION);

        for other in [Project::CURRENT_SCHEMA_VERSION + 1, Project::CURRENT_SCHEMA_VERSION - 1] {
            let replaced = text.replacen(&current, &format!("\"schema_version\":{other}"), 1);
            assert_ne!(replaced, text, "the fixture must carry the schema line");
            let error = serde_json::from_str::<Project>(&replaced).expect_err("another schema is refused");
            assert!(error.to_string().contains("unsupported schema version"), "error was {error}");
        }

        let prefix = text.strip_suffix('}').expect("project JSON is an object");
        let extra = format!("{prefix},\"extra\":1}}");
        assert!(serde_json::from_str::<Project>(&extra).is_err());
    }

    #[test]
    fn a_root_with_a_pivot_offset_is_rejected() {
        let mut wire = serde_json::to_value(sample_project()).expect("serialize project");
        wire["root"]["pivot_local"] = serde_json::json!([1000, 0, 0]);
        let error = serde_json::from_value::<Project>(wire).expect_err("the root defines the world frame");
        assert!(error.to_string().contains("root's pivot must be zero"), "error was {error}");
    }

    #[test]
    fn duplicate_machining_ids_are_rejected_on_load() {
        let project = sample_project();
        let mut wire = serde_json::to_value(&project).expect("serialize project");
        let panel = &mut wire["root"]["children"][0]["panel"];
        panel["machining"] = serde_json::json!([
            {
                "id": 2,
                "anchor": {"kind": "face", "side": "A"},
                "params": {
                    "kind": "cutout",
                    "shape": {"rectangle": {"x": 10_000, "y": 10_000, "width": 100_000, "height": 50_000}},
                    "depth": "through"
                }
            }
        ]);
        let error = serde_json::from_value::<Project>(wire).expect_err("a machining id repeats the panel id");
        assert!(error.to_string().contains("duplicate"), "error was {error}");
    }
}
