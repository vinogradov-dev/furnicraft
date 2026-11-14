# Architecture decisions

One file per decision, short: the situation, the decision, and what it costs us. Decisions are
superseded, never rewritten — add a new record and mark the old one `Superseded by NNNN`.

| # | Title | Status |
|---|---|---|
| [0001](0001-geometry-kernel.md) | Geometry kernel behind a port, implemented with truck | Accepted |
| [0002](0002-vendored-truck-shapeops.md) | Vendored `truck-shapeops` without the VTK stack | Accepted |
| [0003](0003-length-is-integer-micrometres.md) | Lengths are integer micrometres | Accepted |
| [0004](0004-panel-local-frame.md) | Panel-local frame, faces and topology ids | Accepted; frame origin superseded by 0009 |
| [0005](0005-material-catalogue.md) | Material catalogue is external and session-scoped | Accepted |
| [0006](0006-panel-nominal-and-resolved-geometry.md) | Panels store nominal size; geometry is resolved | Accepted; core-origin placement superseded by 0009 |
| [0007](0007-edge-banding-v1.md) | Edge banding v1: ordered bands and butt joints | Accepted; per-path banding unit superseded by 0014 |
| [0008](0008-panel-diagnostics.md) | Panel diagnostics and command invalidation | Accepted |
| [0009](0009-contour-first-panel-model.md) | Contour-first panel geometry and banding paths | Accepted; core-local machining superseded by 0011; banding-path unit superseded by 0014 |
| [0010](0010-analytic-line-arc-contours.md) | Analytic line and arc contours for panel offsets | Accepted |
| [0011](0011-panel-local-machining.md) | Machining uses the nominal panel frame | Accepted |
| [0012](0012-in-panel-cutout-union.md) | In-house panel cutout union | Accepted |
| [0013](0013-geometry-edits-are-never-blocked.md) | Geometry edits are never blocked by banding or machining | Accepted |
| [0014](0014-banding-belongs-to-segments.md) | Banding belongs to one contour segment | Accepted |
| [0015](0015-band-joint-trim.md) | Band joints extend and trim in application order | Accepted; arc scope extended by 0016 |
| [0016](0016-arc-band-joints.md) | Arc band joints use the same rules as straight ones | Accepted |
| [0017](0017-panels-are-atomic-assemblies-are-groups.md) | Panels are atomic leaves, assemblies are editable groups | Superseded by 0021 |
| [0018](0018-picking-ray-casts-the-scene.md) | Picking ray-casts the scene description and returns document ids | Accepted |
| [0019](0019-standard-views-from-a-corner-widget.md) | Standard views come from a projected corner orientation widget | Accepted; pole clamp superseded by 0057 |
| [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md) | The translate gizmo previews a frame and commits one placement | Accepted; batch commit extended by 0033 |
| [0021](0021-assemblies-are-components-groups-are-containers.md) | Assemblies are components, groups are containers | Superseded by 0034; supersedes 0017 |
| [0022](0022-closed-containers-pick-as-one-object.md) | A closed container picks as one object | Accepted |
| [0023](0023-fasad-furniture-and-standard-views.md) | Fasad furniture and standard views | Accepted |
| [0024](0024-rotation-is-a-quaternion-shown-as-xyz-euler-angles.md) | Rotation is a quaternion shown as intrinsic XYZ Euler angles | Accepted |
| [0025](0025-one-manipulation-pivot.md) | Move and Rotate share one pivot at the first corner of the local bounds | Superseded by 0026 |
| [0026](0026-the-manipulation-pivot-is-stored-node-data.md) | The manipulation pivot is stored, editable node data | Accepted; supersedes 0025 |
| [0027](0027-the-rotate-gizmo-uses-world-aligned-rings.md) | The rotate gizmo uses world-aligned rings | Accepted; batch commit extended by 0033 |
| [0028](0028-the-assistant-is-a-client-of-the-command-registry.md) | The assistant is a client of the command registry | Accepted |
| [0029](0029-view-state-belongs-to-the-session.md) | View state belongs to the session | Accepted; camera and light objects carved out by 0036 |
| [0030](0030-construction-knowledge-is-code.md) | Construction knowledge is code, not prose | Superseded by 0034 |
| [0031](0031-external-resources-keep-their-own-system-of-record.md) | External resources keep their own system of record | Accepted |
| [0032](0032-an-order-keeps-its-versions-as-folders.md) | An order keeps its versions as numbered folders | Accepted |
| [0033](0033-a-multi-selection-is-placed-by-one-command.md) | A multi-selection is placed by one command | Accepted |
| [0034](0034-nodes-carry-typed-properties.md) | Nodes carry typed properties; containers are one kind | Accepted; supersedes 0021 and 0030 |
| [0035](0035-furniture-types-form-a-numbered-tree.md) | Furniture types form a numbered tree | Accepted |
| [0036](0036-cameras-and-lights-are-scene-objects.md) | Cameras and lights are scene objects | Accepted |
| [0037](0037-a-geometry-object-is-a-mesh-or-a-feature-history.md) | A geometry object is a baked mesh or a feature history | Accepted |
| [0038](0038-the-scene-file-lives-in-its-own-crate.md) | The scene file lives in its own crate | Accepted |
| [0039](0039-point-lights-use-watts-and-inverse-square-falloff.md) | Point lights use watts and inverse-square falloff | Accepted |
| [0040](0040-a-constructive-contour-is-a-ring-of-segments.md) | A constructive contour is a ring of directed segments | Accepted |
| [0041](0041-every-solid-compiles-to-one-solid-plan.md) | Every solid compiles to one domain-free plan | Accepted |
| [0042](0042-a-contour-panel-keeps-its-ring-and-edits-a-working-copy.md) | A contour panel keeps its ring and edits a working copy | Accepted |
| [0043](0043-region-booleans-trace-the-boundary-of-the-set.md) | Region booleans trace the boundary of the set | Accepted |
| [0044](0044-one-outline-source-resolves-the-panel-core.md) | One outline source resolves the panel core | Accepted |
| [0045](0045-machining-is-an-ordered-list-of-anchored-operations.md) | Machining is an ordered list of anchored operations | Accepted |
| [0046](0046-machining-compiles-to-staged-effects.md) | Machining compiles to staged effects through one descriptor table | Accepted |
| [0047](0047-icons-are-one-ink-on-a-24-grid.md) | Icons are one ink on a 24 grid, in three families | Accepted; refined by 0055; one-ink clause superseded by 0090 |
| [0048](0048-a-flat-drawing-is-a-path-of-segments.md) | A flat drawing is a path of segments | Accepted |
| [0049](0049-the-viewport-toggles-its-own-projection.md) | The viewport toggles its own projection | Accepted |
| [0050](0050-the-work-plane-follows-the-view.md) | The work plane follows the view | Accepted |
| [0051](0051-snapping-composes-the-grid-the-axis-and-the-draft-ends.md) | Snapping composes the grid, the axis and the draft ends | Accepted |
| [0052](0052-a-drawing-commits-as-one-path-on-the-work-plane.md) | A drawing commits as one path on the work plane | Accepted; parent rule superseded by 0056 |
| [0053](0053-the-evaluation-cache-keys-derivation-inputs.md) | The evaluation cache is keyed by derivation inputs | Accepted |
| [0054](0054-a-primitive-keeps-the-parameters-that-built-it.md) | A primitive keeps the parameters that built it | Accepted |
| [0055](0055-the-ui-rasterises-icons-and-emphasis-is-tonal.md) | The UI rasterises icons itself, and emphasis is tonal | Accepted; one-ink clause superseded by 0090 |
| [0056](0056-creation-tools-land-at-the-scene-root.md) | Creation tools land at the scene root | Accepted |
| [0057](0057-the-camera-reaches-the-poles.md) | The camera reaches the poles | Accepted; roll added by 0078 |
| [0058](0058-a-new-drawing-pivots-on-its-own-shape.md) | A new drawing pivots on its own shape | Accepted |
| [0059](0059-a-selected-line-is-a-screen-space-overlay.md) | A selected line is a screen-space overlay | Accepted |
| [0060](0060-an-open-path-is-edited-by-its-corners.md) | An open path is edited by its corners | Accepted |
| [0061](0061-a-rectangle-outline-bakes-into-a-ring.md) | A rectangle outline bakes into a ring | Accepted |
| [0062](0062-a-large-ring-samples-to-fit-its-cap.md) | A large ring samples to fit its cap | Accepted |
| [0063](0063-one-theme-owns-the-window.md) | One theme owns the window's colour and rhythm | Accepted; light variant superseded by 0069 |
| [0064](0064-the-header-is-chrome-that-returns-an-action.md) | The header is chrome that returns an action | Accepted |
| [0065](0065-visibility-and-lock-are-session-state.md) | Visibility and lock are session state | Accepted |
| [0066](0066-the-ribbon-owns-the-tabs-and-a-tool-is-a-labelled-button.md) | The ribbon owns the tabs, and a tool is a labelled button | Accepted |
| [0067](0067-a-ribbon-tool-is-one-measured-cell.md) | A ribbon tool is one measured cell, and a caption never sizes it | Accepted; cell metrics and caption rule superseded by 0091 |
| [0068](0068-the-workspace-lives-outside-the-document.md) | The workspace lives outside the document | Accepted |
| [0069](0069-the-light-variant-is-a-second-palette.md) | The light variant is a second palette | Accepted |
| [0070](0070-display-styles-are-frame-data.md) | Display styles are frame data | Accepted |
| [0071](0071-a-contact-shadow-is-one-transparent-ground-ellipse.md) | A contact shadow is one transparent ground ellipse | Accepted |
| [0072](0072-a-contour-joint-is-judged-with-the-drawing-tolerance.md) | A contour joint is judged with the drawing tolerance | Accepted |
| [0073](0073-a-drawn-figure-remembers-its-shape.md) | A drawn figure remembers its shape | Accepted |
| [0074](0074-an-extrusion-owns-the-path-it-grew-from.md) | An extrusion owns the path it grew from | Accepted |
| [0075](0075-a-grip-names-what-its-number-means.md) | A grip names what its number means | Accepted |
| [0076](0076-a-drawing-tool-separates-the-segment-kind-from-the-snaps.md) | A drawing tool separates the segment kind from the snaps | Accepted |
| [0077](0077-a-pending-arc-is-the-closing-segment.md) | A pending arc is the closing segment | Accepted |
| [0078](0078-a-view-rolls-about-its-own-axis.md) | A view rolls about its own axis | Accepted |
| [0079](0079-every-layout-remembers-its-own-views.md) | Every layout remembers its own views | Accepted; refit-on-open superseded by 0081 |
| [0080](0080-the-tool-strips-float-over-the-design-space.md) | The tool strips float over the design space | Accepted |
| [0081](0081-one-document-per-tab.md) | One document per tab; the window keeps only what it owns | Accepted |
| [0082](0082-the-local-zero-is-the-creation-anchor.md) | The local zero is the creation anchor | Accepted |
| [0083](0083-a-panel-is-anchored-at-its-base-corner.md) | A panel is anchored at its base corner | Accepted |
| [0084](0084-shared-test-fixtures-behind-a-testing-feature.md) | Shared test fixtures live in the model behind a `testing` feature | Accepted |
| [0085](0085-the-plan-stays-markdown.md) | The plan is markdown, written through one tool — not a database | Accepted |
| [0086](0086-the-interface-speaks-two-languages.md) | The interface speaks two languages, and a generated name keeps the one that made it | Accepted |
| [0087](0087-the-viewports-own-colours-follow-the-theme.md) | The viewport's own colours follow the theme variant; material colours never do | Accepted |
| [0088](0088-origin-and-pivot-are-different-points.md) | Origin and pivot are different points | Accepted; refines 0026 and 0082, settles 0083 |
| [0089](0089-three-palettes-are-data-and-derive-their-states.md) | Three palettes are data: one ramp per variant, derived states, one accent override | Accepted |
| [0090](0090-icons-are-coloured-by-semantic-family.md) | An icon is coloured by its semantic family, and the theme maps the literal | Accepted; supersedes the one-ink clause of 0047 and 0055 |
| [0091](0091-the-ribbon-is-one-const-map.md) | The ribbon is one const map, and one tool is one glyph in a 34 x 30 cell | Accepted; supersedes the cell metrics and the caption rule of 0067; fields removed by its amendment |
| [0092](0092-a-field-is-a-number-with-its-unit-outside.md) | A numeric field is a number with its unit outside, and one edit is one undo entry | Accepted |
| [0093](0093-a-preview-fills-a-session-overlay.md) | A previewing operation fills a session overlay, not the document, and one command bar applies it | Accepted |
| [0094](0094-a-class-registry-gives-the-machine-its-semantics.md) | A class registry gives the machine its semantics; the code identifies, the traits decide | Accepted; supersedes the registry role and top-level grouping of 0035 |
| [0095](0095-the-order-package-is-a-published-snapshot.md) | The order package is a published snapshot, not the internal database | Accepted |
| [0096](0096-before-the-first-release-the-format-has-no-past.md) | Before the first release the data format has no past | Accepted; amends the compatibility clause of `AGENTS.md` §4.5; retires the migration clauses until 1.0 |
| [0097](0097-a-product-is-a-container-with-an-authored-envelope.md) | A product is a container with an authored envelope | Accepted; amends the trait list of 0094 |
| [0098](0098-one-product-is-open-and-parts-are-born-inside-it.md) | One product is open, and furniture parts are born inside it | Accepted |
| [0099](0099-a-niche-is-the-space-a-frame-reserves.md) | A niche is the space a frame reserves | Accepted; refines 0097 |
| [0100](0100-a-frame-is-created-opened-and-drawn.md) | A frame is created, opened and drawn: the lifecycle | Accepted; amends 0097 and 0098 |
| [0101](0101-a-frame-and-a-box-are-one-tool.md) | A frame and a box are one tool: draw the base, pull the height | Accepted; retires the cube primitive |
| [0102](0102-a-furniture-entity-is-a-surface.md) | A furniture entity is a surface; a frame is a shell | Accepted; amends the probe and collision clauses of 0099 and 0097 |
| [0103](0103-one-probe-serves-the-niche-the-distances-and-the-move.md) | One probe serves the niche, the distances and the move | Accepted; refines 0099 and 0102 |
| [0104](0104-a-ghost-of-a-missing-node-is-shell-paint.md) | A ghost of a missing node is shell paint, not the overlay | Accepted; narrows 0093 |
| [0104](0104-supply-names-four-sources.md) | Supply names four sources, and a product is assembled | Accepted; amends the trait list of 0094 |
| [0105](0105-a-panel-stance-is-the-flat-frame-turned.md) | A panel stance is the flat frame turned, never mirrored | Accepted |
| [0106](0106-a-niche-is-a-convex-cell-of-wall-planes.md) | A niche is a convex cell of wall planes | Accepted; refines 0099 and 0103; coverage clause superseded by 0109 |
| [0107](0107-an-end-bevel-is-an-angle-about-its-edge.md) | An end bevel is an angle about its outline edge | Accepted |
| [0108](0108-a-shaped-fill-bevels-its-ends-onto-the-walls.md) | A shaped fill bevels its ends onto the walls, and turns the board when it must | Accepted; refines 0105 and 0107 |
| [0109](0109-a-niche-is-bounded-by-the-faces-the-pointer-sees.md) | A niche is bounded by the faces the pointer sees | Accepted; supersedes the coverage clause of 0106 |
| [0110](0110-one-fill-cell-and-its-build-method.md) | One fill cell, and its build method in the cell's own list | Accepted; refines 0106 and the ribbon list of U74 |

## Format

```text
# NNNN. Title

- Status: Accepted | Superseded by NNNN | Accepted; <aspect> superseded by NNNN
- Date: YYYY-MM-DD

## Context
What forced the decision.

## Decision
What we do, in the present tense.

## Consequences
What it buys, what it costs, and the condition under which it should be revisited.
```

The code and `AGENTS.md` stay the authority for rules; a record keeps the reason.
