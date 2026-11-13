# Semantic classes

Status: domain specification. This document defines what an object *is* for the program: the class
registry, its encoding, the traits that carry behaviour, and the rules that keep the registry alive
for the life of the product. It replaces the furniture-type registry of the previous milestone.

The registry's rows are written down in §6; the owner approves them entry by entry, and the table is
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

Inheritance is the authoring rule and the test behind it, not a run-time walk: every row in §6 states
its complete trait set, so a lookup is a bit test. A variant must agree with its type on the *stock*
traits — `supply`, `measure`, `nesting` — and on `level` and `opening`, and its `operations` and
`host` must contain the type's. `mount`, `extent` and `fill` describe the *form* rather than the
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
   count and never; each row in §6 repeats the derived pair so the table reads as a whole.
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

## 5. A class earns its code

A class gets a code when **at least one rule differs** from its siblings, or when **a report, a
drawing or a CNC program must name it separately**. When only the wording differs, it is one class
with one label and an alias.

- `30.11` shelf and `30.01.01` base cabinet are both storage; the rules differ in `level`, so both
  exist.
- `30.11` shelf and a hypothetical "shelf for a shoe cabinet" have identical rules and the same
  report line; that is one class and an alias.
- `70.01` worktop and `70.02` plinth are both `MadeFromLinear`; their `extent`, `operations` and
  report line differ, so they are two classes.
- `50.02` spring hinge and the same hinge with a damper are one class: the damper is an article
  property.
- `50.02` spring hinge and its overlay, inset and half-overlay forms are three classes: the mounting
  rule differs.

## 6. The registry

The table in this section is the registry's content for version 1. It becomes a constant table in
`furnicraft-model`, and a test compares it with this document row for row, so the prose cannot drift
from what the program knows. A change here is a registry change and carries a changelog entry; once
shipped, a code is immortal (§7).

Status of the rows: written, awaiting the owner's approval entry by entry. A row that the owner
rejects changes here before it reaches the code, and a rejected trait leaves the vocabulary.

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
| `host` | the classes it may sit in or on; a code matches itself and everything below it; `—` has no furniture host |
| `opening` | `closes`, `—` |
| `mount` | `floor`, `wall`, `—` |
| `fill` | `w+d` width and depth, `h+d` height and depth, `—` not a void filler |

`measure` and `nesting` are the derived pair of §4; each row states them so the table is complete,
and an override is called out where it appears. Within every group, `TT = 90..99` stays reserved for
a workshop's local codes.

A section is a **family**: its heading carries the code, the slug and the two labels, and every table
row under it is an assignable class. The headings are listed here once so the tree is complete:
`10` `seating` · `20` `lying` · `30` `storage` · `40` `surfaces` · `50` `fittings` · `60` `appliances`
· `70` `boards-trim` · `80` `general`.

### 6.1 Seating — `10`

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

### 6.2 Lying — `20`

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `20.01` | Bed / «Кровать» | `bed` | product | shop | count | never | free | — | — | — | floor | — |
| `20.02` | Mattress / «Матрас» | `mattress` | component | piece | count | never | none | — | `20.01` | — | — | — |

A mattress is bought complete, never resized and never banded, and belongs to the bed. That it is a
component and not a product shows the two axes are independent: `level` groups a report, `supply`
says where the thing comes from.

### 6.3 Storage — `30`

Products. The cabinet type is the parent of the three forms and the class for a cabinet whose form is
not decided yet; the three forms are variants because `mount` differs between a wall cabinet and a
base one (0097 reads it in the move tool) and the report separates them.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `30.01` | Cabinet / «Шкаф корпусный» | `cabinet` | product | shop | count | never | free | — | — | — | floor | — |
| `30.01.01` | Base cabinet / «Шкаф нижний» | `base-cabinet` | product | shop | count | never | free | — | — | — | floor | — |
| `30.01.02` | Wall cabinet / «Шкаф навесной» | `wall-cabinet` | product | shop | count | never | free | — | — | — | wall | — |
| `30.01.03` | Tall cabinet / «Шкаф высокий» | `tall-cabinet` | product | shop | count | never | free | — | — | — | floor | — |
| `30.02` | Wardrobe / «Шкаф платяной» | `wardrobe` | product | shop | count | never | free | — | — | — | floor | — |
| `30.03` | Chest of drawers / «Комод» | `chest` | product | shop | count | never | free | — | — | — | floor | — |

