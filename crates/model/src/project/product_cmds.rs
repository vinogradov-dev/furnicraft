//! Commands that create a product and edit its authored envelope.
//!
//! A product is not a node kind: it is an ordinary container whose class carries `level = Product`
//! and which stores an [`Envelope`] (ADR 0097). The two arrive together here, so the document never
//! passes through a state that has one without the other, and a product is never placed inside
//! another product. A class written later through the property path is checked by
//! [`validate_class_change`], so the pairing survives every edit.

use crate::tree::{find_container, find_container_mut, find_node_mut};
use crate::*;

impl Project {
    /// Adds an empty product under `parent` and bumps the revision once.
    ///
    /// `class` must be an assignable class whose `level` is `Product`; the envelope is stored as
    /// given, and a product is never placed inside another product (ADR 0097). The caller validates
    /// the name. Adding below [`Project::MAX_ASSEMBLY_DEPTH`] is rejected.
    pub fn add_product(
        &mut self,
        parent: Id,
        name: impl Into<String>,
        class: ClassCode,
        envelope: Envelope,
        placement: Placement,
    ) -> Result<Id, ProjectError> {
        if !class.is_product_class() || !class.is_assignable() {
            return Err(ProjectError::NotAProductClass { class });
        }
        let Some((depth, _)) = find_container(&self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        if depth >= Self::MAX_ASSEMBLY_DEPTH {
            return Err(ProjectError::TooDeep { max: Self::MAX_ASSEMBLY_DEPTH });
        }
        if let Some(product) = self.product_of(parent) {
            return Err(ProjectError::ProductInsideProduct { product });
        }
        let next_revision = self.next_revision()?;
        let id = self.allocate_id()?;
        let mut container = Container::new(id, name, placement, Vec::new());
        // The class was checked assignable above, so the reserved-key validation below cannot fail.
        container.set_property(SYSTEM_CLASS, PropertyValue::class(class))?;
        container.set_envelope(Some(envelope));
        let Some((_depth, children)) = find_container_mut(&mut self.root, parent, 1) else {
            return Err(ProjectError::ContainerNotFound { id: parent });
        };
        children.push(Node::Container(container));
        self.revision = next_revision;
        Ok(id)
    }

    /// Replaces a product's authored envelope and bumps the revision once.
    ///
    /// The box is authored data: it is set when the product is created and edited only here, and
    /// no edit to a child resizes it. The content bounds of the children are derived on demand,
    /// never stored. A node that is not a product is refused.
    pub fn set_product_envelope(&mut self, id: Id, envelope: Envelope) -> Result<Revision, ProjectError> {
        if !self.container(id).is_some_and(Container::is_product) {
            return Err(ProjectError::NotAProduct { id });
        }
        let next_revision = self.next_revision()?;
        let Some(node) = find_node_mut(&mut self.root, id) else {
            return Err(ProjectError::NodeNotFound { id });
        };
        let Node::Container(container) = node else {
            return Err(ProjectError::NotAProduct { id });
        };
        container.set_envelope(Some(envelope));
        self.revision = next_revision;
        Ok(next_revision)
    }
}

/// Whether a class write keeps the envelope pairing and the product nesting rule (ADR 0097).
///
/// An envelope exists only on a container whose class has `level = Product`, a product class needs
/// an envelope, and a product is never a descendant of another product. The property commands run
/// this before they touch the tree, so a refused write leaves the revision and the history alone.
pub(crate) fn validate_class_change(project: &Project, node: Id, class: ClassCode) -> Result<(), ProjectError> {
    let envelope = project.container(node).and_then(Container::envelope);
    if class.is_product_class() {
        if envelope.is_none() {
            return Err(ProjectError::ProductClassWithoutEnvelope { node });
        }
        if let Some(parent) = project.parent_of(node)
            && let Some(product) = project.product_of(parent)
        {
            return Err(ProjectError::ProductInsideProduct { product });
        }
    } else if envelope.is_some() {
        return Err(ProjectError::EnvelopeWithoutProductClass { node });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::fixtures::*;
    use crate::*;

    fn envelope(x: i64, y: i64, z: i64) -> Envelope {
        Envelope::new(millimetres(x), millimetres(y), millimetres(z)).expect("a valid envelope")
    }

    /// A project holding one product with a panel and a group inside it.
    fn project_with_product() -> (Project, Id, Id, Id) {
        let mut project = sample_project();
        let root = project.root().id();
        let product = project
            .add_product(root, "Base cabinet", class("30.01.02"), envelope(600, 560, 720), Placement::IDENTITY)
            .expect("add product");
        let group = project.add_component(product, "Carcass", Placement::IDENTITY).expect("add group");
        let shelf = project
            .add_panel(group, "Shelf", sample_nominal(), None, sample_thickness(), Placement::IDENTITY)
            .expect("add shelf");
        (project, product, group, shelf)
    }

    #[test]
    fn an_envelope_is_a_validated_box_of_three_sides() {
        let box_ = envelope(600, 560, 720);
        assert_eq!(box_.components(), [millimetres(600), millimetres(560), millimetres(720)]);
        assert_eq!(box_.x(), millimetres(600));
        assert_eq!(box_.y(), millimetres(560));
        assert_eq!(box_.z(), millimetres(720));

        assert_eq!(
            Envelope::new(Length::ZERO, millimetres(1), millimetres(1)),
            Err(EnvelopeError::NonPositive { side: "width", value: Length::ZERO })
        );
        assert_eq!(
            Envelope::new(millimetres(10), Length::from_micrometres(-1), millimetres(1)),
            Err(EnvelopeError::NonPositive { side: "depth", value: Length::from_micrometres(-1) })
        );
        let too_large = Length::from_micrometres(Envelope::MAX_SIDE.micrometres() + 1);
        assert_eq!(
            Envelope::new(millimetres(10), millimetres(10), too_large),
            Err(EnvelopeError::TooLarge { side: "height", value: too_large, max: Envelope::MAX_SIDE })
        );
    }

    #[test]
    fn an_envelope_round_trips_through_json_and_refuses_a_bad_side() {
        let box_ = envelope(600, 560, 720);
        let text = serde_json::to_string(&box_).expect("serialize envelope");
        assert_eq!(text, r#"{"x":600000,"y":560000,"z":720000}"#);
        assert_eq!(serde_json::from_str::<Envelope>(&text).expect("deserialize envelope"), box_);

        let zero = r#"{"x":0,"y":560000,"z":720000}"#;
        assert!(serde_json::from_str::<Envelope>(zero).is_err());
        let unknown = r#"{"x":600000,"y":560000,"z":720000,"w":1}"#;
        assert!(serde_json::from_str::<Envelope>(unknown).is_err());
    }

    #[test]
    fn a_product_is_created_with_its_class_and_envelope_in_one_revision() {
        let mut project = sample_project();
        let root = project.root().id();
        let revision = project.revision();

        let product = project
            .add_product(root, "Wall cabinet", class("30.01.02"), envelope(600, 320, 720), Placement::IDENTITY)
            .expect("add product");

        assert_eq!(project.revision(), revision.checked_next().expect("revision"));
        let container = project.container(product).expect("the product is a container");
        assert_eq!(container.name(), "Wall cabinet");
        assert!(container.is_product());
        assert!(container.is_component(), "a product carries a class, so it is a component");
        assert_eq!(container.class(), class("30.01.02"));
        assert_eq!(container.envelope(), Some(envelope(600, 320, 720)));
        assert_eq!(project.parent_of(product), Some(root));
        assert_eq!(project.product_of(product), Some(product));
        assert_eq!(project.products(), vec![product]);
        project.validate().expect("the tree stays valid");
    }

    #[test]
    fn a_product_with_another_class_or_a_bad_parent_is_refused() {
        let mut project = sample_project();
        let root = project.root().id();
        let (mut nested, product, ..) = project_with_product();
        let revision = project.revision();

        assert_eq!(
            project.add_product(root, "Shelf", class("30.11"), envelope(600, 320, 720), Placement::IDENTITY),
            Err(ProjectError::NotAProductClass { class: class("30.11") })
        );
        let missing = Id::new(99).expect("id");
        assert_eq!(
            project.add_product(missing, "Loose", ClassCode::GENERAL_PRODUCT, envelope(1, 1, 1), Placement::IDENTITY),
            Err(ProjectError::ContainerNotFound { id: missing })
        );
        assert_eq!(project.revision(), revision, "a refused product changes nothing");
        assert!(project.products().is_empty());

        assert_eq!(
            nested.add_product(product, "Inner", ClassCode::GENERAL_PRODUCT, envelope(1, 1, 1), Placement::IDENTITY),
            Err(ProjectError::ProductInsideProduct { product })
        );
    }

    #[test]
    fn the_envelope_is_edited_without_touching_a_child_size() {
        let (mut project, product, _group, shelf) = project_with_product();
        let revision = project.revision();
        let new_box = envelope(900, 560, 2000);

        assert_eq!(
            project.set_product_envelope(product, new_box).expect("set envelope"),
            revision.checked_next().expect("revision")
        );
        assert_eq!(project.container(product).expect("product").envelope(), Some(new_box));
        assert_eq!(project.panel(shelf).expect("shelf").nominal_size(), sample_nominal());
        assert_eq!(project.set_product_envelope(shelf, new_box), Err(ProjectError::NotAProduct { id: shelf }));
    }

    #[test]
    fn a_product_is_found_from_a_part_two_levels_down() {
        let (project, product, group, shelf) = project_with_product();
        assert_eq!(project.product_of(shelf), Some(product));
        assert_eq!(project.product_of(group), Some(product));
        assert_eq!(project.product_of(Id::new(2).expect("sample shelf")), None, "a part outside every product");
        assert_eq!(project.product_of(project.root().id()), None, "the root is not a product");
    }

    #[test]
    fn a_duplicate_copies_the_envelope_and_a_converted_product_drops_it() {
        let (mut project, product, ..) = project_with_product();
        let copy = project.duplicate_node(product).expect("duplicate the product");
        let copied = project.container(copy).expect("the copy is a container");
        assert!(copied.is_product());
        assert_eq!(copied.envelope(), project.container(product).expect("product").envelope());
        assert_eq!(project.products(), vec![product, copy]);

        project.convert_to_group(product).expect("convert the product");
        assert!(project.container(product).is_none(), "the container is flattened away");
        assert_eq!(project.products(), vec![copy]);
        project.validate().expect("the tree stays valid");
    }

    #[test]
    fn the_model_refuses_the_two_envelope_mismatches() {
        let bare_product = Id::new(10).expect("id");
        let without_box = {
            let mut container = Container::new(bare_product, "Product", Placement::IDENTITY, Vec::new());
            container.set_property(SYSTEM_CLASS, PropertyValue::class(ClassCode::GENERAL_PRODUCT)).expect("class");
            let root =
                Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![Node::Container(container)]);
            Project::new(root)
        };
        assert_eq!(without_box, Err(ProjectError::ProductClassWithoutEnvelope { node: bare_product }));

        let boxed_group = Id::new(10).expect("id");
        let without_class = {
            let mut container = Container::new(boxed_group, "Group", Placement::IDENTITY, Vec::new());
            container.set_envelope(Some(envelope(600, 560, 720)));
            let root =
                Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, vec![Node::Container(container)]);
            Project::new(root)
        };
        assert_eq!(without_class, Err(ProjectError::EnvelopeWithoutProductClass { node: boxed_group }));
    }

    #[test]
    fn a_class_write_cannot_break_the_pairing_or_nest_a_product() {
        let (mut project, product, _group, _shelf) = project_with_product();
        let sample_shelf = Id::new(2).expect("sample shelf");

        // Turning the product into a shelf would leave its envelope on a non-product.
        assert_eq!(
            project.set_property(product, SYSTEM_CLASS, PropertyValue::class(class("30.11"))),
            Err(ProjectError::EnvelopeWithoutProductClass { node: product })
        );
        assert_eq!(
            project.clear_property(product, SYSTEM_CLASS),
            Err(ProjectError::EnvelopeWithoutProductClass { node: product })
        );

        // Giving a plain group a product class without an envelope is refused.
        assert_eq!(
            project.set_property(sample_shelf, SYSTEM_CLASS, PropertyValue::class(ClassCode::GENERAL_PRODUCT)),
            Err(ProjectError::ProductClassWithoutEnvelope { node: sample_shelf })
        );

        // Changing one product class into another keeps the box and is allowed.
        project
            .set_property(product, SYSTEM_CLASS, PropertyValue::class(ClassCode::GENERAL_PRODUCT))
            .expect("another product class");
        assert_eq!(project.container(product).expect("product").envelope(), Some(envelope(600, 560, 720)));

        // A component inside a product cannot become a second product.
        let inner = project.add_component(product, "Inner", Placement::IDENTITY).expect("add component");
        assert_eq!(
            project.set_property(inner, SYSTEM_CLASS, PropertyValue::class(ClassCode::GENERAL_PRODUCT)),
            Err(ProjectError::ProductClassWithoutEnvelope { node: inner })
        );
    }

    #[test]
    fn a_product_is_never_drawn_nested_on_load_and_the_root_is_never_one() {
        let (project, product, group, ..) = project_with_product();
        assert_eq!(project.product_of(group), Some(product), "the group starts inside the product");
        let mut wire = serde_json::to_value(&project).expect("serialize project");
        // Move the product's class and envelope onto the group inside it.
        let outer = wire["root"]["children"][2]["container"].clone();
        let properties = outer["properties"].clone();
        let box_ = outer["envelope"].clone();
        let child = &mut wire["root"]["children"][2]["container"]["children"][0]["container"];
        child["properties"] = properties;
        child["envelope"] = box_;
        let error = serde_json::from_value::<Project>(wire).expect_err("a product inside a product");
        assert!(error.to_string().contains(&format!("product {group} cannot sit inside another product")), "{error}");

        let root_product = {
            let mut root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, Vec::new());
            root.set_property(SYSTEM_CLASS, PropertyValue::class(ClassCode::GENERAL_PRODUCT)).expect("class");
            root.set_envelope(Some(envelope(600, 560, 720)));
            Project::new(root)
        };
        assert_eq!(root_product, Err(ProjectError::RootIsAProduct { id: Id::new(1).expect("id") }));
    }

    #[test]
    fn a_product_cannot_be_added_at_the_depth_limit() {
        let mut nested = Vec::new();
        let mut deepest = None;
        for index in 0..(Project::MAX_ASSEMBLY_DEPTH - 1) {
            let id = Id::new(u64::from(index) + 2).expect("id");
            if deepest.is_none() {
                deepest = Some(id);
            }
            let container = Container::new(id, "Nested", Placement::IDENTITY, nested);
            nested = vec![container.into()];
        }
        let root = Container::new(Id::new(1).expect("id"), "Root", Placement::IDENTITY, nested);
        let deepest = deepest.expect("at least one nested container");
        let mut project = Project::new(root).expect("tree at the depth limit");
        assert_eq!(
            project.add_product(
                deepest,
                "Too deep",
                ClassCode::GENERAL_PRODUCT,
                envelope(1, 1, 1),
                Placement::IDENTITY
            ),
            Err(ProjectError::TooDeep { max: Project::MAX_ASSEMBLY_DEPTH })
        );
        assert_eq!(project.products(), Vec::<Id>::new());
        assert_eq!(project.revision(), Revision::ZERO);
    }
}
