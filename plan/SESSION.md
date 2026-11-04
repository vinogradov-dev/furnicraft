# Session

```text
task: U24
goal: one icon contract — the style guide, the file format and the agent read trigger agree.

steps:
  [x] review the guide against assets/README.md and the resvg/egui pipeline
  [x] owner decisions: one ink, 24 grid, actions theme
  [x] rewrite STYLE_GUIDE.md: one ink, 24 grid, three families, service section
  [x] update assets/README.md, add the actions placeholders and ADR 0047
  [x] add AGENTS §4.12, the draw-icon skill and the map row
  [ ] run the gate and close the task

now: cargo xtask ci, then review the staged diff
blocked: nothing
run: cargo xtask ci
resume: cargo xtask ci, then fix the first failure
decisions: one ink + theme tint; 24x24 grid, ink 2..22; actions/ theme; guide owns meaning, README format
```
