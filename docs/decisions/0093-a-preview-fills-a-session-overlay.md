# 0093. A previewing operation fills a session overlay, not the document

- Status: Accepted
- Date: 2026-11-12

## Context

An operation that shows its result before it commits — the extrusion pull today, a boolean later —
has to answer one question first: where does the result live while the user is still deciding?

Writing it into the document and undoing it on cancel is the short path, and it is wrong twice. Every
frame of a drag would dispatch a command, so `Session::dispatch` would push one undo entry per frame
and the file would be dirty before the user accepted anything; a cancel would then be a *second* edit,
and the history would hold a command the user never ran. The alternative — a preview the viewport
draws beside the document — costs one new piece of session state and keeps the history honest: the
one command behind the ✓ is the only thing undo can see.

`plan/questions.md` asked exactly this, and the answer decides what the command bar's check and cross
mean, and what an assistant or a CLI would have to call.

## Decision

**The overlay is session state.** `furnicraft_engine::Preview` maps a node id to the geometry that node
would have, and `Session` holds one. `Session::set_preview_geometry` and `Session::clear_preview` are
the only ways in; neither touches `Project::revision`, the undo stack or the dirty marker, and nothing
in the overlay is ever serialised. `Session::drawn_geometry(id)` is what the viewport asks for, and it
answers with the overlay's entry when there is one and the document's own geometry otherwise.

**The frame follows the overlay like it follows the revision.** The overlay carries a version that
moves on every change, and the shell compares it with the version its uploaded frame was built from,
exactly as it compares the document revision and the editor's visibility revision. A preview
therefore costs a scene rebuild — the previewed node is re-derived through the same evaluator the
document uses, the untouched nodes come from the mesh cache — and nothing else. `EvaluationCache::
object_with` takes the geometry it is handed for this reason: one node id cannot cache two payloads,
so a preview and the document each re-derive the node once when they take the entry over.

**One command bar, one ✓.** A previewing operation draws the bar at the viewport's bottom centre: the
command's own label, the shared 76 pt field with its unit outside, ✓ (Enter) and ✕ (Escape). The field
drives the preview live, ✓ dispatches the operation's single command and drops the overlay, ✕ drops it
and re-evaluates the document. Applying is idempotent because the operation is: the pull closes on the
first ✓, so a second click, a double click or a key and a button in the same frame find no pull and
dispatch nothing.

**A slow rebuild is announced before it runs.** Evaluation is synchronous, so the frame that starts it
cannot paint anything honest about it. `Document` times every rebuild; a scene whose last rebuild
overran 250 ms waits one frame — the frame that paints «Перестроение…» and keeps the last good
geometry — and the work runs on the next, so the window answers input in between. A scene that
rebuilds inside the budget never shows the message.

## Consequences

- The extrusion pull previews the body itself at the pulled height instead of a wireframe stand-in:
  the user accepts the solid they saw, and the preview is the same evaluator the ✓ stores.
- The command bar is a `crates/ui` widget because it reuses `Scrub`; the shell places it and hands it
  the previewing operation's label, value, unit and range. The bar's rectangle is chrome: a press on
  it is never a pick.
- The view cluster moves up by the bar's height while a bar is up, so the two never overlap.
- A boolean, a fillet or any future feature operation plugs into the same overlay: it computes the
  geometry the node would have and sets one entry; the bar and the ✓ are already there. The ribbon's
  boolean tools stay `planned` until the operation itself exists.
- Revisit if a preview ever needs to change a *placement* rather than geometry: the gizmos already
  preview placements in the shell (`gizmo_math::Preview`) and the overlay deliberately holds geometry
  only, so one rule stays one rule.
