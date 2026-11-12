# STYLE_GUIDE.md — Icon Visual Language for Furniture CAD

Status: normative for **meaning and composition**. The file format — viewBox, allowed elements,
naming, theme table — is normative in `README.md` in this directory. When the two disagree, this
guide wins on meaning and the README wins on format; fix the losing file in the same change.

Read this guide only when adding, editing or reviewing an icon. It does not constrain code that
merely displays an icon.

---

## 1. General Interface Character

Application style:

> **Technical, calm, dense, precise, with an energetic action accent.**

Mood reference — Blender:

- dark graphite interface;
- compact panels;
- many working tools without visual noise;
- the accent appears only where there is action, selection or result;
- minimal decorativeness.

But this is not a Blender copy. Our style should be closer to:

> **CAD for cabinet furniture: panel, frame, module, hardware, drawing, cutting layout.**

Icons should look not like "cute pictograms" but as **working engineering signs**.

### 1.1. Three families

Every icon belongs to exactly one family, and the family decides its grammar:

| Family | Examples | Language | Weight |
|---|---|---|---|
| `geometry` | line, extrude, panel, hole, array | flat for 2D, volumetric for 3D (§8) | main 1.8, result 2.4 |
| `view` | orbit, pan, frame all, ortho/perspective | flat, monochrome, navigation metaphor | heavy, 2.0–2.4 |
| `service` | confirm, cancel, undo, save, warning | flat, monochrome chrome (§19) | heavy, 2.0–2.4 or solid |

A geometry command that happens to sit in a toolbar stays a geometry icon. A service action that
appears next to tools stays a service icon. Do not blend the grammars.

Grammar and role ink are two different facts. The grammar above says how the drawing is composed; the
role ink (§5.1) says what the icon means: a `geometry` icon carries the ink of its operation —
`draw` for a sketched entity, `model` for a solid, `scene` for a panel or a light — while a `view`
icon carries `view` and a `service` icon carries `view`, `modify` or `danger` per §19. The six role
families and their literals are in `README.md`.

---

## 2. Main Icon Formula

A geometry icon is built by the formula:

```text
OBJECT + ACTION METHOD + RESULT
```

Before drawing, the agent must understand:

1. **What does the tool act upon?**
   Panel, frame, room, module, profile, curve, hole, hardware, drawing, report.

2. **What is the initial state?**
   Contour, old part, guide, axis, plane, dimension.

3. **What is the new result?**
   New geometry, changed edge, hole, offset contour, array of parts, connection.

4. **What visual feature explains the action?**
   Construction points, axis, arrow, cutout, emphasised face, dashed guide, repeated elements.

5. **What elements can be removed without losing meaning?**
   If an element does not explain the action, it is unnecessary.

Service icons do not follow this formula: they explain one interface verb and follow §19.

---

## 3. Main Rule of Meaning

Every line, point, face and gap must have meaning.

The agent must be able to label each element:

- this is the source contour;
- this is the result;
- this is the rotation axis;
- this is a construction point;
- this is a guide;
- this is removable volume;
- this is an emphasised edge;
- this is a connection;
- this is a dimension;
- this is hidden geometry.

If an element cannot be explained, it cannot be drawn.

---

## 4. Visual Roles of Elements

An icon consists not of random parts but of semantic roles.

| Role | Meaning | Drawn as |
|---|---|---|
| `BASE` | source object | calm contour, main weight |
| `INK` | main contour | clear line |
| `GUIDE` | auxiliary geometry | thin line, dashed only when it survives 16 px |
| `CONTROL` | construction point / handle | small circle |
| `ACTIVE` | action result | heaviest line in the icon |
| `REMOVE` | deletion / cutout | empty area, or a removal mark only when the action destroys data |
| `MEASURE` | dimension / parameter | thin line with ticks |
| `CONNECTION` | connection / assembly | joint markers, short connector sign |
| `PREVIEW` | preview of a future result | outline only, lighter than `ACTIVE` |

