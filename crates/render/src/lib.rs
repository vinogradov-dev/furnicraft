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
use furnicraft_scene::{Grid, Lighting, MAX_POINT_LIGHTS, SceneFrame};
use thiserror::Error;
use wgpu::util::DeviceExt as _;

/// The offscreen colour format. `egui_wgpu::Renderer::register_native_texture` requires this
/// exact format, and it is not an sRGB format, so the shader writes gamma-corrected values.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Largest mesh, in vertices, that the renderer accepts from one drawable.
const MAX_VERTICES: usize = 4_000_000;

/// Largest number of grid divisions on each side of the origin.
const MAX_GRID_DIVISIONS: i64 = 512;

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
}

struct GpuGrid {
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

/// An offscreen scene renderer for one viewport.
///
/// The shell owns the window and the egui integration; this type owns the GPU scene resources and
/// exposes the colour view that egui should display.
pub struct ViewportRenderer {
    mesh_pipeline: wgpu::RenderPipeline,
    grid_pipeline: wgpu::RenderPipeline,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    drawables: Vec<GpuMesh>,
    grid: Option<GpuGrid>,
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

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("FurniCraft camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
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
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("FurniCraft camera bind group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(camera_buffer.as_entire_buffer_binding()),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("FurniCraft scene pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let color_target =
            Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL });

        let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("FurniCraft shaded mesh pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_mesh"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: std::slice::from_ref(&color_target),
            }),
            multiview_mask: None,
            cache: None,
        });

        let grid_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("FurniCraft grid pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_grid"),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_grid"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[color_target],
            }),
            multiview_mask: None,
            cache: None,
        });

        Self {
            mesh_pipeline,
            grid_pipeline,
            camera_buffer,
            camera_bind_group,
            drawables: Vec::new(),
            grid: None,
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
            drawables.push(GpuMesh { vertex_buffer, index_buffer, index_count });
        }

        let grid_vertices = grid_vertices(&frame.grid);
        let grid = if grid_vertices.is_empty() {
            None
        } else {
            let vertex_count =
                u32::try_from(grid_vertices.len()).map_err(|_| RenderError::MeshTooLarge { index: 0 })?;
            let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("FurniCraft grid vertices"),
                contents: bytemuck::cast_slice(&grid_vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            Some(GpuGrid { vertex_buffer, vertex_count })
        };

        self.drawables = drawables;
        self.grid = grid;
        Ok(())
    }

    /// Draws the frame into the offscreen colour target.
    ///
    /// The camera and lighting are read from the frame, so orbit, pan and zoom cost one uniform
    /// write rather than a geometry upload.
    pub fn render(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &SceneFrame) {
        let aspect = f64::from(self.size[0]) / f64::from(self.size[1].max(1));
        let camera = frame.camera;
        let eye = camera.eye();
        let light = frame.lighting.key_direction.try_normalize().unwrap_or(DVec3::Z);
        let (point_count, point_position, point_color) = point_uniforms(&frame.lighting);
        let uniform = CameraUniform {
            view_projection: camera.view_projection(aspect).as_mat4().to_cols_array_2d(),
            camera_position: [eye.x as f32, eye.y as f32, eye.z as f32, 1.0],
            light_direction: [light.x as f32, light.y as f32, light.z as f32, 0.0],
            light_color: [
                frame.lighting.key_color[0],
                frame.lighting.key_color[1],
                frame.lighting.key_color[2],
                frame.lighting.ambient.clamp(0.0, 1.0),
            ],
            point_count,
            point_position,
            point_color,
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&uniform));

        let background = sanitize_color(frame.background);
        let clear = wgpu::Color {
            r: f64::from(background[0]),
            g: f64::from(background[1]),
            b: f64::from(background[2]),
            a: f64::from(background[3]),
        };
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("FurniCraft scene encoder") });
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
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("FurniCraft scene pass"),
                color_attachments: &[color_attachment],
                depth_stencil_attachment: Some(depth_attachment),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.camera_bind_group, &[]);

            if !self.drawables.is_empty() {
                pass.set_pipeline(&self.mesh_pipeline);
                for mesh in &self.drawables {
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }

            if let Some(grid) = &self.grid {
                pass.set_pipeline(&self.grid_pipeline);
                pass.set_vertex_buffer(0, grid.vertex_buffer.slice(..));
                pass.draw(0..grid.vertex_count, 0..1);
            }
        }
        queue.submit(Some(encoder.finish()));
    }
}

fn clamp_size(width: u32, height: u32, max_dimension: u32) -> [u32; 2] {
    let max_dimension = max_dimension.max(1);
    [width.clamp(1, max_dimension), height.clamp(1, max_dimension)]
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

    use super::*;

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
    fn the_scene_shader_is_valid_wgsl() {
        let module = naga::front::wgsl::parse_str(include_str!("scene.wgsl")).expect("the scene shader parses");
        let mut validator =
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
        validator.validate(&module).expect("the scene shader validates");
    }
}
