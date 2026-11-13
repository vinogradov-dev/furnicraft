# Semantic classes

Status: domain specification. This document defines what an object *is* for the program: the class
registry, its encoding, the traits that carry behaviour, and the rules that keep the registry alive
for the life of the product. It replaces the furniture-type registry of the previous milestone.

The registry's rows are written down in §7; the owner approves them entry by entry, and the table is
the source the code is checked against.

Related decisions: [0034](../decisions/0034-nodes-carry-typed-properties.md),
[0094](../decisions/0094-a-class-registry-gives-the-machine-its-semantics.md),
[0095](../decisions/0095-the-order-package-is-a-published-snapshot.md).

## 1. Why a class exists

A node is a body plus a meaning. The body (`Panel`, `Container`, `Object`) says what is drawn and how
it is represented; the class says what the thing is and therefore **what may be done with it**:

- a side panel is cut from sheet stock, is nested, carries edge banding and stretches freely;
- a worktop is a board too, but it is linear stock bought by length, never nested, and must not be
  stretched across its width;
- a hinge is bought by the piece, never nested, never banded, never resized, and admits a cup bore
  and a mounting pattern.

Without the class every one of those sentences becomes a special case written somewhere in the
program, usually about a name or a structural kind. With it, the rules arrive with the object and
every consumer — the inspector, the command `enabled` check, a report, a cutting list, a CNC rule —
reads the same answer.

**The code identifies; the traits decide.** The digits are a stable name, not a compiler: no code path
may compare a code with a literal outside a test. The pattern is
`class.traits().operations().contains(Operation::EdgeBand)`, never `code == 200101`.

## 2. Encoding

Six decimal digits, written `GG.TT.VV` and stored as one integer from `0` to `999_999`:

| Level | Code | Question it answers |
|---|---|---|
| group | `GG.00.00` | the function family: seating, storage, work surface, fitting, appliance, board and trim |
| type | `GG.TT.00` | the thing inside the family: cabinet, shelf, worktop, hinge |
| variant | `GG.TT.VV` | the variant, **only when a rule differs**: hinged versus sliding, overlay versus inset, roller versus ball-bearing runner |

`000000` means unclassified and is the default of every node. It is a sentinel, not the root of the
tree: a query on it matches only nodes without a class.

A zero suffix names the parent: `30.01.00` is the type, `30.00.00` the group, and the dotted form is
presentation only. `100103`, `10.01.03` and `10.01` all parse; display drops a zero variant and a zero
type.

**Three levels are the ceiling.** Anything finer — a 35 mm cup, a damper, a brand, a colour — belongs
to the catalogue article, not to the class. A fourth level is a new schema decision, not an
extension of this one.

**The digits stay.** They sort, compare and prefix-query with no allocation, they read acceptably in
an export table, and six digits match the width the design tradition uses. A mnemonic string in the
stored key was rejected: mnemonics rot (`CAB` stops meaning what it meant) and a stored key cannot be
changed. Each row carries a `slug` (`shelf`, `worktop`, `hinge-spring`) for search, export and program
names; the slug is never stored in a document and never compared.

## 3. The registry row

The registry is a constant table in `furnicraft-model`; it is not a database and not a file the
program loads at run time.

| Field | Meaning |
|---|---|
| `code` | the six-digit identity. Assigned once, never reused, never renumbered |
| `parent` | derived from the digits; a test asserts that the parent sits one level up — a type's parent is its family heading, a variant's parent is its type row |
| `slug` | mnemonic for people, export tables and generated program names |
| `label_key` | the translation key in `furnicraft-text`; names are data, not code |
| `aliases` | search synonyms (`полка`, `полочка`), never used for matching a document |
| `traits` | the behaviour set below; the only thing algorithms read |
| `since` | the release that introduced the row, for the registry changelog |
| `retired` | withdrawn: resolvable and translated, no longer assignable |

**A family is a heading, not a class.** `GG.00.00` names the function family and carries a slug and a
label, but no traits: its members disagree about `supply`, so a trait set for the family would be a
lie and the inheritance rule would have nothing to narrow. A family is therefore never assigned to a
node; a type or a variant is.

Inheritance is the authoring rule and the test behind it, not a run-time walk: every row in §7 states
its complete trait set, so a lookup is a bit test. A variant must agree with its type on the *stock*
traits — `supply`, `measure`, `nesting` — and on `level` and `opening`; its `operations` must contain
the type's, and its `host` may narrow the type's but never widen it (an undermount runner mounts
under the drawer and not on a side panel). `mount`, `extent` and `fill` describe the *form* rather than the
stock, so a variant may state its own: a wall cabinet is the same cabinet as a base one and differs
in `mount`, which is exactly why 0097 made mounting a trait. The test still rejects the contradiction
that matters — `nesting = Member` under a type that is `Never` — because a form never changes what a
thing is made of.

