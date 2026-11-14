# Surfaces

One row per thing the person can see or touch. A new surface, gesture or shortcut gets its row in
the same change that adds it; a row naming a file that does not exist is a defect the reader fixes.
This table is the skill's knowledge of the product — the ADR numbers are why, the paths are where.

| Surface | Owner | Decisions |
|---|---|---|
| Window, title, tabs, header | `apps/furnicraft/src/main.rs`, `header.rs`, `tabs.rs` | ADR 0081 (one document per tab), 0064 (the header returns one action) |
| Ribbon band and its tools | `crates/ui/src/toolbars.rs`, `toolbars/{spec,list,state,ready}.rs`, `theme.rs` (`RIBBON_*`, `ribbon`), `icons.rs` (`ribbon_cell`, `ribbon_split_cell`) | ADR 0066 (tabs and labelled tools), 0067 (one measured cell), 0091 (one const map, amended: a split cell is a full cell plus its caret), X36; U74 — a tool with variants is a split cell: the main face runs the pinned method, the caret face beside it opens the list, and the pin is drawn only on the hot row |
| Floating side panels (scene tree, inspector) | `apps/furnicraft/src/side_panels.rs` (`Panels`, `PanelRects`, `Chrome`), `crates/ui/src/settings.rs` (`PanelsSettings`) | ADR 0080 (the strips float over the design space), U66/U72 |
| Scene tree and its cards | `crates/ui/src/scene_tree.rs`, `panel_card.rs`, `object_card.rs`, `container_card.rs`, `placement_card.rs`, `pivot_card.rs`, `properties_card.rs`, `camera_card.rs`, `light_card.rs`, `machining/` | ADR 0034, 0035 (typed properties, furniture types), 0092 (a field is a number with its unit outside); a product row opens the working product by a click, and the open one reads strong |
| Product-frame card (three sides + type) | `crates/ui/src/product_card.rs`, `class_picker.rs` (`product_rows`), `apps/furnicraft/src/main.rs` (`product_frame_card`) | ADR 0097, 0098, 0100 — the ribbon's *Product frame* cell opens it; three whole-millimetre sides and the registry's product classes behind one search field; the draft becomes `product.create` and the new frame is opened as a session operation; the cell's second method, *Frame by drawing*, arms the box tool with the envelope as its owner (ADR 0101) |
| View cluster (bottom centre) | `apps/furnicraft/src/view_cluster.rs`, `crates/ui/src/display.rs` | ADR 0093 (the cluster lifts for the command bar), 0079 (each layout remembers its views) |
| Split views, dividers, orientation widget | `apps/furnicraft/src/view_layout.rs`, `strip.rs` | ADR 0079, 0080 |
| Command bar (label, field, ✓, ✕) | `crates/ui/src/command_bar.rs`, `crates/ui/src/scrub.rs`, `apps/furnicraft/src/document.rs` (`show_command_bar`, `command_bar_rect`) | ADR 0093 (a preview fills a session overlay) |
| Status bar (three zones) | `apps/furnicraft/src/status_bar.rs`, `document.rs` (`status_info`, `apply_status`) | U64, `Document::busy`, `SLOW_REBUILD` |
| Command palette (`Ctrl+K`) | `crates/ui/src/palette.rs`, `apps/furnicraft/src/shortcuts.rs` | one search field over `CommandRegistry::specs()`; history is shell state |
| Settings window and the workspace file | `crates/ui/src/settings.rs`, `apps/furnicraft/src/settings_store.rs` | ADR 0068 (workspace lives outside the document), 0089 (palettes are data) |
| Home surface | `crates/ui/src/home.rs` | ADR 0068 |
| Theme, palettes, metrics, type scale | `crates/ui/src/theme.rs` | ADR 0063, 0069, 0087 (viewport colours follow the theme), 0089 |
| Icons | `crates/ui/src/icons.rs`, `assets/README.md`, `assets/icons/STYLE_GUIDE.md`, `assets/icons/<theme>/` | ADR 0055 (the UI rasterises icons), 0090 (families carry the colour) |
| Text and language | `crates/text/src/messages.rs`, `crates/ui/src/settings.rs` | ADR 0086 (one typed message per string) |
| Numeric field (scrub) | `crates/ui/src/scrub.rs`, `ui_scale.rs` | ADR 0092 |
| Drawing draft and its preview | `crates/ui/src/draw.rs`, `apps/furnicraft/src/draw_gizmo.rs` | ADR 0048, 0052, 0076, 0077 |
| Grips on a contour | `crates/ui/src/contour_grips.rs`, `apps/furnicraft/src/contour_gizmo.rs` | ADR 0075 |
| Gizmos: move, rotate, extrude arrow, vertex | `apps/furnicraft/src/{translate,rotate,extrude,vertex}_gizmo.rs`, `gizmo.rs`, `gizmo_math.rs` | ADR 0024, 0026, 0027, 0088 |
| Scene icons (camera, light) and picking | `apps/furnicraft/src/scene_gizmo.rs`, `mesh_pick.rs` | ADR 0036 (cameras and lights are scene objects), 0018 (picking ray-casts) |
| Viewport display styles, shadows, edges | `crates/ui/src/display.rs`, `crates/scene/src/lib.rs`, `crates/render/` | ADR 0059, 0070, 0071, 0087 |
| Keymap | `apps/furnicraft/src/shortcuts.rs`, `crates/ui/src/editor/ribbon.rs` | keys are shell state; no shortcut may shadow a text field |

## What the person is doing

Measuring a cabinet, choosing a thickness, setting a material and its grain, banding an edge,
drilling a hole, checking a shelf fits, moving a door on its pivot. Every screen exists for one of
those, and a control that serves none of them is decoration.

## Vocabulary the UI uses

Node kinds: container, component, group, panel, path, body, primitive, camera, light. Panel faces
`A`/`B`; vertices and edges have stable ids; a pivot is not an origin. A "sheet" is a material sheet,
a "part" is a panel or a body, a "band" is edge banding. The canonical definitions live in
`PROJECT.md` and `docs/domain/`; when this list disagrees with them, they win.