Parts. The rule that separates a side from a shelf is `fill`; the rule that separates a back is
`ops`. A bottom, a top and a shelf have the same rules and stay three classes because the parts list
and the assembly drawing name them apart, and a query for “every shelf” must not return bottoms.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `30.10` | Side panel / «Боковина» | `side` | part | sheet | area | member | free | C E | `30.01` | — | — | h+d |
| `30.11` | Shelf / «Полка» | `shelf` | part | sheet | area | member | free | C E | `30.01` | — | — | w+d |
| `30.12` | Bottom panel / «Днище» | `bottom` | part | sheet | area | member | free | C E | `30.01` | — | — | w+d |
| `30.13` | Top panel / «Крыша» | `top` | part | sheet | area | member | free | C E | `30.01` | — | — | w+d |
| `30.14` | Back panel / «Задняя стенка» | `back` | part | sheet | area | member | free | C | `30.01` | — | — | — |
| `30.15` | Divider / «Перегородка» | `divider` | part | sheet | area | member | free | C E | `30.01` | — | — | h+d |
| `30.16` | Drawer box part / «Деталь ящика» | `drawer-box-part` | part | sheet | area | member | free | C E | `30.01` | — | — | — |
| `30.17` | Glass shelf / «Стеклянная полка» | `glass-shelf` | part | piece | area | never | free | C | `30.01` | — | — | w+d |

The glass shelf is the table's one override: glass is bought by area from the glazier, so `measure`
stays area under a `piece` supply, and the sheet is never nested.

The empty-space tool offers exactly the two classes that carry a fill: `30.11` shelf for a
width-and-depth void and `30.15` divider — the shop's «стойка», a post — for a height-and-depth one.

Facades. Each is a component that closes an opening, so it carries the gaps and the facade report.
A door's three forms are variants because the machining rule differs — a hinged leaf takes a cup bore
and a mounting pattern, a sliding leaf takes a groove, a flap takes a pattern.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `30.20` | Door / «Дверь» | `door` | component | sheet | area | member | free | C E | `30.01` | closes | — | — |
| `30.20.01` | Hinged door / «Дверь распашная» | `hinged-door` | component | sheet | area | member | free | C E U M | `30.01` | closes | — | — |
| `30.20.02` | Sliding door / «Дверь раздвижная» | `sliding-door` | component | sheet | area | member | free | C E G | `30.01`, `30.02` | closes | — | — |
| `30.20.03` | Lift-up front / «Откидной фасад» | `lift-up-front` | component | sheet | area | member | free | C E M | `30.01` | closes | — | — |
| `30.21` | Drawer front / «Фасад ящика» | `drawer-front` | component | sheet | area | member | free | C E M | `30.01` | closes | — | — |

A door is a `Panel` body with a component-level class, not a new body: it admits the cup bore and the
mounting pattern its hinges need. Its mounting form — overlay, inset, half-overlay — is carried by
the hinge class instead, because that is where the rule lives; a second axis below the variant would
need a fourth level. A drawer front is a type and not a variant because its report line is its own
and no machining rule separates it from a flap. A door that is also a glass frame needs a cutout and
arrives as a row when the glazed-door tool does.

### 6.4 Work and dining surfaces — `40`

The table type is the parent of its three uses and the class for a table whose use is not decided yet.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `40.01` | Table / «Стол» | `table` | product | shop | count | never | free | — | — | — | floor | — |
| `40.01.01` | Work table / «Рабочий стол» | `work-table` | product | shop | count | never | free | — | — | — | floor | — |
| `40.01.02` | Dining table / «Обеденный стол» | `dining-table` | product | shop | count | never | free | — | — | — | floor | — |
| `40.01.03` | Wall table / «Стол навесной» | `wall-table` | product | shop | count | never | free | — | — | — | wall | — |
| `40.02` | Desk / «Письменный стол» | `desk` | product | shop | count | never | free | — | — | — | floor | — |

A wall table is a variant because `mount` differs; a desk is a type because only its report line
differs. A top and a leg of a table are ordinary panels and legs and take a part row with the
table tool.

### 6.5 Fittings — `50`

A fitting is bought by the piece, admits no machining on itself (the bore it needs is machined into
its host, and its catalogue model generates that), is never nested, never resized and never banded.
Only `host` distinguishes one from another.

