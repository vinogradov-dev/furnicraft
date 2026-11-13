//! What a node's class lets the editor do.
//!
//! The class registry states behaviour and this module is the one place a command or a gizmo asks
//! what that behaviour means (ADR 0094). A worktop resizes by length only, a hinge is never resized
//! and never banded, a fitting never enters nesting, and a sink may sit in a worktop while a hinge
//! may not. Every consumer reads the same accessor, so the command, the palette entry and the gizmo
//! cannot disagree about what a class forbids.
//!
//! A node without a class, or one whose stored code the registry does not know, has no rule to
//! apply: every gate is open. That is what keeps an ordinary scene editable before a maker assigns
//! a class, and it is the same reading the cutting list will use — an unclassified node has no
//! supply, so the list must not invent one.
//!
//! No code path here compares a class with a literal. The digits identify and the traits decide:
//! the accessor reads `extent`, `operations`, `nesting`, `measure` and `host`, and a test changes
//! the class of one node to prove the decision follows the row rather than the command.

use std::fmt;

use furnicraft_model::{ClassCode, Extent, Id, MachiningKind, Measure, Nesting, Operation, Project, TraitSet};

/// One local dimension of a node: the pair the `extent` trait gates.
///
/// The convention is the document's: local X is the length, Y the width and Z the thickness
/// (ADR 0083). A panel's nominal size is its length and width, so a worktop keeps its length and
/// fixes the other two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dimension {
    /// The local X axis.
    Length,
    /// The local Y axis.
    Width,
    /// The local Z axis.
    Thickness,
}

impl Dimension {
    /// Every dimension, in the order of a local frame's axes.
    pub const ALL: [Self; 3] = [Self::Length, Self::Width, Self::Thickness];
}

/// The trait a machining kind performs.
///
/// It lives beside the gates so a new operation kind is a compile error in one place instead of a
/// silent hole: a command asks which operation a record performs, never which kind it is.
pub const fn operation_of(kind: MachiningKind) -> Operation {
    match kind {
        MachiningKind::Cutout => Operation::Cutout,
    }
}

/// Why a class refused an edit the command or the gizmo wanted to make.
///
/// The reason is a value, not a sentence, so a later message table can translate it; the engine
/// prints it in English until that mapping arrives (`plan/debt.md`, `[ui]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassRefusal {
    /// The class does not let this dimension change.
    Dimension(Dimension),
    /// The class admits no machining of this kind.
    Operation(Operation),
    /// The object's class may not sit in the host's class.
    Host {
        /// The class of the node that would receive it.
        host: ClassCode,
    },
}

impl fmt::Display for ClassRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dimension(dimension) => {
                let axis = match dimension {
                    Dimension::Length => "length",
                    Dimension::Width => "width",
                    Dimension::Thickness => "thickness",
                };
                write!(formatter, "the class fixes the {axis}")
            }
            Self::Operation(operation) => {
                let admitted = match operation {
                    Operation::Contour => "outline edits",
                    Operation::EdgeBand => "edge banding",
                    Operation::Holes => "holes",
                    Operation::Grooves => "grooves",
                    Operation::CupBore => "hinge cup bores",
                    Operation::MountPattern => "mounting patterns",
                    Operation::Cutout => "cutouts",
                };
                write!(formatter, "the class admits no {admitted}")
            }
            Self::Host { host } => write!(formatter, "the class may not sit in {host}"),
        }
    }
}

impl std::error::Error for ClassRefusal {}

/// The rules a node's class imposes on the editor.
///
/// Paired with the node's [`ClassCode`], because `host` asks about the object's class and not only
/// its traits. Absent traits mean "no rule": an unclassified node, a family heading that a hostile
/// file could store, and a code the registry lost all leave the editor unrestricted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeRules {
    class: ClassCode,
    traits: Option<TraitSet>,
}

impl NodeRules {
    /// The rules of one class code, for a change that proposes a class before it is stored.
    pub fn of_code(class: ClassCode) -> Self {
        Self { class, traits: class.traits() }
    }

