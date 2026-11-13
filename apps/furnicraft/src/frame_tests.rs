//! The shell's one frame, asserted without a window.
//!
//! `FurniCraftApp::frame` is the whole interface: the header, the ribbon, the floating panels, the
//! scene, the cluster and the modals. These tests drive it with an [`egui::Context`] and
//! [`egui::RawInput`] — no window, no device, no adapter — and assert what a live session used to
//! check by eye: the scene rectangle and its camera aspect in every panel and ribbon state, the fact
//! that the chrome keeps the presses it covers, that the split views tile the scene and their
//! borders stay grabbable, that a modal keeps the keys, and that no chrome gesture writes a revision
//! (U78).
//!
//! Changes when: the frame's order, a chrome gesture, a split layout or the shell's key handling
//! changes.

#![allow(clippy::expect_used)]

use eframe::egui;
use furnicraft_model::{ClassCode, Envelope, Id};
use furnicraft_scene::Camera;
use furnicraft_text::{Lang, Message};

use super::*;

/// The window a headless frame is drawn into: the default launch size (M2).
fn screen() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 840.0))
}

/// Ctrl+Shift, the modifier pair the panel fold takes.
fn ctrl_shift() -> egui::Modifiers {
    egui::Modifiers { shift: true, ..egui::Modifiers::CTRL }
}

/// A headless application, the context that stands in for its window, and the workspace file it
/// would write to.
///
/// Everything below it is the real interface: [`FurniCraftApp::headless`] composes the same shell the
/// window composes, and [`Harness::run`] draws the same frame `eframe::App::ui` draws (U77).
struct Harness {
    ctx: egui::Context,
    app: FurniCraftApp,
    /// Where the workspace would be written; the tests never write it, but a headless run must not
    /// be able to reach the file in the OS config directory.
    path: std::path::PathBuf,
    /// The text the last frame drew, so a test reads what the user would see.
    drawn: Vec<String>,
    /// The scene rectangle the frame before the last one gave the split views, so [`Harness::settle`]
    /// can tell a settled frame from one whose layout is still moving.
    last_scene: Option<egui::Rect>,
}

impl Harness {
    fn new(name: &str) -> Self {
        Self::with_session(name, crate::scene_build::demo_session().expect("the sample session builds"))
    }

    /// A headless application around one session, for a test that wants a document of its own.
    fn with_session(name: &str, session: furnicraft_engine::Session) -> Self {
        let path = furnicraft_testkit::scratch_dir(name).join("settings.json");
        let ctx = egui::Context::default();
        let app = FurniCraftApp::headless(&ctx, path.clone(), Settings::default(), session)
            .expect("the application builds with no device, no adapter and no window");
        Self { ctx, app, path, drawn: Vec::new(), last_scene: None }
    }

    /// Runs one real frame with `events` and returns the chrome it gave the window.
    ///
    /// A window hands the frame's texture deltas to the renderer; a test has none, and epaint asserts
    /// on drop that a delta was applied, so the frame is closed here.
    fn run(&mut self, events: Vec<egui::Event>) -> Chrome {
        let mut chrome = None;
        let mut output =
            self.ctx.run_ui(egui::RawInput { screen_rect: Some(screen()), events, ..Default::default() }, |ui| {
                chrome = Some(self.app.frame(ui));
            });
        self.drawn = crate::test_support::drawn_text(&output.shapes);
        self.last_scene = self.app.tabs.active().scene_rect();
        output.textures_delta.clear();
        chrome.expect("the frame drew the interface")
    }

    /// Runs one idle frame, with no input at all.
    fn frame(&mut self) -> Chrome {
        self.run(Vec::new())
    }

    /// Draws idle frames until the scene rectangle stops moving.
    ///
    /// A gesture leaves work for the frame after it: the side panels narrow over
    /// [`ANIMATION_SECONDS`], and the status bar grows for the one frame that announces a rebuild.
    /// The measurement is read after both have settled, so a test compares the layout and not the
    /// middle of an animation.
    fn settle(&mut self) -> Chrome {
        let mut chrome = self.frame();
        let mut stable = 0;
        for _ in 0..12 {
            let previous = self.last_scene;
            chrome = self.frame();
            stable = if self.last_scene == previous { stable + 1 } else { 0 };
            if stable >= 2 {
                return chrome;
            }
        }
        chrome
    }

    /// Moves the pointer to `pos` and presses `button` there, without releasing it.
    fn press_at(&mut self, pos: egui::Pos2, button: egui::PointerButton) {
        self.run(vec![egui::Event::PointerMoved(pos)]);
        self.run(vec![Self::button(pos, button, true)]);
    }

    /// Releases `button` at `pos`.
    fn release_at(&mut self, pos: egui::Pos2, button: egui::PointerButton) {
        self.run(vec![Self::button(pos, button, false)]);
    }