The roles are **structural, not coloured**. An element is emphasised by weight, size, solidity and a
real gap — never by a hue that exists only in the author's head. See §5.

---

## 5. Emphasis Without Colour

### 5.1. One ink per family

Every icon is drawn in the ink of its semantic family — `draw`, `modify`, `model`, `scene`, `view`
`danger` — and that family comes from what the icon means, not from the surface that shows it. The
file carries the family's literal from the one table in `assets/README.md`; the UI replaces that
literal with the active theme's ink before it rasterises, so the same master serves dark and light.
A shape that genuinely means two things may use a second (or third) literal from the same table,
never a colour of its own.

The emphasis ladder, strongest first:

```text
solid shape  >  result fill (0.45–0.60) + heavy line  >  heavy line (2.2–2.6)
             >  passive fill (0.12–0.18) + main line  >  main line (1.6–1.8)  >  thin guide (1.5)
```

A result can also be separated from its source by a real gap, by size, or by being the only filled
form. Two elements that differ only in colour do not differ at all: the family ink says what an icon
is for, and the form inside it still carries the operation.

### 5.2. What colour belongs to

- The **interface theme** owns the neutrals: background, text, selection, the active tool accent.
- The **semantic family** owns the icon's hue: a line is drawn in the `draw` ink, a panel in the
  `scene` ink, a delete action in the `danger` ink. The author never picks a hue per icon.
- The **theme variant** owns the value: the same family is a lighter ink on dark and a folded one on
  paper, chosen so every role clears 3:1 against the panel and the ribbon.
- If a status colour is applied by the theme, the shape must already carry the meaning alone. Colour
  is always redundant, never the only signal, and the set still reads with no colour at all.

Bad:

```text
two identical contours, one simply recoloured outside the family table
```

Good:

```text
source contour is thinner; result is heavier; a real gap and an arrow show the change
```

### 5.3. The accent rule

The action result gets the single strongest treatment in the icon. Everything else steps back.
"One operation, one emphasis" (§13.2) is the practical form of this rule. The action accent is UI
chrome and is never an icon ink: a chosen tool is a background and a border, not a recoloured glyph.

### 5.4. Tone is alpha, not colour

Mass comes from one of exactly two fills, both the family ink at reduced opacity:

| Role | Fill | Outline |
|---|---|---|
| passive face | `fill-opacity` `0.12–0.18` | full-strength main line, `1.6–1.8` |
| result or changed geometry | `fill-opacity` `0.45–0.60` | heavy line, `2.2–2.6` |

The tone is alpha on the role ink, so one master still serves every theme and the icon still reads
with no colour at all. A fill never replaces an outline and never carries meaning alone: the shape
must say the same thing at zero fill. There is no third tone, no decorative wash, no shadow, and no
per-theme fill. A solid fill is the top of the ladder and belongs to a mark — a dot, an arrowhead, a
service glyph — not to a face.

A flat 2D figure may take the passive tone inside its outline. That is still a flat sign: it gets no
modelled side face, no thickness and no isometric hint (§8.1).

---

## 6. Theme Management

Main rule:

> **One SVG master — the theme maps its role inks. Do not create dark and light versions.**

Consequences:

- the only literals inside the file are role inks from the table in `assets/README.md`; no other
  `#hex`, `class`, `style`, CSS variable or `{{token}}` is allowed;
- no separate file for hover, pressed, disabled or toggled: those are theme states (a hover takes one
  emphasis step and a disabled icon is an alpha multiply, §7.5);
- no separate file per size: the loader rasterises the master at the size it draws, and below a
  20 px raster it falls back to one ink from the same master (§7.5);
- the file is self-contained: no references to sibling files, fonts or the network.

The loader and the naming contract are in `README.md`.

---

## 7. Geometry and Size

### 7.1. Grid

All icons are drawn in:

```svg
viewBox="0 0 24 24"
```

Working area: `3..21` on both axes — an 18 × 18 footprint. The icon must have breathing room; do not
fill the canvas. The footprint and its 20–45 % coverage band are in §7.4.

The numbers that the loader needs (viewBox, width, height, allowed elements) are the README's. This
section describes how to compose inside that grid.

### 7.2. Centre and alignment

The compositional centre is `(12, 12)`. Optical shift is allowed when an arrow, a label or an
asymmetric form demands it.

Put coordinates on half-unit steps where possible, so a stroke lands cleanly at 16, 24 and 32 px.
Never rely on a detail smaller than one unit.

### 7.3. Weights and tones

| Role | Weight on the 24 grid |
|---|---|
| auxiliary line / dashed guide | `1.5` |
| main object contour | `1.6–1.8` |
| action result | `2.2–2.6` |
| view or service icon | `2.0–2.4`, or a solid silhouette |
| passive face | `fill-opacity` `0.12–0.18`, under a full-strength main outline |
| result or changed geometry | `fill-opacity` `0.45–0.60`, under the heavy outline |

All lines use round caps and round joins. Moderate rounding: not sharp as a razor, not a soft toy.
Nothing thinner than `1.5` units, because the loader may draw the icon at 16 px.

A thin plate reads as a line if its thickness is under `1.5` units; give every panel a visible edge.

### 7.4. Footprint and coverage

Every drawn icon keeps its ink inside `3..21` on both axes, and covers **20–45 %** of that fixed
18 × 18 footprint. Coverage is alpha-weighted ink — the sum of the rasterised alpha divided by 255,
against the footprint's area — so a passive 15 % face counts as its tone, not as a solid block.

The band is what makes a set read as one family: below 20 % an icon is a sketch, above 45 % it is a
blob. The optical size of the inked silhouette should sit near the footprint, roughly `15..18` units
across, so sibling icons line up. The UI test in `crates/ui/src/icons.rs` measures both numbers from
the raster and fails outside them.

### 7.5. Role ink and the raster size

A role hue needs mass to read: at 16 px a two-tone fill turns into a muddy blob and the outline loses
its weight, while at 20 px the three tones separate. The threshold is a **pixel count**, not a point
size, so a 0.75 UI scale falls back on its own:

| Raster | Ink |
|---|---|
| 20 px or more | the family ink of the active theme variant |
| below 20 px | one ink: the theme's icon colour multiplied by the master's alpha |

The workspace can also ask for one ink at every size (*Settings → Appearance → Icon colour → Never*)
or for the family ink at every size (*Always*). The fallback is the same drawing, so a file never
needs to know which path drew it. A hover or a press moves the family ink one step away from the
surface; a disabled icon keeps its ink at the style's disabled alpha. There is no per-state file.

---

## 8. 2D and 3D Language

### 8.1. Flat operations

If the tool works with 2D geometry:

- line;
- arc;
- circle;
- contour;
- dimension;
- drawing;
- cutting layout;

the icon must be flat.

2D signs:

- no volumetric faces;
- no isometric panel;
- analytical geometry;
- points, lines, arcs, dimensions.

### 8.2. Volumetric operations

If the tool works with 3D:

- panel;
- cabinet;
- hardware;
- hole in a part;
- extrusion;
- revolution;
- boolean operation;
- assembly;

the icon must be volumetric.

3D signs:

- axonometry;
- visible faces;
- different weights for passive faces (never different hues);
- correct overlaps;
- no perspective.

### 8.3. Mixed operations

If the operation translates 2D to 3D, for example a profile becomes a volume, the icon shows the
transition:

```text
flat source profile → volumetric result
```

Source profile calm; result emphasised.

### 8.4. Service actions

Service icons are neither 2D nor 3D. They are flat chrome and follow §19; §8.1–§8.3 do not apply.

---

## 9. Canonical Axonometry

