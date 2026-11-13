//! Commands that create a product and edit the box it reserves.
//!
//! A product is an ordinary container whose class carries `level = Product` and which stores an
//! authored [`Envelope`] (ADR 0097); the model refuses either half without the other. `product.create`
//! is the one creation path and always leaves a class, using the registry's general-product
//! placeholder when the caller names none. `product.envelope.set` edits the box and never a child
//! size: the content bounds of the parts are derived when asked and never stored.

use furnicraft_model::{Catalog, ClassCode, Envelope, Id, Placement, Project};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::commands::{CommandError, CommandOutcome, CommandSpec};
use crate::node_cmds::{parse, validate_name};
use crate::session::Session;
use furnicraft_text::Message;

/// Stable id of the product creation command.
pub const PRODUCT_CREATE_ID: &str = "product.create";

/// User-facing label of the product creation command.
pub const PRODUCT_CREATE_LABEL: Message = Message::CmdCreateProduct;

/// Parameters for `product.create`.
///
/// A product is a container whose class is level `Product` and which stores its authored envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductCreate {
    /// Id of the container that receives the product; the root or a plain group.
    pub parent: Id,
    /// User-visible name; it must contain a non-whitespace character.
    pub name: String,
    /// The product class; `None` assigns the general-product placeholder, so a product always has
    /// one.
    #[serde(default)]
    pub class: Option<ClassCode>,
    /// The authored width, depth and height of the product.
    pub envelope: Envelope,
    /// Placement relative to the parent container.
    pub placement: Placement,
}

impl ProductCreate {
    /// The class a product gets when the caller names none: «изделие общего назначения».
    pub const PLACEHOLDER_CLASS: ClassCode = ClassCode::GENERAL_PRODUCT;

    /// The class the command will assign.
    pub fn resolved_class(&self) -> ClassCode {
        self.class.unwrap_or(Self::PLACEHOLDER_CLASS)
    }
}

/// The `product.create` command.
pub(crate) const PRODUCT_CREATE: CommandSpec =
    CommandSpec::new(PRODUCT_CREATE_ID, PRODUCT_CREATE_LABEL, product_create_enabled, product_create_run);

fn product_create_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ProductCreate>(params, PRODUCT_CREATE_ID) else {
        return false;
    };
    let project = session.project();
    let class = params.resolved_class();
    !params.name.trim().is_empty()
        && class.is_product_class()
        && class.is_assignable()
        && project.is_container(params.parent)
        && project.product_of(params.parent).is_none()
}

fn product_create_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ProductCreate>(params, PRODUCT_CREATE_ID)?;
    validate_name(&params.name, PRODUCT_CREATE_ID)?;
    let class = params.resolved_class();
    let id = project.add_product(params.parent, params.name, class, params.envelope, params.placement)?;
    Ok(CommandOutcome::created(id))
}

/// Stable id of the product envelope edit command.
pub const PRODUCT_ENVELOPE_SET_ID: &str = "product.envelope.set";

/// User-facing label of the product envelope edit command.
pub const PRODUCT_ENVELOPE_SET_LABEL: Message = Message::CmdSetProductEnvelope;

/// Parameters for `product.envelope.set`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProductEnvelopeSet {
    /// Product whose authored box changes.
    pub node: Id,
    /// The new width, depth and height in the product's own frame.
    pub envelope: Envelope,
}

/// The `product.envelope.set` command.
pub(crate) const PRODUCT_ENVELOPE_SET: CommandSpec = CommandSpec::new(
    PRODUCT_ENVELOPE_SET_ID,
    PRODUCT_ENVELOPE_SET_LABEL,
    product_envelope_set_enabled,
    product_envelope_set_run,
);

fn product_envelope_set_enabled(session: &Session, params: &Value) -> bool {
    let Ok(params) = parse::<ProductEnvelopeSet>(params, PRODUCT_ENVELOPE_SET_ID) else {
        return false;
    };
    session.project().container(params.node).is_some_and(|container| container.is_product())
}