    fn button(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE }
    }

    /// Clicks `pos` with `button`: the pointer arrives, the button goes down and comes back up.
    ///
    /// The first frame lets egui lay the area or the widget out, so the press lands on the rectangle
    /// the user is looking at instead of on last frame's.
    fn click(&mut self, pos: egui::Pos2, button: egui::PointerButton) -> Chrome {
        self.run(vec![egui::Event::PointerMoved(pos)]);
        self.press_at(pos, button);
        self.release_at(pos, button);
        self.settle()
    }

    /// Drags `button` from `from` by `delta`, one frame per step.
    fn drag(&mut self, from: egui::Pos2, delta: egui::Vec2, button: egui::PointerButton) -> Chrome {
        self.press_at(from, button);
        self.run(vec![egui::Event::PointerMoved(from + delta)]);
        self.release_at(from + delta, button);
        self.settle()
    }

    /// Presses and releases `key`, the way a window reports one shortcut.
    fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) -> Chrome {
        let event = |pressed: bool| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        self.run(vec![event(true)]);
        self.run(vec![event(false)]);
        self.settle()
    }

    /// The scene rectangle the last frame gave the split views.
    fn scene(&self) -> egui::Rect {
        self.app.tabs.active().scene_rect().expect("a drawn frame gives the scene a rectangle")
    }

    /// The camera the last frame looked through.
    fn camera(&self) -> Camera {
        self.app.tabs.active().camera()
    }

    /// The document revision the active scene is at.
    fn revision(&self) -> furnicraft_model::Revision {
        self.app.tabs.active().session.project().revision()
    }

    /// The nodes the viewport selection holds.
    fn selected(&self) -> Vec<Id> {
        self.app.tabs.active().editor.selected_nodes().to_vec()
    }

    /// Whether the last frame drew `text`.
    fn drew(&self, text: &str) -> bool {
        self.drawn.iter().any(|line| line.contains(text))
    }
}

/// The aspect ratio the renderer derives from a rectangle: each view's own, not the target's.
fn aspect(rect: egui::Rect) -> f64 {
    f64::from(rect.width()) / f64::from(rect.height())
}

#[test]
fn the_whole_interface_frames_without_a_gpu_and_without_a_window() {
    let mut harness = Harness::new("frame-headless");
    let revision = harness.revision();
    let chrome = harness.frame();
    assert!(chrome.ribbon.height() > 0.0, "the frame gives the ribbon a band");
    assert!(chrome.panels.left.is_some() && chrome.panels.right.is_some(), "and both panels a rectangle");
    assert!(!harness.drawn.is_empty(), "the frame draws the header, the ribbon and the scene");
    harness.frame();
    assert!(!harness.drawn.is_empty(), "a second frame draws the same interface again");
    assert_eq!(harness.revision(), revision, "drawing a frame writes no document");
    assert_eq!(harness.app.tabs.len(), 1, "and opens no tab");
    assert!(harness.app.tabs.active().last_error.is_none(), "nothing failed");
}

#[test]
fn a_headless_run_leaves_the_workspace_file_alone() {
    // The store the headless application is built with points at a scratch file, never at the
    // workspace in the OS config directory, and a frame that changes no setting writes nothing.
    let mut harness = Harness::new("frame-settings");
    let before = b"{\"version\": 1}\n";
    std::fs::write(&harness.path, before).expect("the test writes the workspace file");
    harness.frame();
    harness.frame();
    let after = std::fs::read(&harness.path).expect("the file is still there");
    assert_eq!(after, before, "a headless run leaves the workspace file byte-identical");
    assert!(!harness.path.with_extension("json.tmp").exists(), "and leaves no temporary sibling behind");
}

