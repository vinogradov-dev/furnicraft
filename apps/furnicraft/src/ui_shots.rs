//! The interface rendered offscreen: one PNG per cell of the look matrix (U81).
//!
//! The eyes-only list in `.pi/skills/ui-design/references/checklist.md` — the light theme, a panel
//! at 55 % over the model, a 200 % scale — cannot be answered from rectangles, and until now the
//! only way to answer it was to start the window while the owner was working. A shot draws the real
//! frame into a texture instead of a window, copies the pixels back and writes them under
//! `target/ui-shots`, so a session reads a picture without touching the owner's screen.
//!
//! The one test is `#[ignore]`: `cargo test` never touches a GPU, and a machine with no adapter
//! prints one sentence and skips. The pictures are looked at, never compared pixel for pixel.
//!
//! Changes when: the matrix, a shot's file name or the offscreen paint path changes.

#![allow(clippy::expect_used)]

use std::path::Path;
use std::sync::mpsc;

use eframe::egui;
use eframe::egui_wgpu;
use eframe::wgpu;
use furnicraft_ui::settings::{
    DEFAULT_PANEL_OPACITY, MAX_PANEL_OPACITY, MIN_PANEL_OPACITY, PanelState, Settings, ThemeChoice,
};
use furnicraft_ui::theme::ThemePreset;
use furnicraft_ui::ui_scale;

use crate::FurniCraftApp;

/// The window a shot is drawn for, in points: the default launch size (M2).
const WINDOW: egui::Vec2 = egui::vec2(1280.0, 840.0);

/// How many frames a shot settles over before its image is taken.
///
/// `egui` runs a sizing pass for a new area, and the panels and the ribbon are measured on the frame
/// after they open (U78), so the picture is the third frame and never the first.
const SETTLE_FRAMES: usize = 3;

/// How far a shot moves the fitted view in, so the model runs under both floating panels.
///
/// The launch view fits the whole scene and leaves the middle of the window to it; the checklist
/// asks what a 55 % panel does *over the model*, so the picture is taken from a view the user could
/// have zoomed to, not from the fitted one (U81).
const MODEL_ZOOM: f64 = 2.5;

/// The panel opacities the matrix renders: the two ends of the slider and the default between them.
const OPACITIES: [u32; 3] = [MIN_PANEL_OPACITY, DEFAULT_PANEL_OPACITY, MAX_PANEL_OPACITY];

/// The two theme variants, in the order the matrix renders them.
const VARIANTS: [ThemeChoice; 2] = [ThemeChoice::Dark, ThemeChoice::Light];

/// The two fold states: both panels unfolded, then both folded to their strips.
const FOLDS: [bool; 2] = [false, true];

/// The two scales the matrix renders: the reference and the largest the control offers.
const SCALES: [u32; 2] = [100, ui_scale::MAX_PERCENT];

/// One cell of the matrix: what one picture shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shot {
    /// The look the whole window is painted with.
    preset: ThemePreset,
    /// The theme variant.
    variant: ThemeChoice,
    /// The opacity of the chrome drawn over the scene, in percent.
    opacity: u32,
    /// Whether both side panels are folded to their strips.
    folded: bool,
    /// The UI scale, in percent.
    scale: u32,
}

impl Shot {
    /// The file name of this shot, which says the cell it renders.
    ///
    /// The preset and the variant are the names the workspace file stores, so a picture and the
    /// workspace that would draw it carry the same words.
    fn name(&self) -> String {
        let variant = match self.variant {
            ThemeChoice::Light => "light",
            ThemeChoice::Dark | ThemeChoice::System => "dark",
        };
        let fold = if self.folded { "folded" } else { "expanded" };
        format!("{}-{variant}-opacity{}-{fold}-scale{}.png", self.preset.stored_name(), self.opacity, self.scale)
    }

    /// The workspace settings this cell draws with.
    fn settings(&self) -> Settings {
        let mut settings = Settings::default();
        settings.appearance.palette = self.preset;
        settings.appearance.theme = self.variant;
        settings.appearance.panel_opacity = self.opacity;
        settings.appearance.ui_scale = self.scale;
        let panel = PanelState { visible: true, expanded: !self.folded };
        settings.panels.scene = panel;
        settings.panels.inspector = panel;
        settings
    }
}