fn product_envelope_set_run(
    project: &mut Project,
    _catalog: &Catalog,
    params: &Value,
) -> Result<CommandOutcome, CommandError> {
    let params = parse::<ProductEnvelopeSet>(params, PRODUCT_ENVELOPE_SET_ID)?;
    project.set_product_envelope(params.node, params.envelope)?;
    Ok(CommandOutcome::changed())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{ProjectError, Revision};
    use furnicraft_testkit::{millimetres, sample_catalog, sample_project};

    use crate::CommandRegistry;

    use super::*;

    fn envelope(x: i64, y: i64, z: i64) -> Envelope {
        Envelope::new(millimetres(x), millimetres(y), millimetres(z)).expect("a valid envelope")
    }

    fn create_params(parent: Id, class: Option<ClassCode>) -> Value {
        serde_json::to_value(ProductCreate {
            parent,
            name: "Base cabinet".to_owned(),
            class,
            envelope: envelope(600, 560, 720),
            placement: Placement::IDENTITY,
        })
        .expect("params serialize")
    }

    fn session_with_product() -> (Session, Id) {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let parent = session.project().root().id();
        let product = session
            .dispatch(PRODUCT_CREATE_ID, &create_params(parent, None))
            .expect("create product")
            .created_id()
            .expect("created id");
        (session, product)
    }

    #[test]
    fn creating_a_product_stores_a_class_and_an_envelope_in_one_undo_step() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        let params = create_params(root, Some(ClassCode::GENERAL_PRODUCT));

        let id =
            session.dispatch(PRODUCT_CREATE_ID, &params).expect("create product").created_id().expect("created id");
        assert_eq!(session.project().revision(), Revision::ZERO.checked_next().expect("revision"));
        let product = session.project().container(id).expect("the product is a container");
        assert_eq!(product.name(), "Base cabinet");
        assert!(product.is_product());
        assert!(product.is_component());
        assert_eq!(product.class(), ClassCode::GENERAL_PRODUCT);
        assert_eq!(product.envelope(), Some(envelope(600, 560, 720)));
        assert_eq!(session.project().product_of(id), Some(id));

        session.undo().expect("undo create");
        assert!(session.project().container(id).is_none());
        session.redo().expect("redo create");
        assert!(session.project().container(id).is_some());
    }

    #[test]
    fn a_product_without_a_class_gets_the_placeholder() {
        let (session, product) = session_with_product();
        let container = session.project().container(product).expect("product");
        assert_eq!(container.class(), ProductCreate::PLACEHOLDER_CLASS);
        assert!(container.class().is_product_class());
    }

    #[test]
    fn a_refused_product_writes_nothing() {
        let mut session = Session::with_catalog(sample_project(), sample_catalog());
        let root = session.project().root().id();
        let registry = CommandRegistry::new();
        let create = registry.get(PRODUCT_CREATE_ID).expect("product.create is registered");

        let missing = Id::new(99).expect("id");
        assert!(!create.enabled(&session, &create_params(missing, None)));

        let mut blank = create_params(root, None);
        blank["name"] = serde_json::json!("   ");
        assert!(!create.enabled(&session, &blank));
        assert!(session.dispatch(PRODUCT_CREATE_ID, &blank).is_err());

        let mut shelf_class = create_params(root, None);
        shelf_class["class"] = serde_json::json!("30.11".parse::<ClassCode>().expect("class").value());
        assert!(!create.enabled(&session, &shelf_class));
        assert_eq!(
            session.dispatch(PRODUCT_CREATE_ID, &shelf_class),
            Err(CommandError::Model(ProjectError::NotAProductClass { class: "30.11".parse().expect("class") }))
        );

        let unknown = serde_json::json!({"parent": root.get(), "name": "X", "envelope": {"x": 1}, "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}});
        assert!(!create.enabled(&session, &unknown), "an incomplete envelope is not a product");
        assert!(session.dispatch(PRODUCT_CREATE_ID, &unknown).is_err());

        assert_eq!(session.project().revision(), Revision::ZERO);
        assert!(session.project().products().is_empty());
    }

    #[test]
    fn a_product_is_not_created_inside_another_product() {
        let (mut session, product) = session_with_product();
        let registry = CommandRegistry::new();
        let create = registry.get(PRODUCT_CREATE_ID).expect("product.create is registered");
        let params = create_params(product, None);

        assert!(!create.enabled(&session, &params), "a product does not receive another product");
        assert_eq!(
            session.dispatch(PRODUCT_CREATE_ID, &params),
            Err(CommandError::Model(ProjectError::ProductInsideProduct { product }))
        );
        assert_eq!(session.project().products(), vec![product]);
    }

    #[test]
    fn the_envelope_is_edited_and_the_change_is_one_undo_step() {
        let (mut session, product) = session_with_product();
        let registry = CommandRegistry::new();
        let set = registry.get(PRODUCT_ENVELOPE_SET_ID).expect("product.envelope.set is registered");
        let params = serde_json::to_value(ProductEnvelopeSet { node: product, envelope: envelope(900, 560, 2000) })
            .expect("params serialize");
        assert!(set.enabled(&session, &params));

        let revision = session.project().revision();
        session.dispatch(PRODUCT_ENVELOPE_SET_ID, &params).expect("set envelope");
        assert_eq!(session.project().container(product).expect("product").envelope(), Some(envelope(900, 560, 2000)));
        assert_eq!(session.project().revision(), revision.checked_next().expect("revision"));
        session.undo().expect("undo the envelope edit");
        assert_eq!(session.project().container(product).expect("product").envelope(), Some(envelope(600, 560, 720)));

        let shelf = Id::new(2).expect("sample shelf");
        let wrong = serde_json::to_value(ProductEnvelopeSet { node: shelf, envelope: envelope(1, 1, 1) })
            .expect("params serialize");
        assert!(!set.enabled(&session, &wrong));
        assert_eq!(
            session.dispatch(PRODUCT_ENVELOPE_SET_ID, &wrong),
            Err(CommandError::Model(ProjectError::NotAProduct { id: shelf }))
        );
    }

    #[test]
    fn the_parameters_round_trip_and_refuse_unknown_fields() {
        let request = ProductCreate {
            parent: Id::new(1).expect("id"),
            name: "Wall cabinet".to_owned(),
            class: Some("30.01.02".parse().expect("class")),
            envelope: envelope(600, 320, 720),
            placement: Placement::IDENTITY,
        };
        let value = serde_json::to_value(&request).expect("serialize params");
        assert_eq!(serde_json::from_value::<ProductCreate>(value.clone()).expect("deserialize params"), request);
        let mut object = value.as_object().cloned().expect("params are an object");
        object.insert("extra".to_owned(), Value::Null);
        assert!(serde_json::from_value::<ProductCreate>(Value::Object(object)).is_err());

        let without_class = serde_json::json!({
            "parent": 1,
            "name": "Wall cabinet",
            "envelope": {"x": 600000, "y": 320000, "z": 720000},
            "placement": {"translation": [0.0, 0.0, 0.0], "rotation": [0.0, 0.0, 0.0, 1.0]}
        });
        let decoded: ProductCreate = serde_json::from_value(without_class).expect("an omitted class is allowed");
        assert_eq!(decoded.class, None);
        assert_eq!(decoded.resolved_class(), ClassCode::GENERAL_PRODUCT);
    }
}
