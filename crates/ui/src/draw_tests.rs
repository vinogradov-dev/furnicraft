//! Tests for the drawing draft: each tool's grammar, the closing rule and the refusals.
//!
//! Changes when: a drawing tool, its point grammar or the closing rule changes.

#![allow(clippy::expect_used)]

use furnicraft_testkit::millimetres;

use super::*;

#[test]
fn a_drawing_draft_builds_an_open_path_from_its_clicks() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    assert!(draft.is_empty());
    assert!(!draft.can_close());
    assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 0 }));

    let points =
        [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]];
    for point in points {
        draft.push(point).expect("a fresh point");
    }
    assert_eq!(draft.len(), 2, "the first click starts the chain, the next two make segments");
    assert!(!draft.is_closed());
    assert!(draft.can_close(), "two segments enclose an area");

    let path = draft.to_path().expect("an open path");
    assert!(!path.is_closed());
    assert_eq!(path.entries().len(), 2);
    let mut ends = path.segments().map(|segment| segment.start().expect("start"));
    assert_eq!(ends.next(), Some(points[0]));
    assert_eq!(path.entries().last().and_then(|entry| entry.segment().end().ok()), Some(points[2]));
}

#[test]
fn an_arc_mixed_with_lines_closes_and_reaches_a_contour() {
    // A rounded corner: a straight run, an arc through a control point, then the rest of the box.
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("straight run");
    draft.push_arc_through([millimetres(700), millimetres(100)]).expect("arc through");
    draft.push([millimetres(600), millimetres(400)]).expect("arc end");
    draft.push([millimetres(0), millimetres(400)]).expect("top run");
    draft.close().expect("the chain closes");

    let path = draft.to_path().expect("a mixed path");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 4, "three clicks then the closing segment");
    assert_eq!(
        path.entries().iter().filter(|entry| entry.segment().is_arc()).count(),
        1,
        "the arc survives as one segment"
    );
    let ring = path.to_contour().expect("a mixed chain becomes a ring");
    assert_eq!(ring.entries().len(), 4);
}

#[test]
fn the_next_segment_starts_at_the_stored_arc_end() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push_arc_through([millimetres(50), millimetres(50)]).expect("arc through");
    draft.push([millimetres(100), millimetres(0)]).expect("the far end");
    let arc_end = draft.segments().last().and_then(|segment| segment.end().ok()).expect("a stored end");
    // The stored end is rounded to whole document units, so it can differ from the click.
    assert!((arc_end[0].micrometres() - millimetres(100).micrometres()).abs() <= 1);
    draft.push([millimetres(100), millimetres(400)]).expect("the next straight segment");
    let line = draft.segments().last().expect("a line");
    assert_eq!(line.start().ok(), Some(arc_end), "the line starts on the stored arc end");
}

#[test]
fn a_one_micrometre_gap_still_becomes_a_ring() {
    // A rounded arc end can miss the clicked point by one micrometre. `Path` accepts that joint
    // and, since ADR 0072, so does the ring assembler, so `to_contour` — the only way to an
    // extrusion — takes the chain the user drew instead of refusing it.
    let stored = [millimetres(600), millimetres(400)];
    let gap = [stored[0], Length::from_micrometres(stored[1].micrometres() + 1)];
    let chain = |end: [Length; 2], next_start: [Length; 2]| {
        Path::new(
            vec![
                ContourSegment::line([millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)])
                    .expect("bottom"),
                ContourSegment::line([millimetres(600), millimetres(0)], end).expect("right"),
                ContourSegment::line(next_start, [millimetres(0), millimetres(400)]).expect("top"),
                ContourSegment::line([millimetres(0), millimetres(400)], [millimetres(0), millimetres(0)])
                    .expect("left"),
            ],
            true,
        )
    };
    let drawn = chain(gap, stored).expect("a path tolerates one micrometre");
    assert!(drawn.to_contour().is_ok(), "the ring takes the same tolerance as the path");
    assert!(chain(stored, stored).expect("an exact chain").to_contour().is_ok(), "the exact joint closes");
}