For 3D icons, one technical axonometry is used. It is isometric:

- the vertical axis stays vertical;
- the two horizontal axes recede at 30° above the horizontal, left and right;
- parallel edges stay parallel, equal lengths stay equal;
- no perspective convergence, no vanishing points;
- a circle on a face is a correct ellipse: minor/major ratio `0.577`, major axis horizontal on the
  top face and at 60° on the side faces;
- holes are drawn in the plane of their face, never as circles pasted on top of the object.

### 9.1. The projection

With object axes `u` (right-rear), `v` (left-rear) and `w` (up), a point lands at this offset from
the drawing centre (screen `x` right, `y` down):

```text
x = 0.866 · (u − v)
y = 0.5 · (u + v) − w
```

A cube of edge `s` is a hexagon `1.732 · s` wide and `2 · s` tall, with its three visible faces
meeting at the near vertical edge; the silhouette is one closed path and the three inner edges are
one `M`/`L` run. A horizontal circle of diameter `d` is an ellipse with `rx = d / 2` and
`ry = 0.289 · d`; on the top face the major axis is horizontal, on a side face it is at 60°. There is
no perspective, no second vanishing direction and no hand-picked skew: two icons built on this basis
line up because they were built on the same basis.

### 9.2. The base vocabulary

For furniture CAD the base form is not a cube but a **panel / plate**, so the language is built
around:

- thin rectangular plate: one face `u × v` with `1.2–2` units of visible side;
- upright: the same plate standing on `w`, its thickness in `v`;
- cabinet module: two or three plates sharing an edge;
- cylinder: one top ellipse, two vertical sides, one bottom arc; the side wall is one face;
- sphere: one circle plus its equator ellipse, never shading;
- connection: a solid dot at the joint, never a second fastener.

The same basis serves `cube`, `cylinder`, `panel`, `assembly` and `extrude`: the source profile is a
passive face (`0.12–0.18`) and the geometry the action creates takes the result tone (`0.45–0.60`)
with the heavy outline. A plate stays a plate at every size.

---

## 10. Universal Rules for Building Meaning

### 10.1. If the tool is built by points

Show:

- the final geometry;
- the control points;
- a small air gap between a point and the line.

General rule:

```text
points explain the construction method,
line explains the result.
```

It does not matter whether it is an arc, a spline, a circle or a polyline; the principle is the
same.

### 10.2. If the tool is built from a centre, axis or plane

Show:

- the centre / axis / plane as auxiliary geometry;
- the result as main geometry;
- the link between them through a dashed line, a radius, a guide or a projection.

General rule:

```text
support geometry is thin,
created geometry is expressive.
```

### 10.3. If the tool modifies an existing object

Show:

- the source object neutrally;
- the changed area with the emphasis;
- if necessary, a short arrow for the direction of change.

General rule:

```text
the user must see which part of the object will change.
```

This applies to:

- edge;
- chamfer;
- fillet;
- offset;
- thickening;
- cutout;
- extension;
- splitting;
- parametric change.

### 10.4. If the tool creates a new object from an old one

Show two entities:

```text
source → result
```

Source:

- neutral;
- thinner;
- less contrast.

Result:

- emphasised;
- slightly heavier visually;
- first in attention.

This is how to think about:

- copying;
- array;
- mirror;
- offset;
- extrusion;
- sweep;
- profile revolution.

### 10.5. If the tool removes material

Show not just a symbol but **the absence of material**:

- cutout;
- empty area;
- open inner face;
- removed sector;
- hole;
- slot.

For furniture CAD a hole is a normal production operation, not a danger. Therefore:

- manual deletion of an object — `REMOVE`, with a clear absence or a trash metaphor;
- technical hole or drilling — usually `ACTIVE` or `BASE + ACTIVE`.

Do not paint a production hole red just because it removes material.

### 10.6. If the tool connects objects

Show:

- two distinguishable parts;
- the connection point;
- a short connection marker / fastener / alignment axis.

Do not draw many small hardware pieces. Show the fact of connection.

General rule:

```text
two parts + a clear connection point
```

### 10.7. If the tool is parametric

Show:

- the object;
- the changed parameter;
- a dimension line / handle / direction of change.

Do not use text or numbers inside the icon.

Bad:

```text
W=600
```

Good:

```text
panel + dimension line along the width + an emphasised change handle
```

### 10.8. If the tool relates to room or module layout

Show:

- a simplified room contour;
- the furniture module as a volumetric block;
- the active module or installation location with the emphasis.

Do not draw household details such as handles, legs or decor unless they are the command's meaning.

### 10.9. If the tool relates to drawings, reports or cutting layout

Use flat language:

- sheet;
- projection;
- dimension;
- table;
- cutting rectangles;
- cut lines.

Do not add a 3D panel when the command works only with the document.

---

## 11. Air and Gaps

Air is part of the meaning.

A gap shows that elements are different:

- a construction point is not part of the line;
- a guide is not the object;
- the emphasised element is separated from the base;
- hardware is not merged with the panel;
- a new result is separated from its source.

Minimum gap: `1.5–2` units on the 24 grid.

Air must be real:

- break the path;
- or change the geometry so the gap exists.

Do not cover a line with a background-coloured patch, and do not rely on `mask` or `clipPath`: the
theme may change the background, and the loader does not allow those elements anyway (§15).

---

## 12. Attention Hierarchy

An icon must have one main focus.

Attention order:

1. action result;
2. object being acted upon;
3. auxiliary construction;
4. decorative detail — prohibited.

If the user sees the guide first and not the result, the hierarchy is wrong.

---

## 13. Visual Perception Rules

### 13.1. Form is more important than colour

An icon must be understandable in one ink. That is the normal case, not a fallback: the role ink is
decoration over a drawing that already works.

If two elements are told apart only by colour, the icon is wrong.

### 13.2. One operation, one emphasis

If an icon has three competing active elements, the user does not know what is main.

Allowed:

```text
one changed face
one arrow
one group of holes as a single result
one new contour
```

Not allowed:

```text
arrow + dimensions + points + an entire object highlighted in different ways
```

### 13.3. Similar actions look related

Commands from the same family share a visual pattern:

- different ways to build an arc share the main arc but differ in control elements;
- different array types share repeated elements but differ in guides;
- different edge operations share the panel and the emphasised end.

The difference shows the command difference, not the author's mood.

### 13.4. Opposite actions are visually connected

Paired operations use the same grammar:

- add / remove;
- connect / disconnect;
- expand / shrink;
- enable / disable;
- show / hide.

Deletion still shows a clear sign of absence, not merely a different treatment.

---

## 14. Furniture Specifics

The main object of the system is not an abstract cube but a furniture construction.

Visual dictionary:

| Entity | How to depict |
|---|---|
| furniture dimension | thin frame parallelepiped |
| panel | thin plate / rectangular part |
| upright | vertical panel |
| shelf | horizontal panel |
| facade | front flat panel |
| drawer | elongated volume / box |
| hardware | compact technical element |
| hole | void in a part face |
| edge | emphasised end |
| fastener | connector between parts |
| room | flat or volumetric room contour |
| module | simplified cabinet / block |
| drawing | sheet + projection |
| cutting layout | sheet / plate + nested rectangles |
| report | sheet + table structure |

Do not use wood texture in ordinary icons. Wood, material and texture appear only when the command is
specifically about material.

---

## 15. Technical SVG Rules

The agent creates the SVG by hand. There are no generator scripts and no Python; if automation is
ever needed it is a Rust `xtask` subcommand, never a second toolchain.

Allowed elements:

- `path`;
- `rect` (with `rx` when a rounded corner is intended);
- `circle`;
- `ellipse`;
- `line`;
- `polyline`;
- `polygon`;
- `g` and its `transform`.