    /// The rules of a node as it stands in the document.
    pub fn of(project: &Project, node: Id) -> Self {
        Self::of_code(project.class(node).unwrap_or(ClassCode::UNCLASSIFIED))
    }

    /// The class the rules were read from.
    pub const fn class(self) -> ClassCode {
        self.class
    }

    /// The complete trait set, or `None` when the node has no rule to apply.
    pub const fn traits(self) -> Option<TraitSet> {
        self.traits
    }

    /// Which dimensions a gizmo may change and which resize commands run.
    pub const fn extent(self) -> Extent {
        match self.traits {
            Some(traits) => traits.extent(),
            None => Extent::Free,
        }
    }

    /// Whether the class lets `dimension` change.
    pub const fn may_resize(self, dimension: Dimension) -> bool {
        match self.extent() {
            Extent::Free => true,
            Extent::LengthOnly => matches!(dimension, Dimension::Length),
            Extent::None => false,
        }
    }

    /// Whether the class admits one kind of machining.
    pub const fn admits(self, operation: Operation) -> bool {
        match self.traits {
            Some(traits) => traits.operations().contains(operation),
            None => true,
        }
    }

    /// Whether a cutting plan owns the object, or `None` when the class says nothing about supply.
    ///
    /// The cutting list and the order export read this rather than deriving it from a structural
    /// kind; `None` is the honest answer for a node whose class is missing or unknown.
    pub const fn nesting(self) -> Option<Nesting> {
        match self.traits {
            Some(traits) => Some(traits.nesting()),
            None => None,
        }
    }

    /// The unit of the object's report line, or `None` when the class says nothing about supply.
    pub const fn measure(self) -> Option<Measure> {
        match self.traits {
            Some(traits) => Some(traits.measure()),
            None => None,
        }
    }

    /// Whether the class lets the object sit in or on `host`.
    ///
    /// An unclassified object claims nothing and is not gated; an unclassified host is not a host
    /// either, so a group never refuses its children. A classed object asks its row, which is the
    /// only place the prefix list lives.
    pub fn may_sit_in(self, host: ClassCode) -> bool {
        if self.traits.is_none() || host.is_unclassified() || !host.is_registered() {
            return true;
        }
        self.class.is_hosted_in(host)
    }

    /// `Ok` when the class lets `dimension` change, or the refusal.
    pub const fn allow_resize(self, dimension: Dimension) -> Result<(), ClassRefusal> {
        if self.may_resize(dimension) { Ok(()) } else { Err(ClassRefusal::Dimension(dimension)) }
    }

    /// `Ok` when the class admits the operation, or the refusal.
    pub const fn allow_operation(self, operation: Operation) -> Result<(), ClassRefusal> {
        if self.admits(operation) { Ok(()) } else { Err(ClassRefusal::Operation(operation)) }
    }

