//! Commands that read and write the property bag of a node.

use super::product_cmds::validate_class_change;
use crate::properties::validate_property_value;
use crate::tree::find_node_mut;
use crate::*;

impl Project {
    /// Sets one property on one node and bumps the revision once.
    pub fn set_property(&mut self, id: Id, key: &str, value: PropertyValue) -> Result<Revision, ProjectError> {
        let changes = [PropertyChange { node: id, key: key.to_owned(), value }];
        self.set_properties(&changes)
    }

    /// Sets several properties as one document change: one revision and one undo entry.
    pub fn set_properties(&mut self, changes: &[PropertyChange]) -> Result<Revision, ProjectError> {
        if changes.is_empty() {
            return Err(ProjectError::NoPropertyChanges);
        }
        let root = self.root.id();
        let mut seen: HashSet<(u64, &str)> = HashSet::new();
        for change in changes {
            if change.node == root {
                return Err(ProjectError::RootImmutable { id: change.node });
            }
            if !seen.insert((change.node.get(), change.key.as_str())) {
                return Err(ProjectError::DuplicateProperty { node: change.node, key: change.key.clone() });
            }
            if !self.contains_node(change.node) {
                return Err(ProjectError::NodeNotFound { id: change.node });
            }
            validate_property_value(&change.key, &change.value)?;
            if change.key == SYSTEM_CLASS
                && let PropertyValue::Class { value } = &change.value
            {
                validate_class_change(self, change.node, *value)?;
            }
        }
        let next_revision = self.next_revision()?;
        for change in changes {
            let Some(node) = find_node_mut(&mut self.root, change.node) else {
                return Err(ProjectError::NodeNotFound { id: change.node });
            };
            match node {
                Node::Container(container) => container.set_property(&change.key, change.value.clone())?,
                Node::Panel(panel) => panel.set_property(&change.key, change.value.clone())?,
                Node::Object(object) => object.set_property(&change.key, change.value.clone())?,
                Node::Camera(camera) => camera.set_property(&change.key, change.value.clone())?,
                Node::Light(light) => light.set_property(&change.key, change.value.clone())?,
            }
        }
        self.revision = next_revision;
        Ok(next_revision)
    }

    /// Removes one property from one node and bumps the revision once.
    pub fn clear_property(&mut self, id: Id, key: &str) -> Result<Revision, ProjectError> {
        if id == self.root.id() {
            return Err(ProjectError::RootImmutable { id });
        }
        if key == SYSTEM_CLASS {
            validate_class_change(self, id, ClassCode::UNCLASSIFIED)?;
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let removed = match node {
            Node::Container(container) => container.clear_property(key),
            Node::Panel(panel) => panel.clear_property(key),
            Node::Object(object) => object.clear_property(key),
            Node::Camera(camera) => camera.clear_property(key),
            Node::Light(light) => light.clear_property(key),
        };
        if !removed {
            return Err(ProjectError::PropertyNotFound { node: id, key: key.to_owned() });
        }
        self.revision = next_revision;
        Ok(next_revision)
    }
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    #[test]
    fn projects_set_and_clear_properties_with_one_revision_per_batch() {
        let mut project = sample_project();
        let panel = Id::new(2).expect("panel");
        let drawer = Id::new(3).expect("drawer");
        let revision = project.revision();

        let next = project.set_property(panel, "material.finish", PropertyValue::text("oak")).expect("set property");
        assert_eq!(next.get(), revision.get() + 1);
        assert_eq!(project.properties(panel).and_then(|p| p.get("material.finish")), Some(&PropertyValue::text("oak")));

        let next = project
            .set_properties(&[
                PropertyChange { node: panel, key: "edge".to_owned(), value: PropertyValue::integer(2) },
                PropertyChange { node: drawer, key: "drawer.count".to_owned(), value: PropertyValue::integer(3) },
            ])
            .expect("batch");
        assert_eq!(next.get(), revision.get() + 2);
        assert_eq!(project.revision(), next);
        assert_eq!(project.properties(drawer).and_then(|p| p.get("drawer.count")), Some(&PropertyValue::integer(3)));
    }

    #[test]
    fn project_property_batches_reject_duplicates_unknown_nodes_and_the_root() {
        let mut project = sample_project();
        let root = project.root().id();
        let panel = Id::new(2).expect("panel");
        let missing = Id::new(99).expect("id");
        let revision = project.revision();
        let change = |node, key: &str| PropertyChange { node, key: key.to_owned(), value: PropertyValue::integer(1) };

        assert_eq!(project.set_properties(&[]), Err(ProjectError::NoPropertyChanges));
        assert_eq!(
            project.set_properties(&[change(panel, "a"), change(panel, "a")]),
            Err(ProjectError::DuplicateProperty { node: panel, key: "a".to_owned() })
        );
        assert_eq!(project.set_properties(&[change(missing, "a")]), Err(ProjectError::NodeNotFound { id: missing }));
        assert_eq!(project.set_properties(&[change(root, "a")]), Err(ProjectError::RootImmutable { id: root }));
        assert_eq!(project.revision(), revision);

        assert_eq!(
            project.clear_property(panel, "missing"),
            Err(ProjectError::PropertyNotFound { node: panel, key: "missing".to_owned() })
        );
        assert_eq!(project.revision(), revision);
        project.set_property(panel, "a", PropertyValue::integer(1)).expect("set");
        assert_eq!(project.clear_property(panel, "a").expect("clear").get(), revision.get() + 2);
        assert!(!project.properties(panel).expect("properties").contains_key("a"));
    }
}