/// Every cell of the matrix: preset × variant × opacity × fold × scale.
fn shots() -> Vec<Shot> {
    let count = ThemePreset::ALL.len() * VARIANTS.len() * OPACITIES.len() * FOLDS.len() * SCALES.len();
    let mut shots = Vec::with_capacity(count);
    for preset in ThemePreset::ALL {
        for variant in VARIANTS {
            for opacity in OPACITIES {
                for folded in FOLDS {
                    for scale in SCALES {
                        shots.push(Shot { preset, variant, opacity, folded, scale });
                    }
                }
            }
        }
    }
    shots
}

/// The pixels one shot wrote, kept so the test can compare two cells without reading the files.
struct Picture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl Picture {
    /// The number of pixels whose colour differs between two pictures.
    fn differing_pixels(&self, other: &Self) -> usize {
        if (self.width, self.height) != (other.width, other.height) {
            return self.width as usize * self.height as usize;
        }
        let (a, _) = self.rgba.as_chunks::<4>();
        let (b, _) = other.rgba.as_chunks::<4>();
        a.iter().zip(b).filter(|(a, b)| a != b).count()
    }

    /// Whether the picture is a single flat colour, which no interface is.
    fn is_flat(&self) -> bool {
        let (pixels, _) = self.rgba.as_chunks::<4>();
        let Some(first) = pixels.first() else {
            return true;
        };
        !pixels.iter().any(|pixel| pixel != first)
    }
}

/// A device and a queue with no surface behind them, or the sentence that says why there is none.
fn render_state() -> Result<egui_wgpu::RenderState, String> {
    let configuration = egui_wgpu::WgpuConfiguration::default();
    let instance = wgpu::Instance::default();
    pollster::block_on(egui_wgpu::RenderState::create(
        &configuration,
        &instance,
        None,
        egui_wgpu::RendererOptions::PREDICTABLE,
    ))
    .map_err(|error| format!("this machine has no GPU adapter to render the interface shots with: {error}"))
}

/// Draws one cell offscreen, writes it to `path` and returns what it wrote.
fn capture(render_state: &egui_wgpu::RenderState, shot: &Shot, path: &Path) -> Result<Picture, String> {
    let ctx = egui::Context::default();
    let workspace = furnicraft_testkit::scratch_dir("ui-shots").join("workspace.json");
    let session = crate::scene_build::demo_session()?;
    let settings = shot.settings();
    let mut app = FurniCraftApp::test_app(&ctx, Some(render_state.clone()), workspace, settings, session)?;
    app.tabs.active_mut().zoom_active_view(1.0 / MODEL_ZOOM);

    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW);
    let jobs = settle(&mut app, &ctx, render_state, screen);
    let pixels_per_point = ctx.pixels_per_point();
    let size =
        [(WINDOW.x * pixels_per_point).round().max(1.0) as u32, (WINDOW.y * pixels_per_point).round().max(1.0) as u32];
    let rgba = paint(render_state, &jobs, size, pixels_per_point)?;
    // The scene texture is registered on the window's renderer and does not go away with the
    // document, so the next shot would leave one target per picture behind (U81).
    app.tabs.active_mut().release_target();
    furnicraft_testkit::write_png(path, size[0], size[1], &rgba)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Picture { width: size[0], height: size[1], rgba })
}

/// Runs the interface for [`SETTLE_FRAMES`] frames and returns the last frame's paint jobs.
///
/// Every frame's texture deltas reach the renderer, because the font atlas grows between frames and
/// the last frame's glyphs were uploaded by the ones before it; only the last frame is painted.
fn settle(
    app: &mut FurniCraftApp,
    ctx: &egui::Context,
    render_state: &egui_wgpu::RenderState,
    screen: egui::Rect,
) -> Vec<egui::ClippedPrimitive> {
    let mut jobs = Vec::new();
    for _ in 0..SETTLE_FRAMES {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            app.frame(ui);
        });
        let mut delta = std::mem::take(&mut output.textures_delta);
        {
            let mut renderer = render_state.renderer.write();
            for (id, images) in &delta.set {
                for image in images {
                    renderer.update_texture(&render_state.device, &render_state.queue, *id, image);
                }
            }
            for id in &delta.free {
                renderer.free_texture(id);
            }
        }
        // `epaint` asserts on drop that a delta reached a renderer; the renderer above is that
        // renderer, and `clear` is how it stops asking.
        delta.clear();
        jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    }
    jobs
}

