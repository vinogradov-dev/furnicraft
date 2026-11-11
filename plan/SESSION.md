# Session

```text
task: U50
goal: the panel header loses the count and gains a scene-side fold: the digit next to *Scene* and *Inspector* disappears — the tree and the card already say what they hold, and the number cost a document scan on every rebuild — and the fold chevron moves to the edge facing the viewport, so the left panel folds from its right and the right panel from its left. The collapsed strip keeps its icon and turned title, and the click, edge-double-click and outward-drag restore rules from U40 stay. Tests: neither header draws a count; a left panel's chevron sits at its right edge and a right panel's at its left; folding and restoring keeps the expanded width. Session notes: `panel_header` and `count_chip` in `apps/furnicraft/src/main.rs` draw the current row; `node_count` is cached only for that chip. `PanelSide` already names which edge a panel sits on, so the chevron side follows from it.

steps:
  [x] starting the task
  [x] the header loses the chip and the chevron follows the side

now: starting the task
blocked: nothing
run: cargo xtask task U50
resume: read the first `files:` path and its callers
```