#[test]
fn the_scene_rectangle_and_its_camera_hold_in_every_panel_and_ribbon_state() {
    // The regression U72 fixed: a fold, a hide or a restore moved the scene rectangle and therefore
    // the camera aspect, so the model panned when a panel was folded. Only the frame can prove it:
    // the rectangle the viewport really used, and the camera it really drew with.
    let mut harness = Harness::new("frame-scene-rect");
    harness.frame();
    let mut scenes = Vec::new();
    for ribbon_open in [true, false] {
        if !ribbon_open {
            harness.app.handle_header_action(HeaderAction::ToggleRibbon, Lang::En);
            harness.settle();
        }
        let chrome = harness.settle();
        let reference = harness.scene();
        let camera = harness.camera();
        assert!((reference.top() - chrome.ribbon.bottom()).abs() < f32::EPSILON, "the band sets the scene's top");
        assert!(chrome.panels.left.is_some() && chrome.panels.right.is_some(), "both panels are up");
        // The panels float over the scene, so it runs under them: their rectangles sit inside its
        // horizontal span. That is what makes a fold unable to widen or narrow it.
        let left = chrome.panels.left.expect("the scene tree is up");
        let right = chrome.panels.right.expect("the inspector is up");
        assert!(
            reference.left() <= left.left() && reference.right() >= right.right(),
            "the scene runs under both panels"
        );
        scenes.push(reference);

        // Both panels folded, then both hidden, then back: the cycle the header buttons and the
        // shortcuts run. Every state keeps the rectangle and the camera the ribbon fold fixes.
        for (state, keys) in [
            ("folded", (egui::Key::Num0, ctrl_shift())),
            ("hidden", (egui::Key::Num0, egui::Modifiers::CTRL)),
            ("restored", (egui::Key::Num0, egui::Modifiers::CTRL)),
            ("unfolded", (egui::Key::Num0, ctrl_shift())),
        ] {
            let geometry = harness.press(keys.0, keys.1);
            let scene = harness.scene();
            let context = format!("{state}, ribbon open = {ribbon_open}");
            assert_eq!(geometry.ribbon, chrome.ribbon, "{context}: the band does not move");
            assert_eq!(scene, reference, "{context}: no edge of the scene moves");
            assert!((aspect(scene) - aspect(reference)).abs() < f64::EPSILON, "{context}: the aspect is the same");
            assert_eq!(harness.camera(), camera, "{context}: and the camera is the same");
            let panels = harness.app.shell.panels;
            let (hidden, collapsed) = (!panels.any_visible(), panels.all_collapsed());
            match state {
                "folded" => assert!(collapsed && !hidden, "{context}: the fold shortcut folds both panels"),
                "hidden" => assert!(hidden, "{context}: the hide shortcut hides both panels"),
                "restored" => assert!(!hidden && collapsed, "{context}: a second press brings the folded pair back"),
                _ => assert!(!hidden && !collapsed, "{context}: the fold shortcut unfolds them again"),
            }
        }
    }
    // The ribbon's own fold is the one thing that may move the scene: it gives the band back.
    let (open, folded) = (scenes[0], scenes[1]);
    assert!(folded.height() > open.height(), "a folded ribbon gives the scene its band");
    assert!((folded.width() - open.width()).abs() < f32::EPSILON, "and never a column of it");
}

#[test]
fn a_press_on_a_panel_is_chrome_and_a_press_beside_it_reaches_the_scene() {
    // U66 and U72 were accepted with "a click on a panel selects nothing"; this is that sentence as
    // an assertion on the real frame, with a navigation mode armed so a camera gesture would show.
    let mut harness = Harness::new("frame-chrome-press");
    let chrome = harness.settle();
    let tree = chrome.panels.left.expect("the scene tree is up");
    // A point in the empty body below the tree's rows: the panel's chrome, not a row and not the
    // header's fold button.
    let on_panel = egui::pos2(tree.center().x, tree.bottom() - 12.0);
    assert!(chrome.covers(on_panel), "the whole panel is chrome");

    let selected = harness.selected();
    let camera = harness.camera();
    harness.click(on_panel, egui::PointerButton::Primary);
    assert_eq!(harness.selected(), selected, "a click on a panel is chrome and starts no pick");
    assert!(!harness.app.shell.divider_dragging, "and grabs no border");
    assert_eq!(harness.camera(), camera, "and starts no camera gesture");

    // A drag across the same panel is chrome too, and being chrome is what stops the orbit.
    harness.app.shell.cluster.navigation = Some(crate::view_cluster::NavigationMode::Orbit);
    harness.drag(on_panel, egui::vec2(60.0, 40.0), egui::PointerButton::Primary);
    assert_eq!(harness.camera(), camera, "a drag on a panel orbits nothing");

    // The scene keeps the presses the panels do not cover: one point past the panel's inner edge.
    let beside = egui::pos2(tree.right() + 2.0, tree.center().y);
    assert!(!chrome.covers(beside), "one point past the panel is the scene");
    assert!(harness.scene().contains(beside), "and inside the scene rectangle");
    harness.drag(beside, egui::vec2(60.0, 40.0), egui::PointerButton::Primary);
    assert_ne!(harness.camera(), camera, "the same drag there orbits the view");

    // And a press there selects whatever is under it, so the guard above cannot pass by refusing
    // every pick in the workspace.
    harness.app.shell.cluster.navigation = None;
    let selected = harness.selected();
    harness.click(harness.scene().center(), egui::PointerButton::Primary);
    let after = harness.selected();
    assert!(!after.is_empty() && after != selected, "a click in the middle of the scene selects the model there");
}