## 4. Traits, version 1

| Trait | Values | Semantics | First consumer |
|---|---|---|---|
| `supply` | `MadeFromSheet`, `MadeFromLinear`, `BuyPiece`, `MadeInShop` | where the object comes from: a nesting plan, a length cut, a purchase order, or the workshop's own assembly (0104) | reports, cutting list |
| `measure` | `Area`, `Length`, `Count` | the unit of a report line; derived from `supply` unless a class overrides it with a reason | reports |
| `nesting` | `Member`, `Never` | whether a cutting plan owns the object; derived from `supply` | cutting list |
| `extent` | `Free`, `LengthOnly`, `None` | which dimensions a gizmo may change and which resize commands are offered | gizmos, `enabled` |
| `operations` | `Holes`, `Grooves`, `Contour`, `EdgeBand`, `CupBore`, `MountPattern`, `Cutout` | which machining a class admits | inspector, CNC rules |
| `host` | a set of classes | where the object may sit or be mounted | insertion, mounting |
| `level` | `Product`, `Component`, `Part`, `Fitting` | report grouping and tree presentation | reports |
| `opening` | `Closes`, `None` | whether the object closes an opening, and therefore has gaps and a facade report | reports, drawing |
| `mount` | `Floor`, `Wall` | where the piece stands: on the floor or hung; `None` for anything that is not a product | the furniture move tool (0097) |
| `fill` | `None`, `HeightAndDepth`, `WidthAndDepth` | which axis pair a part takes from a free box when it fills a void | the empty-space tool (0097) |

Rules for the vocabulary:

1. **A trait needs a consumer.** A trait lands with the code that reads it and a test proving the
   wrong class is refused. A trait without a consumer is speculation and is rejected in review.
2. **`measure` and `nesting` are derived** from `supply`, because two independent columns allow an
   impossible row (sheet stock measured by length). An override is allowed with a written reason.
   The derivation is `sheet` → area and member, `linear` → length and never, `piece` and `shop` →
   count and never; each row in §7 repeats the derived pair so the table reads as a whole.
3. **Traits describe physics and economics**, not a workshop's vocabulary: what it is made from, how
   it is measured, what may be done to it, where it may be. That is what makes them survive a decade
   of products.
4. **`level` does not mark a component.** A container carrying a class is a component (0034); `level`
   only tells reports what kind of thing it is.
5. **A trait change is a behaviour change.** Correcting a mistake in an existing row is allowed but
   changes what saved projects do; it is recorded in the registry changelog like a schema change.
6. **The product traits arrived with their consumers** (0097): `mount` is read by the furniture move
   tool, `fill` by the empty-space tool. `mount` applies to `level = Product` rows only; `fill` to
   `level = Part` rows only, and a class may carry neither.
7. **`MadeInShop` marks an assembled object, not a stock item** (0104): a product, or a part built
   from a recipe such as a tamburat panel. It derives to a count and to no nesting, because what is
   nested is its components, not it.

## 5. A class earns its code

A class gets a code when **at least one rule differs** from its siblings, or when **a report, a
drawing or a CNC program must name it separately**. When only the wording differs, it is one class
with one label and an alias.

- `30.11` horizontal panel and `30.01.01` base cabinet are both storage; the rules differ in `level`,
  so both exist.
- `30.11` horizontal panel and a hypothetical "shelf for a shoe cabinet" have identical rules and the
  same line in every report; that is one class and an alias, and the node's own name says “shelf”.
- A shelf, a bottom and a top are that one class for the same reason: the rules are identical and the
  parts specification carries the maker's names, so a second code would select nothing.
- `30.10` vertical panel and `30.11` horizontal panel differ in `fill`, which the empty-space tool
  reads: a post-like part takes height and depth, a shelf-like part width and depth.
- `30.20` made hinged door and `30.21` bought hinged door are one shape in two channels: a made leaf is
  nested and bored here, a bought one is counted, never nested and not bored by us, so a rule differs
  and the two report lines differ too.
- `30.15` tamburat panel is a class of its own although it is made of sheet parts: it is assembled
  here from a recipe, so it is counted once and never nested, while its skins are what the cutting
  list expands.
- `70.01` worktop and `70.02` plinth are both `MadeFromLinear`; their `extent`, `operations` and report
  line differ, so they are two classes. A bought postformed worktop is the same class as a made one:
  it is linear stock cut and cut out identically, and only the supplier changes.
