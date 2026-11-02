# 0041. Every solid compiles to one domain-free plan

- Status: Accepted
- Date: 2026-11-02

## Context

Two body-building paths had grown side by side. A constructive object was evaluated by a bespoke
analytic mesh builder, while a panel called the geometry kernel directly: it extruded a core, then
extruded and subtracted one tool per cutout, and repeated the same call for every band strip. The
two paths shared a shape of reasoning but not a value, so a new operation had to be written twice —
once for the mesh path and once for the kernel path — and every caller had to know the order in
which kernel calls compose.

This sits on the wrong side of the layer boundary. `param` already owns evaluation, but the
feature-to-body translation was spread across panel helpers, and the assistant track will need the
same translation for hardware and for operations it proposes. The document must keep describing
features; only evaluation should know how those features become solids.

## Decision

- `furnicraft-param::solid` defines the one derived description of a solid: `SolidPlan` is a base
  `Extrude` followed by boolean steps, and each step owns a nested `SolidPlan` as its tool.
- A plan is built through `PlanBuilder`, is never serialized, and is rebuilt from the document when
  a feature changes. The document remains the source of truth.
- `SolidPlan` knows profiles, frames and distances in millimetres. It imports `furnicraft-geom` and
  `furnicraft-kernel` and never `furnicraft-model`; a caller converts `Length` to millimetres at
  the boundary.
- `PlanBackend` is the extension point. `evaluate_with` owns the fold, so the same plan drives the
  geometry kernel (`KernelBackend`) and the analytic single-extrusion backend
  (`constructive::MeshBackend`) without a second traversal.
- The analytic backend answers exactly one extrusion, which is exact as polygons, and refuses a
  boolean. That refusal is an explicit `KernelError::Unsupported`; a plan with a boolean must go
  through the kernel.
- `object.collapse`, the viewport's constructive mesh, the panel core, every cutout tool and every
  band strip now build a plan and evaluate it. The panel compiler no longer calls
  `SolidKernel::extrude` or `SolidKernel::boolean`.
- Plan nesting is capped at `MAX_PLAN_DEPTH`; a broken graph is a readable error, never unbounded
  recursion.

## Consequences

- A new panel operation adds steps to a plan instead of a new kernel call site. The registry and
  the operation pipeline (D6) compile machining effects into these plans.
- The analytic path stays the single definition of the collapsed mesh, so collapse cannot drift
  from what the viewport showed.
- The plan has no attachment concept yet: a band is an independent body with its own plan. The
  operation pipeline will add an attachment effect when banding becomes an operation, and the
  reserved post/deform stage will carry a future bend.
- Two backends must agree on the meaning of one extrusion. The analytic backend accepts only a
  world-aligned frame and a profile without holes; anything else is refused rather than guessed.
