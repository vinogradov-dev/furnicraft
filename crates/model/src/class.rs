//! The semantic class: a six-digit `GG.TT.VV` identity, the trait set that carries behaviour, and
//! the registry that is the only assigner (ADR 0094).
//!
//! The code identifies and the traits decide: no algorithm compares a code with a literal outside a
//! test, it asks the row what the class is made of, how it is measured, which machining it admits,
//! where it may sit and which dimensions a gizmo may change. The rows are a constant table in this
//! crate, checked row for row against `docs/domain/classes.md` by a test; the inheritance rules are
//! checked by the same test rather than walked at run time, because every row states its complete
//! trait set, so a lookup is a binary search and a trait is a bit test.
//!
//! A **family** (`GG.00.00`) is a heading, not a class: it carries a slug and a label and no traits,
//! and it is never assigned to a node. A **type** (`GG.TT.00`) and a **variant** (`GG.TT.VV`) are
//! the classes a document may carry.

use std::fmt;
use std::str::FromStr;

use furnicraft_text::Message;
use serde::de::{self, Deserializer};
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

/// The largest class code: two digits for each of group, type and variant.
pub const CLASS_MAX_CODE: u32 = 999_999;

/// Why a class code or its text form is not usable.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ClassCodeError {
    /// The numeric code is outside the supported range.
    #[error("class {value} is outside the supported range 0 to {max}")]
    OutOfRange {
        /// The rejected code.
        value: u32,
        /// The largest supported code.
        max: u32,
    },
    /// The text form is not a valid code.
    #[error("class `{text}` is not a valid code")]
    Invalid {
        /// The rejected text.
        text: String,
    },
}

/// A semantic class code in the strict `GG.TT.VV` tree.
///
/// The value is the stored integer code, from `0` (unclassified) to [`ClassCode::MAX`]. The dotted
/// form is a presentation only: it is produced by [`fmt::Display`] and accepted by `FromStr`, never
/// stored. A zero suffix names the parent: `30.01.00` is the type, `30.00.00` the family.
///
/// A code is valid whether or not the registry knows it: the shape is a property of the digits, the
/// meaning arrives with the row (ADR 0094). An unknown code is kept on load and refused on
/// assignment ([`ClassCode::is_assignable`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassCode(u32);

impl ClassCode {
    /// The unclassified class, `000000`, the default of every node.
    pub const UNCLASSIFIED: Self = Self(0);

    /// The editor's placeholder product class, «изделие общего назначения» / general product.
    ///
    /// `product.create` assigns it when the caller names no class, so a product always carries
    /// one (ADR 0097); a test proves it is the `80.01.00` registry row with `level = Product`.
    pub const GENERAL_PRODUCT: Self = Self(800_100);

    /// The largest assignable code.
    pub const MAX: Self = Self(CLASS_MAX_CODE);

    /// Builds a code from its numeric value, or `None` when the value is not a valid tree position.
    ///
    /// A valid code has a non-zero group and a variant only under a type. Zero is
    /// [`ClassCode::UNCLASSIFIED`].
    pub const fn from_code(code: u32) -> Option<Self> {
        if code == 0 {
            return Some(Self(0));
        }
        let group = code / 10_000;
        let type_ = (code / 100) % 100;
        let variant = code % 100;
        if code <= CLASS_MAX_CODE && group != 0 && (variant == 0 || type_ != 0) { Some(Self(code)) } else { None }
    }

    /// The numeric code without separators, the only stored form.
    pub const fn value(self) -> u32 {
        self.0
    }

    /// Whether the code is unclassified.
    pub const fn is_unclassified(self) -> bool {
        self.0 == 0
    }

    /// The parent code: the type without its variant, or the family without its type.
    pub const fn parent(self) -> Self {
        let variant = self.0 % 100;
        let type_ = (self.0 / 100) % 100;
        if variant != 0 {
            Self(self.0 - variant)
        } else if type_ != 0 {
            Self(self.0 - type_ * 100)
        } else {
            Self(0)
        }
    }

    /// Whether `ancestor` is the code itself or one of its ancestors.
    ///
    /// `000000` is a sentinel, not a family: it matches only unclassified codes, so a report can ask
    /// for "everything without a class" instead of matching the whole tree.
    pub fn is_ancestor_or_self(self, ancestor: Self) -> bool {
        if ancestor.is_unclassified() {
            return self.is_unclassified();
        }
        let mut current = self;
        loop {
            if current == ancestor {
                return true;
            }
            if current.is_unclassified() {
                return false;
            }
            current = current.parent();
        }
    }

    /// The registry row this code names, when the registry knows it.
    pub fn row(self) -> Option<&'static ClassRow> {
        let index = REGISTRY.binary_search_by_key(&self.0, |row| row.code).ok()?;
        REGISTRY.get(index)
    }

    /// Whether the registry knows the code.
    pub fn is_registered(self) -> bool {
        self.row().is_some()
    }

    /// Whether a node may carry the code: unclassified, or a registered class that has not been
    /// withdrawn.
    ///
    /// A family heading is never assigned, neither is a code the registry does not know, and a
    /// retired row still renders but is no longer assignable (ADR 0094). Assignment checks this;
    /// loading keeps the number instead, even when it is unknown or retired.
    pub fn is_assignable(self) -> bool {
        self.is_unclassified() || self.row().is_some_and(|row| row.is_assignable())
    }

    /// The trait set of the class, or `None` for an unclassified code, an unknown code or a family.
    pub fn traits(self) -> Option<TraitSet> {
        self.row()?.traits
    }

    /// Whether the class plays the product role: `level = Product` (ADR 0097).
    ///
    /// The role is a trait and not a code range, so an export reader asks this question without
    /// knowing our digits.
    pub fn is_product_class(self) -> bool {
        self.traits().is_some_and(|traits| traits.level() == Level::Product)
    }

    /// The label pair of the class, or `None` when the registry does not know the code.
    pub fn label(self) -> Option<Message> {
        self.row().map(|row| row.label)
    }

    /// The mnemonic slug of the class, or `None` when the registry does not know the code.
    pub fn slug(self) -> Option<&'static str> {
        self.row().map(|row| row.slug)
    }

    /// Whether an object of this class may sit in or on `host` (ADR 0094).
    ///
    /// An unknown code has no host rule and claims nothing, so an insertion into it is refused.
    pub fn is_hosted_in(self, host: Self) -> bool {
        self.row().is_some_and(|row| row.hosts.iter().any(|prefix| prefix.covers(host)))
    }
}

impl fmt::Display for ClassCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let group = self.0 / 10_000;
        let type_ = (self.0 / 100) % 100;
        let variant = self.0 % 100;
        if variant != 0 {
            write!(formatter, "{group}.{:02}.{:02}", type_, variant)
        } else if type_ != 0 {
            write!(formatter, "{group}.{:02}", type_)
        } else {
            write!(formatter, "{group}")
        }
    }
}