#[test]
fn two_columns_and_quad_tile_the_scene_rect_and_their_borders_stay_grabbable() {
    use crate::view_layout::LayoutKind;

    let mut harness = Harness::new("frame-layout");
    harness.settle();
    for kind in [LayoutKind::TwoColumns, LayoutKind::TwoRows, LayoutKind::Quad] {
        harness.app.tabs.active_mut().set_layout(kind);
        let chrome = harness.settle();
        let scene = harness.scene();
        let layout = harness.app.tabs.active().layout();
        let regions = layout.regions(scene);
        assert_eq!(regions.len(), kind.leaves(), "{kind:?} draws one view per leaf");
        let mut tiled = 0.0;
        for (_, view) in &regions {
            assert!(scene.contains_rect(*view), "{kind:?}: every view stays inside the scene: {view:?}");
            assert!(view.width() > 0.0 && view.height() > 0.0, "{kind:?}: a view has an area");
            tiled += view.width() * view.height();
        }
        assert!(
            (tiled - scene.width() * scene.height()).abs() < 1.0,
            "{kind:?} tiles the scene rect exactly: {tiled} against {}",
            scene.width() * scene.height()
        );

        // A border is inside the scene, outside the chrome, and a press there grabs it.
        let divider = harness.app.tabs.active().layout().dividers(scene).into_iter().next().expect("a border");
        let handle = divider.hit_rect().center();
        assert!(scene.contains(handle), "{kind:?}: the border band is inside the scene");
        assert!(!chrome.covers(handle), "{kind:?}: and not under a panel");
        let selected = harness.selected();
        harness.press_at(handle, egui::PointerButton::Primary);
        assert!(harness.app.shell.divider_dragging, "{kind:?}: a press there grabs the border");
        assert_eq!(harness.selected(), selected, "{kind:?}: and selects nothing");
        harness.release_at(handle, egui::PointerButton::Primary);
        harness.settle();
        assert!(!harness.app.shell.divider_dragging, "{kind:?}: and the release ends the drag");
    }
}

#[test]
fn a_modal_owns_the_keys() {
    // The sentence ADR 0076 makes: a modal keeps the keys for the frame it is up. Before U78 this was
    // re-verified by hand, and the drawing-tool table was not gated at all.
    let mut harness = Harness::new("frame-modal");
    harness.settle();
    let revision = harness.revision();

    // One real edit, through the keyboard and the pointer, so undo and redo have something to do:
    // the rectangle tool and two corners of the work plane.
    harness.press(egui::Key::R, egui::Modifiers::NONE);
    assert!(harness.app.tabs.active().editor.drawing().is_some(), "R starts the rectangle tool");
    let scene = harness.scene();
    harness.click(scene.center() - egui::vec2(120.0, 90.0), egui::PointerButton::Primary);
    harness.click(scene.center() + egui::vec2(120.0, 90.0), egui::PointerButton::Primary);
    let edited = harness.revision();
    assert_ne!(edited, revision, "the two clicks commit one path");
    assert!(harness.app.tabs.active().editor.drawing().is_none(), "and the tool ends with the figure");
    assert!(harness.app.tabs.active().can_undo(), "the edit is undoable");
    // A committed drawing hands over to the corner grips; leave them, so a tool key has room to run.
    harness.press(egui::Key::Escape, egui::Modifiers::NONE);
    assert!(harness.app.tabs.active().editor.contour_edit().is_none(), "Escape leaves the contour edit");

    // A camera the fit would change, so a reframe under the modal would show.
    let resting = harness.camera();
    harness.drag(harness.scene().center(), egui::vec2(50.0, 0.0), egui::PointerButton::Middle);
    let orbited = harness.camera();
    assert_ne!(orbited, resting, "a middle drag orbits the view");

    harness.app.handle_header_action(HeaderAction::Settings, Lang::En);
    harness.settle();
    assert!(harness.drew(Message::SettingsTitle.text(Lang::En)), "the Settings window is up: {:?}", harness.drawn);

    harness.press(egui::Key::Z, egui::Modifiers::CTRL);
    assert_eq!(harness.revision(), edited, "Ctrl+Z under a modal undoes nothing");
    assert!(harness.app.tabs.active().can_undo(), "the edit is still there to undo");
    harness.press(egui::Key::F, egui::Modifiers::NONE);
    assert_eq!(harness.camera(), orbited, "F under a modal does not reframe");
    harness.press(egui::Key::R, egui::Modifiers::NONE);
    assert!(harness.app.tabs.active().editor.drawing().is_none(), "and no tool starts under it");
    harness.press(egui::Key::S, egui::Modifiers::CTRL);
    assert!(
        !harness.drew(Message::SaveSceneAsTitle.text(Lang::En)),
        "Ctrl+S under a modal opens no save dialog: {:?}",
        harness.drawn
    );

    // The gate is the modal and not a dead key: with it closed, the same press undoes the edit.
    harness.app.settings_open = false;
    harness.settle();
    harness.press(egui::Key::Z, egui::Modifiers::CTRL);
    assert!(!harness.app.tabs.active().can_undo(), "the undo ran once the modal was gone");
    assert!(harness.app.tabs.active().can_redo(), "and one edit was one undo entry");
}