#[test]
fn a_closing_arc_click_finishes_the_ring() {
    // A chain of two lines, then an arc back to the first point. The stored arc end is derived
    // from a rounded centre, radius and angle, so it misses the click by a few micrometres on a
    // long radius; the closing click still finishes the ring (ADR 0072).
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    let first = [millimetres(0), millimetres(0)];
    draft.push(first).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("bottom");
    draft.push([millimetres(600), millimetres(400)]).expect("right");
    assert!(draft.toggle_arc_mode());
    draft.push_arc_through([millimetres(300), millimetres(300)]).expect("arm the closing arc");
    draft.push(first).expect("the arc ends on the first point");

    let stored_end = draft.segments().last().and_then(|segment| segment.end().ok()).expect("a stored end");
    assert_ne!(stored_end, first, "the derived arc end is rounded away from the click");
    assert!(draft.is_closed(), "the rounded end still closes the ring");
    let path = draft.to_path().expect("a closed path");
    assert_eq!(path.entries().len(), 3, "the closing joint needs no sliver segment");
    assert_eq!(path.to_contour().expect("a ring").entries().len(), 3);
}

#[test]
fn undo_cancels_a_pending_arc_before_the_last_segment() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("a line");
    draft.push_arc_through([millimetres(650), millimetres(50)]).expect("a pending arc");
    assert!(draft.pending_arc().is_some());
    assert!(!draft.is_finished(), "an arc still waits for its end");
    assert_eq!(
        draft.close(),
        Err(DrawDraftError::TooFewPoints { need: 3, got: 2 }),
        "one line plus the closing arc is still two segments, not a ring"
    );

    // The next click ends the arc instead of starting a straight segment.
    draft.push([millimetres(700), millimetres(0)]).expect("the arc end");
    assert!(draft.pending_arc().is_none());
    assert_eq!(draft.len(), 2);
    assert!(draft.segments().last().is_some_and(|segment| segment.is_arc()));

    assert!(draft.undo(), "the last segment goes");
    assert_eq!(draft.len(), 1, "the line before the arc survives");
    assert!(draft.undo());
    assert!(draft.is_empty());
}

#[test]
fn a_pending_arc_closes_the_chain_the_user_already_curved() {
    // Two straight runs, then Tab puts the pointer into arc mode and the user places the
    // midpoint. The *Close* button must finish the ring with that arc: the chain already has the
    // two segments a ring needs and the arc is the third (ADR 0077).
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    let first = [millimetres(0), millimetres(0)];
    draft.push(first).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("bottom run");
    draft.push([millimetres(600), millimetres(400)]).expect("right run");
    assert!(draft.can_close());
    assert!(draft.toggle_arc_mode());
    let through = [millimetres(300), millimetres(300)];
    draft.push_arc_through(through).expect("arm the closing arc");

    assert!(draft.pending_arc().is_some());
    assert!(draft.can_close(), "the pending arc is the closing segment");
    assert!(!draft.is_finished(), "the arc still waits for a click that ends it");
    draft.close().expect("close through the arc");

    assert!(draft.is_closed());
    assert!(draft.is_finished(), "a closed draft is finished even with the arc waiting");
    let path = draft.to_path().expect("a closed path");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 3, "two straight runs and the closing arc");
    assert!(path.entries().last().is_some_and(|entry| entry.segment().is_arc()), "the close used the arc");
    let arc_end = path.entries().last().and_then(|entry| entry.segment().end().ok()).expect("the arc end");
    // The arc stores a rounded centre, radius and angle, so its derived end lands within the
    // drawing joint tolerance of the first point rather than exactly on it (ADR 0072).
    assert!(
        (arc_end[0].micrometres() - first[0].micrometres()).abs() <= 10
            && (arc_end[1].micrometres() - first[1].micrometres()).abs() <= 10,
        "the closing arc ends on the first point: {arc_end:?}"
    );
    let ring = path.to_contour().expect("the arc closes the ring");
    assert_eq!(ring.entries().len(), 3);
}

#[test]
fn a_degenerate_pending_arc_is_refused_by_close() {
    // The midpoint sits on the straight line from the stored end to the first start, so no arc
    // can pass through it. The refusal leaves the draft open, editable and waiting.
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("bottom run");
    draft.push([millimetres(600), millimetres(400)]).expect("right run");
    assert!(draft.toggle_arc_mode());
    let collinear = [millimetres(300), millimetres(200)];
    draft.push_arc_through(collinear).expect("arm the collinear arc");

    assert!(draft.close().is_err(), "three collinear points are no arc");
    assert!(!draft.is_closed(), "a refused close leaves the draft open");
    assert_eq!(draft.pending_arc(), Some(collinear), "the arc waits for a usable end");
    assert_eq!(draft.len(), 2, "no segment was stored");
    assert!(draft.can_close(), "the draft can still be closed once the midpoint moves");
}

