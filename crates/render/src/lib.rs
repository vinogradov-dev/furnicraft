//! The wgpu renderer.
//!
//! Turns one scene frame into pixels: it owns the offscreen colour target, the depth and stencil
//! attachment, the uploaded geometry and the passes that draw it. The pipeline state lives in
//! `pipelines.rs`, the buffer wrappers in `gpu.rs` and the vertex builders in `geometry.rs`. It
//! knows nothing about `egui` and never
//! creates a window: the shell registers the colour texture with `egui_wgpu` and displays it as an
//! image. Scene geometry is uploaded when the document changes; camera changes only rewrite a small
//! uniform buffer.
//!
//! Layer: L5, above `scene` (which says what to draw) and below `ui` (which never links wgpu).
//!
//! Refuses: malformed geometry is a [`RenderError`] the shell can report, never a panic and never a
//! silently missing object; a hostile target size is clamped, not trusted.
//!
//! Does not build a single vertex: `geometry.rs` flattens what `scene` hands over, and a pass here
//! draws the buffers that came back. It does not decide which frame to draw, and it holds no
//! document data.
//!
//! Changes when: what a frame uploads, how a pass draws it, or the order the passes run in changes.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

mod geometry;
mod gpu;
mod pipelines;

use bytemuck::{Pod, Zeroable};
use furnicraft_geom::DVec3;
use furnicraft_scene::{Camera, ContactShadows, Lighting, MAX_POINT_LIGHTS, SceneFrame, ShadingStyle};
use thiserror::Error;
use wgpu::util::DeviceExt as _;

use self::geometry::{
    EdgeSlice, edge_geometry, grid_vertices, mesh_centre, polyline_vertices, product_envelope_vertices, sanitize_color,
    shadow_geometry, sort_back_to_front, stencil_slot, world_vertices,
};
use self::gpu::{GpuLineList, GpuMesh, GpuTriangles, clamp_size, line_list, triangle_list};
use self::pipelines::{
    create_line_pipeline, create_shadow_pipeline, create_triangle_pipeline, edge_depth_state, line_depth_state,
};

/// The offscreen colour format. `egui_wgpu::Renderer::register_native_texture` requires this
/// exact format, and it is not an sRGB format, so the shader writes gamma-corrected values.
pub(crate) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
/// The depth and stencil attachment of the offscreen target.
///
/// Depth chooses between the surfaces the frame draws. The stencil value records which drawable owns
/// each pixel, which is what keeps the *hidden edges* style honest: a line shows through its own
/// object and never through the object in front of it.
pub(crate) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// The most views one frame may split into; the camera uniform holds one slot per view.
pub const MAX_VIEWS: usize = 4;

/// The passes one display style turns on, derived without a GPU.
///
/// A style promises passes, not pipeline objects: the renderer turns this into the four pipelines it
/// built, and a test can assert the promise without a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StylePasses {
    /// Filled faces drawn with a depth write.
    pub opaque_faces: bool,
    /// Filled faces blended with no depth write.
    pub translucent_faces: bool,
    /// Edges tested against the faces.
    pub visible_edges: bool,
    /// Edges drawn through the faces.
    pub through_edges: bool,
}

impl StylePasses {
    /// The passes one style promises.
    pub const fn new(style: ShadingStyle) -> Self {
        Self {
            opaque_faces: style.draws_faces() && !style.is_translucent(),
            translucent_faces: style.is_translucent(),
            visible_edges: style.draws_edges() && !style.edges_through_faces(),
            through_edges: style.draws_edges() && style.edges_through_faces(),
        }
    }
}

/// A reason why scene geometry could not be uploaded.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum RenderError {
    /// A drawable has no triangles.
    #[error("drawable {index} has an empty mesh")]
    EmptyMesh {
        /// Position of the drawable in the frame.
        index: usize,
    },
    /// A drawable has malformed vertices, normals or indices.
    #[error("drawable {index} has an invalid mesh")]
    InvalidMesh {
        /// Position of the drawable in the frame.
        index: usize,
    },
    /// A drawable exceeds the renderer's vertex budget.
    #[error("drawable {index} has too many vertices")]
    MeshTooLarge {
        /// Position of the drawable in the frame.
        index: usize,
    },
    /// A polyline has fewer than two points, so it has no segment to draw.
    #[error("polyline {index} has fewer than two points")]
    EmptyPolyline {
        /// Position of the polyline in the frame.
        index: usize,
    },
    /// A polyline has malformed points or a non-finite transform.
    #[error("polyline {index} has invalid points")]
    InvalidPolyline {
        /// Position of the polyline in the frame.
        index: usize,
    },
    /// A polyline exceeds the renderer's vertex budget.
    #[error("polyline {index} has too many points")]
    PolylineTooLarge {
        /// Position of the polyline in the frame.
        index: usize,
    },
    /// A product envelope has a non-finite or non-positive size, or a malformed transform.
    #[error("product envelope {index} is malformed")]
    InvalidProductEnvelope {
        /// Position of the envelope in the frame.
        index: usize,
    },
    /// A product envelope's dashes exceed the renderer's vertex budget.
    #[error("product envelope {index} has too many dashes")]
    ProductEnvelopeTooLarge {
        /// Position of the envelope in the frame.
        index: usize,
    },
    /// A contact shadow has a non-finite centre or a degenerate radii pair.
    #[error("contact shadow {index} is malformed")]
    InvalidShadow {
        /// Position of the shadow in the frame.
        index: usize,
    },
    /// The frame carries more contact shadows than the renderer budgets.
    #[error("{count} contact shadows exceed the renderer's budget of {max}")]
    TooManyShadows {
        /// Number of shadows in the frame.
        count: usize,
        /// The renderer's budget.
        max: usize,
    },
}

/// Uniform data shared by the mesh and grid pipelines.
///
/// One slot describes one view: the view-projection, the eye, and the lighting that view shades
/// with. Splitting the workspace into several views therefore adds slots to one buffer instead of a
/// second camera buffer per view.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CameraUniform {
    view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
    light_direction: [f32; 4],
    light_color: [f32; 4],
    point_count: [u32; 4],
    point_position: [[f32; 4]; MAX_POINT_LIGHTS],
    /// Linear RGB and, in `a`, the radiant power in watts.
    point_color: [[f32; 4]; MAX_POINT_LIGHTS],
    /// Gamma-space RGB and, in `a`, the contact-shadow strength.
    shadow: [f32; 4],
}

struct RenderTargets {
    _color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    _depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
}

impl RenderTargets {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        // `COPY_SRC` in a test build is what lets an offscreen test read the picture back; the
        // product never copies its colour target.
        let copy = if cfg!(test) { wgpu::TextureUsages::COPY_SRC } else { wgpu::TextureUsages::empty() };
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("FurniCraft viewport colour"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | copy,
            view_formats: &[],
        });
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("FurniCraft viewport depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let color_view = color.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = depth.create_view(&wgpu::TextureViewDescriptor::default());
        Self { _color_texture: color, color_view, _depth_texture: depth, depth_view }
    }
}

/// One view's rectangle inside the shared offscreen target, in physical pixels.
///
/// `x` and `y` are the top-left corner counted from the target's top-left, which is also the
/// framebuffer origin wgpu uses: a scissor and a viewport share it, so no axis is flipped. Every
/// view is drawn with its own scissor into the one texture, so several views share one egui texture
/// id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewRegion {
    /// Left edge, in pixels.
    pub x: u32,
    /// Top edge, in pixels.
    pub y: u32,
    /// Width, in pixels; at least one.
    pub width: u32,
    /// Height, in pixels; at least one.
    pub height: u32,
}

