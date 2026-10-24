# 0009. Contour-first panel geometry

- Status: Accepted; core-local machining superseded by 0011; banding-path unit superseded by 0014
- Date: 2026-10-23

## Context

The first panel model is a rectangle with four stable edges. Included edge banding reduces the core,
but the panel frame and placement were anchored at the core corner. That makes an included bottom or
left band appear outside the nominal rectangle, and it does not generalise to chamfered or rounded
corners, bowed edges, through cutouts that cross the panel boundary, or banding on cutout edges.

The owner confirmed the target behaviour:

- a corner radius that is tangent to the adjoining faces is banded continuously through the radius;
- a chamfer or a non-tangent arc produces a sharp joint and is a separate banding path;
- a bow is a signed arc in the panel plane, outward or inward;
- all through cutouts form one boolean region, as they would on a real board;
- no operation may divide the panel into disconnected pieces.

## Decision

This record supersedes the core-origin placement in ADR 0004 and ADR 0006; their faces, topology ids
and nominal-size formulas remain in force. It refines ADR 0007: the v1 banding order and butt-joint
rules remain, while coordinates are read in the nominal frame.

The panel-local frame is anchored at the **nominal minimum corner**. The nominal rectangle is the
reference for dimensions and placements. Derived panel geometry uses three contours:

- `N` — the nominal outer contour: the target finished outline, including corner features and bows;
- `C` — the core contour: `N` with each included banded path offset inward by the band thickness;
- `O` — the outer envelope: `N` with each non-included banded path offset outward.

A band occupies the region between `C` and `O` along its path. The core is one solid; every band is a
separate solid with its own material. Core and bands are not unioned.

### Banding paths

A banding path is a maximal chain of contour segments between **sharp** joints. A joint is smooth
when the incoming and outgoing tangent directions agree within a documented tolerance; otherwise it
is sharp. Smooth joints are internal to a path; sharp joints end a path.

A path is one banding unit: one material, one thickness, one width and one `included_in_size` flag.
A tangent radius therefore joins its adjacent faces into one path. A chamfer or a non-tangent arc is a
separate path with a butt joint at each end. Two paths meeting at a sharp corner keep the v1 rule:
the later band covers the corner and is trimmed flush with the earlier band.

Parameters are stored on the source segments for compatibility with existing files. The resolver
groups segments into paths and checks that the parameters are compatible. A command applies the
parameters to the whole path; if a geometry edit would merge paths with incompatible banding, the
command is rejected. Loading such a state produces a problem-panel diagnostic, never data loss.

### Corner features and bows

A chamfer is a line segment. A radius is an arc. A bow is a circular arc with a signed sagitta:
positive bows outward from the nominal reference, negative inward. The offset of a line or arc is
computed in the material direction; for an arc the radius changes by the band thickness and the sign
depends on which side contains the material.

A band thickness that consumes a feature, makes an offset self-intersect or makes the core
non-positive is an error or a diagnostic, never a panic.

### Through cutouts

All through cutouts are unioned into one region `K`. The final core region is `C \ K`:

- a cutout inside the core becomes a hole;
- a cutout crossing the boundary becomes an open notch;
- overlapping cutouts merge, with no duplicate internal boundaries;
- a cutout fully outside the panel is a rejected command; a loaded file reports a warning;
- a cutout that would split the core into more than one material component is rejected.

The core material must remain one connected component with positive area. Holes are allowed; an
island of material, a point or zero-width bridge, and an empty panel are not. This invariant applies
to resizing, corner features, bows and cutouts. Loading a file that violates it preserves the data as
a problem panel.

Blind cutouts are not part of the 2D union. They remain 3D pockets. A blind cutout that crosses an
edge creates an open pocket or rabbet and is handled by the 3D kernel later.

### Banding on cutout edges

The same contour and path rules apply to cutout edges: a closed path around a hole, an open path
around a notch. `included_in_size` keeps the nominal opening when true and lets the band protrude
into the opening when false. A path does not automatically continue from the outer contour into a
cutout edge at a boolean intersection; that needs explicit compatible banding.

## Staged implementation

1. Rectangular contour with the nominal frame and the core origin (this decision's first step).
2. Chamfers, radii and bows, plus tangent-continuous paths and arc offsets.
3. Through-cutout union, boundary notches and the connectivity invariant.
4. Banding on cutout edges.
5. Blind cutouts and future 3D edge profiles.

The first step does not add a new crate or dependency. The later contour work was to need a robust 2D
line/arc offset and boolean layer; offsets landed in stage 2 as analytic contour code (ADR 0010), and
the boolean layer lands in stage 3 as the in-house `geom::region` module (ADR 0012), still with no new
crate or dependency.

## Consequences

- `Placement` anchors the nominal minimum corner. An included band moves the core inward instead of
  moving the whole panel outward.
- Machining stays core-local; the parametric layer translates it by the core origin.
- Overall size is derived from the finished contour, not from a fixed core corner. With corner
  features and bows it may not be a simple sum of lengths.
- The current v1 restrictions on cutouts outside the core and overlapping cutouts are temporary;
  stage 3 replaces them with the union and connectivity rules above.
- Existing rectangle files load unchanged: no corner features, no bows, four sharp corners.
