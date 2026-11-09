# 0080. The tool strips float over the design space

- Status: Accepted
- Date: 2026-11-09

## Context

The ribbon was a `Panel::top` between the header and the side panels. A panel takes height from the
central area, so the scene image could never start at the top of the design space: the grid and the
model stopped below a solid band of chrome, and the band's opacity setting — *Ribbon → Background
opacity* — blended the ribbon with the window background, because that was the only thing under it.
The band also owned a real layout edge, so a future strip would have had to negotiate its height with
every other panel.

The viewport is a rendered texture under an egui surface. egui already lets a surface float over it
on the middle layer: the viewport cluster does exactly that, pinned to the viewport's bottom centre.

## Decision

- A tool strip is an overlay on `Order::Middle`, not a panel. The ribbon is `strip_area` in the shell:
  an `egui::Area` at the top-left of the design space — the window below the header — as wide as the
  design space, so the scene texture is what a translucent strip blends with (U51).
- The strip spans the design rect, not the central rect, so resizing a side panel never changes its
  anchor or its width and never reflows its cells. The side panels are inset by the strip's height
  instead, so their own header never starts under it.
- The strip's height is deterministic from its state (`toolbars::height(open)`) and the shell reserves
  that band: the central area, the side-panel inset and the framing all read the same number. The
  drawn rectangle wins once it is measured, because a narrow window grows the body with its scroll
  bar; the first frame of an area is egui's sizing pass and is not visible.
- The render target and the pick rectangle keep covering the whole central rect, so the scene runs
  under the strip. The strip is above the viewport layer, so a click on it never reaches the viewport
  and orbit, pan and zoom keep working outside it.
- What must stay readable moves into the uncovered part of the view: the orientation widget is placed
  there, and *Frame all* fits the scene into the part of the active view below the strip — the fit
  distance is scaled by the covered share and the target is panned half the covered height, so the
  model is centred in what the user can see rather than behind the chrome.
- The header and the status bar stay panels: they are window edges, not tools, and nothing is drawn
  under them.

## Consequences

The design space starts at the header and the scene image runs under the ribbon; a translucent ribbon
blends with the furniture, and the side panels no longer share an edge with the ribbon. A new strip
inherits the same policy by calling `strip_area` and declaring its height.

The cost is that the ribbon's height is a number the shell and the ribbon must agree on: it is
`toolbars::height(open)` in the UI crate and a test draws the ribbon to prove the reserved band and
the drawn band are the same. The side panels start one strip lower than the header, and opening the
ribbon body reflows them vertically — a price the design accepts for a strip that never jumps
sideways.
