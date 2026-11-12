//! The floating tool strip's band and the view rectangles it feeds.
//!
//! [`strip_rect`] reserves the band the ribbon covers at the top of the design space,
//! [`strip_area`] draws the strip into it on the middle layer, [`uncovered`] keeps the orientation
//! widget out from under it, and [`view_region`] converts one view's point rectangle into the pixel
//! rectangle the renderer draws that view into.
//!
//! The band is in egui points and the region is rounded to whole physical pixels from
//! `points_per_pixel`, each edge separately, so two views that share an edge in points share exactly
//! one edge in pixels. A view narrower than half a pixel still reaches one pixel; nothing here reads
//! the document.
//!
//! Changes when: where the strip sits, how much of the design space it owns, or how a view maps to
//! the render target changes.

use eframe::egui;

/// The band a floating tool strip covers at the top of `design`.
///
/// The strip spans the design space's width and sits at its top edge; the height is the one the
/// strip's own state fixes ([`furnicraft_ui::toolbars::height`]) or the rectangle it measured on the
/// previous frame.
pub(crate) fn strip_rect(design: egui::Rect, height: f32) -> egui::Rect {
    egui::Rect::from_min_size(design.min, egui::vec2(design.width().max(0.0), height.max(0.0)))
}

/// Draws a tool strip over the band `band` as a floating overlay and returns the rectangle it covered.
///
/// The strip spans the design space's width — the window's width — and sits on the middle layer, so
/// the rendered scene runs under it and a translucent fill blends with the furniture (U51, ADR 0080).
/// The band is the strip's own: the whole of it is chrome, so the pointer over any part of it — a
/// tool, a gap, a margin — belongs to the ribbon and never to the view behind it. A hover-only area
/// left the gaps transparent to the viewport, which let a drawing tool's rubber band follow the
/// pointer onto the toolbar (X23). `None` is egui's sizing pass, where the content is measured but
/// not shown.
pub(crate) fn strip_area(
    ctx: &egui::Context,
    id: &str,
    band: egui::Rect,
    fill: egui::Color32,
    add: impl FnOnce(&mut egui::Ui),
) -> Option<egui::Rect> {
    let [margin_x, margin_y] = furnicraft_ui::toolbars::RIBBON_FRAME_MARGIN;
    let width = band.width().max(0.0);
    let height = band.height().max(0.0);
    let mut sizing = false;
    let response =
        egui::Area::new(egui::Id::unique(id)).order(egui::Order::Middle).fixed_pos(band.min).show(ctx, |ui| {
            sizing = ui.is_sizing_pass();
            // Registered before the tools so the buttons drawn after it stay on top and keep their
            // clicks, and on the sizing pass not at all: that pass shows nothing the pointer can
            // hit. It senses a drag as well as a click because the viewport behind it does: a widget
            // that only senses clicks leaves a drag-sensing widget under it hovered, and the drawing
            // tools read the hover, not the click.
            if !sizing {
                ui.interact(band, egui::Id::unique((id, "chrome")), egui::Sense::click_and_drag());
            }
            egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(margin_x, margin_y)).show(ui, |ui| {
                ui.set_width((width - 2.0 * f32::from(margin_x)).max(0.0));
                ui.set_min_height((height - 2.0 * f32::from(margin_y)).max(0.0));
                add(ui);
            });
        });
    (!sizing).then_some(response.response.rect)
}

/// The part of `view` the floating strip leaves uncovered: the view below the band `strip` covers.
///
/// A view the strip does not reach — a bottom-row leaf of a split, or a strip that has not been
/// measured yet — is returned unchanged, so only an overlay that really covers the view shrinks it.
pub(crate) fn uncovered(view: egui::Rect, strip: egui::Rect) -> egui::Rect {
    let covered = (strip.bottom() - view.top()).clamp(0.0, view.height());
    egui::Rect::from_min_max(egui::pos2(view.left(), view.top() + covered), view.max)
}

