# Session

```text
task: U14
goal: A drawing tool can build a chain of plane points in the editor, undo the last point, close or cancel it, and the document never changes until commit.

steps:
  [ ] the draft type: points, closing, the ring cap and `to_path`
  [ ] `Editor` owns the draft and exposes push, undo, close and cancel
  [ ] tests for the chain, undo, closing, the cap and the untouched document
  [ ] the full gate and the closing commit

now: `crates/ui/src/contour_edit.rs`, adding `DrawDraft`
blocked: nothing
run: cargo test -p furnicraft-ui
resume: open `crates/ui/src/contour_edit.rs` after `ContourEdit::apply` and add `DrawDraft`
map: the "Contour editing" row, because the draft is the same view-state pattern
decisions: the draft stores whole-micrometre work-plane points, so no float enters the document; undo reopens a closed draft first, because closing is the last action; a close needs three points and the chain is capped at the ring size; the draft converts to a `Path` but never dispatches a command — U17 owns the commit
```