- `50.02` cup hinge with a damper is one class: the damper is an article property, not a rule.
- `50.02` cup hinge and its overlay, inset and half-overlay forms are three variants: the mounting
  rule differs.
- `50.11` sliding door system is a fitting and not a door variant: two wardrobes with different
  systems have the same door class and different articles.

## 6. Why the rows exist: three scenarios

A row earns its code by being *used*: by a behaviour the program must apply, or by a line that a
report, a drawing or a CNC program must name. The two consumers are the program and the world
outside it, and a class that neither reads is speculation and leaves the table (§5).

**The program** asks three questions of a class: *what may be done to it* (which axis resizes, which
machining is admitted, whether an insertion is legal), *where it belongs* (a product, a component of
a product, a part inside one, a fitting on it) and *where it stands or what it fills* (the floor or
the wall, an empty space).

**The shop** asks where the line goes. Reports and the order package (0095) are nine queries, and
every trait selects exactly one of them or one behaviour:

| Line or behaviour | Selected by | What it answers |
|---|---|---|
| parts specification, per product | `level` and the product walk | every node of the piece: name, size, material, banding, machining |
| cutting list | `supply = sheet` with `nesting = member` | what is nested on a sheet |
| linear cut list | `supply = linear` | what is cut to length |
| purchase list | `supply = piece` | what is bought, with a quantity |
| shop-made list | `supply = shop` | what the workshop assembles itself, and from what |
| facade report | `opening = closes` | every front, its gaps, its mounting form and its channel |
| banding report | `operations` contains `EdgeBand` | the edge banding metres and the material |
| CNC program | `operations` | the machining we do ourselves |
| purchase grouping | `level` | a fitting, an appliance, a lighting line |
| resize, banding and cutout commands | `extent`, `operations` | what the editor must refuse |
| insertion and mounting | `host` | whether a part may sit there |
| the furniture move tool | `mount` | floor-bound or free |
| the empty-space tool | `fill` | which axis pair a part takes from a void |

That table is the answer to "why this row".

### 6.1 A kitchen

A full set: base, wall and tall cabinets (`30.01.01`–`30.01.03`) with their vertical and horizontal
panels (`30.10`, `30.11`) and backs (`30.12`); a worktop (`70.01`), a wall panel (`70.04`), a plinth
(`70.02`), a cornice (`70.03`), a filler (`70.07`); made doors (`30.20`) and bought doors (`30.21`);
made drawers (`30.30`) and bought drawer boxes (`30.31`); drawer fronts (`30.26`); hinges, runners,
handles, connectors and shelf pins (`50.01`–`50.06`); a basket (`50.10`); an LED profile (`50.09`);
a sink, a hob, an oven, an extractor, a dishwasher and a refrigerator (`60.01`–`60.07`); a shaped
tamburat door (`30.15`) and a bent end panel (`30.16`).

Follow one object through the lines:

- **A made door** (`30.20`) is sheet stock, so it is nested and its area is reported; it closes the
  opening, so the facade report carries it with its gaps; it admits the cup bore and the mounting
  pattern, so the CNC program gets those records; it is a component, so the parts specification
  groups it under its cabinet.
- **A bought door** (`30.21`) has the same shape, the same gaps and the same mounting form, and is a
  different class for one rule: the shop does nothing to it. It is counted, never nested, and admits
  no in-house machining, so its hinge bores belong to the supplier's specification and its line
  belongs to the purchase list. That is the owner's “MDF with a plastic finish, bought ready or made
  here”.
- **A tamburat door** (`30.15`) is made here, so it is counted once and never nested; its recipe —
  two 8 mm skins and the rails between them — is what the cutting list expands. The program draws
  one panel, the shop cuts five parts. A recipe is a production mechanism like the hardware model
  that generates machining, not a class: the class only has to say that this is ours to make.
- **A bent end panel** (`30.16`) is the same story with a curved outline, which the contour already
  carries.
- **The worktop** (`70.01`) is linear stock cut to length, measured by the metre, resized on its
  length only; its sink and hob cutouts are `Cutout` operations; it never enters the cutting list.
  A bought postformed slab is the same class, because it is linear stock cut and cut out the same
  way — the supplier is the material, not the rule.
- **The LED profile** (`50.09`) is a fitting by `level` and a linear item by `supply`: bought by the
  metre and cut to length, so its line lands in the linear cut list.
- **The appliances** (`60.01`–`60.07`) are bought whole, counted, with no envelope and no `mount`;
  the sink and the hob name the worktop as their host.

The reports: a parts specification per cabinet; a cutting list of sheet parts; a linear list of the
worktop, plinth, cornice, filler and light profile; a purchase list of the bought doors, drawer
boxes, fittings, accessories and appliances; a shop-made list of the tamburat and bent parts; a
facade report split into made and bought; a banding report; and a CNC program.