/// The physical pixels one view's rectangle covers inside the workspace.
///
/// The view rect is a point rectangle in the central panel; the target starts at the panel's
/// top-left. Both edges are rounded separately and the size is the difference, so two neighbours
/// that share an edge in points also share exactly one edge in pixels instead of overlapping or
/// leaving a seam. A view narrower than half a pixel still reaches one pixel.
pub(crate) fn view_region(area: egui::Rect, view: egui::Rect, points_per_pixel: f32) -> furnicraft_render::ViewRegion {
    let scale = |value: f32| (value * points_per_pixel).round().max(0.0) as u32;
    let left = scale(view.left() - area.left());
    let top = scale(view.top() - area.top());
    let right = scale(view.right() - area.left());
    let bottom = scale(view.bottom() - area.top());
    furnicraft_render::ViewRegion {
        x: left,
        y: top,
        width: right.saturating_sub(left).max(1),
        height: bottom.saturating_sub(top).max(1),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use furnicraft_scene::Camera;
    use furnicraft_ui::display::{OpeningView, ShadingStyle};
    use furnicraft_ui::theme;

    use super::*;

    #[test]
    fn one_view_region_covers_the_whole_target_and_four_tile_it() {
        let area = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1000.0, 600.0));
        let whole = view_region(area, area, 1.0);
        assert_eq!(whole, furnicraft_render::ViewRegion { x: 0, y: 0, width: 1000, height: 600 });

        // Two columns and two rows at the default ratio, converted at 2 physical pixels per point.
        let half = egui::vec2(500.0, 300.0);
        let quadrants = [
            egui::Rect::from_min_size(area.min, half),
            egui::Rect::from_min_size(area.min + egui::vec2(0.0, 300.0), half),
            egui::Rect::from_min_size(area.min + egui::vec2(500.0, 0.0), half),
            egui::Rect::from_min_size(area.min + egui::vec2(500.0, 300.0), half),
        ];
        let regions: Vec<_> = quadrants.iter().map(|quadrant| view_region(area, *quadrant, 2.0)).collect();
        assert_eq!(
            regions,
            vec![
                furnicraft_render::ViewRegion { x: 0, y: 0, width: 1000, height: 600 },
                furnicraft_render::ViewRegion { x: 0, y: 600, width: 1000, height: 600 },
                furnicraft_render::ViewRegion { x: 1000, y: 0, width: 1000, height: 600 },
                furnicraft_render::ViewRegion { x: 1000, y: 600, width: 1000, height: 600 },
            ]
        );
        let covered: u32 = regions.iter().map(|region| region.width * region.height).sum();
        assert_eq!(covered as f32, 1000.0 * 600.0 * 4.0, "four views cover the target exactly");
    }

    #[test]
    fn a_view_is_never_a_zero_pixel_region() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(10.0, 10.0));
        let sliver = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(0.1, 0.1));
        let region = view_region(area, sliver, 0.5);
        assert_eq!((region.width, region.height), (1, 1), "a rounding edge still reaches one pixel");
    }

    #[test]
    fn adjacent_view_regions_share_one_edge_without_overlapping() {
        // The point rectangles tile the area, but each region is rounded from its own rect. Both
        // sides compute the same corner from the same number, so they meet exactly; the widths are
        // rounded separately, so the pair can leave at most one pixel for the background clear.
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(801.0, 601.0));
        let x = area.left() + area.width() * 0.37;
        let left = egui::Rect::from_min_max(area.min, egui::pos2(x, area.bottom()));
        let right = egui::Rect::from_min_max(egui::pos2(x, area.top()), area.max);
        for scale in [1.0, 1.25, 2.0] {
            let left = view_region(area, left, scale);
            let right = view_region(area, right, scale);
            let whole = view_region(area, area, scale);
            assert_eq!(left.x, 0);
            assert_eq!(left.y, 0);
            assert_eq!(right.x, left.x + left.width, "the right view starts where the left one ends");
            assert_eq!(right.x + right.width, whole.width, "the pair covers the target exactly");
            assert!(left.width >= 1 && right.width >= 1);
        }
    }

    #[test]
    fn a_pointer_in_a_view_reaches_the_camera_drawn_under_it() {
        // The shell tests the pointer against top-left rectangles and the renderer scissors each
        // view from the same rectangle unchanged. A flip on one side alone would draw one camera
        // under another view's pointer, so this pins the two together.
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut layout =
            crate::view_layout::Layout::new(Camera::default(), OpeningView::Axonometric, ShadingStyle::default());
        layout.set_kind(crate::view_layout::LayoutKind::Quad);
        let regions = layout.regions(area);
        assert_eq!(regions.len(), 4);

        for (leaf, view) in &regions {
            let region = view_region(area, *view, 1.0);
            // The pointer at the centre of this view resolves to this leaf.
            let pointer = view.center();
            let hits = regions.iter().find(|(_, rect)| rect.contains(pointer)).map(|(leaf, _)| *leaf);
            assert_eq!(hits, Some(*leaf), "the pointer over {view:?} names that view");

            // And the scissor of this leaf covers the pixel under that pointer.
            let pixel = (pointer.x as u32, pointer.y as u32);
            let (x, y, width, height) = (region.x, region.y, region.width, region.height);
            assert!(
                (x..x + width).contains(&pixel.0) && (y..y + height).contains(&pixel.1),
                "the camera under {pointer:?} is drawn inside {x},{y} {width}×{height}"
            );
        }

        // The top half of the workspace holds the top row of views, in both coordinate systems.
        let top_left = egui::pos2(200.0, 100.0);
        let (leaf, view) = regions
            .iter()
            .find(|(_, rect)| rect.contains(top_left))
            .map(|(leaf, rect)| (*leaf, *rect))
            .expect("a view under the pointer");
        let region = view_region(area, view, 1.0);
        assert_eq!((region.x, region.y), (0, 0), "the top-left view is drawn in the target's top-left");
        assert_eq!(layout.preset(leaf), Some(OpeningView::Front), "the top-left view is the standard front elevation");
        assert_eq!(layout.leaf(leaf).map(|leaf| leaf.camera().target()), Some(Camera::default().target()));
    }

    /// Runs a fake shell layout: a header panel, the design space it leaves, then a side panel of
    /// `panel_width`, and returns the design space, the viewport it leaves and the strip band.
    fn strip_layout(ctx: &egui::Context, panel_width: f32, open: bool) -> (egui::Rect, egui::Rect, egui::Rect) {
        let mut design = egui::Rect::ZERO;
        let mut viewport = egui::Rect::ZERO;
        let mut strip = egui::Rect::ZERO;
        let input = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 840.0))),
            ..Default::default()
        };
        let output = ctx.run_ui(input(), |ui| {
            egui::Panel::top("header").show(ui, |ui| {
                ui.set_height(theme::HEADER_HEIGHT);
            });
            design = ui.available_rect_before_wrap();
            strip = strip_rect(design, furnicraft_ui::toolbars::height(open));
            egui::Panel::left("side").exact_size(panel_width).show(ui, |_ui| {});
            viewport = ui.available_rect_before_wrap();
        });
        output.drop_without_applying_deltas();
        (design, viewport, strip)
    }

    #[test]
    fn resizing_a_side_panel_never_reflows_the_floating_ribbon() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let (_, narrow_viewport, narrow_strip) = strip_layout(&ctx, 200.0, true);
        let (_, wide_viewport, wide_strip) = strip_layout(&ctx, 320.0, true);
        assert_ne!(narrow_viewport, wide_viewport, "the side panel did change the viewport");
        assert_eq!(narrow_strip, wide_strip, "the strip is anchored to the window, not to the viewport");
        assert!((narrow_strip.width() - 1280.0).abs() < 0.5, "the strip spans the window: {narrow_strip:?}");
        assert!(
            (narrow_viewport.top() - narrow_strip.top()).abs() < 0.5,
            "the viewport starts at the strip's top, so the scene runs under it"
        );
        assert!(narrow_viewport.intersects(narrow_strip), "the strip covers a part of the rendered scene");
    }

    #[test]
    fn a_click_inside_the_ribbon_does_not_reach_the_viewport() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let design = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let height = furnicraft_ui::toolbars::height(true);
        // One hover, press and release at `pos`; returns whether the viewport and the strip's button
        // took the click.
        let click_at = |pos: egui::Pos2| -> (bool, bool) {
            let press = |pressed: bool| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let mut result = (false, false);
            for events in [
                // The first frame is egui's sizing pass for the strip, so the pointer needs one more
                // frame before the press can land on it.
                vec![egui::Event::PointerMoved(pos)],
                vec![egui::Event::PointerMoved(pos)],
                vec![press(true)],
                vec![press(false)],
            ] {
                let input = egui::RawInput { screen_rect: Some(design), events, ..Default::default() };
                let output = ctx.run_ui(input, |ui| {
                    // The viewport stand-in: the whole design space, under the strip.
                    let (_, response) = ui.allocate_exact_size(design.size(), egui::Sense::click());
                    result.0 |= response.clicked();
                    let band = strip_rect(design, height);
                    strip_area(ui.ctx(), "ribbon_click_test", band, theme::surface(ui), |ui| {
                        let (_, response) = ui.allocate_exact_size(egui::vec2(200.0, 20.0), egui::Sense::click());
                        result.1 |= response.clicked();
                    });
                });
                output.drop_without_applying_deltas();
            }
            result
        };
        // A click on the ribbon's empty chrome: the strip is above the viewport and takes the click.
        let (viewport, button) = click_at(egui::pos2(500.0, design.top() + 10.0));
        assert!(!viewport, "a click on the strip must not reach the viewport behind it");
        assert!(!button, "the click landed on the strip's chrome, not on its button");
        // The same gesture below the strip still selects in the viewport.
        let (viewport, _) = click_at(egui::pos2(500.0, design.top() + height + 100.0));
        assert!(viewport, "the viewport keeps the clicks the strip does not cover");
    }

    #[test]
    fn a_pointer_over_the_ribbon_band_does_not_hover_the_viewport() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let design = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let height = furnicraft_ui::toolbars::height(true);
        let band = strip_rect(design, height);
        // Hovers `pos` for a few frames: the first frame sizes the strip, and egui resolves the
        // hover against the previous frame's layers. Returns whether the viewport took the hover.
        let hover_at = |pos: egui::Pos2| -> bool {
            let mut hovered = false;
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 840.0))),
                    events: vec![egui::Event::PointerMoved(pos)],
                    ..Default::default()
                };
                let output = ctx.run_ui(input, |ui| {
                    // The viewport stand-in: the whole design space, under the strip. It senses a
                    // drag like the real one, which is what makes the strip's band the harder case.
                    let (_, response) = ui.allocate_exact_size(design.size(), egui::Sense::click_and_drag());
                    hovered = response.hovered();
                    // The strip draws one small tool and leaves the rest of its band empty.
                    strip_area(ui.ctx(), "ribbon_hover_test", band, theme::surface(ui), |ui| {
                        ui.allocate_exact_size(egui::vec2(200.0, 20.0), egui::Sense::click());
                    });
                });
                output.drop_without_applying_deltas();
            }
            hovered
        };
        // A point on the strip's empty chrome: the whole band is the strip's, so the viewport behind
        // it takes neither the hover nor the rubber band that follows it (X23).
        assert!(
            !hover_at(egui::pos2(500.0, band.bottom() - 1.0)),
            "the strip's own band must not hover the viewport behind it"
        );
        // The same gesture below the strip still hovers the viewport.
        assert!(
            hover_at(egui::pos2(500.0, band.bottom() + 100.0)),
            "the viewport keeps the pointer the strip does not cover"
        );
    }

    #[test]
    fn the_strip_overlay_fills_the_design_width_and_its_reserved_height() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let design = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(1000.0, 600.0));
        let height = furnicraft_ui::toolbars::height(true);
        let mut rect = None;
        // The first frame is egui's sizing pass for the area; the second reports the real rectangle.
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 640.0))),
                ..Default::default()
            };
            let output = ctx.run_ui(input, |ui| {
                let band = strip_rect(design, height);
                rect = strip_area(ui.ctx(), "ribbon_size_test", band, theme::surface(ui), |_ui| {});
            });
            output.drop_without_applying_deltas();
        }
        let rect = rect.expect("a measured strip");
        assert!((rect.width() - design.width()).abs() < 0.5, "the strip fills the design width: {rect:?}");
        assert!((rect.height() - height).abs() < 0.5, "the strip is its reserved height: {rect:?}");
        assert!((rect.min - design.min).length() < 0.5, "the strip is anchored to the design's top-left");
    }

    #[test]
    fn framing_keeps_the_scene_inside_the_uncovered_view() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(800.0, 600.0));
        let design = egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(800.0, 700.0));
        let strip = strip_rect(design, 130.0);
        let visible = uncovered(view, strip);
        assert!((visible.top() - (view.top() + 130.0)).abs() < 0.5, "the band is cut off the top");
        assert!((visible.height() - (view.height() - 130.0)).abs() < 0.5);
        // A view the strip does not reach keeps its whole height.
        let bottom = egui::Rect::from_min_size(egui::pos2(0.0, 400.0), egui::vec2(800.0, 300.0));
        assert_eq!(uncovered(bottom, strip), bottom);
    }
}