impl FromStr for ClassCode {
    type Err = ClassCodeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let text = text.trim();
        let invalid = || ClassCodeError::Invalid { text: text.to_owned() };
        if text.is_empty() {
            return Err(invalid());
        }
        if !text.contains('.') {
            let raw = text.parse::<u32>().map_err(|_| invalid())?;
            // One or two digits name a family, the display form of a family code; a longer run is a
            // raw integer code. Both round-trip through [`fmt::Display`].
            let code = if text.len() <= 2 { raw * 10_000 } else { raw };
            return ClassCode::from_code(code).ok_or(ClassCodeError::OutOfRange { value: code, max: CLASS_MAX_CODE });
        }
        let mut parts = text.split('.');
        let group_text = parts.next().ok_or_else(invalid)?;
        let type_text = parts.next();
        let variant_text = parts.next();
        if parts.next().is_some() {
            return Err(invalid());
        }
        let parse_part = |part: &str, digits: usize| -> Result<u32, ClassCodeError> {
            if part.is_empty() || part.len() > digits || !part.chars().all(|c| c.is_ascii_digit()) {
                return Err(invalid());
            }
            let mut value = 0u32;
            for c in part.chars() {
                value = value * 10 + u32::from(c as u8 - b'0');
            }
            Ok(value)
        };
        let group = parse_part(group_text, 2)?;
        if group == 0 {
            return Err(invalid());
        }
        let type_ = match type_text {
            Some(part) => parse_part(part, 2)?,
            None => 0,
        };
        let variant = match variant_text {
            Some(part) => parse_part(part, 2)?,
            None => 0,
        };
        let code = group * 10_000 + type_ * 100 + variant;
        ClassCode::from_code(code).ok_or(ClassCodeError::OutOfRange { value: code, max: CLASS_MAX_CODE })
    }
}

impl Serialize for ClassCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u32(self.0)
    }
}

impl<'de> Deserialize<'de> for ClassCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let code = u32::deserialize(deserializer)?;
        ClassCode::from_code(code).ok_or_else(|| {
            de::Error::custom(format_args!("class {code} is outside the supported range 0 to {CLASS_MAX_CODE}"))
        })
    }
}

/// Where an object comes from: a nesting plan, a length cut, a purchase order, or the workshop's own
/// assembly (ADR 0104).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Supply {
    /// Cut from a sheet and nested.
    MadeFromSheet,
    /// Cut to length from linear stock.
    MadeFromLinear,
    /// Bought as a piece.
    BuyPiece,
    /// Assembled from a recipe by the shop, and counted once.
    MadeInShop,
}

impl Supply {
    /// The measure the supply implies when a class does not override it.
    pub const fn measure(self) -> Measure {
        match self {
            Self::MadeFromSheet => Measure::Area,
            Self::MadeFromLinear => Measure::Length,
            Self::BuyPiece | Self::MadeInShop => Measure::Count,
        }
    }

    /// The nesting the supply implies when a class does not override it.
    pub const fn nesting(self) -> Nesting {
        match self {
            Self::MadeFromSheet => Nesting::Member,
            Self::MadeFromLinear | Self::BuyPiece | Self::MadeInShop => Nesting::Never,
        }
    }

    const fn bits(self) -> u32 {
        match self {
            Self::MadeFromSheet => 0,
            Self::MadeFromLinear => 1,
            Self::BuyPiece => 2,
            Self::MadeInShop => 3,
        }
    }

    const fn from_bits(bits: u32) -> Self {
        match bits {
            0 => Self::MadeFromSheet,
            1 => Self::MadeFromLinear,
            2 => Self::BuyPiece,
            _ => Self::MadeInShop,
        }
    }
}

/// The unit of a report line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Measure {
    /// Square metres of sheet.
    Area,
    /// Linear metres of stock.
    Length,
    /// Pieces.
    Count,
}

impl Measure {
    const fn bits(self) -> u32 {
        match self {
            Self::Area => 0,
            Self::Length => 1,
            Self::Count => 2,
        }
    }

    const fn from_bits(bits: u32) -> Self {
        match bits {
            0 => Self::Area,
            1 => Self::Length,
            _ => Self::Count,
        }
    }
}

/// Whether a cutting plan owns the object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nesting {
    /// Nested on a sheet.
    Member,
    /// Never nested.
    Never,
}

impl Nesting {
    const fn bits(self) -> u32 {
        match self {
            Self::Member => 0,
            Self::Never => 1,
        }
    }

    const fn from_bits(bits: u32) -> Self {
        match bits {
            0 => Self::Member,
            _ => Self::Never,
        }
    }
}

/// Which dimensions a gizmo may change and which resize commands are offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Extent {
    /// Every size.
    Free,
    /// The length only.
    LengthOnly,
    /// Not resized at all.
    None,
}

impl Extent {
    const fn bits(self) -> u32 {
        match self {
            Self::Free => 0,
            Self::LengthOnly => 1,
            Self::None => 2,
        }
    }

    const fn from_bits(bits: u32) -> Self {
        match bits {
            0 => Self::Free,
            1 => Self::LengthOnly,
            _ => Self::None,
        }
    }
}

/// Report grouping and the tree presentation: what kind of thing the class is.
///
/// `Level` never marks a component; a container carrying a class is a component (ADR 0034).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// A whole piece.
    Product,
    /// A bought or assembled part of a product.
    Component,
    /// A part cut or bought for a product.
    Part,
    /// A fitting mounted on one.
    Fitting,
}

impl Level {
    const fn bits(self) -> u32 {
        match self {
            Self::Product => 0,
            Self::Component => 1,
            Self::Part => 2,
            Self::Fitting => 3,
        }
    }

    const fn from_bits(bits: u32) -> Self {
        match bits {
            0 => Self::Product,
            1 => Self::Component,
            2 => Self::Part,
            _ => Self::Fitting,
        }
    }
}

/// Whether the object closes an opening, and therefore has gaps and a facade report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opening {
    /// The object closes an opening.
    Closes,
}

impl Opening {
    const fn bits(self) -> u32 {
        match self {
            Self::Closes => 1,
        }
    }

    const fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => None,
            _ => Some(Self::Closes),
        }
    }
}

/// Where a product stands: on the floor or hung (ADR 0097).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mount {
    /// Stands on the floor.
    Floor,
    /// Hung on a wall.
    Wall,
}

impl Mount {
    const fn bits(self) -> u32 {
        match self {
            Self::Floor => 1,
            Self::Wall => 2,
        }
    }

    const fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => None,
            1 => Some(Self::Floor),
            _ => Some(Self::Wall),
        }
    }
}

/// Which axis pair a part takes from a free box when it fills a void (ADR 0097).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fill {
    /// Height and depth, the post-like fill.
    HeightAndDepth,
    /// Width and depth, the shelf-like fill.
    WidthAndDepth,
}

impl Fill {
    const fn bits(self) -> u32 {
        match self {
            Self::HeightAndDepth => 1,
            Self::WidthAndDepth => 2,
        }
    }

    const fn from_bits(bits: u32) -> Option<Self> {
        match bits {
            0 => None,
            1 => Some(Self::HeightAndDepth),
            _ => Some(Self::WidthAndDepth),
        }
    }
}

/// One kind of machining a class admits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Operation {
    /// A shaped outline.
    Contour,
    /// Edge banding on an outline edge.
    EdgeBand,
    /// Drilled holes.
    Holes,
    /// Milled grooves.
    Grooves,
    /// A hinge cup bore.
    CupBore,
    /// A hinge mounting pattern.
    MountPattern,
    /// A sink or hob cutout.
    Cutout,
}