#[test]
fn no_chrome_gesture_writes_a_revision() {
    // A fold, a hide, a ribbon fold and a camera move are view state: the document must not gain an
    // undo entry, a dirty marker or a rebuild from any of them.
    let mut harness = Harness::new("frame-view-state");
    harness.settle();
    let revision = harness.revision();
    let camera = harness.camera();

    harness.press(egui::Key::Num0, ctrl_shift());
    assert!(harness.app.shell.panels.all_collapsed(), "the fold shortcut folded both panels");
    harness.press(egui::Key::Num0, egui::Modifiers::CTRL);
    assert!(!harness.app.shell.panels.any_visible(), "the hide shortcut hid both panels");
    harness.app.handle_header_action(HeaderAction::ToggleRibbon, Lang::En);
    harness.settle();
    assert!(!harness.app.tabs.active().ribbon_open(), "the ribbon folded");
    harness.drag(harness.scene().center(), egui::vec2(40.0, 0.0), egui::PointerButton::Middle);
    assert_ne!(harness.camera(), camera, "the drag orbited the view");

    assert_eq!(harness.revision(), revision, "no chrome gesture writes the document");
    assert!(!harness.app.tabs.active().is_dirty(), "and none of them marks the scene unsaved");
    assert!(!harness.app.tabs.active().can_undo(), "and none of them adds an undo entry");
}

#[test]
fn each_tab_keeps_its_own_open_product() {
    // Openness is session view state (ADR 0098), and a tab owns its session: a new tab starts with
    // every product closed, and the first tab still works in the frame it opened.
    let mut harness = Harness::new("frame-product-tabs");
    harness.settle();
    let first_tab = harness.app.tabs.active_id();
    let draft = furnicraft_ui::product_card::ProductDraft {
        envelope: Envelope::new(
            furnicraft_testkit::millimetres(600),
            furnicraft_testkit::millimetres(560),
            furnicraft_testkit::millimetres(720),
        )
        .expect("a valid box"),
        class: ClassCode::GENERAL_PRODUCT,
    };
    harness.app.tabs.active_mut().create_product(draft, Lang::En).expect("create the frame");
    let first = harness.app.tabs.active().session.active_product().expect("the first tab works in it");

    harness.app.new_tab().expect("a new tab");
    let second_tab = harness.app.tabs.active_id();
    assert_ne!(first_tab, second_tab);
    assert_eq!(harness.app.tabs.active().session.active_product(), None, "a new tab opens no product");

    assert!(harness.app.tabs.activate(first_tab));
    assert_eq!(harness.app.tabs.active().session.active_product(), Some(first), "the first tab kept it");
    assert!(harness.app.tabs.activate(second_tab));
    assert_eq!(harness.app.tabs.active().session.active_product(), None);
}

#[test]
fn the_product_card_creates_a_frame_that_is_open_and_receives_the_next_part() {
    let mut harness = Harness::new("frame-product-card");
    harness.settle();
    let before = harness.revision();
    harness.app.product_card.start();
    harness.settle();
    assert!(harness.app.product_card.is_open(), "the card is up");

    harness.press(egui::Key::Enter, egui::Modifiers::NONE);
    let document = harness.app.tabs.active();
    let product = document.session.active_product().expect("the card created a frame and opened it");
    assert!(document.session.project().container(product).is_some_and(|product| product.is_product()));
    assert_eq!(harness.revision().get(), before.get() + 1, "a frame is one command");
    assert!(!harness.app.product_card.is_open(), "the confirmed card closed");
    assert_eq!(
        document.editor.creation_target(document.session.project()),
        product,
        "the next part lands inside the new frame"
    );
}

#[test]
fn the_drawn_frame_lands_as_one_product_of_exactly_the_ghost_size() {
    // The frame is created and edited like a box (ADR 0101): the same base rectangle and height
    // pull, and the ✓ dispatches `product.create` instead of `object.primitive.create`.
    let mut harness = Harness::new("frame-drawn-product");
    harness.settle();
    let before = harness.revision();
    let mm = furnicraft_testkit::millimetres;
    {
        let document = harness.app.tabs.active_mut();
        document.editor.arm_box(furnicraft_ui::BoxOwner::Product);
        assert!(!document.editor.click_box([mm(600), mm(560)]).expect("the anchor"), "the first click only anchors");
        assert!(document.editor.click_box([mm(0), mm(0)]).expect("the base"), "the second fixes the base");
        document.editor.pull_box_height(mm(720)).expect("the pull");
        let ghost = document.editor.box_draft().and_then(furnicraft_ui::BoxDraft::sizes).expect("the ghost sizes");
        assert_eq!(ghost, [mm(600), mm(560), mm(720)]);
        document.finish_box(Lang::En).expect("the ✓ commits the drawn frame");
    }
    let document = harness.app.tabs.active();
    let frame = document.session.active_product().expect("the drawn frame is opened for its parts");
    let container = document.session.project().container(frame).expect("a container");
    assert!(container.is_product(), "a drawn frame is a product");
    let sides = container.envelope().expect("an envelope").components();
    assert_eq!(sides, [mm(600), mm(560), mm(720)], "the frame keeps exactly the ghost's three sides");
    assert_eq!(container.class(), ClassCode::GENERAL_PRODUCT, "the drawn gesture takes the placeholder class");
    assert_eq!(harness.revision().get(), before.get() + 1, "the whole gesture is one command");
    assert_eq!(document.editor.selected_nodes(), [frame], "the new frame is selected, so its grips show");
    assert_eq!(document.editor.creation_target(document.session.project()), frame, "the next part lands inside");
}