| Code | English / Russian | slug | level | supply | measure | nesting | extent | ops | host | opening | mount | fill |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `50.01` | Card hinge / «Петля карточная» | `card-hinge` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.01.01` | Card hinge, overlay / «накладная» | `card-hinge-overlay` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.01.02` | Card hinge, inset / «вкладная» | `card-hinge-inset` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.01.03` | Card hinge, half-overlay / «полунакладная» | `card-hinge-half` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.02` | Cup hinge / «Петля чашечная» | `cup-hinge` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.02.01` | Cup hinge, overlay / «накладная» | `cup-hinge-overlay` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.02.02` | Cup hinge, inset / «вкладная» | `cup-hinge-inset` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.02.03` | Cup hinge, half-overlay / «полунакладная» | `cup-hinge-half` | fitting | piece | count | never | none | — | `30.10`, `30.20` | — | — | — |
| `50.03` | Runner / «Направляющая» | `runner` | fitting | piece | count | never | none | — | `30.10`, `30.16` | — | — | — |
| `50.03.01` | Roller runner / «роликовая» | `runner-roller` | fitting | piece | count | never | none | — | `30.10`, `30.16` | — | — | — |
| `50.03.02` | Ball-bearing runner / «шариковая» | `runner-ball` | fitting | piece | count | never | none | — | `30.10`, `30.16` | — | — | — |
| `50.03.03` | Undermount runner / «скрытая» | `runner-undermount` | fitting | piece | count | never | none | — | `30.16` | — | — | — |
| `50.04` | Handle / «Ручка» | `handle` | fitting | piece | count | never | none | — | `30.20`, `30.21` | — | — | — |
| `50.05` | Cam connector / «Эксцентрик» | `cam-connector` | fitting | piece | count | never | none | — | `30.10`, `30.12`, `30.13`, `30.15` | — | — | — |
| `50.06` | Shelf pin / «Полкодержатель» | `shelf-pin` | fitting | piece | count | never | none | — | `30.10`, `30.15` | — | — | — |
| `50.07` | Lift mechanism / «Подъёмный механизм» | `lift-mechanism` | fitting | piece | count | never | none | — | `30.20.03` | — | — | — |
| `50.08` | Lock / «Замок» | `lock` | fitting | piece | count | never | none | — | `30.20`, `30.21` | — | — | — |

The hinge is the worked case. A card hinge and a cup hinge are two types: the first is screwed flat
and the second bores a 35 mm cup through a mounting pattern, and the shop buys them as separate
lines. Overlay, inset and half-overlay are variants because the mounting rule differs; the same hinge
with a damper is the same class, because a damper is an article property and not a rule. A bar, a
knob and a profile handle are one class with aliases: no rule differs and the article names the shape.

### 6.6 Appliances — `60`

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

### 6.7 Boards and trim — `70`

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

### 6.8 The placeholder and the reserved ranges

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

## 7. Assignment and lifecycle

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

## 8. What the class does not contain

Material, thickness, finish, dimensions, price, supplier, article number, placement, prototype
identity and instance identity. A specific hinge is a catalogue article pointing at the class
`50.02.01`; the class stays the same when the supplier changes.

## 9. The fixture set

The nine objects every test scene and every tool demo starts from, resolved against the table, with
the report line and the editor behaviour each row implies. A behaviour that does not appear here has
no consumer yet and does not belong in the table.

| Object | Body | Class | Rules | Report line | Editor behaviour |
|---|---|---|---|---|---|
| Cabinet, base | `Container` | `30.01.01` | product, shop, floor | products, counted | frame drawn, opened, parts born inside, moves on the floor plane |
| Shelf | `Panel` | `30.11` | sheet, area, member, free, C E, fill w+d | parts, area, nests | resizes freely, banding offered, offered by the void tool for a width+depth fill |
| Side | `Panel` | `30.10` | sheet, area, member, free, C E, fill h+d | parts, area, nests | banding offered, offered by the void tool for a height+depth fill |
| Back | `Panel` | `30.14` | sheet, area, member, free, C | parts, area, nests | no banding command, no void fill |
| Door, hinged | `Panel` | `30.20.01` | component, sheet, area, member, free, C E U M, closes | facades, with gaps | closes an opening, admits cup bores and a mounting pattern |
| Spring hinge, overlay | `Container` | `50.02.01` | piece, count, never, none | fittings, overlay | no resize, no banding, never nested |
| Runner, undermount | `Container` | `50.03.03` | piece, count, never, none | fittings, undermount | no resize, no banding, never nested |
| Worktop | `Panel` | `70.01` | linear, length, never, length, C X | parts, length, linear purchase | resizes by length only, cutouts allowed, never nested |
| Plinth | `Panel` | `70.02` | linear, length, never, length, C | parts, length, linear purchase | resizes by length only, no cutout, never nested |

## 10. Export

The package's `classes` table (see [order export](order-export.md)) carries the code, slug, label,
parent and the traits that a reader needs to place an object in a report. That is deliberate: a report
written in another language decides "worktop goes to linear purchase, never to nesting" from the
table, not from our digits.

## 11. Open questions

- Whether a workshop may add private classes at run time, and if so where that table lives, is not
  decided. The reserved range exists so the answer can be yes later without touching official codes.
- Whether an object may carry more than one class (hardware that is also a fitting family) is still
  open; a second class would need a rule for which one an algorithm reads.
- `host` is written as a code that matches its subtree, so `30.01` means every cabinet form. Whether a
  rule needs an exact class rather than a subtree — a runner on a drawer box and not on a shelf, for
  example — is decided when the mounting check is written (T4).
- A product kind beyond the ones listed (a sideboard, a shoe cabinet, a corner cabinet) takes its row
  when its rules or its report line differ from an existing row; a corner cabinet is a ruled
  difference of the envelope, not a class difference, so it may never need one.