impl Operation {
    /// Every operation, in the order the documentation lists them.
    pub const ALL: [Self; 7] =
        [Self::Contour, Self::EdgeBand, Self::Holes, Self::Grooves, Self::CupBore, Self::MountPattern, Self::Cutout];

    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// The set of machining operations a class admits, as a bit mask.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct OperationSet(u8);

impl OperationSet {
    /// No machining at all.
    pub const NONE: Self = Self(0);

    /// Every operation.
    pub const ALL: Self = Self((1 << Operation::ALL.len()) - 1);

    /// Builds a set from the operations it contains.
    pub const fn of(operations: &[Operation]) -> Self {
        let mut bits = 0u8;
        let mut index = 0;
        while index < operations.len() {
            bits |= operations[index].bit();
            index += 1;
        }
        Self(bits)
    }

    /// Whether the set admits `operation`.
    pub const fn contains(self, operation: Operation) -> bool {
        self.0 & operation.bit() != 0
    }

    /// Whether the set admits no machining.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// How many operations the set admits.
    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    /// The admitted operations, in [`Operation::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = Operation> {
        Operation::ALL.into_iter().filter(move |operation| self.contains(*operation))
    }
}

/// The leading digits of a code, the way `host` names the classes an object may sit in.
///
/// `30` covers every storage class, `30.2` every facade and `30.20` exactly one class, so the rule
/// keeps its prefix shape instead of listing every member (ADR 0094). A prefix never names a
/// document value; it is the registry's way of writing a set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClassPrefix {
    value: u32,
    digits: u8,
}

impl ClassPrefix {
    /// Builds a prefix from the digits it holds, `digits` wide, without separators.
    ///
    /// `30` is `of(30, 2)`, `30.2` is `of(302, 3)` and `30.01.01` is `of(300101, 6)`. The registry
    /// test rejects a prefix outside `2..=6` digits or wider than its value.
    pub const fn of(value: u32, digits: u8) -> Self {
        Self { value, digits }
    }

    /// The digits without separators.
    pub const fn value(self) -> u32 {
        self.value
    }

    /// How many digits the prefix holds.
    pub const fn digits(self) -> u8 {
        self.digits
    }

    /// Whether the prefix covers `code`.
    pub const fn covers(self, code: ClassCode) -> bool {
        let divisor = match self.digits {
            2 => 10_000,
            3 => 1_000,
            4 => 100,
            5 => 10,
            _ => 1,
        };
        self.value == code.value() / divisor
    }
}

/// The behaviour of a class, packed into one bit mask.
///
/// A row states its complete set, so a lookup is a shift and a test rather than a walk up the
/// inheritance chain; the registry test checks that every variant agrees with its type instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TraitSet(u32);

/// Where the supply occupies the mask.
const SUPPLY_SHIFT: u32 = 0;
/// Where the measure occupies the mask.
const MEASURE_SHIFT: u32 = 2;
/// Where the nesting occupies the mask.
const NESTING_SHIFT: u32 = 4;
/// Where the extent occupies the mask.
const EXTENT_SHIFT: u32 = 5;
/// Where the level occupies the mask.
const LEVEL_SHIFT: u32 = 7;
/// Where the opening occupies the mask.
const OPENING_SHIFT: u32 = 9;
/// Where the mount occupies the mask.
const MOUNT_SHIFT: u32 = 10;
/// Where the fill occupies the mask.
const FILL_SHIFT: u32 = 12;
/// Where the operation set occupies the mask.
const OPERATIONS_SHIFT: u32 = 14;

impl TraitSet {
    /// Builds a trait set from its values.
    ///
    /// `opening`, `mount` and `fill` are absent when the class does not have the trait. One argument
    /// per trait keeps a registry row as readable as the document's column order; a struct literal
    /// would repeat nine field names in sixty-five rows.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        supply: Supply,
        measure: Measure,
        nesting: Nesting,
        extent: Extent,
        level: Level,
        opening: Option<Opening>,
        mount: Option<Mount>,
        fill: Option<Fill>,
        operations: OperationSet,
    ) -> Self {
        let opening = match opening {
            Some(opening) => opening.bits(),
            None => 0,
        };
        let mount = match mount {
            Some(mount) => mount.bits(),
            None => 0,
        };
        let fill = match fill {
            Some(fill) => fill.bits(),
            None => 0,
        };
        Self(
            (supply.bits() << SUPPLY_SHIFT)
                | (measure.bits() << MEASURE_SHIFT)
                | (nesting.bits() << NESTING_SHIFT)
                | (extent.bits() << EXTENT_SHIFT)
                | (level.bits() << LEVEL_SHIFT)
                | (opening << OPENING_SHIFT)
                | (mount << MOUNT_SHIFT)
                | (fill << FILL_SHIFT)
                | ((operations.0 as u32) << OPERATIONS_SHIFT),
        )
    }

    /// Where the object comes from.
    pub const fn supply(self) -> Supply {
        Supply::from_bits(self.field(SUPPLY_SHIFT, 0b11))
    }

    /// The unit of its report line.
    pub const fn measure(self) -> Measure {
        Measure::from_bits(self.field(MEASURE_SHIFT, 0b11))
    }

    /// Whether a cutting plan owns it.
    pub const fn nesting(self) -> Nesting {
        Nesting::from_bits(self.field(NESTING_SHIFT, 0b1))
    }

    /// Which dimensions a gizmo may change.
    pub const fn extent(self) -> Extent {
        Extent::from_bits(self.field(EXTENT_SHIFT, 0b11))
    }

    /// What kind of thing it is, for report grouping.
    pub const fn level(self) -> Level {
        Level::from_bits(self.field(LEVEL_SHIFT, 0b11))
    }

    /// Whether it closes an opening.
    pub const fn opening(self) -> Option<Opening> {
        Opening::from_bits(self.field(OPENING_SHIFT, 0b1))
    }

    /// Where the product stands, when it is a product.
    pub const fn mount(self) -> Option<Mount> {
        Mount::from_bits(self.field(MOUNT_SHIFT, 0b11))
    }

    /// Which axis pair it takes from a void, when it fills one.
    pub const fn fill(self) -> Option<Fill> {
        Fill::from_bits(self.field(FILL_SHIFT, 0b11))
    }

    /// The machining it admits.
    pub const fn operations(self) -> OperationSet {
        OperationSet(((self.0 >> OPERATIONS_SHIFT) & 0b111_1111) as u8)
    }

    /// One packed field, masked to its width.
    const fn field(self, shift: u32, mask: u32) -> u32 {
        (self.0 >> shift) & mask
    }
}

/// One row of the class registry.
///
/// A family row has no traits; a class row has them. The code is the raw six-digit value and the
/// registry test proves every row is a valid [`ClassCode`], because a constant table cannot return
/// an error the way a constructor does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassRow {
    code: u32,
    slug: &'static str,
    label: Message,
    traits: Option<TraitSet>,
    hosts: &'static [ClassPrefix],
    aliases: &'static [&'static str],
    retired: bool,
}