impl ViewRegion {
    /// The wgpu scissor and viewport rectangle for this region.
    ///
    /// The rectangle is clamped to the target, so a rounding edge at the workspace boundary can
    /// never ask for an out-of-range scissor, which wgpu would reject.
    fn scissor(self, target_width: u32, target_height: u32) -> (u32, u32, u32, u32) {
        let x = self.x.min(target_width.saturating_sub(1));
        let y = self.y.min(target_height.saturating_sub(1));
        let width = self.width.min(target_width.saturating_sub(x)).max(1);
        let height = self.height.min(target_height.saturating_sub(y)).max(1);
        (x, y, width, height)
    }
}

/// One view's camera, rectangle and display style.
///
/// The style travels per view, so two neighbours in one workspace can draw the same scene with
/// different passes while one texture and one edge buffer still serve them (ADR 0070).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewSpec {
    /// The camera this view looks through.
    pub camera: Camera,
    /// The rectangle it owns in the shared target, in physical pixels.
    pub region: ViewRegion,
    /// The display style it draws with.
    pub shading: ShadingStyle,
}

impl ViewSpec {
    /// The passes this view's style turns on.
    ///
    /// The renderer asks each view for its own passes inside the pass loop; a test can assert the
    /// mapping without a device.
    pub const fn passes(self) -> StylePasses {
        StylePasses::new(self.shading)
    }
}

/// An offscreen scene renderer for one viewport.
///
/// The shell owns the window and the egui integration; this type owns the GPU scene resources and
/// exposes the colour view that egui should display.
pub struct ViewportRenderer {
    mesh_pipeline: wgpu::RenderPipeline,
    translucent_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    edge_pipeline: wgpu::RenderPipeline,
    hidden_edge_pipeline: wgpu::RenderPipeline,
    /// The unlit, blended triangle pass that draws the contact shadows.
    shadow_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    /// The stride between two camera slots, which is the device's uniform offset alignment.
    camera_stride: u64,
    drawables: Vec<GpuMesh>,
    /// World-space centres parallel to `drawables`, in upload order.
    centres: Vec<DVec3>,
    /// A reusable index list, sorted back to front for each translucent view.
    draw_order: Vec<usize>,
    /// The edges of every solid drawable, flattened into one line list.
    edges: Option<GpuLineList>,
    /// One slice of [`Self::edges`] per drawable, parallel to `drawables`, in upload order.
    ///
    /// The visible-edge style draws the whole buffer at once. The hidden-edge style needs each
    /// drawable's own slice, because its stencil reference is what limits an inside edge to the
    /// object it belongs to.
    edge_slices: Vec<EdgeSlice>,
    grid: Option<GpuLineList>,
    polylines: Option<GpuLineList>,
    /// The dashed boxes of the product envelopes, in one line buffer.
    product_envelopes: Option<GpuLineList>,
    /// The contact shadows of every floor-standing object, in one triangle buffer.
    shadows: Option<GpuTriangles>,
    targets: RenderTargets,
    size: [u32; 2],
}