Forbidden:

- `defs`, `clipPath`, `mask`, `style`, `class`;
- filters, shadows, blur, gradients, patterns;
- embedded raster images, fonts, text;
- external references, editor metadata, `<title>`/`<desc>`.

Drawing rules:

- `stroke` is the icon's family ink from the table in `README.md`; a stroke-only shape keeps
  `fill="none"`;
- a face takes the same ink with `fill-opacity` `0.12–0.18` (passive) or `0.45–0.60` (result or
  changed geometry) and keeps its full-strength outline;
- a solid silhouette, a dot or an arrowhead may fill with the family ink and take no stroke — that is
  the top of the emphasis ladder, not a face;
- one to three inks, all from the table, and the family's ink is always one of them; never a colour
  outside it;
- round caps and round joins;
- no `scale` transform to fake another size: the loader rasterises at the size it draws;
- at most two decimal places;
- one drawing per file, one file per id.

The naming, the folder layout and the exact attribute list are in `README.md`.

---

## 16. What the Agent Must Do Before Drawing

Before creating the SVG, formulate:

```text
1. Object:
   What is depicted?

2. Action:
   What changes or is created?

3. Method:
   What input elements explain the method?

4. Result:
   Which part carries the emphasis?

5. Unnecessary:
   What details can be removed?
```

For a service icon add:

```text
6. Convention:
   Which standard glyph does the user already know for this verb,
   and which enclosure (circle, triangle) marks its status?
```

Example, as a thought process rather than a rigid form:

```text
Object: curve.
Action: construction by control points.
Method: points must be visible.
Result: the curve itself.
Unnecessary: axes, dimensions, decorative markers.
```

---

## 17. Quality Check

An icon is ready only if:

- the action is understandable without text;
- the result matters more than the source;
- auxiliary elements do not compete with the result;
- the icon uses one role ink and survives having no colour at all;
- it reads at 16 px on the panel background and does not turn into mush;
- no decorative lines;
- no random colours;
- no background-coloured patch instead of a real gap;
- the geometric metaphor matches the real operation;
- a service icon is the same glyph on every surface that shows the action, and a destructive action
  keeps its label or confirmation;
- status and action are told apart by shape (enclosure), not by hue.

---

## 18. Most Important Rule for the Agent

Do not draw a command icon as an abstract symbol.

Draw:

```text
a visual explanation of the action
```

The user must see:

- what was;
- what will change;
- by what method it is built;
- which part is the result.

If an icon is beautiful but the action is unclear, it is bad.

If an icon is simple but a professional understands the command immediately, it is good.

---

Final style formulation:

> **Graphite technical interface, one role ink per meaning, emphasis carried by weight and real gaps. Neutral furniture
> volumes, thin guides, clear control points, the strongest treatment only on the result. Flat for
> sketches and documents, volumetric for parts and form operations; service chrome is flat, heavy and
> shaped. Each icon explains the action through geometry, not through decorativeness.**

---

## 19. Service and Interface Actions

These icons do not explain a CAD operation. They explain an interface verb: confirm, cancel, undo,
delete, save, warning. They are chrome and follow their own grammar; §8–§10 do not apply.

### 19.1. Scope

Confirm/apply, dismiss/close surface, cancel pending input, undo/redo, add/remove, delete, duplicate,
rename, copy/paste, file new/open/save/export/import, print, settings, search, visibility, lock,
expand/collapse, warning/error/info/help.

A geometry command that happens to sit in a toolbar is not a service icon. A service icon that
appears in a toolbar does not become a geometry icon.

### 19.2. Grammar

- Flat, monochrome, no axonometry, no panel, no guide, no control point, no dashed line, no
  dimension.
- One glyph = one action, identical in menu, toolbar, dialog and shortcut reference. Never two
  metaphors for one verb.