### 6.2 A bedroom

A wardrobe, a bed, a table, a nightstand and a chest. The rule *the trade name is the piece's own
name, the class says what the piece is* settles them: the wardrobe is a tall cabinet (`30.01.03`),
the nightstand and the chest are base cabinets (`30.01.01`), the table is `40.01.01`, and the bed is
`20.01` with a bought mattress (`20.02`) inside it. The wardrobe's sliding doors are `30.22` or
`30.23` and its hanging rail is `70.06`, hosted by the tall cabinet.

A bedroom report is therefore four product rows with their parts and their user names, not a
vocabulary of trade classes that no rule reads.

### 6.3 Sliding wardrobes and their systems

The system — the tracks, the rollers, the guides — is a bought set with a supplier, a price and a
weight limit, so it is a fitting (`50.11`) hosted by the door and the cabinet. The door is ours:
`30.22` made, with the groove for the track and the gap; or `30.23` bought, where the supplier cuts
and drills it.

That split is why the class does not carry the system: two wardrobes with different systems have the
same door class and different fittings, so the drawing, the gap rule and the door's machining stay
put when the supplier changes. A system that changes *our* door — a framed leaf with an infill
instead of a solid one — becomes a door row only when such a door is built, because the parts
rather than the article differ.

### 6.4 What the scenarios removed

- **The chest, the nightstand and the wardrobe**: no rule and no line of their own; they are a base
  cabinet, a base cabinet and a tall cabinet with the maker's name on them (6.2).
- **Bottom, top and shelf as three classes**: identical rules and the same line in every report. The
  parts specification carries the maker's name and the cutting list groups by material, so they are
  one class, `30.11` horizontal panel.
- **Side and divider as two classes**: one class, `30.10` vertical panel; the void tool treats a
  post, a side and a divider alike.
- **A second worktop class for the bought slab**: the same rules, only a different supplier.
- **A class per sliding system**: the system is an article, not a rule of the door.

## 7. The registry

The table in this section is the registry's content for version 1. It becomes a constant table in
`furnicraft-model`, and a test compares it with this document row for row, so the prose cannot drift
from what the program knows. A change here is a registry change and carries a changelog entry; once
shipped, a code is immortal (§8).

Status of the rows: **approved by the owner on 2026-11-13**, entry by entry, with one question left
open — how a workshop-made part such as a tamburat panel is decomposed for production (§12). Practice
with real orders revisits the rows; a change here is a registry change and needs the owner again.

Read a row as a sentence: *this code names this thing; it is at this level; it comes from this
supply; it is measured and nested this way; it may be resized this way; it admits these operations;
it sits in these hosts; it closes an opening or it does not; it stands this way; and it fills a void
this way.*

| Column | Values |
|---|---|
| `level` | `product`, `component`, `part`, `fitting` (§4) |
| `supply` | `sheet` (MadeFromSheet), `linear` (MadeFromLinear), `piece` (BuyPiece), `shop` (MadeInShop, 0104) |
| `measure` | `area`, `length`, `count` |
| `nesting` | `member`, `never` |
| `extent` | `free` (every size), `length` (length only), `none` |
| `ops` | `C` contour, `E` edge band, `H` holes, `G` grooves, `U` cup bore, `M` mounting pattern, `X` cutout; `—` admits no machining |
| `host` | code prefixes it may sit in or on: `30` is every storage class, `30.2` every facade, `30.01` every cabinet form, `30.20` exactly one class; `—` has no furniture host |
| `opening` | `closes`, `—` |
| `mount` | `floor`, `wall`, `—` |
| `fill` | `w+d` width and depth, `h+d` height and depth, `—` not a void filler |

`measure` and `nesting` are the derived pair of §4; each row states them so the table is complete,
and an override is called out where it appears. Within every group, `TT = 90..99` stays reserved for
a workshop's local codes.

A section is a **family**: its heading carries the code, the slug and the two labels, and every table
row under it is an assignable class. The headings are listed here once so the tree is complete:
`10` `seating` — Seating / «Места для сидения» · `20` `lying` — Lying / «Места для лежания» ·
`30` `storage` — Storage / «Хранение» · `40` `surfaces` — Surfaces / «Поверхности» ·
`50` `fittings` — Fittings / «Фурнитура» · `60` `appliances` — Appliances / «Бытовая техника» ·
`70` `boards-trim` — Boards and trim / «Плиты и погонаж» · `80` `general` — General / «Общее».

### 7.1 Seating — `10`