#[test]
fn undoing_a_close_keeps_the_closing_arc() {
    // A close through a pending arc leaves that arc waiting, because the segment is derived from
    // it on demand. Undo must reopen the chain before it takes the arc, or the arc that closed
    // the ring disappears and the chain looks straight.
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("bottom run");
    draft.push([millimetres(600), millimetres(400)]).expect("right run");
    assert!(draft.toggle_arc_mode());
    let through = [millimetres(300), millimetres(300)];
    draft.push_arc_through(through).expect("arm the closing arc");
    draft.close().expect("close through the arc");

    assert!(draft.undo(), "the first undo reopens the chain");
    assert!(!draft.is_closed());
    assert_eq!(draft.pending_arc(), Some(through), "the closing arc is still waiting");
    assert!(draft.can_close(), "the reopened chain can close again");
    draft.close().expect("close it again through the same arc");
    assert!(draft.undo(), "a second close is undone before the arc");
    assert!(!draft.is_closed());
    assert_eq!(draft.pending_arc(), Some(through));
    assert!(draft.undo(), "the next undo takes the arc that would have closed it");
    assert_eq!(draft.pending_arc(), None);
}

#[test]
fn closing_a_drawing_draft_adds_the_last_segment_once() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    for point in
        [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
    {
        draft.push(point).expect("point");
    }
    draft.close().expect("a triangle closes");
    assert!(draft.is_closed());
    assert!(!draft.can_close());
    assert_eq!(draft.len(), 2, "the closing segment is built on demand, not stored as a click");

    let path = draft.to_path().expect("a closed path");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 3);
    let last = path.entries().last().expect("the closing segment");
    assert_eq!(last.segment().end().ok(), Some([millimetres(0), millimetres(0)]));

    assert_eq!(draft.close(), Err(DrawDraftError::AlreadyClosed));
    assert_eq!(draft.push([millimetres(1), millimetres(1)]), Err(DrawDraftError::AlreadyClosed));
}

#[test]
fn tab_switches_a_polyline_between_a_line_and_an_arc() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    assert!(!draft.arc_mode(), "a fresh chain draws straight segments");
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(600), millimetres(0)]).expect("straight run");
    assert!(draft.toggle_arc_mode());
    assert!(draft.arc_mode());
    draft.push_arc_through([millimetres(700), millimetres(100)]).expect("arc through");
    draft.push([millimetres(600), millimetres(400)]).expect("arc end");

    assert_eq!(draft.len(), 2);
    assert!(!draft.segments().first().expect("line").is_arc());
    assert!(draft.segments().get(1).expect("arc").is_arc(), "Tab made the second segment an arc");
    assert!(!draft.toggle_arc_mode());
    assert!(!draft.arc_mode());

    // Only a polyline has a second input mode; a circle keeps its own gesture.
    let mut circle = DrawDraft::new(DrawTool::Circle);
    assert!(!circle.toggle_arc_mode());
}

#[test]
fn an_arc_that_returns_to_the_start_closes_the_chain() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.toggle_arc_mode();
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(40), millimetres(0)]).expect("straight run");
    draft.push_arc_through([millimetres(50), millimetres(10)]).expect("first arc");
    draft.push([millimetres(40), millimetres(40)]).expect("first arc end");
    draft.push_arc_through([millimetres(20), millimetres(50)]).expect("closing arc");
    draft.push([millimetres(0), millimetres(0)]).expect("the arc ends on the start");

    assert!(draft.is_closed(), "an arc that reaches the first start closes the chain");
    let path = draft.to_path().expect("a closed path");
    assert!(path.is_closed());
    // The stored arc end is derived from rounded angles, so it meets the exact first start only
    // within the joint tolerance; that tolerance is the closing joint itself, so the ring holds
    // exactly the three drawn segments and no sliver tie (ADR 0072).
    assert_eq!(path.entries().len(), 3, "three drawn segments and no closing tie");
    assert_eq!(path.entries().iter().filter(|entry| entry.segment().is_arc()).count(), 2);
}

#[test]
fn a_degenerate_arc_step_is_refused_and_the_arc_waits() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("start");
    draft.push([millimetres(400), millimetres(0)]).expect("straight run");
    let through = [millimetres(200), millimetres(100)];
    draft.push_arc_through(through).expect("arc through");
    assert_eq!(draft.push(through), Err(DrawDraftError::DuplicatePoint));
    assert_eq!(draft.pending_arc(), Some(through), "the refused click leaves the arc waiting");
    assert_eq!(draft.len(), 1, "no segment was stored");

    draft.push([millimetres(400), millimetres(200)]).expect("a valid end finishes the arc");
    assert_eq!(draft.pending_arc(), None);
    assert_eq!(draft.len(), 2);
}