/// Paints the interface into a texture of `size` and copies its pixels back, top row first.
fn paint(
    render_state: &egui_wgpu::RenderState,
    jobs: &[egui::ClippedPrimitive],
    size: [u32; 2],
    pixels_per_point: f32,
) -> Result<Vec<u8>, String> {
    // No surface means the renderer picks `Rgba8Unorm`, which is the channel order the PNG gets;
    // a device that ever answers with another order would write a blue sky red, so it is refused.
    if render_state.target_format != wgpu::TextureFormat::Rgba8Unorm {
        return Err(format!("a shot needs an RGBA target, this device paints {:?}", render_state.target_format));
    }
    let device = &render_state.device;
    let queue = &render_state.queue;
    let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("FurniCraft interface shot"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render_state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let descriptor = egui_wgpu::ScreenDescriptor { size_in_pixels: size, pixels_per_point };
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("FurniCraft interface shot") });
    let buffers = {
        let mut renderer = render_state.renderer.write();
        renderer.update_buffers(device, queue, &mut encoder, jobs, &descriptor)
    };
    {
        let attachment = Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            // The frame paints its own background on every pixel, so the clear colour is only what
            // a gap would show: a hostile layout is easier to see against black than against white.
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        });
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("FurniCraft interface shot"),
            color_attachments: &[attachment],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let mut pass = pass.forget_lifetime();
        render_state.renderer.read().render(&mut pass, jobs, &descriptor);
    }

    // A copy out of a texture pads every row to a 256-byte boundary, so the buffer is wider than
    // the picture and the padding is dropped on the way back to a tightly packed row.
    let row_bytes = size[0].checked_mul(4).ok_or_else(|| format!("a {} pixel row does not fit", size[0]))?;
    let padded = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("FurniCraft interface shot pixels"),
        size: u64::from(padded) * u64::from(size[1]),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(size[1]),
            },
        },
        extent,
    );
    queue.submit(buffers.into_iter().chain(std::iter::once(encoder.finish())));

    let slice = buffer.slice(..);
    let (sender, receiver) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|error| format!("the shot did not reach the CPU: {error}"))?;
    receiver
        .recv()
        .map_err(|error| format!("the shot was never mapped: {error}"))?
        .map_err(|error| format!("the shot could not be mapped: {error}"))?;

    let (padded, row_bytes) = (padded as usize, row_bytes as usize);
    let mut rgba = Vec::with_capacity(row_bytes.saturating_mul(size[1] as usize));
    let mut short = None;
    {
        let mapped = slice.get_mapped_range().map_err(|error| format!("the shot could not be read: {error}"))?;
        for row in 0..size[1] as usize {
            match mapped.get(row * padded..row * padded + row_bytes) {
                Some(line) => rgba.extend_from_slice(line),
                None => {
                    short = Some(row);
                    break;
                }
            }
        }
    }
    buffer.unmap();
    match short {
        Some(row) => Err(format!("the copy of row {row} is shorter than the picture")),
        None => Ok(rgba),
    }
}

