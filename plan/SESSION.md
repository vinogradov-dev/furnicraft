# Session

```text
task: X8
goal: the scene stays dark in the light theme: with the workspace on *Light*, the viewport still clears to the dark chrome, the floor grid keeps its light-on-dark lines, and the rendered frame reads as a dark hole in a light window. Expectation: a theme change repaints the scene background, the grid and the non-material lines to the active variant at once, leaving the furniture's own material colours untouched, and switching back restores the dark values. Suspected cause: the frame background and the grid line colours are literals in `crates/scene`, and the shipped frame never gets them from the active `egui::Visuals` when `theme::install_with` changes. Tests: a light frame carries the light background and grid; switching variant changes the uploaded scene without touching the document revision.

steps:
  [x] starting the task
  [x] the theme inks, tests and note

now: starting the task
blocked: nothing
run: cargo xtask task X8
resume: read the first `files:` path and its callers
```
