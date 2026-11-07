//! The wgpu renderer.
//!
//! Owns an offscreen colour target, a depth buffer, pipelines and buffers. It knows nothing about
//! `egui` and never creates a window: the shell registers the colour texture with `egui_wgpu` and
//! displays it as an image. Scene geometry is uploaded when the document changes; camera changes
//! only rewrite a small uniform buffer.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

use bytemuck::{Pod, Zeroable};
use furnicraft_geom::{DMat4, DVec3, MeshData};
use furnicraft_scene::{Camera, Drawable, Grid, Lighting, MAX_POINT_LIGHTS, Polyline, SceneFrame, ShadingStyle};
use thiserror::Error;
use wgpu::util::DeviceExt as _;

/// The offscreen colour format. `egui_wgpu::Renderer::register_native_texture` requires this
/// exact format, and it is not an sRGB format, so the shader writes gamma-corrected values.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Largest vertex count the renderer accepts for one uploaded primitive: a drawable's mesh or the
/// whole frame's line list.
const MAX_VERTICES: usize = 4_000_000;

/// Largest number of grid divisions on each side of the origin.
const MAX_GRID_DIVISIONS: i64 = 512;

/// The most views one frame may split into; the camera uniform holds one slot per view.
pub const MAX_VIEWS: usize = 4;

/// Gamma-space colour of the mesh edge pass.
///
/// The line pipeline is unlit and the colour is already in gamma space, like a grid or path colour.
/// A dark ink reads over an oak panel and a white one without changing the shaded faces.
const EDGE_COLOR: [f32; 4] = [0.06, 0.07, 0.08, 1.0];

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
}

/// A shaded vertex in world space.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct MeshVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];

fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    }
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
}

struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    /// World-space centre, used to order translucent objects back to front.
    centre: DVec3,
}