#[test]
fn a_drag_on_a_frame_corner_resizes_the_envelope_in_one_undo_step() {
    // The grips read a frame exactly as they read a solid (ADR 0101): a press mints the edit scope,
    // every move writes `product.envelope.set`, and the release leaves one undo entry.
    let mut harness = Harness::with_session("frame-grip-drag", crate::scene_build::empty_session().expect("empty"));
    harness.settle();
    harness.app.product_card.start();
    harness.settle();
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);
    // The new frame is the only object, so the fit frames its reserved box and every grip is on the
    // scene rectangle.
    harness.app.tabs.active_mut().frame_all();
    harness.settle();
    let (frame, grip, reached, before) = {
        let document = harness.app.tabs.active_mut();
        let frame = document.session.active_product().expect("the card opened a frame");
        let project = document.session.project();
        let envelope = project.container(frame).and_then(|frame| frame.envelope()).expect("an envelope");
        let [width, depth, _height] = envelope.components().map(|side| side.millimetres_f64());
        let world = project.world_placement(frame).expect("the frame's world placement");
        let rect = document.scene_rect().expect("a drawn frame gives the scene a rectangle");
        let camera = document.camera();
        let at = |x: f64, y: f64| {
            gizmo_math::screen_point(camera, rect, world.transform_point(furnicraft_geom::DVec3::new(x, y, 0.0)))
                .expect("the frame's base is on screen")
        };
        (frame, at(width, depth), at(width - 100.0, depth - 100.0), harness.revision())
    };
    assert!(harness.app.tabs.active().editor.selected_node() == Some(frame), "the new frame is selected");
    let rect = harness.app.tabs.active().scene_rect().expect("a scene rectangle");
    assert!(rect.contains(grip) && rect.contains(reached), "the drag stays on the scene: {grip:?} {reached:?}");

    harness.press_at(grip, egui::PointerButton::Primary);
    harness.run(vec![egui::Event::PointerMoved(reached)]);
    harness.release_at(reached, egui::PointerButton::Primary);
    harness.settle();

    let envelope_sides = |harness: &Harness| {
        harness
            .app
            .tabs
            .active()
            .session
            .project()
            .container(frame)
            .and_then(|frame| frame.envelope())
            .expect("the envelope is still there")
            .components()
            .map(|side| side.millimetres_f64())
    };
    let sides = envelope_sides(&harness);
    assert!((sides[0] - 500.0).abs() <= 5.0, "the width followed the corner: {sides:?}");
    assert!((sides[1] - 460.0).abs() <= 5.0, "the depth followed the corner: {sides:?}");
    assert_eq!(harness.revision().get(), before.get() + 1, "the whole drag is one command");
    harness.app.tabs.active_mut().session.undo().expect("one undo takes the drag back");
    let sides = envelope_sides(&harness);
    assert!((sides[0] - 600.0).abs() < 1.0e-9 && (sides[1] - 560.0).abs() < 1.0e-9, "the frame is back: {sides:?}");
}