impl ViewportRenderer {
    /// Creates a renderer with an offscreen target of the given physical size.
    ///
    /// The size is clamped to at least one pixel and to the device's maximum texture dimension.
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let size = clamp_size(width, height, device.limits().max_texture_dimension_2d);
        let targets = RenderTargets::new(device, size[0], size[1]);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("FurniCraft scene shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });

        let uniform_size = std::mem::size_of::<CameraUniform>() as u64;
        let alignment = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(1);
        let camera_stride = uniform_size.next_multiple_of(alignment);
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("FurniCraft camera uniform"),
            size: camera_stride * MAX_VIEWS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("FurniCraft camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(uniform_size),
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("FurniCraft camera bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &camera_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(uniform_size),
                }),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("FurniCraft scene pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let mesh_pipeline = create_triangle_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft shaded mesh pipeline",
            "fs_mesh",
            ShadingStyle::Shaded,
        );
        let translucent_pipeline = create_triangle_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft translucent mesh pipeline",
            "fs_mesh_translucent",
            ShadingStyle::Translucent,
        );
        let line_pipeline =
            create_line_pipeline(device, &pipeline_layout, &shader, "FurniCraft line pipeline", line_depth_state());
        let edge_pipeline = create_line_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft edge pipeline",
            edge_depth_state(false),
        );
        let hidden_edge_pipeline = create_line_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft hidden edge pipeline",
            edge_depth_state(true),
        );
        let shadow_pipeline = create_shadow_pipeline(device, &pipeline_layout, &shader);

        Self {
            mesh_pipeline,
            translucent_pipeline,
            line_pipeline,
            edge_pipeline,
            hidden_edge_pipeline,
            shadow_pipeline,
            camera_buffer,
            camera_bind_group,
            camera_stride,
            drawables: Vec::new(),
            centres: Vec::new(),
            draw_order: Vec::new(),
            edges: None,
            edge_slices: Vec::new(),
            grid: None,
            polylines: None,
            product_envelopes: None,
            shadows: None,
            targets,
            size,
        }
    }

    /// The current colour target's size in physical pixels.
    pub const fn size(&self) -> [u32; 2] {
        self.size
    }

    /// The view egui should sample.
    pub const fn color_view(&self) -> &wgpu::TextureView {
        &self.targets.color_view
    }

    /// Recreates the colour and depth targets when their size changes.
    ///
    /// Returns `true` when the target was recreated and the shell must re-register the colour view
    /// with `egui_wgpu`.
    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        let size = clamp_size(width, height, device.limits().max_texture_dimension_2d);
        if size == self.size {
            return false;
        }
        self.targets = RenderTargets::new(device, size[0], size[1]);
        self.size = size;
        true
    }

    /// Rebuilds the GPU geometry for a scene frame.
    ///
    /// Call this only when the document revision changes; camera movement does not need an upload.
    pub fn upload(&mut self, device: &wgpu::Device, frame: &SceneFrame) -> Result<(), RenderError> {
        let mut drawables = Vec::with_capacity(frame.drawables().len());
        for (index, drawable) in frame.drawables().iter().enumerate() {
            let (vertices, indices) = world_vertices(index, &drawable.mesh, drawable.world_transform, drawable.color)?;
            if vertices.is_empty() || indices.is_empty() {
                return Err(RenderError::EmptyMesh { index });
            }
            let index_count = u32::try_from(indices.len()).map_err(|_| RenderError::MeshTooLarge { index })?;
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("FurniCraft drawable vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("FurniCraft drawable indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let centre = mesh_centre(&drawable.mesh, drawable.world_transform);
            drawables.push(GpuMesh {
                vertex_buffer,
                index_buffer,
                index_count,
                centre,
                stencil_reference: stencil_slot(index),
            });
        }

        let grid = line_list(device, "FurniCraft grid vertices", grid_vertices(&frame.grid))?;
        let polylines =
            line_list(device, "FurniCraft path vertices", polyline_vertices(frame.polylines(), frame.line_ink)?)?;
        let product_envelopes = line_list(
            device,
            "FurniCraft product envelope vertices",
            product_envelope_vertices(
                frame.product_envelopes(),
                frame.product_envelope_active,
                frame.product_envelope_inactive,
            )?,
        )?;
        let (edge_vertices, edge_slices) = edge_geometry(frame.drawables(), frame.all_edges, frame.background);
        let edges = line_list(device, "FurniCraft edge vertices", edge_vertices)?;
        let (shadow_vertices, shadow_indices) = shadow_geometry(frame.shadows())?;
        let shadows = triangle_list(device, "FurniCraft contact shadows", &shadow_vertices, &shadow_indices);

        let draw_order: Vec<usize> = (0..drawables.len()).collect();
        self.drawables = drawables;
        self.centres = self.drawables.iter().map(|mesh| mesh.centre).collect();
        self.draw_order = draw_order;
        self.edges = edges;
        self.edge_slices = edge_slices;
        self.grid = grid;
        self.polylines = polylines;
        self.product_envelopes = product_envelopes;
        self.shadows = shadows;
        Ok(())
    }

    /// Draws the frame into the offscreen colour target.
    ///
    /// The camera and lighting are read from the frame, so orbit, pan and zoom cost one uniform
    /// write rather than a geometry upload.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &SceneFrame) {
        self.render_with_lighting(device, queue, frame, &frame.lighting);
    }

    /// Draws the frame into the whole target with lighting that may differ from the uploaded scene.
    ///
    /// A drag preview commits nothing, but the uniform can still follow a moved light: a sun
    /// direction or a point-light position updates here without touching geometry buffers.
    pub fn render_with_lighting(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        lighting: &Lighting,
    ) {
        let region = ViewRegion { x: 0, y: 0, width: self.size[0], height: self.size[1] };
        let view = ViewSpec { camera: frame.camera, region, shading: frame.shading };
        self.render_views(device, queue, frame, lighting, &[view]);
    }

    /// Draws the frame once per view into one target, each pass scissored to its own rectangle.
    ///
    /// One texture and one egui texture id serve the whole workspace, so splitting the viewport
    /// costs a pass and a uniform slot rather than a second texture. The passes share one cleared
    /// attachment: the first clears the colour and depth, every other pass loads them, and each
    /// draws inside its own viewport and scissor so a view never writes over its neighbour.
    ///
    /// Each view carries its own camera, rectangle and display style, so a split workspace draws a
    /// neighbour with a different pass set without a second target or edge buffer.
    ///
    /// At most [`MAX_VIEWS`] views are drawn; a longer slice is truncated. An empty slice clears the
    /// target and draws nothing.
    pub fn render_views(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        lighting: &Lighting,
        views: &[ViewSpec],
    ) {
        let background = sanitize_color(frame.background);
        let clear = wgpu::Color {
            r: f64::from(background[0]),
            g: f64::from(background[1]),
            b: f64::from(background[2]),
            a: f64::from(background[3]),
        };

        let views = &views[..views.len().min(MAX_VIEWS)];
        let mut offsets = Vec::with_capacity(views.len());
        let mut uniforms = Vec::with_capacity(views.len());
        for (index, view) in views.iter().enumerate() {
            let region = view.region;
            let aspect = f64::from(region.width.max(1)) / f64::from(region.height.max(1));
            uniforms.push(self.camera_uniform(view.camera, lighting, aspect, &frame.contact_shadows));
            offsets.push((index as u64 * self.camera_stride) as u32);
            let (x, y, width, height) = region.scissor(self.size[0], self.size[1]);
            debug_assert!(x.saturating_add(width) <= self.size[0] && y.saturating_add(height) <= self.size[1]);
        }
        for (index, uniform) in uniforms.iter().enumerate() {
            queue.write_buffer(&self.camera_buffer, index as u64 * self.camera_stride, bytemuck::bytes_of(uniform));
        }

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("FurniCraft scene encoder") });
        // One clear for the whole target, so a view that does not cover every pixel of a rounding
        // edge leaves the background there instead of the previous frame's image.
        {
            let color_attachment = Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(clear), store: wgpu::StoreOp::Store },
            });
            let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
            };
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("FurniCraft clear pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: Some(depth_attachment),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }

        // The shared clear resets depth and stencil together. The stencil slot is what tells the
        // hidden-edge pass whose inside it is looking at.
        for (view, offset) in views.iter().zip(offsets) {
            let camera = view.camera;
            let region = view.region;
            // Each view derives its own passes, so a neighbour may draw wireframe while this one
            // draws shaded faces with hidden edges.
            let passes = view.passes();
            let (x, y, width, height) = region.scissor(self.size[0], self.size[1]);
            let color_attachment = Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            });
            let depth_attachment = wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_view,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                // Each view owns its own rectangle but shares the stencil buffer, so the slots a
                // neighbour stamped are reset before this view stamps its own.
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("FurniCraft view pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: Some(depth_attachment),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(x as f32, y as f32, width as f32, height as f32, 0.0, 1.0);
            pass.set_scissor_rect(x, y, width, height);
            pass.set_bind_group(0, &self.camera_bind_group, &[offset]);

            if passes.opaque_faces && !self.drawables.is_empty() {
                pass.set_pipeline(&self.mesh_pipeline);
                for mesh in &self.drawables {
                    // The reference is the slot this drawable stamps; the hidden-edge pass reads it
                    // back before drawing the same drawable's lines.
                    pass.set_stencil_reference(mesh.stencil_reference);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }

            // Contact shadows lie on the floor: they draw after the opaque faces, so the depth
            // buffer hides the part an object covers, and before the unlit grid lines, which sit a
            // fraction of a millimetre higher. The pass is blended and writes no depth, so two
            // overlapping footprints simply tint each other.
            if frame.contact_shadows.draws()
                && let Some(shadows) = &self.shadows
            {
                pass.set_pipeline(&self.shadow_pipeline);
                pass.set_vertex_buffer(0, shadows.vertex_buffer.slice(..));
                pass.set_index_buffer(shadows.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..shadows.index_count, 0, 0..1);
            }

            if self.grid.is_some() || self.polylines.is_some() {
                pass.set_pipeline(&self.line_pipeline);
                if let Some(grid) = &self.grid {
                    pass.set_vertex_buffer(0, grid.vertex_buffer.slice(..));
                    pass.draw(0..grid.vertex_count, 0..1);
                }
                if let Some(polylines) = &self.polylines {
                    pass.set_vertex_buffer(0, polylines.vertex_buffer.slice(..));
                    pass.draw(0..polylines.vertex_count, 0..1);
                }
            }

            // Blended faces go after the opaque scene and back to front, so a nearer panel tints a
            // farther one instead of hiding it. The order is per view: two views see the same
            // centre from opposite sides.
            if passes.translucent_faces && !self.drawables.is_empty() {
                sort_back_to_front(&self.centres, camera.eye(), &mut self.draw_order);
                pass.set_pipeline(&self.translucent_pipeline);
                for &index in &self.draw_order {
                    let Some(mesh) = self.drawables.get(index) else {
                        continue;
                    };
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }

            // Edges last, so a line sits on top of the faces it belongs to. *Shaded with hidden
            // edges* uses the stencil-gated pass, *wireframe* reaches here with no faces before it.
            //
            // A single flattened list is drawn once for the visible style. The hidden style needs
            // the drawable's stencil slot as the reference value, so it walks the drawables and
            // draws each drawable's own slice of the same buffer.
            if passes.visible_edges
                && let Some(edges) = &self.edges
            {
                pass.set_pipeline(&self.edge_pipeline);
                pass.set_vertex_buffer(0, edges.vertex_buffer.slice(..));
                pass.draw(0..edges.vertex_count, 0..1);
            } else if passes.through_edges
                && let Some(edges) = &self.edges
            {
                pass.set_pipeline(&self.hidden_edge_pipeline);
                pass.set_vertex_buffer(0, edges.vertex_buffer.slice(..));
                for slice in &self.edge_slices {
                    if slice.vertex_count == 0 {
                        continue;
                    }
                    pass.set_stencil_reference(slice.stencil_reference);
                    pass.draw(slice.first_vertex..slice.first_vertex + slice.vertex_count, 0..1);
                }
            }

            // The product envelopes draw last, after every solid pass: a box is reserved space,
            // never a body, so its dashes write no depth and a panel in front still hides it.
            if let Some(envelopes) = &self.product_envelopes {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_vertex_buffer(0, envelopes.vertex_buffer.slice(..));
                pass.draw(0..envelopes.vertex_count, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    /// The uniform one view shades with.
    fn camera_uniform(
        &self,
        camera: Camera,
        lighting: &Lighting,
        aspect: f64,
        shadows: &ContactShadows,
    ) -> CameraUniform {
        let eye = camera.eye();
        let light = lighting.key_direction.try_normalize().unwrap_or(DVec3::Z);
        let (point_count, point_position, point_color) = point_uniforms(lighting);
        CameraUniform {
            view_projection: camera.view_projection(aspect).as_mat4().to_cols_array_2d(),
            camera_position: [eye.x as f32, eye.y as f32, eye.z as f32, 1.0],
            light_direction: [light.x as f32, light.y as f32, light.z as f32, 0.0],
            light_color: [
                lighting.key_color[0],
                lighting.key_color[1],
                lighting.key_color[2],
                lighting.ambient.clamp(0.0, 1.0),
            ],
            point_count,
            point_position,
            point_color,
            shadow: shadow_uniform(shadows),
        }
    }
}

/// The shadow colour and strength the fragment shader reads.
///
/// A switch that is off, a zero strength or a hostile value collapses to zero alpha, so the pass
/// blends nothing; the colour is kept so turning the strength back up needs no upload.
fn shadow_uniform(shadows: &ContactShadows) -> [f32; 4] {
    let strength = if shadows.draws() { shadows.strength.clamp(0.0, 1.0) } else { 0.0 };
    [shadows.color[0], shadows.color[1], shadows.color[2], strength]
}

/// Packs the point lights into the fixed uniform arrays, stopping at the frame budget.
fn point_uniforms(lighting: &Lighting) -> ([u32; 4], [[f32; 4]; MAX_POINT_LIGHTS], [[f32; 4]; MAX_POINT_LIGHTS]) {
    let mut positions = [[0.0; 4]; MAX_POINT_LIGHTS];
    let mut colors = [[0.0; 4]; MAX_POINT_LIGHTS];
    let mut count = 0usize;
    for (index, point) in lighting.points.iter().take(MAX_POINT_LIGHTS).enumerate() {
        let position = if point.position.is_finite() { point.position } else { DVec3::ZERO };
        let color = if point.color.iter().all(|component| component.is_finite()) { point.color } else { [1.0; 3] };
        let power = if point.power.is_finite() && point.power >= 0.0 { point.power } else { 0.0 };
        positions[index] = [position.x as f32, position.y as f32, position.z as f32, 0.0];
        colors[index] = [color[0], color[1], color[2], power];
        count += 1;
    }
    ([count as u32, 0, 0, 0], positions, colors)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::sync::Arc;

    use super::*;
    use crate::geometry::{
        EDGE_COLOR_DARK, EDGE_COLOR_LIGHT, MAX_CONTACT_SHADOWS, MAX_GRID_DIVISIONS, SHADOW_SEGMENTS, edge_color,
    };
    use crate::pipelines::{FACE_DEPTH_BIAS, MAX_STENCIL_OWNERS, STENCIL_MASK, face_depth_state, shadow_depth_state};
    use furnicraft_geom::{DMat4, MeshData};
    use furnicraft_model::Id;
    use furnicraft_scene::{
        ContactShadow, Drawable, Grid, Polyline, ProductEnvelope, ProductEnvelopeState, SceneColors,
    };

    fn triangle() -> MeshData {
        MeshData {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn clamp_size_never_returns_zero_or_exceeds_the_device_limit() {
        assert_eq!(clamp_size(0, 9999, 4096), [1, 4096]);
        assert_eq!(clamp_size(800, 600, 4096), [800, 600]);
    }

    #[test]
    fn vertices_are_transformed_into_world_space() {
        let mesh = triangle();
        let transform = DMat4::from_translation(DVec3::new(100.0, 20.0, 5.0));
        let (vertices, indices) = world_vertices(0, &mesh, transform, [1.0, 0.5, 0.25, 1.0]).expect("valid mesh");

        assert_eq!(indices, vec![0, 1, 2]);
        assert_eq!(vertices.len(), 3);
        assert_eq!(vertices[0].position, [100.0, 20.0, 5.0]);
        assert_eq!(vertices[1].position, [110.0, 20.0, 5.0]);
        assert!((vertices[0].normal[2] - 1.0).abs() < 1.0e-6);
        assert_eq!(vertices[0].color, [1.0, 0.5, 0.25, 1.0]);
    }

    #[test]
    fn meshes_without_normals_are_expanded_for_flat_shading() {
        let mesh = MeshData {
            positions: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
            normals: Vec::new(),
            indices: vec![0, 1, 2],
        };
        let (vertices, indices) = world_vertices(0, &mesh, DMat4::IDENTITY, [1.0; 4]).expect("valid mesh");
        assert_eq!(vertices.len(), 3);
        assert_eq!(indices, vec![0, 1, 2]);
        assert!((vertices[0].normal[2] - 1.0).abs() < 1.0e-6);
    }

    fn line_chain(transform: DMat4) -> Polyline {
        let points: Arc<[DVec3]> =
            Arc::from(vec![DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0), DVec3::new(10.0, 10.0, 0.0)]);
        Polyline::new(Id::new(1).expect("id"), points, transform, [1.0, 0.5, 0.25, 1.0])
    }

    #[test]
    fn a_polyline_becomes_one_line_list_segment_per_pair() {
        let transform = DMat4::from_translation(DVec3::new(100.0, 20.0, 5.0));
        let vertices =
            polyline_vertices(&[line_chain(transform)], SceneFrame::default().line_ink).expect("valid polyline");

        assert_eq!(vertices.len(), 4, "two segments, two vertices each");
        assert_eq!(vertices[0].position, [100.0, 20.0, 5.0]);
        assert_eq!(vertices[1].position, [110.0, 20.0, 5.0]);
        assert_eq!(vertices[2].position, [110.0, 20.0, 5.0]);
        assert_eq!(vertices[3].position, [110.0, 30.0, 5.0]);
        assert_eq!(vertices[0].color, [1.0, 0.5, 0.25, 1.0]);
    }

    #[test]
    fn a_polyline_needs_two_finite_points() {
        let id = Id::new(1).expect("id");
        let empty = Polyline::new(id, Arc::from(Vec::<DVec3>::new()), DMat4::IDENTITY, [1.0; 4]);
        assert_eq!(
            polyline_vertices(&[empty], SceneFrame::default().line_ink),
            Err(RenderError::EmptyPolyline { index: 0 })
        );

        let bad_point =
            Polyline::new(id, Arc::from(vec![DVec3::ZERO, DVec3::new(f64::NAN, 0.0, 0.0)]), DMat4::IDENTITY, [1.0; 4]);
        assert_eq!(
            polyline_vertices(&[bad_point], SceneFrame::default().line_ink),
            Err(RenderError::InvalidPolyline { index: 0 })
        );

        let bad_transform = Polyline::new(
            id,
            Arc::from(vec![DVec3::ZERO, DVec3::X]),
            DMat4::from_translation(DVec3::new(f64::NAN, 0.0, 0.0)),
            [1.0; 4],
        );
        assert_eq!(
            polyline_vertices(&[bad_transform], SceneFrame::default().line_ink),
            Err(RenderError::InvalidPolyline { index: 0 })
        );
    }

    fn product_envelope(id: u64, size: DVec3, state: ProductEnvelopeState) -> ProductEnvelope {
        ProductEnvelope { id: Id::new(id).expect("id"), size, world_transform: DMat4::IDENTITY, state }
    }

    #[test]
    fn a_product_envelope_becomes_dashed_lines_in_its_state_colour() {
        let active = SceneColors::DARK.product_envelope_active;
        let inactive = SceneColors::DARK.product_envelope_inactive;
        let open = product_envelope(1, DVec3::new(600.0, 400.0, 700.0), ProductEnvelopeState::Open);
        let vertices = product_envelope_vertices(&[open], active, inactive).expect("a valid envelope");

        // Twelve edges, each at most one dash cycle per 1/24 of the longest side.
        assert_eq!(vertices.len() % 2, 0, "a line list is drawn in pairs");
        assert!(vertices.len() > 12, "twelve edges need more than one dash each");
        assert!(vertices.len() <= 12 * 2 * 26, "the dash cycle bounds the vertex count: {}", vertices.len());
        assert!(vertices.iter().all(|vertex| vertex.color == active), "an open product draws in the active ink");

        // The first edge runs along X from the zero corner; the first dash is painted and the next
        // one starts after a gap, so the edge is visibly not a solid line.
        assert_eq!(vertices[0].position, [0.0, 0.0, 0.0]);
        let first_dash_end = vertices[1].position[0];
        assert!(first_dash_end > 0.0 && first_dash_end < 600.0, "a dash stops before the corner");
        let second_dash_start = vertices[2].position[0];
        assert!(second_dash_start > first_dash_end, "a gap separates two dashes");
        assert!(first_dash_end < 20.0, "the first dash is short: {first_dash_end}");

        let closed = product_envelope(2, DVec3::splat(500.0), ProductEnvelopeState::ClosedEmpty);
        let vertices = product_envelope_vertices(&[closed], active, inactive).expect("a valid envelope");
        assert!(vertices.iter().all(|vertex| vertex.color == inactive), "a closed empty piece is inactive");
        assert!(!vertices.is_empty());

        let with_parts = product_envelope(3, DVec3::splat(500.0), ProductEnvelopeState::ClosedWithParts);
        assert!(
            product_envelope_vertices(&[with_parts], active, inactive).expect("a valid envelope").is_empty(),
            "a closed piece with parts draws no lines"
        );
    }

    #[test]
    fn a_product_envelope_follows_its_transform_and_refuses_a_malformed_box() {
        let active = SceneColors::DARK.product_envelope_active;
        let inactive = SceneColors::DARK.product_envelope_inactive;
        let mut moved = product_envelope(1, DVec3::splat(100.0), ProductEnvelopeState::Open);
        moved.world_transform = DMat4::from_translation(DVec3::new(1000.0, 0.0, 0.0));
        let vertices = product_envelope_vertices(&[moved], active, inactive).expect("a valid envelope");
        assert!(vertices.iter().all(|vertex| vertex.position[0] >= 1000.0), "the box is drawn in world space");

        let zero = product_envelope(2, DVec3::ZERO, ProductEnvelopeState::Open);
        assert_eq!(
            product_envelope_vertices(&[zero], active, inactive),
            Err(RenderError::InvalidProductEnvelope { index: 0 })
        );
        let nan = product_envelope(3, DVec3::new(f64::NAN, 100.0, 100.0), ProductEnvelopeState::Open);
        assert_eq!(
            product_envelope_vertices(&[nan], active, inactive),
            Err(RenderError::InvalidProductEnvelope { index: 0 })
        );
        assert!(
            product_envelope_vertices(&[], active, inactive).is_ok_and(|vertices| vertices.is_empty()),
            "no envelopes cost no vertices"
        );
    }

    #[test]
    fn a_closed_envelope_with_parts_is_not_sent_to_the_gpu() {
        let active = SceneColors::LIGHT.product_envelope_active;
        let inactive = SceneColors::LIGHT.product_envelope_inactive;
        let hidden = product_envelope(1, DVec3::splat(100.0), ProductEnvelopeState::ClosedWithParts);
        let open = product_envelope(2, DVec3::splat(100.0), ProductEnvelopeState::Open);
        let vertices = product_envelope_vertices(&[hidden, open], active, inactive).expect("valid envelopes");
        assert!(vertices.iter().all(|vertex| vertex.color == active), "only the open box is drawn");
    }

    #[test]
    fn a_polyline_without_a_colour_draws_in_the_frame_ink() {
        let points: Arc<[DVec3]> = Arc::from(vec![DVec3::ZERO, DVec3::X]);
        let id = Id::new(1).expect("id");
        let chain = Polyline::in_ink(id, points, DMat4::IDENTITY);

        let mut frame = SceneFrame::default();
        frame.push_polyline(chain);
        frame.apply_colors(furnicraft_scene::SceneColors::LIGHT);
        let vertices = polyline_vertices(frame.polylines(), frame.line_ink).expect("valid polyline");
        assert_eq!(vertices[0].color, furnicraft_scene::SceneColors::LIGHT.line_ink);
        assert_ne!(vertices[0].color, furnicraft_scene::SceneColors::DARK.line_ink, "the ink followed the theme");
    }

    #[test]
    fn malformed_meshes_are_rejected_before_upload() {
        let mut mesh = triangle();
        mesh.indices[2] = 3;
        assert_eq!(world_vertices(4, &mesh, DMat4::IDENTITY, [1.0; 4]), Err(RenderError::InvalidMesh { index: 4 }));

        let mut mesh = triangle();
        mesh.normals.pop();
        assert_eq!(world_vertices(0, &mesh, DMat4::IDENTITY, [1.0; 4]), Err(RenderError::InvalidMesh { index: 0 }));

        let empty = MeshData::default();
        assert_eq!(world_vertices(0, &empty, DMat4::IDENTITY, [1.0; 4]), Err(RenderError::EmptyMesh { index: 0 }));
    }

    #[test]
    fn grid_geometry_has_axes_and_major_lines() {
        let grid = Grid { spacing_mm: 100.0, extent_mm: 200.0, major_every: 2, ..Grid::default() };
        let vertices = grid_vertices(&grid);

        assert_eq!(vertices.len(), 5 * 4, "five lines in each direction");
        // Five offsets from -200 to 200; at offset zero the X line starts at vertex 8 and the
        // Y line at vertex 10.
        assert_eq!(vertices[8].color, grid.axis_x_color);
        assert_eq!(vertices[10].color, grid.axis_y_color);
        assert_eq!(vertices[0].color, grid.major_color);
        assert_eq!(vertices[4].color, grid.minor_color);
        assert!(vertices.iter().all(|vertex| vertex.position[2] > 0.0));
    }

    #[test]
    fn grid_values_are_clamped_to_a_bounded_line_count() {
        let grid = Grid { spacing_mm: 1.0, extent_mm: f64::MAX, major_every: 1, ..Grid::default() };
        let vertices = grid_vertices(&grid);
        assert_eq!(vertices.len(), (2 * MAX_GRID_DIVISIONS as usize + 1) * 4);
    }

    #[test]
    fn a_hidden_grid_costs_no_vertices() {
        let grid = Grid { visible: false, ..Grid::default() };
        assert!(grid_vertices(&grid).is_empty(), "a hidden grid must not build line geometry");
    }

    fn contact_shadow() -> ContactShadow {
        ContactShadow { id: Id::new(1).expect("id"), center: DVec3::new(10.0, 20.0, 0.01), radii: [100.0, 50.0] }
    }

    #[test]
    fn a_contact_shadow_becomes_one_radial_triangle_fan_on_the_floor() {
        let shadow = contact_shadow();
        let (vertices, indices) = shadow_geometry(&[shadow]).expect("a valid shadow");
        assert_eq!(vertices.len(), SHADOW_SEGMENTS + 1, "a centre and one rim vertex per segment");
        assert_eq!(indices.len(), SHADOW_SEGMENTS * 3, "one triangle per segment");
        assert_eq!(vertices[0].position, [10.0, 20.0, 0.01], "the centre sits on the shadow plane");
        assert_eq!(vertices[0].color[3], 1.0, "the centre is the opaque end of the falloff");
        assert!(vertices[1..].iter().all(|vertex| vertex.color[3] == 0.0), "the rim fades to nothing");
        for vertex in &vertices[1..] {
            let x = (f64::from(vertex.position[0]) - 10.0) / 100.0;
            let y = (f64::from(vertex.position[1]) - 20.0) / 50.0;
            assert!(x * x + y * y <= 1.0 + 1.0e-5, "every rim vertex lies on the ellipse");
        }
        assert!(shadow_geometry(&[]).is_ok_and(|(vertices, indices)| vertices.is_empty() && indices.is_empty()));
    }

    #[test]
    fn a_malformed_contact_shadow_is_refused() {
        let id = Id::new(2).expect("id");
        let non_finite = ContactShadow { id, center: DVec3::new(f64::NAN, 0.0, 0.0), radii: [1.0, 1.0] };
        assert_eq!(shadow_geometry(&[non_finite]), Err(RenderError::InvalidShadow { index: 0 }));
        let degenerate = ContactShadow { id, center: DVec3::ZERO, radii: [0.0, 1.0] };
        assert_eq!(shadow_geometry(&[degenerate]), Err(RenderError::InvalidShadow { index: 0 }));

        let many = vec![contact_shadow(); MAX_CONTACT_SHADOWS + 1];
        assert!(matches!(shadow_geometry(&many), Err(RenderError::TooManyShadows { .. })));
    }

    #[test]
    fn the_shadow_pass_lies_on_the_floor_and_writes_no_depth() {
        let state = shadow_depth_state();
        assert_eq!(state.depth_write_enabled, Some(false), "a shadow never occludes the object above it");
        assert_eq!(state.depth_compare, Some(wgpu::CompareFunction::LessEqual));
        assert_eq!(state.stencil, wgpu::StencilState::default());
        assert_eq!(state.bias, wgpu::DepthBiasState::default());
    }

    #[test]
    fn the_shadow_switch_collapses_the_uniform_to_zero_alpha() {
        let shadows = ContactShadows::default();
        let on = shadow_uniform(&shadows);
        assert!(on[3] > 0.0, "the default draws a shadow");
        let off = shadow_uniform(&ContactShadows { visible: false, ..shadows });
        assert_eq!(off[3], 0.0, "the switch blends nothing");
        assert_eq!(off[..3], on[..3], "the colour survives the switch, so turning it back on costs no upload");
        let faint = shadow_uniform(&ContactShadows { strength: 0.0, ..shadows });
        assert_eq!(faint[3], 0.0);
    }

    #[test]
    fn point_lights_fill_the_uniform_and_stop_at_the_budget() {
        let mut lighting = Lighting::default();
        for index in 0..MAX_POINT_LIGHTS + 2 {
            lighting.points.push(furnicraft_scene::PointLight {
                position: DVec3::new(index as f64, 0.0, 0.0),
                color: [0.25, 0.5, 0.75],
                power: 1000.0,
            });
        }

        let (count, positions, colors) = point_uniforms(&lighting);
        assert_eq!(count[0] as usize, MAX_POINT_LIGHTS);
        assert_eq!(positions[0], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(positions[MAX_POINT_LIGHTS - 1], [(MAX_POINT_LIGHTS - 1) as f32, 0.0, 0.0, 0.0]);
        assert_eq!(colors[0], [0.25, 0.5, 0.75, 1000.0]);
    }

    #[test]
    fn each_display_style_maps_to_the_passes_it_promises() {
        assert_eq!(
            StylePasses::new(ShadingStyle::ShadedWithEdges),
            StylePasses { opaque_faces: true, translucent_faces: false, visible_edges: true, through_edges: false }
        );
        assert_eq!(
            StylePasses::new(ShadingStyle::Shaded),
            StylePasses { opaque_faces: true, translucent_faces: false, visible_edges: false, through_edges: false }
        );
        let wireframe = StylePasses::new(ShadingStyle::Wireframe);
        assert!(!wireframe.opaque_faces, "wireframe draws no triangle");
        assert!(!wireframe.translucent_faces);
        assert!(wireframe.visible_edges, "it draws the edge pass alone");
        assert!(!wireframe.through_edges);
        let hidden = StylePasses::new(ShadingStyle::ShadedWithHiddenEdges);
        assert!(hidden.opaque_faces, "hidden edges keep the shaded faces");
        assert!(hidden.through_edges, "and draw the edges through them");
        assert!(!hidden.visible_edges);
        let ghost = StylePasses::new(ShadingStyle::Translucent);
        assert!(ghost.translucent_faces);
        assert!(!ghost.opaque_faces, "one face pass, blended, not two");
        assert!(!ghost.visible_edges && !ghost.through_edges);
    }

    #[test]
    fn each_view_derives_its_own_passes_from_its_style() {
        // The pass loop asks each view for its own style, so two neighbours in one target may draw
        // different passes from the same uploaded geometry and edge buffer (U63).
        let region = ViewRegion { x: 0, y: 0, width: 100, height: 100 };
        let views = [
            ViewSpec { camera: Camera::default(), region, shading: ShadingStyle::Wireframe },
            ViewSpec { camera: Camera::default(), region, shading: ShadingStyle::Shaded },
        ];
        assert_eq!(views[0].passes(), StylePasses::new(ShadingStyle::Wireframe));
        assert_eq!(views[1].passes(), StylePasses::new(ShadingStyle::Shaded));
        assert_ne!(views[0].passes(), views[1].passes(), "different styles turn on different passes");
    }

    #[test]
    fn a_translucent_face_pass_writes_no_depth() {
        let ghost = face_depth_state(ShadingStyle::Translucent);
        assert_eq!(ghost.depth_write_enabled, Some(false), "a ghost must not occlude the next object");
        assert_eq!(ghost.depth_compare, Some(wgpu::CompareFunction::LessEqual));

        let opaque = face_depth_state(ShadingStyle::Shaded);
        assert_eq!(opaque.depth_write_enabled, Some(true));
        assert_eq!(opaque.depth_compare, Some(wgpu::CompareFunction::Less));
    }

    #[test]
    fn hidden_edges_ignore_the_depth_test_and_visible_ones_are_only_compared() {
        assert_eq!(edge_depth_state(true).depth_compare, Some(wgpu::CompareFunction::Always));
        assert_eq!(edge_depth_state(false).depth_compare, Some(wgpu::CompareFunction::LessEqual));
        assert_eq!(edge_depth_state(true).depth_write_enabled, Some(false));
        assert_eq!(edge_depth_state(false).depth_write_enabled, Some(false));
        // wgpu rejects DepthBiasState on a line topology, so the unfavourable comparison is the whole
        // mechanism on the edge side; the bias lives on the faces instead.
        assert_eq!(edge_depth_state(false).bias, wgpu::DepthBiasState::default());
        assert_eq!(edge_depth_state(true).bias, wgpu::DepthBiasState::default());
    }

    #[test]
    fn the_face_pass_owns_the_depth_bias_and_stamps_its_drawable() {
        let opaque = face_depth_state(ShadingStyle::ShadedWithEdges);
        assert_eq!(opaque.bias, FACE_DEPTH_BIAS, "the faces are pushed back so an unbias line wins");
        assert_ne!(opaque.bias, wgpu::DepthBiasState::default());
        assert_eq!(opaque.stencil.front.pass_op, wgpu::StencilOperation::Replace, "an opaque face claims the pixel");
        assert_eq!(opaque.stencil.write_mask, STENCIL_MASK);

        let ghost = face_depth_state(ShadingStyle::Translucent);
        assert_eq!(ghost.bias, wgpu::DepthBiasState::default(), "a blended face competes with nothing");
        assert_eq!(ghost.stencil, wgpu::StencilState::default(), "a blended face owns no pixel");
    }

    #[test]
    fn the_hidden_edge_pass_only_accepts_its_own_stencil_slot() {
        let hidden = edge_depth_state(true);
        assert_eq!(hidden.stencil.front.compare, wgpu::CompareFunction::Equal);
        assert_eq!(hidden.stencil.front.pass_op, wgpu::StencilOperation::Keep);
        assert_eq!(hidden.stencil.write_mask, 0, "the pass never rewrites the ownership it tests");
        assert_eq!(hidden.stencil.read_mask, STENCIL_MASK);

        // The visible pass tests nothing in the stencil buffer; the depth test is its only filter.
        assert_eq!(edge_depth_state(false).stencil, wgpu::StencilState::default());
    }

    #[test]
    fn a_stencil_slot_is_one_based_and_never_zero() {
        assert_eq!(stencil_slot(0), 1, "zero is reserved for 'no surface here'");
        assert_eq!(stencil_slot(4), 5);
        // The count wraps instead of overflowing past the byte the buffer stores.
        assert_eq!(stencil_slot(MAX_STENCIL_OWNERS), 1);
        assert_eq!(stencil_slot(MAX_STENCIL_OWNERS * 2 + 3), 4);
    }

    #[test]
    fn translucent_drawables_are_ordered_far_to_near() {
        let centres = [DVec3::new(0.0, 0.0, 1.0), DVec3::new(0.0, 0.0, 10.0), DVec3::new(0.0, 0.0, 5.0)];
        let mut order = Vec::new();
        sort_back_to_front(&centres, DVec3::ZERO, &mut order);
        assert_eq!(order, vec![1, 2, 0]);

        // A second view reuses the vector instead of allocating, and a garbage eye sorts at the origin.
        sort_back_to_front(&centres, DVec3::new(f64::NAN, 0.0, 0.0), &mut order);
        assert_eq!(order.len(), centres.len());
    }

    #[test]
    fn a_drawable_centre_is_the_world_space_centre_of_its_bounds() {
        let transform = DMat4::from_translation(DVec3::new(100.0, 20.0, 5.0));
        let centre = mesh_centre(&triangle(), transform);
        assert!((centre - DVec3::new(105.0, 25.0, 5.0)).length() < 1.0e-9);
    }

    #[test]
    fn edges_become_world_space_lines_and_skip_malformed_entries() {
        let mut mesh = triangle();
        mesh.normals.clear();
        let edges: Arc<[u32]> = Arc::from(vec![0, 1, 0, 9]);
        let transform = DMat4::from_translation(DVec3::new(100.0, 20.0, 5.0));
        let drawable = Drawable::new(Id::new(1).expect("id"), Arc::new(mesh), transform, [1.0; 4]).with_edges(edges);

        let (vertices, slices) = edge_geometry(&[drawable], false, SceneFrame::default().background);
        assert_eq!(vertices.len(), 2, "the edge naming a missing vertex is skipped");
        assert_eq!(vertices[0].position, [100.0, 20.0, 5.0]);
        assert_eq!(vertices[1].position, [110.0, 20.0, 5.0]);
        assert_eq!(vertices[0].color, EDGE_COLOR_LIGHT, "the default viewport is dark, so the ink is light");
        assert_eq!(slices, vec![EdgeSlice { first_vertex: 0, vertex_count: 2, stencil_reference: 1 }]);
    }

    #[test]
    fn every_drawable_gets_a_slice_and_its_own_stencil_slot() {
        let id = |value| Id::new(value).expect("id");
        let with_edges = |value, edges: &[u32]| {
            Drawable::new(id(value), Arc::new(triangle()), DMat4::IDENTITY, [1.0; 4])
                .with_edges(Arc::from(edges.to_vec()))
        };
        // The middle drawable has no lines at all; the third and fourth do. The slice list must stay
        // parallel to the drawables so a stencil reference never lands on a neighbour's lines.
        let drawables = [
            with_edges(1, &[0, 1, 1, 2]),
            Drawable::new(id(2), Arc::new(triangle()), DMat4::IDENTITY, [1.0; 4]),
            with_edges(3, &[2, 0]),
        ];
        let (vertices, slices) = edge_geometry(&drawables, false, SceneFrame::default().background);

        assert_eq!(vertices.len(), 6, "two lines plus one line, the empty drawable adds none");
        assert_eq!(slices.len(), 3);
        assert_eq!(
            slices,
            vec![
                EdgeSlice { first_vertex: 0, vertex_count: 4, stencil_reference: 1 },
                EdgeSlice { first_vertex: 4, vertex_count: 0, stencil_reference: 2 },
                EdgeSlice { first_vertex: 4, vertex_count: 2, stencil_reference: 3 },
            ]
        );
    }

    #[test]
    fn the_all_edges_style_reads_the_full_triangulation_list() {
        let mesh = triangle();
        let feature: Arc<[u32]> = Arc::from(vec![0, 1]);
        let all: Arc<[u32]> = Arc::from(vec![0, 1, 1, 2, 2, 0]);
        let id = Id::new(1).expect("id");
        let drawable =
            Drawable::new(id, Arc::new(mesh), DMat4::IDENTITY, [1.0; 4]).with_edges(feature).with_all_edges(all);
        let background = SceneFrame::default().background;

        let form = edge_geometry(std::slice::from_ref(&drawable), false, background).0;
        let every = edge_geometry(std::slice::from_ref(&drawable), true, background).0;
        assert_eq!(form.len(), 2);
        assert_eq!(every.len(), 6);
    }

    #[test]
    fn edge_ink_follows_the_background() {
        assert_eq!(edge_color([0.10, 0.11, 0.13, 1.0]), EDGE_COLOR_LIGHT, "a dark viewport needs light edges");
        assert_eq!(edge_color([0.92, 0.92, 0.94, 1.0]), EDGE_COLOR_DARK, "a light viewport needs dark edges");
        assert_eq!(edge_color([f32::NAN; 4]), EDGE_COLOR_DARK, "a hostile colour falls back to the light ink");
    }

    /// A device and a queue with no surface behind them, or the sentence that says why there is
    /// none; the ignored pixel test below uses it.
    fn gpu_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()
    }

    /// Copies the renderer's colour target back, top row first and tightly packed.
    fn read_pixels(renderer: &ViewportRenderer, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u8> {
        let [width, height] = renderer.size();
        let row_bytes = width.checked_mul(4).expect("a row fits");
        let padded = row_bytes.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("FurniCraft product envelope shot pixels"),
            size: u64::from(padded) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &renderer.targets._color_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the shot waits for the map");
        receiver.recv().expect("the shot is mapped").expect("the shot could be mapped");
        let mut pixels = Vec::with_capacity((row_bytes * height) as usize);
        {
            let mapped = slice.get_mapped_range().expect("the shot is readable");
            for line in 0..height as usize {
                let start = line * padded as usize;
                if let Some(chunk) = mapped.get(start..start + row_bytes as usize) {
                    pixels.extend_from_slice(chunk);
                }
            }
        }
        buffer.unmap();
        pixels
    }

    /// How many pixels carry `color`, within a rounding step per channel.
    fn pixels_of(pixels: &[u8], color: [f32; 4]) -> usize {
        let expected = color.map(|channel| (channel * 255.0).round() as i32);
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| {
                pixel.iter().enumerate().all(|(channel, value)| (i32::from(*value) - expected[channel]).abs() <= 2)
            })
            .count()
    }

    #[test]
    #[ignore = "needs a GPU adapter; writes target/ui-shots/product-envelope-*.png"]
    fn the_product_envelope_draws_both_inks_and_nothing_when_it_has_parts() {
        let Some((device, queue)) = gpu_device() else {
            println!("this machine has no GPU adapter to render the product envelope with");
            return;
        };
        let camera = Camera::new(DVec3::new(0.0, 0.0, 350.0), 4500.0).expect("camera");
        let mut frame = SceneFrame { camera, ..SceneFrame::default() };
        let box_size = DVec3::new(500.0, 400.0, 700.0);
        for (id, x, state) in [
            (1, -800.0, ProductEnvelopeState::Open),
            (2, 0.0, ProductEnvelopeState::ClosedEmpty),
            (3, 800.0, ProductEnvelopeState::ClosedWithParts),
        ] {
            frame.push_product_envelope(ProductEnvelope {
                id: Id::new(id).expect("id"),
                size: box_size,
                world_transform: DMat4::from_translation(DVec3::new(x, 0.0, 0.0)),
                state,
            });
        }
        let mut renderer = ViewportRenderer::new(&device, 1280, 800);
        let directory = furnicraft_testkit::ui_shots_dir().expect("the shots directory is created");

        for (name, colors) in [("dark", SceneColors::DARK), ("light", SceneColors::LIGHT)] {
            frame.apply_colors(colors);
            renderer.upload(&device, &frame).expect("the envelopes upload");
            renderer.render(&device, &queue, &frame);
            let pixels = read_pixels(&renderer, &device, &queue);
            let path = directory.join(format!("product-envelope-{name}.png"));
            furnicraft_testkit::write_png(&path, 1280, 800, &pixels).expect("the shot is written");
            let active = pixels_of(&pixels, colors.product_envelope_active);
            let inactive = pixels_of(&pixels, colors.product_envelope_inactive);
            assert!(active > 20, "{name}: the open envelope is drawn, {active} pixels");
            assert!(inactive > 20, "{name}: the closed empty envelope is drawn, {inactive} pixels");
        }

        // A closed product with parts keeps no line at all: the same frame without the other two
        // boxes paints neither ink.
        let mut hidden = frame.clone();
        hidden.product_envelopes.retain(|envelope| envelope.state == ProductEnvelopeState::ClosedWithParts);
        renderer.upload(&device, &hidden).expect("the hidden envelope uploads");
        renderer.render(&device, &queue, &hidden);
        let pixels = read_pixels(&renderer, &device, &queue);
        let colors = SceneColors::DARK;
        assert_eq!(pixels_of(&pixels, colors.product_envelope_active), 0);
        assert_eq!(pixels_of(&pixels, colors.product_envelope_inactive), 0, "a piece with parts draws no box");
    }

    #[test]
    fn the_scene_shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(include_str!("scene.wgsl")).expect("the scene shader parses");
        let mut validator =
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
        validator.validate(&module).expect("the scene shader validates");
    }

    #[test]
    fn four_view_regions_tile_the_target_in_framebuffer_coordinates() {
        // A view rect and a scissor share the framebuffer origin, which is the top-left corner, so
        // the region is used unchanged. Flipping it would draw the top view's camera in the bottom
        // half of the texture while the shell still reads the pointer against the top rect.
        let target_height = 600;
        let regions = [
            ViewRegion { x: 0, y: 0, width: 400, height: 300 },
            ViewRegion { x: 400, y: 0, width: 400, height: 300 },
            ViewRegion { x: 0, y: 300, width: 400, height: 300 },
            ViewRegion { x: 400, y: 300, width: 400, height: 300 },
        ];
        let scissors: Vec<_> = regions.iter().map(|region| region.scissor(800, target_height)).collect();
        assert_eq!(scissors, vec![(0, 0, 400, 300), (400, 0, 400, 300), (0, 300, 400, 300), (400, 300, 400, 300)]);
        for (region, scissor) in regions.iter().zip(&scissors) {
            assert_eq!((scissor.0, scissor.1), (region.x, region.y), "the scissor keeps the top-left origin");
        }

        // A region reaching past the target clamps to it rather than making an invalid scissor.
        assert_eq!(ViewRegion { x: 0, y: 0, width: 10, height: 10 }.scissor(5, 5), (0, 0, 5, 5));
        // An offset past the far edge still yields a drawable one-pixel scissor inside the target.
        assert_eq!(ViewRegion { x: 900, y: 700, width: 10, height: 10 }.scissor(800, 600), (799, 599, 1, 1));
    }

    #[test]
    fn a_camera_slot_stride_is_aligned_for_a_dynamic_offset() {
        let uniform = std::mem::size_of::<CameraUniform>() as u64;
        // The default device alignment is 256; a slot must start on a multiple of it.
        let stride = uniform.next_multiple_of(256);
        assert!(stride >= uniform, "the stride never overlaps the next slot");
        assert_eq!(stride % 256, 0, "and keeps the dynamic offset aligned");
        assert!(stride * MAX_VIEWS as u64 > uniform, "four views need four distinct slots");
    }

    #[test]
    fn an_aspect_ratio_is_per_view_not_per_target() {
        let camera = Camera::default();
        let whole = camera.view_projection(2.0);
        let half = camera.view_projection(1.0);
        assert_ne!(whole, half, "a narrow view must not borrow the target's aspect");
    }
}