/// One line-list vertex buffer: the grid and the frame's polylines each own one.
struct GpuLineList {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
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
        let color = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("FurniCraft viewport colour"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
    grid: Option<GpuLineList>,
    polylines: Option<GpuLineList>,
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
        let line_pipeline = create_line_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft line pipeline",
            "vs_line",
            line_depth_state(),
        );
        let edge_pipeline = create_line_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft edge pipeline",
            "vs_edge",
            edge_depth_state(false),
        );
        let hidden_edge_pipeline = create_line_pipeline(
            device,
            &pipeline_layout,
            &shader,
            "FurniCraft hidden edge pipeline",
            "vs_edge",
            edge_depth_state(true),
        );

        Self {
            mesh_pipeline,
            translucent_pipeline,
            line_pipeline,
            edge_pipeline,
            hidden_edge_pipeline,
            camera_buffer,
            camera_bind_group,
            camera_stride,
            drawables: Vec::new(),
            centres: Vec::new(),
            draw_order: Vec::new(),
            edges: None,
            grid: None,
            polylines: None,
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
            drawables.push(GpuMesh { vertex_buffer, index_buffer, index_count, centre });
        }

        let grid = line_list(device, "FurniCraft grid vertices", grid_vertices(&frame.grid))?;
        let polylines = line_list(device, "FurniCraft path vertices", polyline_vertices(frame.polylines())?)?;
        let edges = line_list(device, "FurniCraft edge vertices", edge_vertices(frame.drawables()))?;

        let draw_order: Vec<usize> = (0..drawables.len()).collect();
        self.drawables = drawables;
        self.centres = self.drawables.iter().map(|mesh| mesh.centre).collect();
        self.draw_order = draw_order;
        self.edges = edges;
        self.grid = grid;
        self.polylines = polylines;
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
        self.render_views(device, queue, frame, lighting, &[(frame.camera, region)]);
    }

    /// Draws the frame once per view into one target, each pass scissored to its own rectangle.
    ///
    /// One texture and one egui texture id serve the whole workspace, so splitting the viewport
    /// costs a pass and a uniform slot rather than a second texture. The passes share one cleared
    /// attachment: the first clears the colour and depth, every other pass loads them, and each
    /// draws inside its own viewport and scissor so a view never writes over its neighbour.
    ///
    /// At most [`MAX_VIEWS`] views are drawn; a longer slice is truncated. An empty slice clears the
    /// target and draws nothing.
    pub fn render_views(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &SceneFrame,
        lighting: &Lighting,
        views: &[(Camera, ViewRegion)],
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
        for (index, (camera, region)) in views.iter().enumerate() {
            let aspect = f64::from(region.width.max(1)) / f64::from(region.height.max(1));
            uniforms.push(self.camera_uniform(*camera, lighting, aspect));
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
                stencil_ops: None,
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

        let passes = StylePasses::new(frame.shading);
        for ((camera, region), offset) in views.iter().zip(offsets) {
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
                stencil_ops: None,
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
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
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
            // edges* uses the always pass, *wireframe* reaches here with no faces before it.
            if (passes.visible_edges || passes.through_edges)
                && let Some(edges) = &self.edges
            {
                pass.set_pipeline(if passes.through_edges { &self.hidden_edge_pipeline } else { &self.edge_pipeline });
                pass.set_vertex_buffer(0, edges.vertex_buffer.slice(..));
                pass.draw(0..edges.vertex_count, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
    }

    /// The uniform one view shades with.
    fn camera_uniform(&self, camera: Camera, lighting: &Lighting, aspect: f64) -> CameraUniform {
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
        }
    }
}

/// One opaque or translucent face pipeline at the scene's colour format.
///
/// A translucent style blends and writes no depth; an opaque one replaces the fragment and wins the
/// depth test. The two differ only in these two states, so one constructor serves both.
fn create_triangle_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    label: &str,
    fragment_entry: &str,
    style: ShadingStyle,
) -> wgpu::RenderPipeline {
    let translucent = style.is_translucent();
    let target = face_target(if translucent { Some(wgpu::BlendState::ALPHA_BLENDING) } else { None });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_mesh"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout())],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            // Panels are thin solids; showing their back faces avoids holes when the camera
            // moves inside a cutout.
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(face_depth_state(style)),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: std::slice::from_ref(&target),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The depth state of the face pass: opaque faces write depth, translucent faces do not.
fn face_depth_state(style: ShadingStyle) -> wgpu::DepthStencilState {
    let translucent = style.is_translucent();
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        // A translucent surface tests against the opaque scene but must not occlude a later
        // object, so it writes no depth.
        depth_write_enabled: Some(!translucent),
        depth_compare: Some(if translucent { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Less }),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The scene colour target with an optional blend state.
fn face_target(blend: Option<wgpu::BlendState>) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })
}

/// One line pipeline with the given vertex entry and depth state.
///
/// The grid and paths use `vs_line`; the edge pass uses `vs_edge`, whose clip-space nudge replaces
/// the depth bias wgpu cannot apply to a line topology.
fn create_line_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    label: &str,
    vertex_entry: &str,
    depth: wgpu::DepthStencilState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex_entry),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(vertex_layout())],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(depth),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_line"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: std::slice::from_ref(&face_target(None)),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The depth state of the grid and path lines: tested against the scene, writing nothing.
fn line_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// Depth comparison of the edge pass.
///
/// wgpu rejects `DepthBiasState` on a line topology, so the nudge toward the camera lives in the
/// edge vertex shader (`vs_edge` in `scene.wgsl`) and this state only chooses the comparison.
///
/// `through` is *shaded with hidden edges*: the comparison is always satisfied, so every edge shows
/// regardless of the faces in front of it.
fn edge_depth_state(through: bool) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(if through { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::LessEqual }),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The world-space centre of a drawable's local bounds, used to order translucent objects.
///
/// A missing or non-finite position is ignored, and a mesh with none has no centre to sort by.
fn mesh_centre(mesh: &MeshData, transform: DMat4) -> DVec3 {
    let mut min = DVec3::splat(f64::INFINITY);
    let mut max = DVec3::splat(f64::NEG_INFINITY);
    let mut seen = false;
    for position in &mesh.positions {
        let world = transform.transform_point3(vector(*position));
        if !world.is_finite() {
            continue;
        }
        min = min.min(world);
        max = max.max(world);
        seen = true;
    }
    if seen { (min + max) * 0.5 } else { DVec3::ZERO }
}