#[test]
fn every_new_tool_builds_its_own_figure() {
    let mut arc = DrawDraft::new(DrawTool::ArcFromCenter);
    for point in [
        [millimetres(100), millimetres(100)],
        [millimetres(200), millimetres(100)],
        [millimetres(100), millimetres(200)],
    ] {
        arc.push(point).expect("an arc from centre");
    }
    let path = arc.to_path().expect("one open arc");
    assert!(!path.is_closed());
    let ContourSegment::Arc { center, radius, .. } = path.entries().first().expect("arc").segment() else {
        panic!("the figure is one arc");
    };
    assert_eq!(center, [millimetres(100), millimetres(100)]);
    assert_eq!(radius, millimetres(100));

    let mut rectangle = DrawDraft::new(DrawTool::RectangleFromCenter);
    rectangle.push([millimetres(100), millimetres(100)]).expect("centre");
    rectangle.push([millimetres(300), millimetres(200)]).expect("corner");
    let path = rectangle.to_path().expect("a rectangle");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 4);

    let mut directed = DrawDraft::new(DrawTool::RectangleFromCenterDirected);
    directed.push([millimetres(100), millimetres(100)]).expect("centre");
    directed.push([millimetres(200), millimetres(100)]).expect("side direction");
    directed.push([millimetres(200), millimetres(200)]).expect("corner");
    assert!(directed.to_path().expect("a directed rectangle").is_closed());

    let mut circle = DrawDraft::new(DrawTool::CircleThreePoints);
    for point in
        [[millimetres(0), millimetres(0)], [millimetres(100), millimetres(0)], [millimetres(50), millimetres(100)]]
    {
        circle.push(point).expect("a circle through three points");
    }
    assert!(circle.to_path().expect("a circle").is_closed());

    let mut polygon = DrawDraft::with_sides(DrawTool::RegularPolygon, 5);
    polygon.push([millimetres(100), millimetres(100)]).expect("centre");
    polygon.push([millimetres(200), millimetres(100)]).expect("rim");
    assert_eq!(polygon.sides(), 5);
    let path = polygon.to_path().expect("a pentagon");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 5);

    // The ribbon may hand over any number; the draft holds what the model accepts.
    assert_eq!(DrawDraft::with_sides(DrawTool::RegularPolygon, 2).sides(), MIN_POLYGON_SIDES);
    assert_eq!(DrawDraft::with_sides(DrawTool::RegularPolygon, 999).sides(), MAX_POLYGON_SIDES);
}

#[test]
fn a_drawing_draft_needs_two_segments_before_it_can_close() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("point");
    assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 0 }));
    draft.push([millimetres(600), millimetres(0)]).expect("point");
    // One drawn segment plus the closing one is two, and a ring needs three.
    assert_eq!(draft.close(), Err(DrawDraftError::TooFewPoints { need: 3, got: 2 }));
    assert!(!draft.can_close(), "one segment cannot close");
    draft.push([millimetres(600), millimetres(400)]).expect("a second segment");
    assert!(draft.can_close(), "two segments plus the closing one make a ring");
    draft.close().expect("the triangle closes");
}

#[test]
fn undoing_a_drawing_draft_reopens_before_it_drops_a_segment() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    for point in
        [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
    {
        draft.push(point).expect("point");
    }
    draft.close().expect("close");

    assert!(draft.undo(), "the first undo opens the chain");
    assert!(!draft.is_closed());
    assert_eq!(draft.len(), 2, "opening keeps every segment");
    assert!(draft.undo(), "the next undo drops the last segment");
    assert_eq!(draft.len(), 1);
    assert!(draft.undo());
    assert!(draft.is_empty());
    assert!(!draft.undo(), "an empty open draft has nothing to undo");
}