    /// `Ok` when the object may sit in the host's class, or the refusal.
    pub fn allow_host(self, host: ClassCode) -> Result<(), ClassRefusal> {
        if self.may_sit_in(host) { Ok(()) } else { Err(ClassRefusal::Host { host }) }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_model::{PropertyValue, SYSTEM_CLASS};
    use furnicraft_testkit::sample_project;

    use super::*;

    fn class(text: &str) -> ClassCode {
        text.parse().expect("a class code")
    }

    /// The sample shelf, classed as `code`, in a project whose root has no class.
    fn shelf_with_class(code: &str) -> (Project, Id) {
        let mut project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        project.set_property(shelf, SYSTEM_CLASS, PropertyValue::class(class(code))).expect("assign the class");
        (project, shelf)
    }

    #[test]
    fn a_worktop_resizes_by_length_only_and_a_shelf_stays_free() {
        let (project, shelf) = shelf_with_class("70.01");
        let rules = NodeRules::of(&project, shelf);
        assert_eq!(rules.extent(), Extent::LengthOnly);
        assert!(rules.may_resize(Dimension::Length));
        assert!(!rules.may_resize(Dimension::Width));
        assert!(!rules.may_resize(Dimension::Thickness));
        assert_eq!(rules.allow_resize(Dimension::Width), Err(ClassRefusal::Dimension(Dimension::Width)));

        let (project, shelf) = shelf_with_class("30.11");
        let rules = NodeRules::of(&project, shelf);
        assert!(Dimension::ALL.iter().all(|dimension| rules.may_resize(*dimension)));
    }

    #[test]
    fn a_hinge_offers_no_resize_no_banding_and_no_nesting() {
        let (project, node) = shelf_with_class("50.02.01");
        let rules = NodeRules::of(&project, node);
        assert_eq!(rules.extent(), Extent::None);
        assert!(!rules.may_resize(Dimension::Length));
        assert!(!rules.admits(Operation::EdgeBand));
        assert_eq!(rules.allow_operation(Operation::EdgeBand), Err(ClassRefusal::Operation(Operation::EdgeBand)));
        assert_eq!(rules.nesting(), Some(Nesting::Never));
        assert_eq!(rules.measure(), Some(Measure::Count));
    }

    #[test]
    fn a_fitting_never_enters_nesting_while_a_made_door_does() {
        let (project, node) = shelf_with_class("50.11");
        let fitting = NodeRules::of(&project, node);
        assert_eq!(fitting.nesting(), Some(Nesting::Never));
        assert_eq!(fitting.measure(), Some(Measure::Count));

        let (project, node) = shelf_with_class("30.20");
        let door = NodeRules::of(&project, node);
        assert_eq!(door.nesting(), Some(Nesting::Member));
        assert_eq!(door.measure(), Some(Measure::Area));
        assert!(door.admits(Operation::CupBore));
    }

    #[test]
    fn an_unclassified_node_has_no_rule_and_therefore_no_refusal() {
        let project = sample_project();
        let shelf = Id::new(2).expect("shelf id");
        let rules = NodeRules::of(&project, shelf);
        assert!(rules.traits().is_none());
        assert_eq!(rules.extent(), Extent::Free);
        assert!(rules.admits(Operation::EdgeBand));
        assert_eq!(rules.nesting(), None);
        assert_eq!(rules.measure(), None);
        assert!(rules.may_sit_in(class("70.01")), "an unclassified object claims nothing");
    }

    #[test]
    fn changing_the_row_flips_the_behaviour_without_touching_a_command() {
        let (project, shelf) = shelf_with_class("70.01");
        assert!(!NodeRules::of(&project, shelf).may_resize(Dimension::Width));

        let (project, shelf) = shelf_with_class("70.02");
        assert_eq!(NodeRules::of(&project, shelf).extent(), Extent::LengthOnly);
        assert!(!NodeRules::of(&project, shelf).admits(Operation::Cutout), "a plinth admits no cutout");

        let (project, shelf) = shelf_with_class("70.01");
        assert!(NodeRules::of(&project, shelf).admits(Operation::Cutout));
    }

    #[test]
    fn mounting_checks_the_host_class() {
        // A sink names the worktop as its host.
        let sink = NodeRules::of_code(class("60.01"));
        assert!(sink.may_sit_in(class("70.01")));
        assert!(!sink.may_sit_in(class("30.01.01")), "a sink does not sit in a base cabinet");

        // A hinge names a vertical panel and every facade, never a worktop.
        let hinge = NodeRules::of_code(class("50.02"));
        assert!(hinge.may_sit_in(class("30.20")));
        assert!(hinge.may_sit_in(class("30.10")));
        assert_eq!(hinge.allow_host(class("70.01")), Err(ClassRefusal::Host { host: class("70.01") }));

        // A group has no class, and an unclassified object claims nothing.
        assert!(hinge.may_sit_in(ClassCode::UNCLASSIFIED));
        assert!(NodeRules::of_code(ClassCode::UNCLASSIFIED).may_sit_in(class("70.01")));
    }

    #[test]
    fn a_machining_kind_names_its_operation() {
        assert_eq!(operation_of(MachiningKind::Cutout), Operation::Cutout);
    }
}