/// Flattens the edge list of every drawable into one line list in world space.
///
/// The edge list is derived data that `crates/mesh` cached beside the mesh (ADR 0053), so this is a
/// transform and a copy, never a search. A malformed edge or a non-finite vertex is skipped: an
/// edge is decoration and must never turn a whole frame into an error.
fn edge_vertices(drawables: &[Drawable]) -> Vec<MeshVertex> {
    let mut vertices = Vec::new();
    for drawable in drawables {
        let Some(edges) = drawable.edges.as_deref() else {
            continue;
        };
        if !drawable.world_transform.is_finite() {
            continue;
        }
        for edge in edges.as_chunks::<2>().0 {
            let (Some(a), Some(b)) = (position_at(&drawable.mesh, edge[0]), position_at(&drawable.mesh, edge[1]))
            else {
                continue;
            };
            let a = drawable.world_transform.transform_point3(a);
            let b = drawable.world_transform.transform_point3(b);
            if !a.is_finite() || !b.is_finite() {
                continue;
            }
            if vertices.len().saturating_add(2) > MAX_VERTICES {
                return vertices;
            }
            push_line(&mut vertices, [a.x, a.y, a.z], [b.x, b.y, b.z], EDGE_COLOR);
        }
    }
    vertices
}

/// Rewrites `order` so far objects come first, which is the order a blended pass needs.
///
/// The comparison is on squared distance to the eye, so no square root is taken per pair. A
/// non-finite eye or centre sorts as the origin, which keeps the order total instead of panicking.
fn sort_back_to_front(centres: &[DVec3], eye: DVec3, order: &mut Vec<usize>) {
    order.clear();
    order.extend(0..centres.len());
    let eye = if eye.is_finite() { eye } else { DVec3::ZERO };
    order.sort_by(|&a, &b| {
        let far = centres.get(a).map_or(0.0, |centre| (centre - eye).length_squared());
        let near = centres.get(b).map_or(0.0, |centre| (centre - eye).length_squared());
        near.total_cmp(&far)
    });
}

fn clamp_size(width: u32, height: u32, max_dimension: u32) -> [u32; 2] {
    let max_dimension = max_dimension.max(1);
    [width.clamp(1, max_dimension), height.clamp(1, max_dimension)]
}

/// Uploads a line-list vertex buffer, or nothing when there are no vertices.
fn line_list(
    device: &wgpu::Device,
    label: &str,
    vertices: Vec<MeshVertex>,
) -> Result<Option<GpuLineList>, RenderError> {
    if vertices.is_empty() {
        return Ok(None);
    }
    let vertex_count = u32::try_from(vertices.len()).map_err(|_| RenderError::MeshTooLarge { index: 0 })?;
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    Ok(Some(GpuLineList { vertex_buffer, vertex_count }))
}

/// Flattens every frame polyline into one line-list vertex buffer.
///
/// Points are transformed into world space here, the same way a drawable's mesh is. A malformed
/// chain is an error instead of a silently missing path, so the shell can report it.
fn polyline_vertices(polylines: &[Polyline]) -> Result<Vec<MeshVertex>, RenderError> {
    let mut vertices = Vec::new();
    for (index, polyline) in polylines.iter().enumerate() {
        push_polyline_vertices(index, polyline, &mut vertices)?;
    }
    Ok(vertices)
}

/// Appends one polyline as pairs of line-list vertices in world space.
fn push_polyline_vertices(
    index: usize,
    polyline: &Polyline,
    vertices: &mut Vec<MeshVertex>,
) -> Result<(), RenderError> {
    let point_count = polyline.points.len();
    if point_count < 2 {
        return Err(RenderError::EmptyPolyline { index });
    }
    let needed = (point_count - 1).checked_mul(2).ok_or(RenderError::PolylineTooLarge { index })?;
    let total = vertices.len().checked_add(needed).ok_or(RenderError::PolylineTooLarge { index })?;
    if total > MAX_VERTICES {
        return Err(RenderError::PolylineTooLarge { index });
    }
    if !polyline.world_transform.is_finite() || polyline.points.iter().any(|point| !point.is_finite()) {
        return Err(RenderError::InvalidPolyline { index });
    }
    let color = sanitize_color(polyline.color);
    let mut previous = None;
    for point in polyline.points.iter() {
        let world = polyline.world_transform.transform_point3(*point);
        if !world.is_finite() {
            return Err(RenderError::InvalidPolyline { index });
        }
        let position = to_array(world);
        if let Some(previous) = previous {
            vertices.push(MeshVertex { position: previous, normal: [0.0, 0.0, 1.0], color });
            vertices.push(MeshVertex { position, normal: [0.0, 0.0, 1.0], color });
        }
        previous = Some(position);
    }
    Ok(())
}