#[test]
fn the_empty_space_tool_creates_the_ghost_as_one_classed_part_in_one_undo_step() {
    let mut harness = Harness::new("frame-fill");
    harness.settle();
    harness.app.product_card.start();
    harness.settle();
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);

    let (product, before, pointer) = {
        let document = harness.app.tabs.active_mut();
        let product = document.session.active_product().expect("the card opens the frame");
        let project = document.session.project();
        let envelope = project.container(product).and_then(|product| product.envelope()).expect("an envelope");
        let size = furnicraft_geom::DVec3::new(
            envelope.x().millimetres_f64(),
            envelope.y().millimetres_f64(),
            envelope.z().millimetres_f64(),
        );
        let placement = project.world_placement(product).expect("a frame");
        let centre = placement.transform_point(size * 0.5);
        let rect = document.scene_rect().expect("a drawn frame gives the scene a rectangle");
        let camera = document.camera();
        let pointer = gizmo_math::screen_point(camera, rect, centre).expect("the frame is on screen");
        assert!(rect.contains(pointer), "the press lands on the scene: {pointer:?} in {rect:?}");
        document
            .editor
            .arm_fill(&document.session, ClassCode::HORIZONTAL_PANEL, Lang::En)
            .expect("the shelf fill arms inside the open frame");
        (product, harness.revision(), pointer)
    };

    harness.click(pointer, egui::PointerButton::Primary);

    let created = {
        let document = harness.app.tabs.active();
        // The ghost the click used is still on screen: it moved with the pointer, so the part that
        // was created is exactly what the maker was looking at, not a second measurement.
        let ghost = document.fill_gizmo.ghost().cloned().expect("the ghost that the click created");
        let created = document.editor.selected_node().expect("the new part is selected");
        let panel = document.session.project().panel(created).expect("the click created a panel");
        assert_eq!(panel.nominal_size(), ghost.nominal_size, "the part is exactly the ghost's size");
        assert_eq!(panel.placement(), ghost.placement, "and exactly the ghost's placement");
        assert_eq!(document.session.project().parent_of(created), Some(product), "it lands in the open frame");
        assert_eq!(
            document.session.project().class(created),
            Some(ClassCode::HORIZONTAL_PANEL),
            "the tool's class travels with the same command"
        );
        created
    };
    assert!(harness.revision() > before, "the click writes the document once");

    // One undo takes the whole fill back, class and all: the click was one entry, not two.
    let document = harness.app.tabs.active_mut();
    assert!(document.session.can_undo());
    document.session.undo().expect("one undo step");
    assert!(document.session.project().panel(created).is_none(), "one undo removes the part");
}

#[test]
fn a_shelf_in_a_splayed_niche_is_created_with_the_ghost_outline() {
    // Two posts stand at an angle, so the free section is a trapezoid: the ghost draws the polygon
    // and the click stores exactly that ring, not the rectangle around it (ADR 0099).
    let mut harness = Harness::new("frame-fill-shaped");
    harness.settle();
    harness.app.product_card.start();
    harness.settle();
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);

    let (product, pointer) = {
        let document = harness.app.tabs.active_mut();
        let product = document.session.active_product().expect("the card opens the frame");
        // The posts' inner faces run from (100, 0) to (200, 560) and from (500, 0) to (400, 560).
        let slant = 100.0 / 560.0_f64.hypot(100.0);
        let drop = 560.0 / 560.0_f64.hypot(100.0);
        for (through, normal) in [((100.0, 0.0), (drop, -slant)), ((500.0, 0.0), (-drop, -slant))] {
            let direction = (-normal.1, normal.0);
            let start_y = 280.0 - 600.0 * direction.1;
            let start_x = through.0 + (start_y - through.1) * (direction.0 / direction.1);
            let rotation = furnicraft_geom::DQuat::from_rotation_z(normal.1.atan2(normal.0))
                * furnicraft_geom::DQuat::from_rotation_y(std::f64::consts::FRAC_PI_2);
            let translation = furnicraft_geom::DVec3::new(start_x, start_y, 720.0)
                - furnicraft_geom::DVec3::new(normal.0, normal.1, 0.0) * 18.0;
            let placement = furnicraft_model::Placement::new(translation, rotation).expect("a placement");
            let nominal = furnicraft_model::PanelNominalSize::new(
                furnicraft_testkit::millimetres(720),
                furnicraft_testkit::millimetres(1200),
            )
            .expect("a post size");
            document
                .editor
                .create_panel_sized(&mut document.session, product, "Post".to_owned(), nominal, None, placement, None)
                .expect("add a post");
        }
        let project = document.session.project();
        let envelope = project.container(product).and_then(|product| product.envelope()).expect("an envelope");
        let size = furnicraft_geom::DVec3::new(
            envelope.x().millimetres_f64(),
            envelope.y().millimetres_f64(),
            envelope.z().millimetres_f64(),
        );
        let centre = project.world_placement(product).expect("a frame").transform_point(size * 0.5);
        let rect = document.scene_rect().expect("a drawn frame gives the scene a rectangle");
        let pointer = gizmo_math::screen_point(document.camera(), rect, centre).expect("the frame is on screen");
        assert!(rect.contains(pointer), "the press lands on the scene: {pointer:?} in {rect:?}");
        document
            .editor
            .arm_fill(&document.session, ClassCode::HORIZONTAL_PANEL, Lang::En)
            .expect("the shelf fill arms inside the open frame");
        (product, pointer)
    };

    harness.click(pointer, egui::PointerButton::Primary);

    let document = harness.app.tabs.active();
    let ghost = document.fill_gizmo.ghost().cloned().expect("the splayed void took a ghost");
    let outline = ghost.outline.clone().expect("the ghost carries the measured polygon");
    let created = document.editor.selected_node().expect("the new part is selected");
    let panel = document.session.project().panel(created).expect("the click created a panel");
    assert!(panel.is_contour(), "the shelf takes the polygon, not the rectangle around it");
    assert_eq!(Some(outline), panel.outline_contour().cloned(), "the stored ring is the ghost's ring");
    assert_eq!(panel.placement(), ghost.placement, "and the ghost's placement");
    assert_eq!(panel.nominal_size(), ghost.nominal_size, "and the ghost's size");
    assert_eq!(document.session.project().parent_of(created), Some(product), "it lands in the open frame");

    let document = harness.app.tabs.active_mut();
    document.session.undo().expect("one undo step");
    assert!(document.session.project().panel(created).is_none(), "one undo removes the shaped part");
}