#[test]
#[ignore = "writes target/ui-shots/*.png; run `cargo test -p furnicraft --bin furnicraft -- --ignored`"]
fn the_interface_renders_offscreen_to_one_png_per_cell() {
    let render_state = match render_state() {
        Ok(render_state) => render_state,
        Err(reason) => {
            println!("{reason}");
            return;
        }
    };
    let directory = furnicraft_testkit::ui_shots_dir().expect("the shots directory is created");

    // The one pair the test compares: the same cell at the two ends of the opacity slider. Every
    // other cell is written and dropped, because a picture of a 200 % window is seventeen megabytes
    // and the matrix would hold most of a gigabyte to compare two of them.
    let transparent = Shot {
        preset: ThemePreset::GraphiteCopper,
        variant: ThemeChoice::Dark,
        opacity: MIN_PANEL_OPACITY,
        folded: false,
        scale: 100,
    };
    let opaque = Shot { opacity: MAX_PANEL_OPACITY, ..transparent };
    let mut pair: [Option<Picture>; 2] = [None, None];

    let shots = shots();
    for shot in &shots {
        let path = directory.join(shot.name());
        let picture = capture(&render_state, shot, &path).expect("every cell renders");
        assert!(!picture.is_flat(), "{} draws less than one colour", path.display());
        if *shot == transparent {
            pair[0] = Some(picture);
        } else if *shot == opaque {
            pair[1] = Some(picture);
        }
    }
    println!("{} interface shots in {}", shots.len(), directory.display());

    // A 55 % panel and an opaque one reach different pixels: the scene under the chrome shows
    // through one and not the other. This is the only comparison the test makes; contrast, washout
    // and whether the model is legible through the fill are for the person looking.
    let transparent = pair[0].take().expect("the matrix drew the 55 % cell");
    let opaque = pair[1].take().expect("the matrix drew the 100 % cell");
    let differing = transparent.differing_pixels(&opaque);
    assert!(
        differing > 1_000,
        "the chrome reaches {differing} pixels at {MIN_PANEL_OPACITY} % but {MAX_PANEL_OPACITY} % draws the same picture"
    );
}

/// Draws the box tool's base rectangle and pulled height into one picture.
///
/// The gesture is driven through the real shell with pointer events, so the shot proves the whole
/// path — the plane mapping, the wireframe prism, the live size and the grips — not only the shape
/// the editor holds. Escape drops the draft afterwards: the shot writes no scene.
#[test]
#[ignore = "needs a GPU adapter; writes target/ui-shots/box-tool.png"]
fn the_box_tool_gesture_is_painted_offscreen() {
    let render_state = match render_state() {
        Ok(render_state) => render_state,
        Err(reason) => {
            println!("{reason}");
            return;
        }
    };
    let ctx = egui::Context::default();
    let workspace = furnicraft_testkit::scratch_dir("ui-shots").join("box-workspace.json");
    let session = furnicraft_engine::Session::with_catalog(
        furnicraft_testkit::sample_project(),
        furnicraft_testkit::sample_catalog(),
    );
    let mut app = FurniCraftApp::test_app(&ctx, Some(render_state.clone()), workspace, Settings::default(), session)
        .expect("the application builds");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW);
    let _ = settle(&mut app, &ctx, &render_state, screen);

    // Two points on the work plane, and the height the pointer pulls above the base.
    let corners = {
        let document = app.tabs.active_mut();
        document.editor.arm_box();
        let rect = document.scene_rect().expect("the settled frame gives the scene a rectangle");
        let camera = document.camera();
        let project = document.session.project();
        let _ = project;
        let corner = |x: f64, y: f64, z: f64| {
            crate::gizmo_math::screen_point(camera, rect, furnicraft_geom::DVec3::new(x, y, z))
                .expect("the work plane is on screen")
        };
        // The pull follows the normal through the base corner, which is where a maker drags it.
        let anchor = corner(-300.0, -200.0, 0.0);
        let pull = egui::Pos2::new(anchor.x, anchor.y - 140.0);
        (anchor, corner(300.0, 200.0, 0.0), pull)
    };
    let click = |pointer: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos: pointer,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let frames = vec![
        vec![egui::Event::PointerMoved(corners.0), click(corners.0, true), click(corners.0, false)],
        vec![egui::Event::PointerMoved(corners.1)],
        vec![click(corners.1, true), click(corners.1, false)],
        vec![egui::Event::PointerMoved(corners.2)],
        // One more frame at the same pointer: the bar's area lays itself out on the frame it
        // appears, so the picture is taken once it has.
        vec![egui::Event::PointerMoved(corners.2)],
    ];
    let mut jobs = Vec::new();
    for events in frames {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
            app.frame(ui);
        });
        let mut delta = std::mem::take(&mut output.textures_delta);
        {
            let mut renderer = render_state.renderer.write();
            for (id, images) in &delta.set {
                for image in images {
                    renderer.update_texture(&render_state.device, &render_state.queue, *id, image);
                }
            }
            for id in &delta.free {
                renderer.free_texture(id);
            }
        }
        delta.clear();
        jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    }
    {
        let document = app.tabs.active_mut();
        let draft = document.editor.box_draft().copied().expect("the gesture is pulling its height");
        assert!(draft.is_pulling() && draft.height().is_positive(), "the height follows the pointer");
        assert!(document.command_bar_is_up(), "the height pull draws its command bar: {draft:?}");
    }
    let pixels_per_point = ctx.pixels_per_point();
    let size =
        [(WINDOW.x * pixels_per_point).round().max(1.0) as u32, (WINDOW.y * pixels_per_point).round().max(1.0) as u32];
    let rgba = paint(&render_state, &jobs, size, pixels_per_point).expect("the frame paints");
    let directory = furnicraft_testkit::ui_shots_dir().expect("the shots directory is created");
    let path = directory.join("box-tool.png");
    furnicraft_testkit::write_png(&path, size[0], size[1], &rgba).expect("the picture is written");
    app.tabs.active_mut().release_target();
    let picture = Picture { width: size[0], height: size[1], rgba };
    assert!(!picture.is_flat(), "{} draws less than one colour", path.display());
    println!("the box tool is in {}", path.display());
}