/// Converts a local mesh into world-space vertices and indices.
///
/// A mesh without normals is expanded into flat-shaded triangles. The fallback matters because
/// the geometry port allows normals to be absent; panels normally arrive with per-face normals.
fn world_vertices(
    index: usize,
    mesh: &MeshData,
    transform: DMat4,
    color: [f32; 4],
) -> Result<(Vec<MeshVertex>, Vec<u32>), RenderError> {
    if mesh.positions.is_empty() || mesh.indices.is_empty() || !mesh.indices.len().is_multiple_of(3) {
        return Err(RenderError::EmptyMesh { index });
    }
    if mesh.positions.len() > MAX_VERTICES {
        return Err(RenderError::MeshTooLarge { index });
    }
    if !transform.is_finite()
        || mesh.positions.iter().flatten().any(|value| !value.is_finite())
        || mesh.normals.iter().flatten().any(|value| !value.is_finite())
        || (!mesh.normals.is_empty() && mesh.normals.len() != mesh.positions.len())
    {
        return Err(RenderError::InvalidMesh { index });
    }
    let vertex_count = u32::try_from(mesh.positions.len()).map_err(|_| RenderError::MeshTooLarge { index })?;
    if mesh.indices.iter().any(|vertex| *vertex >= vertex_count) {
        return Err(RenderError::InvalidMesh { index });
    }
    let color = sanitize_color(color);

    if mesh.normals.is_empty() {
        return flat_vertices(index, mesh, transform, color);
    }

    let mut vertices = Vec::with_capacity(mesh.positions.len());
    for (position, normal) in mesh.positions.iter().zip(&mesh.normals) {
        let position = transform.transform_point3(vector(*position));
        if !position.is_finite() {
            return Err(RenderError::InvalidMesh { index });
        }
        let normal = transform.transform_vector3(vector(*normal)).try_normalize().unwrap_or(DVec3::Z);
        vertices.push(MeshVertex { position: to_array(position), normal: to_array(normal), color });
    }
    Ok((vertices, mesh.indices.clone()))
}