#[test]
fn a_drawing_draft_refuses_a_repeat_and_a_point_out_of_range() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    draft.push([millimetres(0), millimetres(0)]).expect("point");
    draft.push([millimetres(600), millimetres(0)]).expect("point");
    assert_eq!(draft.push([millimetres(600), millimetres(0)]), Err(DrawDraftError::DuplicatePoint));
    draft.push([millimetres(600), millimetres(400)]).expect("a chain may touch an earlier point");
    draft.push([millimetres(0), millimetres(0)]).expect("a path need not be simple");

    let too_far = MAX_CONTOUR_COORDINATE.checked_add(Length::from_micrometres(1)).expect("length");
    assert_eq!(
        draft.push([too_far, Length::ZERO]),
        Err(DrawDraftError::PointOutOfRange),
        "a point outside the document range never enters a draft"
    );
}

#[test]
fn a_drawing_draft_stops_at_the_ring_cap() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    // The first click anchors the chain, so one more click than the segment cap is tried.
    for index in 0..=MAX_CONTOUR_SEGMENTS {
        let x = Length::from_micrometres(i64::try_from(index).expect("index") * 1_000);
        let result = draft.push([x, Length::ZERO]);
        if index == MAX_CONTOUR_SEGMENTS {
            assert_eq!(result, Err(DrawDraftError::Full), "a click past the cap is refused");
        } else {
            result.expect("a point below the cap");
        }
    }
    assert_eq!(draft.len(), MAX_CONTOUR_SEGMENTS - 1);
    assert_eq!(
        draft.push([Length::from_micrometres(MAX_CONTOUR_SEGMENTS as i64 * 1_000), Length::ZERO]),
        Err(DrawDraftError::Full),
        "the anchor click fills the count, so the next one is past the cap"
    );
}

#[test]
fn a_drawing_draft_refuses_a_close_that_cannot_form_a_path() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    for point in
        [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(0), millimetres(0)]]
    {
        draft.push(point).expect("a path need not be simple");
    }
    assert!(draft.close().is_err(), "two segments cannot become a ring");
    assert!(!draft.is_closed(), "a refused close leaves the draft open and editable");
    assert_eq!(draft.len(), 2);
}

#[test]
fn the_line_tool_makes_one_open_segment() {
    let mut draft = DrawDraft::new(DrawTool::Line);
    assert_eq!(draft.tool(), DrawTool::Line);
    assert!(!draft.is_complete());
    draft.push([millimetres(0), millimetres(0)]).expect("first point");
    assert_eq!(draft.to_path(), Err(DrawDraftError::TooFewPoints { need: 2, got: 1 }));
    draft.push([millimetres(600), millimetres(0)]).expect("second point");
    assert!(draft.is_complete());
    assert_eq!(
        draft.push([millimetres(600), millimetres(400)]),
        Err(DrawDraftError::Complete { tool: DrawTool::Line }),
        "a fixed tool stops at its point count"
    );

    let path = draft.to_path().expect("a line");
    assert!(!path.is_closed());
    assert_eq!(path.entries().len(), 1);
    assert_eq!(draft.close(), Err(DrawDraftError::NotClosable { tool: DrawTool::Line }), "a line is always open");
}

#[test]
fn the_arc_tool_makes_one_open_arc_through_its_middle_point() {
    let mut draft = DrawDraft::new(DrawTool::Arc);
    for point in
        [[millimetres(0), millimetres(0)], [millimetres(50), millimetres(50)], [millimetres(100), millimetres(0)]]
    {
        draft.push(point).expect("an arc point");
    }
    assert!(draft.is_complete());
    assert_eq!(draft.push([millimetres(0), millimetres(100)]), Err(DrawDraftError::Complete { tool: DrawTool::Arc }));
    let path = draft.to_path().expect("an arc");
    assert!(!path.is_closed());
    assert_eq!(path.entries().len(), 1);
    assert!(path.entries()[0].segment().is_arc());
}

#[test]
fn the_rectangle_tool_closes_two_corners() {
    let mut draft = DrawDraft::new(DrawTool::Rectangle);
    draft.push([millimetres(600), millimetres(400)]).expect("first corner");
    draft.push([millimetres(0), millimetres(0)]).expect("opposite corner");
    assert!(draft.tool().is_closed(), "the tool produces a closed path");
    assert!(!draft.is_closed(), "the user did not close anything");
    assert!(!draft.can_close(), "there is nothing left to close");
    let path = draft.to_path().expect("a rectangle");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 4);
    assert_eq!(path.bounds_micrometres(), ([0, 0], [600_000, 400_000]));
}

