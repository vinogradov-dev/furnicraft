//! The render pipelines and the GPU state each of them encodes.
//!
//! Holds six pipeline constructors (two face, three line, one shadow), the vertex layout they all
//! share and the depth, blend and stencil states those pipelines are built from.
//!
//! Contract: every constructor takes the device, the pipeline layout and the shader module the
//! renderer built once, and returns a pipeline against [`crate::COLOR_FORMAT`] and
//! [`crate::DEPTH_FORMAT`]. The state a pass encodes is a value: `face_depth_state`,
//! `line_depth_state`, `shadow_depth_state` and `edge_depth_state` build it without a device, so a
//! test asserts the promise of a style without a GPU.
//!
//! Refuses: nothing. A wrong state is a compile error, not a runtime one; wgpu validates the
//! pipeline on creation and a mistake there is a programming error, not user data.
//!
//! Does not draw: no pass, no buffer, no camera uniform. The renderer records the draw calls; this
//! module only says what a pass may do to the depth and stencil attachments.
//!
//! Changes when: a pass, a topology or a depth, blend or stencil rule changes.

use furnicraft_scene::ShadingStyle;

use crate::{COLOR_FORMAT, DEPTH_FORMAT, MeshVertex};

const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4];

/// The vertex layout every pipeline of the scene reads.
pub(crate) fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<MeshVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &VERTEX_ATTRIBUTES,
    }
}

/// The stencil value an opaque face stamps so the *hidden edges* pass can recognise its own surface.
///
/// Zero is the cleared value and therefore means "no surface here". A drawable's slot counts from
/// one, so a scene with more drawables than slots makes the nearest ones share a slot; that can
/// only make one object's inside visible through another at a pixel both own, never a wrong object
/// appearing out of nothing.
pub(crate) const STENCIL_MASK: u32 = 0xff;
pub(crate) const MAX_STENCIL_OWNERS: usize = STENCIL_MASK as usize;

/// The depth bias that lets an edge pass win against the face it lies on.
///
/// A line is one pixel wide and lies exactly on the surface it outlines, so without a bias the two
/// fight for the pixel and the outline breaks up. A `LineList` pipeline cannot carry a depth bias at
/// all — wgpu rejects it, because the slope term is undefined for a line — so the bias is applied
/// where it belongs instead: the *faces* are pushed away from the camera by this much, and the
/// unbias lines then win the comparison. A constant term in depth units is distance-independent,
/// which is what a clip-space nudge is not: shifting NDC depth by a constant moves a far surface
/// hundreds of millimetres and a near one barely at all.
///
/// At 24-bit depth one unit is about 6e-8 of the depth range, so `2` is far below any visible offset
/// and far above the rounding error of the transform.
pub(crate) const FACE_DEPTH_BIAS: wgpu::DepthBiasState =
    wgpu::DepthBiasState { constant: 2, slope_scale: 1.0, clamp: 0.0 };

/// One opaque or translucent face pipeline at the scene's colour format.
///
/// A translucent style blends and writes no depth; an opaque one replaces the fragment and wins the
/// depth test. The two differ only in these two states, so one constructor serves both.
pub(crate) fn create_triangle_pipeline(
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
///
/// An opaque face also stamps the stencil buffer with the drawable's slot, because that stamp is
/// what the *hidden edges* pass tests against. A translucent face stamps nothing: it never claims
/// to occlude anything, so it cannot own a pixel either.
pub(crate) fn face_depth_state(style: ShadingStyle) -> wgpu::DepthStencilState {
    let translucent = style.is_translucent();
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        // A translucent surface tests against the opaque scene but must not occlude a later
        // object, so it writes no depth.
        depth_write_enabled: Some(!translucent),
        depth_compare: Some(if translucent { wgpu::CompareFunction::LessEqual } else { wgpu::CompareFunction::Less }),
        stencil: if translucent {
            wgpu::StencilState::default()
        } else {
            // Every fragment that reaches this point has already won the depth test, so the nearest
            // surface owns the pixel: write the drawable's slot.
            let stamp = wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Replace,
            };
            wgpu::StencilState { front: stamp, back: stamp, read_mask: STENCIL_MASK, write_mask: STENCIL_MASK }
        },
        // Only the opaque pass competes with the edge pass for a pixel, so only it is pushed away.
        bias: if translucent { wgpu::DepthBiasState::default() } else { FACE_DEPTH_BIAS },
    }
}

/// The scene colour target with an optional blend state.
fn face_target(blend: Option<wgpu::BlendState>) -> Option<wgpu::ColorTargetState> {
    Some(wgpu::ColorTargetState { format: COLOR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL })
}

/// One line pipeline with the given depth state.
///
/// The grid, the paths and both edge passes share `vs_line`; only the depth and stencil state
/// differs. Nothing nudges a line toward the camera here: the faces are biased instead, so a line
/// wins against its own surface at any distance.
pub(crate) fn create_line_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    label: &str,
    depth: wgpu::DepthStencilState,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_line"),
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

/// One unlit, blended triangle pipeline for the contact shadows.
///
/// The shadow has no faces to light and no texture: the vertex fan carries a radial alpha and the
/// fragment entry returns the uniform's shadow colour times that alpha. It shares `vs_mesh`
/// because a shadow is still a world-space triangle, only without a shading model.
pub(crate) fn create_shadow_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("FurniCraft contact shadow pipeline"),
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
            // The ellipse is seen from either side when the camera dips below the floor.
            cull_mode: None,
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: Some(shadow_depth_state()),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_shadow"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: std::slice::from_ref(&face_target(Some(wgpu::BlendState::ALPHA_BLENDING))),
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// The depth state of the shadow pass: it lies on the floor, so it is tested but never occludes.
pub(crate) fn shadow_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The depth state of the grid and path lines: tested against the scene, writing nothing.
///
/// The visible-edge style uses the same depth comparison; it is a separate pipeline only because its
/// stencil state may differ.
pub(crate) fn line_depth_state() -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    }
}

/// The depth and stencil state of the edge pass.
///
/// The pass draws after the faces, so the depth buffer already holds the nearest surface of every
/// object. Two things decide what a line may show:
///
/// - *visible edges* (`through == false`) test depth with `LessEqual`, so a line behind a face
///   disappears. The face pass carries [`FACE_DEPTH_BIAS`], so the line that lies on its own face
///   still wins the pixel without the line being moved.
/// - *hidden edges* (`through == true`) ignore depth, so the inside of an object stays visible.
///   The stencil test stops that from leaking to the rest of the scene: an opaque face stamps its
///   drawable's slot, and a hidden line is drawn only where the pixel is still uncovered
///   (`stencil = 0`) or stamped with the line's own slot. A node behind the frontmost one carries a
///   different slot, so its edges stay hidden exactly as in a visible-edge pass.
///
/// The reference value is the drawable's slot, set per drawable before its slice is drawn.
pub(crate) fn edge_depth_state(through: bool) -> wgpu::DepthStencilState {
    let stencil = if through {
        // Keep the buffer untouched; only the comparison and the reference decide what shows.
        let accept = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Keep,
        };
        wgpu::StencilState { front: accept, back: accept, read_mask: STENCIL_MASK, write_mask: 0 }
    } else {
        wgpu::StencilState::default()
    };
    wgpu::DepthStencilState {
        format: DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(if through { wgpu::CompareFunction::Always } else { wgpu::CompareFunction::LessEqual }),
        stencil,
        bias: wgpu::DepthBiasState::default(),
    }
}