fn flat_vertices(
    index: usize,
    mesh: &MeshData,
    transform: DMat4,
    color: [f32; 4],
) -> Result<(Vec<MeshVertex>, Vec<u32>), RenderError> {
    let triangle_count = mesh.indices.len() / 3;
    let capacity = triangle_count.checked_mul(3).ok_or(RenderError::MeshTooLarge { index })?;
    if capacity > MAX_VERTICES {
        return Err(RenderError::MeshTooLarge { index });
    }
    let mut vertices = Vec::with_capacity(capacity);
    let mut indices = Vec::with_capacity(capacity);
    for triangle in mesh.indices.as_chunks::<3>().0 {
        let a = position_at(mesh, triangle[0]);
        let b = position_at(mesh, triangle[1]);
        let c = position_at(mesh, triangle[2]);
        let (Some(a), Some(b), Some(c)) = (a, b, c) else {
            return Err(RenderError::InvalidMesh { index });
        };
        let a = transform.transform_point3(a);
        let b = transform.transform_point3(b);
        let c = transform.transform_point3(c);
        if !a.is_finite() || !b.is_finite() || !c.is_finite() {
            return Err(RenderError::InvalidMesh { index });
        }
        let normal = (b - a).cross(c - a).try_normalize().unwrap_or(DVec3::Z);
        let base = u32::try_from(vertices.len()).map_err(|_| RenderError::MeshTooLarge { index })?;
        for position in [a, b, c] {
            vertices.push(MeshVertex { position: to_array(position), normal: to_array(normal), color });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    Ok((vertices, indices))
}

fn position_at(mesh: &MeshData, index: u32) -> Option<DVec3> {
    let position = mesh.positions.get(index as usize)?;
    Some(vector(*position))
}

fn grid_vertices(grid: &Grid) -> Vec<MeshVertex> {
    if !grid.visible {
        return Vec::new();
    }
    let spacing = if grid.spacing_mm.is_finite() && grid.spacing_mm >= 1.0 {
        grid.spacing_mm.min(furnicraft_scene::MAX_DISTANCE_MM)
    } else {
        100.0
    };
    let requested_extent = if grid.extent_mm.is_finite() {
        grid.extent_mm.clamp(spacing, furnicraft_scene::MAX_DISTANCE_MM)
    } else {
        5000.0
    };
    let divisions = ((requested_extent / spacing).floor() as i64).clamp(0, MAX_GRID_DIVISIONS);
    let extent = divisions as f64 * spacing;
    let major_every = i64::from(grid.major_every.max(1));
    let mut vertices = Vec::with_capacity((2 * divisions as usize + 1) * 4);

    for step in -divisions..=divisions {
        let offset = step as f64 * spacing;
        let x_color = if step == 0 {
            sanitize_color(grid.axis_x_color)
        } else if step % major_every == 0 {
            sanitize_color(grid.major_color)
        } else {
            sanitize_color(grid.minor_color)
        };
        let y_color = if step == 0 {
            sanitize_color(grid.axis_y_color)
        } else if step % major_every == 0 {
            sanitize_color(grid.major_color)
        } else {
            sanitize_color(grid.minor_color)
        };
        // Lift the grid a fraction of a millimetre above the floor so a panel resting at z = 0
        // does not z-fight with it.
        push_line(&mut vertices, [-extent, offset, 0.02], [extent, offset, 0.02], x_color);
        push_line(&mut vertices, [offset, -extent, 0.02], [offset, extent, 0.02], y_color);
    }

    vertices
}

fn push_line(vertices: &mut Vec<MeshVertex>, start: [f64; 3], end: [f64; 3], color: [f32; 4]) {
    for point in [start, end] {
        vertices.push(MeshVertex {
            position: [point[0] as f32, point[1] as f32, point[2] as f32],
            normal: [0.0, 0.0, 1.0],
            color,
        });
    }
}

fn vector(value: [f32; 3]) -> DVec3 {
    DVec3::new(f64::from(value[0]), f64::from(value[1]), f64::from(value[2]))
}

fn to_array(value: DVec3) -> [f32; 3] {
    [value.x as f32, value.y as f32, value.z as f32]
}

fn sanitize_color(color: [f32; 4]) -> [f32; 4] {
    if color.iter().all(|component| component.is_finite()) { color } else { [0.8, 0.8, 0.8, 1.0] }
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
    use furnicraft_model::Id;

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
        let vertices = polyline_vertices(&[line_chain(transform)]).expect("valid polyline");

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
        assert_eq!(polyline_vertices(&[empty]), Err(RenderError::EmptyPolyline { index: 0 }));

        let bad_point =
            Polyline::new(id, Arc::from(vec![DVec3::ZERO, DVec3::new(f64::NAN, 0.0, 0.0)]), DMat4::IDENTITY, [1.0; 4]);
        assert_eq!(polyline_vertices(&[bad_point]), Err(RenderError::InvalidPolyline { index: 0 }));

        let bad_transform = Polyline::new(
            id,
            Arc::from(vec![DVec3::ZERO, DVec3::X]),
            DMat4::from_translation(DVec3::new(f64::NAN, 0.0, 0.0)),
            [1.0; 4],
        );
        assert_eq!(polyline_vertices(&[bad_transform]), Err(RenderError::InvalidPolyline { index: 0 }));
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
        // wgpu rejects DepthBiasState on a line topology; the shader nudges the edge instead.
        assert_eq!(edge_depth_state(false).bias, wgpu::DepthBiasState::default());
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

        let vertices = edge_vertices(&[drawable]);
        assert_eq!(vertices.len(), 2, "the edge naming a missing vertex is skipped");
        assert_eq!(vertices[0].position, [100.0, 20.0, 5.0]);
        assert_eq!(vertices[1].position, [110.0, 20.0, 5.0]);
        assert_eq!(vertices[0].color, EDGE_COLOR);
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