A piece a person sits on. The family's parts — a seat board, a back, a leg — arrive with the seating
tools, because a part earns its row when a report must name it and a tool exists to create it.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `10.01` | Chair / «Стул» | `chair` | product | shop | count | never | free | — | — | — | floor | — |
| `10.02` | Stool / «Табурет» | `stool` | product | shop | count | never | free | — | — | — | floor | — |
| `10.03` | Bench / «Скамья» | `bench` | product | shop | count | never | free | — | — | — | floor | — |
| `10.04` | Sofa / «Диван» | `sofa` | product | shop | count | never | free | — | — | — | floor | — |

A chair and a stool have the same rules and different report lines, so both exist; a dining chair, an
office chair and a kitchen chair are one class with aliases.

### 7.2 Lying — `20`

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `20.01` | Bed / «Кровать» | `bed` | product | shop | count | never | free | — | — | — | floor | — |
| `20.02` | Mattress / «Матрас» | `mattress` | component | piece | count | never | none | — | `20.01` | — | — | — |

A mattress is bought complete, never resized and never banded, and belongs to the bed. That it is a
component and not a product shows the two axes are independent: `level` groups a report, `supply`
says where the thing comes from.

### 7.3 Storage — `30`

Products. The cabinet type is the parent of the three forms and the class for a cabinet whose form is
not decided yet; the forms are variants because `mount` differs between a wall cabinet and a base one
(0097 reads it in the move tool) and the report separates them. A wardrobe, a nightstand, a chest and
a shoe cabinet are these classes with the maker's own name on the node (6.2).

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `30.01` | Cabinet / «Шкаф корпусный» | `cabinet` | product | shop | count | never | free | — | — | — | floor | — |
| `30.01.01` | Base cabinet / «Шкаф нижний» | `base-cabinet` | product | shop | count | never | free | — | — | — | floor | — |
| `30.01.02` | Wall cabinet / «Шкаф навесной» | `wall-cabinet` | product | shop | count | never | free | — | — | — | wall | — |
| `30.01.03` | Tall cabinet / «Шкаф высокий» | `tall-cabinet` | product | shop | count | never | free | — | — | — | floor | — |

Parts. The rule that separates a vertical panel from a horizontal one is `fill`: the void tool offers
`30.10` for a post-like fill and `30.11` for a shelf-like one. The rule that separates the back is
`ops`: it is the one sheet part that takes no banding. Tamburat and bent parts are made here from a
recipe, so they are counted once and their components are what is nested — how that recipe is
written down is still open (§12).

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `30.10` | Vertical panel / «Вертикальная панель» | `vertical-panel` | part | sheet | area | member | free | C E | `30.01` | — | — | h+d |
| `30.11` | Horizontal panel / «Горизонтальная панель» | `horizontal-panel` | part | sheet | area | member | free | C E | `30.01` | — | — | w+d |
| `30.12` | Back panel / «Задняя стенка» | `back-panel` | part | sheet | area | member | free | C | `30.01` | — | — | — |
| `30.13` | Drawer box part / «Деталь ящика» | `drawer-box-part` | part | sheet | area | member | free | C E | `30.30` | — | — | — |
| `30.14` | Glass shelf / «Стеклянная полка» | `glass-shelf` | part | piece | area | never | free | C | `30.01` | — | — | w+d |
| `30.15` | Tamburat panel / «Плита тамбурат» | `tamburat-panel` | part | shop | count | never | free | C E | `30.01` | — | — | — |
| `30.16` | Bent panel / «Гнутая панель» | `bent-panel` | part | shop | count | never | free | C E | `30.01` | — | — | — |

The aliases carry the workshop's own words, which are not classes: `30.10` is «боковина»,
«перегородка» and «стойка»; `30.11` is «полка», «днище» and «крыша»; `30.13` is a drawer side, bottom
or back. The glass shelf is the table's one override: glass is bought by area from the glazier, so
`measure` stays area under a `piece` supply, and no sheet is nested.