/// Draws a selected box with its ten grips into one picture.
///
/// The grips are the three-side editor (ADR 0101): four base corners move two sides and six face
/// centres one, so the shot proves their placement, size and tone against the shaded box.
#[test]
#[ignore = "needs a GPU adapter; writes target/ui-shots/box-grips.png"]
fn the_box_grips_are_painted_offscreen() {
    let render_state = match render_state() {
        Ok(render_state) => render_state,
        Err(reason) => {
            println!("{reason}");
            return;
        }
    };
    let ctx = egui::Context::default();
    let workspace = furnicraft_testkit::scratch_dir("ui-shots").join("box-grips-workspace.json");
    let session = furnicraft_engine::Session::with_catalog(
        furnicraft_testkit::sample_project(),
        furnicraft_testkit::sample_catalog(),
    );
    let mut app = FurniCraftApp::test_app(&ctx, Some(render_state.clone()), workspace, Settings::default(), session)
        .expect("the application builds");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW);
    let _ = settle(&mut app, &ctx, &render_state, screen);
    {
        let document = app.tabs.active_mut();
        let root = document.session.project().root().id();
        let params = serde_json::to_value(furnicraft_engine::ObjectPrimitiveCreate {
            parent: root,
            name: "Box".to_owned(),
            primitive: furnicraft_model::Primitive::cuboid(
                furnicraft_testkit::millimetres(600),
                furnicraft_testkit::millimetres(400),
                furnicraft_testkit::millimetres(720),
            )
            .expect("a box"),
            placement: furnicraft_model::Placement::IDENTITY,
        })
        .expect("params");
        let box_id = document
            .session
            .dispatch(furnicraft_engine::object_cmds::OBJECT_PRIMITIVE_CREATE_ID, &params)
            .expect("the box is created")
            .created_id()
            .expect("an id");
        document.editor.select_node(Some(box_id));
        document.upload_scene().expect("the frame follows the new node");
    }
    let mut jobs = Vec::new();
    for _ in 0..SETTLE_FRAMES {
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            app.frame(ui);
        });
        let mut delta = std::mem::take(&mut output.textures_delta);
        {
            let mut renderer = render_state.renderer.write();
            for (id, images) in &delta.set {
                for image in images {
                    renderer.update_texture(&render_state.device, &render_state.queue, *id, image);
                }
            }
            for id in &delta.free {
                renderer.free_texture(id);
            }
        }
        delta.clear();
        jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    }
    let pixels_per_point = ctx.pixels_per_point();
    let size =
        [(WINDOW.x * pixels_per_point).round().max(1.0) as u32, (WINDOW.y * pixels_per_point).round().max(1.0) as u32];
    let rgba = paint(&render_state, &jobs, size, pixels_per_point).expect("the frame paints");
    let directory = furnicraft_testkit::ui_shots_dir().expect("the shots directory is created");
    let path = directory.join("box-grips.png");
    furnicraft_testkit::write_png(&path, size[0], size[1], &rgba).expect("the picture is written");
    app.tabs.active_mut().release_target();
    let picture = Picture { width: size[0], height: size[1], rgba };
    assert!(!picture.is_flat(), "{} draws less than one colour", path.display());
    println!("the box grips are in {}", path.display());
}