- Readable at 16 px: a solid silhouette or a stroke of at least 2 units on the 24 grid; no detail
  below 1 unit. Remove detail instead of shrinking it.
- Right/down means forward, apply, export, open; left/up means back, import, undo. Never rotate an
  icon to reuse it.
- Paired actions share weight, anchor and grammar; only the distinguishing element mirrors
  (undo/redo, add/remove, lock/unlock, show/hide).
- At most one modifier mark per glyph (plus, minus, exclamation), 7–8 units, always in the same
  corner, separated by a real 1.5-unit gap.
- Bare glyph = action; enclosed glyph (circle or triangle) = status. A bare check means confirm; a
  check inside a circle means success. This keeps action and status apart without colour.
- Hover, pressed, disabled and toggled are theme states. Never a second SVG and never an extra shape
  for a state.

### 19.3. Metaphors

| Action | Glyph | Rule |
|---|---|---|
| confirm / apply | bare check | commits the pending change |
| dismiss / close surface | X | closes a panel or dialog; not a document operation |
| cancel pending input | X or a back arrow | discards what was typed; never a trash |
| undo / redo | open arc with an arrowhead, mirrored | a closed loop is refresh, not undo |
| delete | trash can | never an X; irreversible delete keeps its label |
| add / remove | plus / minus | remove-from-list is a minus, not a trash |
| duplicate | two offset sheets | |
| rename | sheet + pencil | |
| copy / paste | two sheets / clipboard | |
| file new | sheet with a folded corner | |
| file open | folder | |
| file save | sheet + down arrow into a tray | |
| file export / import | sheet + arrow out right / in left | |
| print | sheet above a printer bar | |
| settings | gear, 6–8 teeth | |
| search | magnifier, handle at 45° | |
| command palette | a prompt field: a frame with `>` and a caret | opens the command list |
| visible / hidden | eye / eye with a slash | |
| lock / unlock | padlock, shackle closed / open | |
| pin | pushpin, head over a needle | keeps one method of a tool as the click default |
| expand / collapse | chevron down / right | one direction convention |
| warning | triangle with exclamation | risk of data loss or invalid geometry |
| error | circle with exclamation | a failure that already happened |
| info | circle with i | |
| help | circle with question mark | |

A rare action does not earn a glyph. Save-as, replace, go-to and similar stay text-only until a
second surface needs them; then they get one glyph, not a decorated variant.

### 19.4. Emphasis and colour

Service icons carry the role ink of what they do: `danger` for a destructive command only, `modify`
for an edit such as undo, copy or visibility, and `view` for the rest of the chrome. The ink is a
redundant tone; the shape must carry the meaning alone and must read at 16 px with no colour at all.
The action accent belongs to the interface theme, never to chrome: a save icon is not orange because
saving is useful.

### 19.5. Size

Service icons share the 24 grid and are drawn heavier and simpler than geometry icons (2.0–2.4
units, solid shapes). They are not shrunken toolbar drawings.

### 19.6. Files

`assets/icons/actions/<name>.svg`, kebab-case: `confirm`, `dismiss`, `cancel`, `undo`, `redo`,
`delete`, `add`, `remove`, `duplicate`, `rename`, `copy`, `paste`, `file-new`, `file-open`,
`file-save`, `file-export`, `file-import`, `print`, `settings`, `search`, `command`, `visible`,
`hidden`, `lock`, `unlock`, `expand`, `collapse`, `panel-left`, `panel-right`, `warning`, `error`,
`info`, `help`, `pin`.

A theme owns its icons: `contour/close` closes a ring, `actions/dismiss` dismisses a surface. They are
different actions with different glyphs; never rename a geometry action to free a name.

### 19.7. Additions to the quality check

- reads at 16 px without colour;
- one glyph used by every surface that shows the action;
- destructive actions keep a label or a confirmation;
- status and action are told apart by enclosure, not by hue;
- the ink is the one its family names in `README.md`, and no other.