Components. A front closes an opening, so it carries the gaps and the facade report. A made front is
sheet stock cut, banded and bored here; a bought front arrives finished — the shop does nothing to
it, so it is counted, never nested and admits no in-house machining. The two are separate classes
because one rule differs, which is exactly the split the facade report needs. A drawer is the same
pair: a made one is a box of our own parts, a bought one is an article.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `30.20` | Hinged door, made / «Дверь распашная, своя» | `hinged-door-made` | component | sheet | area | member | free | C E U M | `30.01` | closes | — | — |
| `30.21` | Hinged door, bought / «Дверь распашная, покупная» | `hinged-door-bought` | component | piece | count | never | free | — | `30.01` | closes | — | — |
| `30.22` | Sliding door, made / «Дверь раздвижная, своя» | `sliding-door-made` | component | sheet | area | member | free | C E G | `30.01` | closes | — | — |
| `30.23` | Sliding door, bought / «Дверь раздвижная, покупная» | `sliding-door-bought` | component | piece | count | never | free | — | `30.01` | closes | — | — |
| `30.24` | Lift-up front, made / «Откидной фасад, свой» | `lift-up-made` | component | sheet | area | member | free | C E M | `30.01` | closes | — | — |
| `30.25` | Lift-up front, bought / «Откидной фасад, покупной» | `lift-up-bought` | component | piece | count | never | free | — | `30.01` | closes | — | — |
| `30.26` | Drawer front, made / «Фасад ящика, свой» | `drawer-front-made` | component | sheet | area | member | free | C E M | `30.01` | closes | — | — |
| `30.27` | Drawer front, bought / «Фасад ящика, покупной» | `drawer-front-bought` | component | piece | count | never | free | — | `30.01` | closes | — | — |
| `30.30` | Drawer, made / «Ящик, свой» | `drawer-made` | component | shop | count | never | free | — | `30.01` | — | — | — |
| `30.31` | Drawer, bought / «Ящик, покупной» | `drawer-bought` | component | piece | count | never | none | — | `30.01` | — | — | — |

A front is a `Panel` body with a component-level class, not a new body: it admits the cup bore and the
mounting pattern its hinges need. Which of the four forms it is — hinged, sliding, lift-up, drawer
front — is a *type* and not a variant, because the channel already owns the variant level in no other
way: each form comes made and bought, and a variant may not change `supply` (§3). The mounting form —
overlay, inset, half-overlay — is carried by the hinge class instead, because that is where the rule
lives; a third axis below the variant would need a fourth level.

### 7.4 Work and dining surfaces — `40`

The type row is the parent of the two forms and the class for a table whose mount is not decided yet.
A work table, a dining table and a desk are one class with the maker's name; only `mount` differs.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `40.01` | Table / «Стол» | `table` | product | shop | count | never | free | — | — | — | floor | — |
| `40.01.01` | Floor table / «Стол напольный» | `floor-table` | product | shop | count | never | free | — | — | — | floor | — |
| `40.01.02` | Wall table / «Стол навесной» | `wall-table` | product | shop | count | never | free | — | — | — | wall | — |

A top and a leg of a table are an ordinary horizontal panel and an ordinary leg and take their rows
with the table tool.

### 7.5 Fittings — `50`