/// Draws the empty-space tool's ghost into one picture, so its accent and callouts can be looked at.
///
/// The matrix above varies the window's look; this shot varies the tool. It arms the shelf fill
/// inside an open product, moves the pointer onto the piece and renders the real frame offscreen, so
/// the ghost's wireframe and its three sizes are checked as pixels and not only as shapes.
#[test]
#[ignore = "needs a GPU adapter; writes target/ui-shots/fill-ghost.png"]
fn the_empty_space_ghost_is_painted_offscreen() {
    let render_state = match render_state() {
        Ok(render_state) => render_state,
        Err(reason) => {
            println!("{reason}");
            return;
        }
    };
    let ctx = egui::Context::default();
    let workspace = furnicraft_testkit::scratch_dir("ui-shots").join("fill-workspace.json");
    let (project, product) = furnicraft_testkit::sample_product_project();
    let mut session = furnicraft_engine::Session::with_catalog(project, furnicraft_testkit::sample_catalog());
    assert!(session.open_product(product), "the fixture product opens");
    let mut app = FurniCraftApp::test_app(&ctx, Some(render_state.clone()), workspace, Settings::default(), session)
        .expect("the application builds");
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW);
    let _ = settle(&mut app, &ctx, &render_state, screen);

    let pointer = {
        let document = app.tabs.active_mut();
        document
            .editor
            .arm_fill(&document.session, furnicraft_model::ClassCode::HORIZONTAL_PANEL, furnicraft_text::Lang::En)
            .expect("the shelf fill arms inside the open product");
        let project = document.session.project();
        let envelope = project.container(product).and_then(|product| product.envelope()).expect("an envelope");
        let size = furnicraft_geom::DVec3::new(
            envelope.x().millimetres_f64(),
            envelope.y().millimetres_f64(),
            envelope.z().millimetres_f64(),
        );
        let placement = project.world_placement(product).expect("a frame");
        let centre = placement.transform_point(size * 0.5);
        let rect = document.scene_rect().expect("the settled frame gives the scene a rectangle");
        crate::gizmo_math::screen_point(document.camera(), rect, centre).expect("the piece is on screen")
    };

    let mut jobs = Vec::new();
    for frame in 0..SETTLE_FRAMES {
        let events = if frame == 0 { vec![egui::Event::PointerMoved(pointer)] } else { Vec::new() };
        let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
            app.frame(ui);
        });
        let mut delta = std::mem::take(&mut output.textures_delta);
        {
            let mut renderer = render_state.renderer.write();
            for (id, images) in &delta.set {
                for image in images {
                    renderer.update_texture(&render_state.device, &render_state.queue, *id, image);
                }
            }
            for id in &delta.free {
                renderer.free_texture(id);
            }
        }
        delta.clear();
        jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    }
    assert!(app.tabs.active().fill_gizmo.ghost().is_some(), "the pointer measured a ghost to draw");
    let pixels_per_point = ctx.pixels_per_point();
    let size =
        [(WINDOW.x * pixels_per_point).round().max(1.0) as u32, (WINDOW.y * pixels_per_point).round().max(1.0) as u32];
    let rgba = paint(&render_state, &jobs, size, pixels_per_point).expect("the frame paints");
    let directory = furnicraft_testkit::ui_shots_dir().expect("the shots directory is created");
    let path = directory.join("fill-ghost.png");
    furnicraft_testkit::write_png(&path, size[0], size[1], &rgba).expect("the picture is written");
    app.tabs.active_mut().release_target();
    let picture = Picture { width: size[0], height: size[1], rgba };
    assert!(!picture.is_flat(), "{} draws less than one colour", path.display());
    println!("the fill ghost is in {}", path.display());
}