#[test]
fn a_typed_distance_places_the_fill_exactly_and_the_click_creates_it() {
    let mut harness = Harness::new("frame-fill-typed");
    harness.settle();
    harness.app.product_card.start();
    harness.settle();
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);

    let (product, pointer, away) = {
        let document = harness.app.tabs.active_mut();
        let product = document.session.active_product().expect("the card opens the frame");
        let project = document.session.project();
        let envelope = project.container(product).and_then(|product| product.envelope()).expect("an envelope");
        let size = furnicraft_geom::DVec3::new(
            envelope.x().millimetres_f64(),
            envelope.y().millimetres_f64(),
            envelope.z().millimetres_f64(),
        );
        let placement = project.world_placement(product).expect("a frame");
        let centre = placement.transform_point(size * 0.5);
        // A point inside the void, off the centre line the two distance fields sit on, so the
        // creating click never lands on a plate.
        let away = placement.transform_point(furnicraft_geom::DVec3::new(size.x * 0.2, size.y * 0.2, size.z * 0.5));
        let rect = document.scene_rect().expect("a drawn frame gives the scene a rectangle");
        let camera = document.camera();
        let pointer = gizmo_math::screen_point(camera, rect, centre).expect("the frame is on screen");
        let away = gizmo_math::screen_point(camera, rect, away).expect("the left face is on screen");
        assert!(rect.contains(pointer) && rect.contains(away), "{away:?} in {rect:?}");
        document
            .editor
            .arm_fill(&document.session, ClassCode::HORIZONTAL_PANEL, Lang::En)
            .expect("the shelf fill arms inside the open frame");
        (product, pointer, away)
    };
    assert_eq!(product, harness.app.tabs.active().session.active_product().expect("the open frame"));

    // The pointer measures the void and the low field takes the keyboard without a click.
    harness.run(vec![egui::Event::PointerMoved(pointer)]);
    harness.settle();
    let low = harness.app.tabs.active().distance_rects.first().copied().expect("the low distance field drew");
    assert!(screen().contains_rect(low), "the field is on screen: {low:?}");
    // Aiming at the low field makes it the nearest one, so it owns the keyboard without a click.
    harness.run(vec![egui::Event::PointerMoved(low.center())]);
    harness.settle();
    harness.run(vec![egui::Event::Text("120".to_owned())]);
    harness.press(egui::Key::Enter, egui::Modifiers::NONE);

    // Typing and Enter pinned the low face at 120 mm: no document write has happened yet.
    let (before, ghost) = {
        let document = harness.app.tabs.active();
        let ghost = document.fill_gizmo.ghost().cloned().expect("the ghost is still measured");
        assert!((ghost.placement.translation().z - 120.0).abs() < 1.0e-9, "{:?}", ghost.placement);
        (harness.revision(), ghost)
    };
    let document = harness.app.tabs.active();
    assert_eq!(document.session.project().revision(), before, "the overlay writes nothing");

    // Moving the pointer away does not follow it: the pinned face stays where the number put it.
    harness.run(vec![egui::Event::PointerMoved(away)]);
    let still = harness.app.tabs.active().fill_gizmo.ghost().cloned().expect("the ghost stays");
    assert!((still.placement.translation().z - 120.0).abs() < 1.0e-9, "{:?}", still.placement);
    assert_eq!(still.placement, ghost.placement, "a pinned distance holds the pointer out");

    harness.click(away, egui::PointerButton::Primary);
    let document = harness.app.tabs.active();
    let created = document.editor.selected_node().expect("the click created a part");
    let panel = document.session.project().panel(created).expect("it is a panel");
    assert!((panel.placement().translation().z - 120.0).abs() < 1.0e-9, "{:?}", panel.placement());
    assert_eq!(panel.placement(), ghost.placement, "the click writes exactly the pinned ghost");
    assert_eq!(document.session.project().parent_of(created), Some(product));
    assert!(harness.revision() > before, "the click is the one write of the gesture");

    let document = harness.app.tabs.active_mut();
    document.session.undo().expect("one undo step");
    assert!(document.session.project().panel(created).is_none(), "one undo removes the pinned part");
}