#[test]
fn the_circle_tool_closes_a_centre_and_a_rim_point() {
    let mut draft = DrawDraft::new(DrawTool::Circle);
    draft.push([millimetres(100), millimetres(50)]).expect("centre");
    draft.push([millimetres(130), millimetres(90)]).expect("rim");
    assert!(draft.is_complete());
    let path = draft.to_path().expect("a circle");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 4);
    assert!(path.entries().iter().all(|entry| entry.segment().is_arc()));
    assert_eq!(path.bounds_micrometres(), ([50_000, 0], [150_000, 100_000]));
}

#[test]
fn the_polyline_tool_takes_points_until_it_is_closed() {
    let mut draft = DrawDraft::new(DrawTool::Polyline);
    assert!(!draft.is_complete(), "a polyline needs two points");
    draft.push([millimetres(0), millimetres(0)]).expect("first");
    assert!(!draft.is_complete());
    draft.push([millimetres(600), millimetres(0)]).expect("second");
    assert!(draft.is_complete());
    draft.push([millimetres(600), millimetres(400)]).expect("a polyline keeps taking points");
    assert!(draft.can_close());
    draft.close().expect("close the triangle");
    let path = draft.to_path().expect("a closed polyline");
    assert!(path.is_closed());
    assert_eq!(path.entries().len(), 3);
}

#[test]
fn a_draft_finishes_when_its_tool_is_done() {
    let mut line = DrawDraft::new(DrawTool::Line);
    line.push([millimetres(0), millimetres(0)]).expect("first");
    assert!(!line.is_finished());
    line.push([millimetres(600), millimetres(0)]).expect("second");
    assert!(line.is_finished(), "a complete line commits on its last click");

    let mut polyline = DrawDraft::new(DrawTool::Polyline);
    for point in
        [[millimetres(0), millimetres(0)], [millimetres(600), millimetres(0)], [millimetres(600), millimetres(400)]]
    {
        polyline.push(point).expect("point");
    }
    assert!(!polyline.is_finished(), "an open chain waits for close or commit");
    polyline.close().expect("close");
    assert!(polyline.is_finished());

    let mut circle = DrawDraft::new(DrawTool::Circle);
    circle.push([millimetres(100), millimetres(50)]).expect("centre");
    assert!(!circle.is_finished());
    circle.push([millimetres(130), millimetres(90)]).expect("rim");
    assert!(circle.is_finished());
}

#[test]
fn every_tool_states_its_point_count_and_name() {
    let cases = [
        (DrawTool::Line, Some(2), false, false, "Line"),
        (DrawTool::Polyline, None, true, true, "Polyline"),
        (DrawTool::Arc, Some(3), false, false, "Arc"),
        (DrawTool::ArcFromCenter, Some(3), false, false, "Arc from centre"),
        (DrawTool::Rectangle, Some(2), false, false, "Rectangle"),
        (DrawTool::RectangleFromCenter, Some(2), false, false, "Rectangle from centre"),
        (DrawTool::RectangleFromCenterDirected, Some(3), false, false, "Directed rectangle"),
        (DrawTool::Circle, Some(2), false, false, "Circle"),
        (DrawTool::CircleThreePoints, Some(3), false, false, "Circle through 3 points"),
        (DrawTool::RegularPolygon, Some(2), false, false, "Polygon"),
    ];
    for (tool, need, closes, open_ended, label) in cases {
        assert_eq!(tool.points(), need, "{}", tool.label());
        assert_eq!(tool.closes(), closes, "{}", tool.label());
        assert_eq!(tool.points().is_none(), open_ended, "{}", tool.label());
        assert_eq!(tool.label(), label);
        assert!(!DrawDraft::new(tool).is_complete(), "a fresh {} draft is empty", tool.label());
    }
}

#[test]
fn a_drawing_draft_shows_its_reason_to_a_user() {
    assert_eq!(DrawDraftError::NotStarted.to_string(), "no drawing is in progress");
    assert_eq!(
        DrawDraftError::TooFewPoints { need: 3, got: 1 }.to_string(),
        "the drawing needs at least 3 points, it has 1"
    );
    assert_eq!(DrawDraftError::Complete { tool: DrawTool::Arc }.to_string(), "the Arc tool is complete");
    assert_eq!(DrawDraftError::NotClosable { tool: DrawTool::Line }.to_string(), "the Line tool cannot be closed");
    assert!(!DrawDraftError::Full.to_string().is_empty());
    assert!(!DrawDraftError::DuplicatePoint.to_string().is_empty());
    assert!(!DrawDraftError::PointOutOfRange.to_string().is_empty());
    assert!(!DrawDraftError::AlreadyClosed.to_string().is_empty());
}