impl ClassRow {
    /// A family heading: a code, a slug and a label, and no traits.
    pub const fn family(code: u32, slug: &'static str, label: Message) -> Self {
        Self { code, slug, label, traits: None, hosts: &[], aliases: &[], retired: false }
    }

    /// One assignable class, with its complete trait set and the classes it may sit in.
    pub const fn class(
        code: u32,
        slug: &'static str,
        label: Message,
        traits: TraitSet,
        hosts: &'static [ClassPrefix],
    ) -> Self {
        Self { code, slug, label, traits: Some(traits), hosts, aliases: &[], retired: false }
    }

    /// The same row with search synonyms, which the picker matches but a document never stores.
    pub const fn with_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// The same row withdrawn: it still resolves and renders, but it cannot be assigned again.
    pub const fn retired(mut self) -> Self {
        self.retired = true;
        self
    }

    /// The six-digit code.
    pub const fn code(self) -> u32 {
        self.code
    }

    /// The mnemonic slug: for people, export tables and search, never compared and never stored.
    pub const fn slug(self) -> &'static str {
        self.slug
    }

    /// The label pair the interface draws; the [`Message`] variant is the translation key.
    pub const fn label(self) -> Message {
        self.label
    }

    /// The complete trait set, or `None` for a family heading.
    pub const fn traits(self) -> Option<TraitSet> {
        self.traits
    }

    /// The classes an object of this class may sit in or on, written as code prefixes.
    pub const fn hosts(self) -> &'static [ClassPrefix] {
        self.hosts
    }

    /// Search synonyms a person may type instead of the label: `shelf` for a horizontal panel.
    ///
    /// They are searched, never displayed and never stored in a document: the slug is the stable
    /// mnemonic, the alias is only a second word for the same class.
    pub const fn aliases(self) -> &'static [&'static str] {
        self.aliases
    }

    /// Whether the row is a family heading rather than a class.
    pub const fn is_family(self) -> bool {
        self.traits.is_none()
    }

    /// Whether the row is a class rather than a family heading.
    pub const fn is_class(self) -> bool {
        self.traits.is_some()
    }

    /// Whether the row is withdrawn: still resolvable, no longer assignable.
    pub const fn is_retired(self) -> bool {
        self.retired
    }

    /// Whether a node may carry this row: a class that has not been withdrawn.
    pub const fn is_assignable(self) -> bool {
        self.is_class() && !self.retired
    }

    /// The parent code, one level up.
    pub fn parent(self) -> Option<ClassCode> {
        ClassCode::from_code(self.code).map(ClassCode::parent)
    }
}

/// Builds one row's trait set in the column order of the document table.
#[allow(clippy::too_many_arguments)]
const fn traits(
    level: Level,
    supply: Supply,
    measure: Measure,
    nesting: Nesting,
    extent: Extent,
    operations: &[Operation],
    opening: Option<Opening>,
    mount: Option<Mount>,
    fill: Option<Fill>,
) -> TraitSet {
    TraitSet::new(supply, measure, nesting, extent, level, opening, mount, fill, OperationSet::of(operations))
}

