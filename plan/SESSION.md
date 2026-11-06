# Session

```text
task: U27
goal: a drawn toolbar icon takes the theme's colour, at 20 pt for tools and 16 pt inline

steps:
  [x] crates/ui/src/icons.rs rasterises the embedded master at pt × pixels_per_point and paints it in the theme colour
  [x] the masters carry white ink and the format docs say so
  [x] the pixel test draws one icon in both themes, asserts the theme colour, one alpha silhouette and the raster size
  [ ] cargo xtask ci is green and the change is committed

now: run cargo xtask ci, then commit and close U27 in the plan
blocked: nothing
run: cargo xtask ci
resume: run cargo xtask ci; fix the first failing step, then commit
map: plan/map.md — UI icon resources; crates/ui/src/icons.rs (ADR 0055)
decisions: the tint is baked into the uploaded pixels so the cache key is (uri, px, colour) and the test can inspect them; Painter::image adds no tint
```