A fitting is bought, admits no machining on itself (the bore it needs is machined into its host, and
its catalogue model generates that), is never nested, never resized and never banded. `host`
distinguishes one from another, and one of them is bought by the metre rather than by the piece.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `50.01` | Card hinge / «Петля карточная» | `card-hinge` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.01.01` | Card hinge, overlay / «накладная» | `card-hinge-overlay` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.01.02` | Card hinge, inset / «вкладная» | `card-hinge-inset` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.01.03` | Card hinge, half-overlay / «полунакладная» | `card-hinge-half` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.02` | Cup hinge / «Петля чашечная» | `cup-hinge` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.02.01` | Cup hinge, overlay / «накладная» | `cup-hinge-overlay` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.02.02` | Cup hinge, inset / «вкладная» | `cup-hinge-inset` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.02.03` | Cup hinge, half-overlay / «полунакладная» | `cup-hinge-half` | fitting | piece | count | never | none | — | `30.10`, `30.2` | — | — | — |
| `50.03` | Runner / «Направляющая» | `runner` | fitting | piece | count | never | none | — | `30.10`, `30.3` | — | — | — |
| `50.03.01` | Roller runner / «роликовая» | `runner-roller` | fitting | piece | count | never | none | — | `30.10`, `30.3` | — | — | — |
| `50.03.02` | Ball-bearing runner / «шариковая» | `runner-ball` | fitting | piece | count | never | none | — | `30.10`, `30.3` | — | — | — |
| `50.03.03` | Undermount runner / «скрытая» | `runner-undermount` | fitting | piece | count | never | none | — | `30.3` | — | — | — |
| `50.04` | Handle / «Ручка» | `handle` | fitting | piece | count | never | none | — | `30.2` | — | — | — |
| `50.05` | Cam connector / «Эксцентрик» | `cam-connector` | fitting | piece | count | never | none | — | `30.10`, `30.11`, `30.12` | — | — | — |
| `50.06` | Shelf pin / «Полкодержатель» | `shelf-pin` | fitting | piece | count | never | none | — | `30.10` | — | — | — |
| `50.07` | Lift mechanism / «Подъёмный механизм» | `lift-mechanism` | fitting | piece | count | never | none | — | `30.24`, `30.25` | — | — | — |
| `50.08` | Lock / «Замок» | `lock` | fitting | piece | count | never | none | — | `30.2` | — | — | — |
| `50.09` | Light profile / «Профиль подсветки» | `light-profile` | fitting | linear | length | never | length | C | `30.01.02` | — | — | — |
| `50.10` | Accessory / «Аксессуар» | `accessory` | fitting | piece | count | never | none | — | `30.01` | — | — | — |
| `50.11` | Sliding door system / «Система раздвижных дверей» | `sliding-system` | fitting | piece | count | never | none | — | `30.22`, `30.23`, `30.01.03` | — | — | — |

The hinge is the worked case. A card hinge and a cup hinge are two types: the first is screwed flat
and the second bores a 35 mm cup through a mounting pattern, and the shop buys them as separate
lines. Overlay, inset and half-overlay are variants because the mounting rule differs; the same hinge
with a damper is the same class, because a damper is an article property and not a rule. A bar, a
knob and a profile handle are one class with aliases: no rule differs and the article names the shape.

The light profile is the family's exception that proves the axes independent: `level = fitting` puts
its line with the bought parts, `supply = linear` puts it in the linear cut list, because the shop
buys it by the metre and cuts it.

### 7.6 Appliances — `60`

An appliance is bought whole, occupies space and never acquires parts, so it is a component and not a
product: no envelope, no `mount`, and its size comes from the catalogue article. Inset and
under-mount sinks, gas and induction hobs, integrated and free-standing machines are one class each
with aliases; when the rule “an integrated appliance takes a facade” lands, that rule earns its
variant.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `60.01` | Sink / «Мойка» | `sink` | component | piece | count | never | none | — | `70.01` | — | — | — |
| `60.02` | Hob / «Варочная панель» | `hob` | component | piece | count | never | none | — | `70.01` | — | — | — |
| `60.03` | Oven / «Духовой шкаф» | `oven` | component | piece | count | never | none | — | `30.01` | — | — | — |
| `60.04` | Extractor / «Вытяжка» | `extractor` | component | piece | count | never | none | — | — | — | — | — |
| `60.05` | Dishwasher / «Посудомоечная машина» | `dishwasher` | component | piece | count | never | none | — | `30.01.01` | — | — | — |
| `60.06` | Refrigerator / «Холодильник» | `refrigerator` | component | piece | count | never | none | — | — | — | — | — |
| `60.07` | Washer / «Стиральная машина» | `washer` | component | piece | count | never | none | — | `30.01.01` | — | — | — |

### 7.7 Boards and trim — `70`

Linear stock cut to length, and the boards that belong to a surface rather than to a case. The family
holds two supplies on purpose: a worktop is linear, a wall panel is sheet and is nested.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `70.01` | Worktop / «Столешница» | `worktop` | part | linear | length | never | length | C X | `30.01.01`, `30.01.03` | — | — | — |
| `70.02` | Plinth / «Цоколь» | `plinth` | part | linear | length | never | length | C | `30.01.01`, `30.01.03` | — | — | — |
| `70.03` | Cornice / «Карниз» | `cornice` | part | linear | length | never | length | C | `30.01.02`, `30.01.03`, `30.02` | — | — | — |
| `70.04` | Wall panel / «Стеновая панель» | `wall-panel` | part | sheet | area | member | free | C X E | — | — | — | — |
| `70.05` | Edge profile / «Кромочный профиль» | `edge-profile` | part | linear | length | never | length | C | — | — | — | — |
| `70.06` | Hanging rail / «Штанга» | `hanging-rail` | part | linear | length | never | length | H | `30.02` | — | — | — |
| `70.07` | Filler strip / «Добор» | `filler` | part | linear | length | never | length | C | `30.01.01`, `30.01.02`, `30.01.03` | — | — | — |

The worktop admits a cutout and the plinth does not; that one operation difference plus the report
line earns both codes. A straight and an L-shaped worktop are one class: the shape is geometry, not
a rule, and the report line is the same.

### 7.8 The placeholder and the reserved ranges

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `80.01` | General product / «Изделие общего назначения» | `general-product` | product | shop | count | never | free | — | — | — | floor | — |

`product.create` assigns this row when the card names no type, so a product always has a class
(0097); the maker picks the real one later from the frame's card. It is `level = Product` and is
never offered when the target is a part or a fitting. The family is named `80` «Общее» / “General”
and holds this row alone, because the placeholder has no function: putting it into a real family
would make that family's report line lie, and a family that exists only to hold it costs one code.

Group `90` is reserved for a workshop's own group, and `TT = 90..99` inside every group is reserved
for its local classes; an official row never lands in either range.

## 8. Assignment and lifecycle

- `class` is a reserved system property of every node. The inspector assigns it from the registry
  tree (search by label and alias, retired rows hidden, unclassified available); the model rejects a
  code the registry does not know **on assignment**.
- Loading does not validate: an unknown number is kept, the node is marked *not in the registry*, and
  the scene loads. The registry is append-only, so this should never happen for a code that shipped.
- Setting a class on a container marks it as a component; clearing removes the marker unless
  `prototype.id` or `component = true` is also present (0034).
- A product always has a class. `product.create` assigns `80.01`, «изделие общего назначения» /
  “general product”, when the card is not given a type; the placeholder is a `level = Product` row
  that is never offered for a part, and the maker changes it later from the frame's card.
- A code, once in a released build, is **immortal**: never reused, never renumbered, never moved to
  another parent. A withdrawn class is set `retired`, stays resolvable and translated, and stops being
  offered for assignment.
- Each group reserves `TT = 90..99` for local and experimental codes, so a workshop's private class
  never collides with a future official one.

## 9. What the class does not contain

Material, thickness, finish, dimensions, price, supplier, article number, placement, prototype
identity and instance identity. A specific hinge is a catalogue article pointing at the class
`50.02.01`; the class stays the same when the supplier changes.

## 10. The fixture set

The ten objects every test scene and every tool demo starts from, resolved against the table, with
the report line and the editor behaviour each row implies. A behaviour that does not appear here has
no consumer yet and does not belong in the table; the made and the bought door appear twice because
that pair is the one rule the scenarios added.

| Object | Body | Class | Rules | Report line | Editor behaviour |
|---|---|---|---|---|---|
| Cabinet, base | `Container` | `30.01.01` | product, shop, floor | products, counted | frame drawn, opened, parts born inside, moves on the floor plane |
| Shelf | `Panel` | `30.11` | sheet, area, member, free, C E, fill w+d | parts, area, nests | resizes freely, banding offered, offered by the void tool for a width+depth fill |
| Side | `Panel` | `30.10` | sheet, area, member, free, C E, fill h+d | parts, area, nests | banding offered, offered by the void tool for a height+depth fill |
| Back | `Panel` | `30.12` | sheet, area, member, free, C | parts, area, nests | no banding command, no void fill |
| Door, hinged, made | `Panel` | `30.20` | component, sheet, area, member, free, C E U M, closes | facades, made, with gaps | closes an opening, admits cup bores and a mounting pattern, nested with the panels |
| Door, hinged, bought | `Panel` | `30.21` | component, piece, count, never, —, closes | facades, bought | closes an opening, never nested, no in-house machining |
| Spring hinge, overlay | `Container` | `50.02.01` | piece, count, never, none | fittings, overlay | no resize, no banding, never nested |
| Runner, undermount | `Container` | `50.03.03` | piece, count, never, none | fittings, undermount | no resize, no banding, never nested |
| Worktop | `Panel` | `70.01` | linear, length, never, length, C X | parts, length, linear purchase | resizes by length only, cutouts allowed, never nested |
| Plinth | `Panel` | `70.02` | linear, length, never, length, C | parts, length, linear purchase | resizes by length only, no cutout, never nested |

## 11. Export

The package's `classes` table (see [order export](order-export.md)) carries the code, slug, label,
parent and the traits that a reader needs to place an object in a report. That is deliberate: a report
written in another language decides "worktop goes to linear purchase, never to nesting" from the
table, not from our digits.

## 12. Open questions

- Whether a workshop may add private classes at run time, and if so where that table lives, is not
  decided. The reserved range exists so the answer can be yes later without touching official codes.
- Whether an object may carry more than one class (hardware that is also a fitting family) is still
  open; a second class would need a rule for which one an algorithm reads.
- `host` is written as a code prefix, so `30.2` means every facade and `30.01` every cabinet form.
  Whether a rule needs an exact class rather than a prefix — a runner on a drawer box and not on a
  shelf, for example — is decided when the mounting check is written (T4).
- A product kind beyond the ones listed (a sideboard, a shoe cabinet, a corner cabinet) takes its row
  when its rules or its report line differ from an existing row; a corner cabinet is a ruled
  difference of the envelope, not a class difference, so it may never need one.
- **The recipe of a workshop-made part is open.** `30.15` tamburat and `30.16` bent are classes
  because the shop assembles them, and `supply = shop` keeps them counted once and out of the cutting
  list. Whether their components travel as a recipe on the object (two 8 mm skins and the rails,
  expanded by the exporter) or as ordinary sheet parts in the tree is not decided; the class rows do
  not change either way, the production mechanism does. The owner left it to practice after the first
  real tamburat order.