/// Every registry row, families and classes together, ascending by code.
///
/// The table is the registry's content for version 1: a test compares it with
/// `docs/domain/classes.md` row for row, so the prose cannot drift from what the program knows. A
/// code is append-only and immortal once shipped; a change to an existing row's traits is a
/// behaviour change and needs its own record (ADR 0094).
const REGISTRY: &[ClassRow] = &[
    // 10 — seating
    ClassRow::family(100_000, "seating", Message::ClassFamilySeating),
    ClassRow::class(
        100_100,
        "chair",
        Message::ClassChair,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    )
    .with_aliases(&["dining chair", "office chair", "kitchen chair"]),
    ClassRow::class(
        100_200,
        "stool",
        Message::ClassStool,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        100_300,
        "bench",
        Message::ClassBench,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        100_400,
        "sofa",
        Message::ClassSofa,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    // 20 — lying
    ClassRow::family(200_000, "lying", Message::ClassFamilyLying),
    ClassRow::class(
        200_100,
        "bed",
        Message::ClassBed,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        200_200,
        "mattress",
        Message::ClassMattress,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(2001, 4)],
    ),
    // 30 — storage
    ClassRow::family(300_000, "storage", Message::ClassFamilyStorage),
    ClassRow::class(
        300_100,
        "cabinet",
        Message::ClassCabinet,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        300_101,
        "base-cabinet",
        Message::ClassBaseCabinet,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    )
    .with_aliases(&["nightstand", "chest", "тумба", "комод"]),
    ClassRow::class(
        300_102,
        "wall-cabinet",
        Message::ClassWallCabinet,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Wall),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        300_103,
        "tall-cabinet",
        Message::ClassTallCabinet,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    )
    .with_aliases(&["wardrobe", "гардероб"]),
    ClassRow::class(
        301_000,
        "vertical-panel",
        Message::ClassVerticalPanel,
        traits(
            Level::Part,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand],
            None,
            None,
            Some(Fill::HeightAndDepth),
        ),
        &[ClassPrefix::of(3001, 4)],
    )
    .with_aliases(&["side", "side panel", "боковина", "перегородка", "стойка"]),
    ClassRow::class(
        301_100,
        "horizontal-panel",
        Message::ClassHorizontalPanel,
        traits(
            Level::Part,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand],
            None,
            None,
            Some(Fill::WidthAndDepth),
        ),
        &[ClassPrefix::of(3001, 4)],
    )
    .with_aliases(&["shelf", "bottom", "top", "полка", "полочка", "днище", "крыша"]),
    ClassRow::class(
        301_200,
        "back-panel",
        Message::ClassBackPanel,
        traits(
            Level::Part,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        301_300,
        "drawer-box-part",
        Message::ClassDrawerBoxPart,
        traits(
            Level::Part,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(3030, 4)],
    )
    .with_aliases(&[
        "drawer side",
        "drawer bottom",
        "drawer back",
        "боковина ящика",
        "дно ящика",
        "задняя стенка ящика",
    ]),
    ClassRow::class(
        301_400,
        "glass-shelf",
        Message::ClassGlassShelf,
        traits(
            Level::Part,
            Supply::BuyPiece,
            Measure::Area,
            Nesting::Never,
            Extent::Free,
            &[Operation::Contour],
            None,
            None,
            Some(Fill::WidthAndDepth),
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        301_500,
        "tamburat-panel",
        Message::ClassTamburatPanel,
        traits(
            Level::Part,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        301_600,
        "bent-panel",
        Message::ClassBentPanel,
        traits(
            Level::Part,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_000,
        "hinged-door-made",
        Message::ClassHingedDoorMade,
        traits(
            Level::Component,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand, Operation::CupBore, Operation::MountPattern],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_100,
        "hinged-door-bought",
        Message::ClassHingedDoorBought,
        traits(
            Level::Component,
            Supply::BuyPiece,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_200,
        "sliding-door-made",
        Message::ClassSlidingDoorMade,
        traits(
            Level::Component,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand, Operation::Grooves],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_300,
        "sliding-door-bought",
        Message::ClassSlidingDoorBought,
        traits(
            Level::Component,
            Supply::BuyPiece,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_400,
        "lift-up-made",
        Message::ClassLiftUpMade,
        traits(
            Level::Component,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand, Operation::MountPattern],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_500,
        "lift-up-bought",
        Message::ClassLiftUpBought,
        traits(
            Level::Component,
            Supply::BuyPiece,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_600,
        "drawer-front-made",
        Message::ClassDrawerFrontMade,
        traits(
            Level::Component,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::EdgeBand, Operation::MountPattern],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        302_700,
        "drawer-front-bought",
        Message::ClassDrawerFrontBought,
        traits(
            Level::Component,
            Supply::BuyPiece,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            Some(Opening::Closes),
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        303_000,
        "drawer-made",
        Message::ClassDrawerMade,
        traits(
            Level::Component,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        303_100,
        "drawer-bought",
        Message::ClassDrawerBought,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3001, 4)],
    ),
    // 40 — surfaces
    ClassRow::family(400_000, "surfaces", Message::ClassFamilySurfaces),
    ClassRow::class(
        400_100,
        "table",
        Message::ClassTable,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        400_101,
        "floor-table",
        Message::ClassFloorTable,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
    ClassRow::class(
        400_102,
        "wall-table",
        Message::ClassWallTable,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Wall),
            None,
        ),
        &[],
    ),
    // 50 — fittings
    ClassRow::family(500_000, "fittings", Message::ClassFamilyFittings),
    ClassRow::class(
        500_100,
        "card-hinge",
        Message::ClassCardHinge,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_101,
        "card-hinge-overlay",
        Message::ClassCardHingeOverlay,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_102,
        "card-hinge-inset",
        Message::ClassCardHingeInset,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_103,
        "card-hinge-half",
        Message::ClassCardHingeHalf,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_200,
        "cup-hinge",
        Message::ClassCupHinge,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_201,
        "cup-hinge-overlay",
        Message::ClassCupHingeOverlay,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_202,
        "cup-hinge-inset",
        Message::ClassCupHingeInset,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_203,
        "cup-hinge-half",
        Message::ClassCupHingeHalf,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_300,
        "runner",
        Message::ClassRunner,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(303, 3)],
    ),
    ClassRow::class(
        500_301,
        "runner-roller",
        Message::ClassRunnerRoller,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(303, 3)],
    ),
    ClassRow::class(
        500_302,
        "runner-ball",
        Message::ClassRunnerBall,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(303, 3)],
    ),
    ClassRow::class(
        500_303,
        "runner-undermount",
        Message::ClassRunnerUndermount,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(303, 3)],
    ),
    ClassRow::class(
        500_400,
        "handle",
        Message::ClassHandle,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(302, 3)],
    )
    .with_aliases(&["bar", "knob", "profile handle"]),
    ClassRow::class(
        500_500,
        "cam-connector",
        Message::ClassCamConnector,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4), ClassPrefix::of(3011, 4), ClassPrefix::of(3012, 4)],
    ),
    ClassRow::class(
        500_600,
        "shelf-pin",
        Message::ClassShelfPin,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3010, 4)],
    ),
    ClassRow::class(
        500_700,
        "lift-mechanism",
        Message::ClassLiftMechanism,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3024, 4), ClassPrefix::of(3025, 4)],
    ),
    ClassRow::class(
        500_800,
        "lock",
        Message::ClassLock,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(302, 3)],
    ),
    ClassRow::class(
        500_900,
        "light-profile",
        Message::ClassLightProfile,
        traits(
            Level::Fitting,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Contour],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(300102, 6)],
    ),
    ClassRow::class(
        501_000,
        "accessory",
        Message::ClassAccessory,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        501_100,
        "sliding-system",
        Message::ClassSlidingSystem,
        traits(Level::Fitting, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3022, 4), ClassPrefix::of(3023, 4), ClassPrefix::of(300103, 6)],
    ),
    // 60 — appliances
    ClassRow::family(600_000, "appliances", Message::ClassFamilyAppliances),
    ClassRow::class(
        600_100,
        "sink",
        Message::ClassSink,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(7001, 4)],
    )
    .with_aliases(&["inset sink", "under-mount sink", "врезная мойка"]),
    ClassRow::class(
        600_200,
        "hob",
        Message::ClassHob,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(7001, 4)],
    )
    .with_aliases(&["gas hob", "induction hob", "газовая панель", "индукционная панель"]),
    ClassRow::class(
        600_300,
        "oven",
        Message::ClassOven,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(3001, 4)],
    ),
    ClassRow::class(
        600_400,
        "extractor",
        Message::ClassExtractor,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[],
    ),
    ClassRow::class(
        600_500,
        "dishwasher",
        Message::ClassDishwasher,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(300101, 6)],
    )
    .with_aliases(&["integrated dishwasher", "free-standing dishwasher"]),
    ClassRow::class(
        600_600,
        "refrigerator",
        Message::ClassRefrigerator,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[],
    )
    .with_aliases(&["integrated refrigerator", "free-standing refrigerator"]),
    ClassRow::class(
        600_700,
        "washer",
        Message::ClassWasher,
        traits(Level::Component, Supply::BuyPiece, Measure::Count, Nesting::Never, Extent::None, &[], None, None, None),
        &[ClassPrefix::of(300101, 6)],
    )
    .with_aliases(&["integrated washer", "free-standing washer"]),
    // 70 — boards-trim
    ClassRow::family(700_000, "boards-trim", Message::ClassFamilyBoardsTrim),
    ClassRow::class(
        700_100,
        "worktop",
        Message::ClassWorktop,
        traits(
            Level::Part,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Contour, Operation::Cutout],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(300101, 6), ClassPrefix::of(300103, 6)],
    )
    .with_aliases(&["countertop", "postformed worktop"]),
    ClassRow::class(
        700_200,
        "plinth",
        Message::ClassPlinth,
        traits(
            Level::Part,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Contour],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(300101, 6), ClassPrefix::of(300103, 6)],
    ),
    ClassRow::class(
        700_300,
        "cornice",
        Message::ClassCornice,
        traits(
            Level::Part,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Contour],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(300102, 6), ClassPrefix::of(300103, 6), ClassPrefix::of(3002, 4)],
    ),
    ClassRow::class(
        700_400,
        "wall-panel",
        Message::ClassWallPanel,
        traits(
            Level::Part,
            Supply::MadeFromSheet,
            Measure::Area,
            Nesting::Member,
            Extent::Free,
            &[Operation::Contour, Operation::Cutout, Operation::EdgeBand],
            None,
            None,
            None,
        ),
        &[],
    ),
    ClassRow::class(
        700_500,
        "edge-profile",
        Message::ClassEdgeProfile,
        traits(
            Level::Part,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Contour],
            None,
            None,
            None,
        ),
        &[],
    ),
    ClassRow::class(
        700_600,
        "hanging-rail",
        Message::ClassHangingRail,
        traits(
            Level::Part,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Holes],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(3002, 4)],
    ),
    ClassRow::class(
        700_700,
        "filler",
        Message::ClassFiller,
        traits(
            Level::Part,
            Supply::MadeFromLinear,
            Measure::Length,
            Nesting::Never,
            Extent::LengthOnly,
            &[Operation::Contour],
            None,
            None,
            None,
        ),
        &[ClassPrefix::of(300101, 6), ClassPrefix::of(300102, 6), ClassPrefix::of(300103, 6)],
    ),
    // 80 — general
    ClassRow::family(800_000, "general", Message::ClassFamilyGeneral),
    ClassRow::class(
        800_100,
        "general-product",
        Message::ClassGeneralProduct,
        traits(
            Level::Product,
            Supply::MadeInShop,
            Measure::Count,
            Nesting::Never,
            Extent::Free,
            &[],
            None,
            Some(Mount::Floor),
            None,
        ),
        &[],
    ),
];

