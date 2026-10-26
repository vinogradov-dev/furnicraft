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
| [0019](0019-standard-views-from-a-corner-widget.md) | Standard views come from a projected corner orientation widget | Accepted |
| [0020](0020-the-translate-gizmo-previews-and-commits-one-placement.md) | The translate gizmo previews a frame and commits one placement | Accepted |
| [0021](0021-assemblies-are-components-groups-are-containers.md) | Assemblies are components, groups are containers | Accepted; supersedes 0017 |
| [0022](0022-closed-containers-pick-as-one-object.md) | A closed container picks as one object | Accepted |

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
| [0023](0023-fasad-furniture-and-standard-views.md) | Standard views from the furniture facade and updated lighting |