/// Every registry row, families and classes together, ascending by code.
pub fn registry() -> &'static [ClassRow] {
    REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;

    use furnicraft_text::Lang;

    use crate::fixtures::class;

    #[test]
    fn class_codes_encode_three_levels_and_parents() {
        let cabinet = class("30.01.02");
        assert_eq!(cabinet.value(), 300_102);
        assert_eq!(cabinet.to_string(), "30.01.02");
        assert_eq!(cabinet.parent(), class("30.01"));
        assert_eq!(cabinet.parent().parent(), class("30"));
        assert_eq!(cabinet.parent().parent().parent(), ClassCode::UNCLASSIFIED);
        assert!(cabinet.is_ancestor_or_self(cabinet));
        assert!(cabinet.is_ancestor_or_self(class("30")));
        assert!(!class("30").is_ancestor_or_self(cabinet));
        assert!(!cabinet.is_ancestor_or_self(ClassCode::UNCLASSIFIED));
        assert!(ClassCode::UNCLASSIFIED.is_ancestor_or_self(ClassCode::UNCLASSIFIED));
        assert_eq!(class("30").value(), 300_000);
        assert_eq!(format!("{}", class("30")), "30");
        assert_eq!(format!("{}", ClassCode::UNCLASSIFIED), "0");
    }

    #[test]
    fn the_general_product_placeholder_is_a_product_row() {
        let placeholder = ClassCode::GENERAL_PRODUCT;
        let row = placeholder.row().expect("the placeholder is a registry row");
        assert_eq!(row.slug(), "general-product");
        assert_eq!(row.label(), Message::ClassGeneralProduct);
        assert_eq!(row.traits().expect("a class row carries traits").level(), Level::Product);
        assert!(placeholder.is_assignable());
        assert!(placeholder.is_product_class());
        assert!(!class("30.11").is_product_class(), "a shelf is a part, not a product");
    }

    #[test]
    fn class_codes_reject_bad_text_and_out_of_range_codes() {
        assert_eq!(ClassCode::from_code(1_000_000), None);
        assert_eq!(ClassCode::from_code(CLASS_MAX_CODE), Some(ClassCode::MAX));
        assert!("".parse::<ClassCode>().is_err());
        assert!("0.0.0".parse::<ClassCode>().is_err());
        assert!("30.1.2.3".parse::<ClassCode>().is_err());
        assert!("30.123".parse::<ClassCode>().is_err());
        assert!("thirty".parse::<ClassCode>().is_err());
        assert_eq!("300102".parse::<ClassCode>().expect("bare code"), class("30.01.02"));
        assert_eq!("30.01".parse::<ClassCode>().expect("type form"), class("30.01"));
        assert_eq!("30".parse::<ClassCode>().expect("family form"), class("30"));
    }

    #[test]
    fn class_codes_serialize_as_integers() {
        let value = serde_json::to_value(class("30.01.02")).expect("serialize class");
        assert_eq!(value, serde_json::json!(300_102));
        let decoded: ClassCode = serde_json::from_value(value).expect("deserialize class");
        assert_eq!(decoded, class("30.01.02"));
        assert!(serde_json::from_value::<ClassCode>(serde_json::json!(1_000_000)).is_err());
    }

    #[test]
    fn a_lookup_finds_the_row_at_every_level() {
        let family = class("30");
        let type_row = class("30.01");
        let variant = class("30.01.02");

        assert!(family.row().expect("family row").is_family());
        assert!(type_row.row().expect("type row").is_class());
        assert!(variant.row().expect("variant row").is_class());
        assert_eq!(variant.parent(), type_row);
        assert_eq!(type_row.parent(), family);
        assert_eq!(variant.label(), Some(Message::ClassWallCabinet));
        assert_eq!(variant.slug(), Some("wall-cabinet"));
        assert_eq!(variant.row().expect("variant row").parent(), Some(type_row));
    }

    #[test]
    fn an_unknown_code_is_not_in_the_registry_and_is_not_assignable() {
        let unknown = class("20.01.09");
        assert!(!unknown.is_registered());
        assert_eq!(unknown.row(), None);
        assert_eq!(unknown.traits(), None);
        assert!(!unknown.is_assignable());
        assert!(ClassCode::UNCLASSIFIED.is_assignable());
        assert!(class("30.01.02").is_assignable());
        assert!(!class("30").is_assignable(), "a family heading is never assigned");
    }

    #[test]
    fn a_retired_row_still_renders_and_is_not_assignable() {
        let retired = ClassRow::class(
            200_100,
            "bed",
            Message::ClassBed,
            traits(
                Level::Product,
                Supply::MadeInShop,
                Measure::Count,
                Nesting::Never,
                Extent::Free,
                &[],
                None,
                Some(Mount::Floor),
                None,
            ),
            &[],
        )
        .retired();
        assert!(retired.is_retired());
        assert!(retired.is_class());
        assert!(!retired.is_assignable(), "a withdrawn class is never offered again");
        assert_eq!(retired.label(), Message::ClassBed, "it still renders under its own label");
        assert!(retired.traits().is_some(), "it still answers its traits");
        assert!(!ClassRow::family(200_000, "lying", Message::ClassFamilyLying).is_assignable());
    }

    #[test]
    fn aliases_are_search_words_and_never_repeat() {
        for row in registry() {
            let mut seen = std::collections::HashSet::new();
            for alias in row.aliases() {
                assert!(!alias.trim().is_empty(), "{} has an empty alias", row.slug());
                assert_eq!(alias.trim(), *alias, "{} has an alias with stray space", row.slug());
                assert!(seen.insert(alias.to_lowercase()), "{} repeats the alias `{alias}`", row.slug());
            }
        }
        let shelf = class("30.11").row().expect("the horizontal panel row");
        assert!(shelf.aliases().contains(&"shelf"));
        assert!(shelf.aliases().contains(&"полка"));
        assert!(shelf.aliases().contains(&"полочка"));
    }

    #[test]
    fn a_class_answers_its_traits_and_hosts() {
        let shelf = class("30.11");
        let traits = shelf.traits().expect("a shelf is a class");
        assert_eq!(traits.level(), Level::Part);
        assert_eq!(traits.supply(), Supply::MadeFromSheet);
        assert_eq!(traits.measure(), Measure::Area);
        assert_eq!(traits.nesting(), Nesting::Member);
        assert_eq!(traits.extent(), Extent::Free);
        assert_eq!(traits.operations(), OperationSet::of(&[Operation::Contour, Operation::EdgeBand]));
        assert_eq!(traits.opening(), None);
        assert_eq!(traits.fill(), Some(Fill::WidthAndDepth));
        assert!(shelf.is_hosted_in(class("30.01.01")));
        assert!(shelf.is_hosted_in(class("30.01")));
        assert!(!shelf.is_hosted_in(class("70.01")));

        let door = class("30.20");
        let door_traits = door.traits().expect("a door is a class");
        assert_eq!(door_traits.opening(), Some(Opening::Closes));
        assert_eq!(door_traits.operations().len(), 4);
        assert!(door_traits.operations().contains(Operation::CupBore));

        let hinge = class("50.02.01");
        let hinge_traits = hinge.traits().expect("a hinge is a class");
        assert_eq!(hinge_traits.extent(), Extent::None);
        assert!(hinge_traits.operations().is_empty());
        assert!(hinge.is_hosted_in(class("30.20")), "30.2 covers every facade");
        assert!(hinge.is_hosted_in(class("30.10")));
        assert!(!hinge.is_hosted_in(class("30.30")));
    }

    #[test]
    fn a_supply_derives_the_measure_and_nesting_of_a_row() {
        for row in registry() {
            let Some(traits) = row.traits() else { continue };
            // The glass shelf is the table's one documented override: glass is bought by area.
            if row.code() == class("30.14").value() {
                assert_eq!(traits.measure(), Measure::Area);
                continue;
            }
            assert_eq!(traits.measure(), traits.supply().measure(), "{} measure", row.slug());
            assert_eq!(traits.nesting(), traits.supply().nesting(), "{} nesting", row.slug());
        }
    }

    #[test]
    fn the_registry_is_a_sorted_consistent_tree() {
        check_table(registry()).expect("the shipped registry");
    }

    #[test]
    fn a_contradictory_override_is_rejected() {
        let family = ClassRow::family(300_000, "storage", Message::ClassFamilyStorage);
        let door = ClassRow::class(
            300_200,
            "hinged-door-made",
            Message::ClassHingedDoorMade,
            traits(
                Level::Component,
                Supply::MadeFromSheet,
                Measure::Area,
                Nesting::Member,
                Extent::Free,
                &[Operation::Contour],
                Some(Opening::Closes),
                None,
                None,
            ),
            &[],
        );
        let agreeing = ClassRow::class(
            300_201,
            "hinged-door-narrow",
            Message::ClassHingedDoorMade,
            traits(
                Level::Component,
                Supply::MadeFromSheet,
                Measure::Area,
                Nesting::Member,
                Extent::Free,
                &[Operation::Contour, Operation::EdgeBand],
                Some(Opening::Closes),
                None,
                None,
            ),
            &[],
        );
        assert!(check_table(&[family, door, agreeing]).is_ok());

        let contradicting = ClassRow::class(
            300_201,
            "hinged-door-never-nested",
            Message::ClassHingedDoorMade,
            traits(
                Level::Component,
                Supply::MadeFromSheet,
                Measure::Area,
                Nesting::Never,
                Extent::Free,
                &[Operation::Contour],
                Some(Opening::Closes),
                None,
                None,
            ),
            &[],
        );
        assert!(check_table(&[family, door, contradicting]).is_err(), "a stock trait may not change");

        let losing_an_operation = ClassRow::class(
            300_201,
            "hinged-door-unbored",
            Message::ClassHingedDoorMade,
            traits(
                Level::Component,
                Supply::MadeFromSheet,
                Measure::Area,
                Nesting::Member,
                Extent::Free,
                &[],
                Some(Opening::Closes),
                None,
                None,
            ),
            &[],
        );
        assert!(check_table(&[family, door, losing_an_operation]).is_err(), "a variant keeps its type's operations");
    }

    #[test]
    fn the_registry_matches_the_document_table() {
        let rows = document_rows();
        assert_eq!(
            rows.len(),
            registry().iter().filter(|row| row.is_class()).count(),
            "the document and the registry disagree on how many classes exist"
        );
        for cells in rows {
            let code_text = cells[0].trim_matches('`');
            let code = code_text.parse::<ClassCode>().expect("a code in the table");
            let row = code.row().unwrap_or_else(|| panic!("{code_text} is in the document and not in the registry"));
            assert_eq!(row.code(), code.value(), "{code_text} code");
            assert_eq!(row.slug(), cells[2].trim_matches('`'), "{code_text} slug");
            let (english, russian) = cells[1].split_once(" / ").expect("two labels");
            assert_eq!(row.label().text(Lang::En), english, "{code_text} English label");
            assert_eq!(
                row.label().text(Lang::Ru),
                russian.trim_matches(|c| c == '«' || c == '»'),
                "{code_text} Russian label"
            );
            let traits = row.traits().expect("a table row is a class");
            assert_eq!(traits.level(), parse_level(&cells[3]), "{code_text} level");
            assert_eq!(traits.supply(), parse_supply(&cells[4]), "{code_text} supply");
            assert_eq!(traits.measure(), parse_measure(&cells[5]), "{code_text} measure");
            assert_eq!(traits.nesting(), parse_nesting(&cells[6]), "{code_text} nesting");
            assert_eq!(traits.extent(), parse_extent(&cells[7]), "{code_text} extent");
            assert_eq!(traits.operations(), parse_operations(&cells[8]), "{code_text} operations");
            assert_eq!(row.hosts(), parse_hosts(&cells[9]).as_slice(), "{code_text} host");
            assert_eq!(traits.opening(), parse_opening(&cells[10]), "{code_text} opening");
            assert_eq!(traits.mount(), parse_mount(&cells[11]), "{code_text} mount");
            assert_eq!(traits.fill(), parse_fill(&cells[12]), "{code_text} fill");
        }
    }

    #[test]
    fn the_registry_matches_the_document_families() {
        let section = document_section();
        let start = section.find("The headings are listed").expect("the family paragraph");
        let paragraph = section[start..].split("\n\n").next().expect("a paragraph").replace('\n', " ");
        let mut compared = 0;
        for part in paragraph.split(" · ") {
            let fields = part.split('`').collect::<Vec<_>>();
            if fields.len() < 5 {
                continue;
            }
            let code = fields[1].parse::<ClassCode>().expect("a family code in the paragraph");
            let row =
                code.row().unwrap_or_else(|| panic!("the document names the family {code} the registry does not"));
            assert!(row.is_family(), "{code} is a family heading");
            assert_eq!(row.slug(), fields[3], "{code} slug");
            let labels = fields[4].trim().trim_start_matches('—').trim();
            let (english, russian) = labels.split_once(" / ").expect("two labels");
            assert_eq!(row.label().text(Lang::En), english, "{code} English label");
            assert_eq!(
                row.label().text(Lang::Ru),
                russian.trim_matches(|c| matches!(c, '«' | '»' | '.')),
                "{code} Russian label"
            );
            compared += 1;
        }
        assert_eq!(
            compared,
            registry().iter().filter(|row| row.is_family()).count(),
            "a family is missing from the paragraph"
        );
    }

    /// The document the registry is checked against.
    const DOCUMENT: &str = include_str!("../../../docs/domain/classes.md");

    fn document_section() -> &'static str {
        let after = DOCUMENT.split("## 7. The registry").nth(1).expect("the registry section");
        after.split("## 8.").next().expect("the section ends")
    }

    fn document_rows() -> Vec<Vec<String>> {
        document_section()
            .lines()
            .filter(|line| line.starts_with("| `"))
            .map(|line| line.trim().trim_matches('|').split('|').map(|cell| cell.trim().to_owned()).collect::<Vec<_>>())
            .filter(|cells| cells.len() == 13)
            .collect()
    }

    /// Checks the structural rules of a registry table, so the shipped one and a broken sample share
    /// one judge.
    fn check_table(rows: &[ClassRow]) -> Result<(), String> {
        for (index, row) in rows.iter().enumerate() {
            let code = ClassCode::from_code(row.code()).ok_or_else(|| format!("{} is not a class code", row.code()))?;
            if let Some(previous) = index.checked_sub(1).and_then(|previous| rows.get(previous))
                && previous.code() >= row.code()
            {
                return Err(format!("{} is not after {}", row.code(), previous.code()));
            }
            for host in row.hosts() {
                if !(2..=6).contains(&host.digits()) {
                    return Err(format!("{} has a host prefix of {} digits", code, host.digits()));
                }
                let widest = 10u32.saturating_pow(u32::from(host.digits()));
                if host.value() >= widest {
                    return Err(format!("{} has the host prefix {} wider than its digits", code, host.value()));
                }
            }
            if row.is_family() {
                if code.value() % 10_000 != 0 {
                    return Err(format!("the family heading {code} names a type"));
                }
                if !row.hosts().is_empty() {
                    return Err(format!("the family heading {code} cannot host anything"));
                }
                continue;
            }
            let parent_code =
                ClassCode::from_code(row.code()).ok_or_else(|| format!("{} is not a code", row.code()))?.parent();
            let parent = rows
                .iter()
                .find(|candidate| candidate.code() == parent_code.value())
                .ok_or_else(|| format!("{code} has no parent row"))?;
            if code.value() % 100 == 0 {
                if !parent.is_family() {
                    return Err(format!("the type {code} hangs under {parent_code}, which is a class"));
                }
                continue;
            }
            let Some(traits) = row.traits() else {
                return Err(format!("{code} is a class without traits"));
            };
            let Some(parent_traits) = parent.traits() else {
                return Err(format!("the variant {code} hangs under a family heading"));
            };
            if traits.level() != parent_traits.level() {
                return Err(format!("the variant {code} changes its type's level"));
            }
            if traits.supply() != parent_traits.supply() {
                return Err(format!("the variant {code} changes its type's supply"));
            }
            if traits.measure() != parent_traits.measure() {
                return Err(format!("the variant {code} changes its type's measure"));
            }
            if traits.nesting() != parent_traits.nesting() {
                return Err(format!("the variant {code} changes its type's nesting"));
            }
            if traits.opening() != parent_traits.opening() {
                return Err(format!("the variant {code} changes its type's opening"));
            }
            for operation in parent_traits.operations().iter() {
                if !traits.operations().contains(operation) {
                    return Err(format!(
                        "the variant {code} drops the {} operation of its type",
                        operation_name(operation)
                    ));
                }
            }
            for host in row.hosts() {
                if !parent.hosts().iter().any(|wider| covers_prefix(*wider, *host)) {
                    return Err(format!("the variant {code} adds the host {} its type does not have", host.value()));
                }
            }
        }
        Ok(())
    }

    /// Whether `wider` covers every code `narrower` covers.
    fn covers_prefix(wider: ClassPrefix, narrower: ClassPrefix) -> bool {
        if wider.digits() > narrower.digits() {
            return false;
        }
        let factor = 10u32.saturating_pow(u32::from(narrower.digits() - wider.digits()));
        narrower.value() / factor == wider.value()
    }

    fn operation_name(operation: Operation) -> &'static str {
        match operation {
            Operation::Contour => "contour",
            Operation::EdgeBand => "edge band",
            Operation::Holes => "holes",
            Operation::Grooves => "grooves",
            Operation::CupBore => "cup bore",
            Operation::MountPattern => "mounting pattern",
            Operation::Cutout => "cutout",
        }
    }

    fn parse_level(text: &str) -> Level {
        match text {
            "product" => Level::Product,
            "component" => Level::Component,
            "part" => Level::Part,
            "fitting" => Level::Fitting,
            other => panic!("unknown level {other}"),
        }
    }

    fn parse_supply(text: &str) -> Supply {
        match text {
            "sheet" => Supply::MadeFromSheet,
            "linear" => Supply::MadeFromLinear,
            "piece" => Supply::BuyPiece,
            "shop" => Supply::MadeInShop,
            other => panic!("unknown supply {other}"),
        }
    }

    fn parse_measure(text: &str) -> Measure {
        match text {
            "area" => Measure::Area,
            "length" => Measure::Length,
            "count" => Measure::Count,
            other => panic!("unknown measure {other}"),
        }
    }

    fn parse_nesting(text: &str) -> Nesting {
        match text {
            "member" => Nesting::Member,
            "never" => Nesting::Never,
            other => panic!("unknown nesting {other}"),
        }
    }

    fn parse_extent(text: &str) -> Extent {
        match text {
            "free" => Extent::Free,
            "length" => Extent::LengthOnly,
            "none" => Extent::None,
            other => panic!("unknown extent {other}"),
        }
    }

    fn parse_operations(text: &str) -> OperationSet {
        if text == "—" {
            return OperationSet::NONE;
        }
        let operations = text
            .split_whitespace()
            .map(|letter| match letter {
                "C" => Operation::Contour,
                "E" => Operation::EdgeBand,
                "H" => Operation::Holes,
                "G" => Operation::Grooves,
                "U" => Operation::CupBore,
                "M" => Operation::MountPattern,
                "X" => Operation::Cutout,
                other => panic!("unknown operation {other}"),
            })
            .collect::<Vec<_>>();
        OperationSet::of(&operations)
    }

    fn parse_hosts(text: &str) -> Vec<ClassPrefix> {
        if text == "—" {
            return Vec::new();
        }
        text.split(", ")
            .map(|piece| {
                let digits = piece.trim_matches('`').replace('.', "");
                let length = u8::try_from(digits.len()).expect("a prefix of at most six digits");
                ClassPrefix::of(digits.parse().expect("a prefix of digits"), length)
            })
            .collect()
    }

    fn parse_opening(text: &str) -> Option<Opening> {
        match text {
            "closes" => Some(Opening::Closes),
            "—" => None,
            other => panic!("unknown opening {other}"),
        }
    }

    fn parse_mount(text: &str) -> Option<Mount> {
        match text {
            "floor" => Some(Mount::Floor),
            "wall" => Some(Mount::Wall),
            "—" => None,
            other => panic!("unknown mount {other}"),
        }
    }

    fn parse_fill(text: &str) -> Option<Fill> {
        match text {
            "w+d" => Some(Fill::WidthAndDepth),
            "h+d" => Some(Fill::HeightAndDepth),
            "—" => None,
            other => panic!("unknown fill {other}"),
        }
    }
}
