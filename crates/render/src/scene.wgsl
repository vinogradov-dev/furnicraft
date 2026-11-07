const MAX_POINT_LIGHTS: u32 = 8u;
const PI: f32 = 3.141592653589793;

// Fixed viewport exposure: a 1000 W point light at one metre shades a white surface at about half
// intensity. A scene exposure control can replace this constant without touching the light data.
const POINT_EXPOSURE: f32 = 0.006;

struct Camera {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    light_direction: vec4<f32>,
    // rgb = key colour, a = ambient fraction
    light_color: vec4<f32>,
    point_count: vec4<u32>,
    // xyz = world position, w = unused
    point_position: array<vec4<f32>, 8>,
    // rgb = linear colour, a = radiant power in watts
    point_color: array<vec4<f32>, 8>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct MeshOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_mesh(vertex: VertexIn) -> MeshOut {
    var out: MeshOut;
    out.clip_position = camera.view_projection * vec4<f32>(vertex.position, 1.0);
    out.normal = vertex.normal;
    out.world_position = vertex.position;
    out.color = vertex.color;
    return out;
}

fn safe_normalize(value: vec3<f32>) -> vec3<f32> {
    let length = sqrt(dot(value, value));
    return select(vec3<f32>(0.0), value / length, length > 1.0e-9);
}

// Inverse-square falloff with no hard range cutoff. The power is in watts and the exposure maps
// 1000 W at one metre to about half intensity.
fn point_lighting(normal: vec3<f32>, world_position: vec3<f32>, view_direction: vec3<f32>) -> vec3<f32> {
    var total = vec3<f32>(0.0);
    let count = min(camera.point_count.x, MAX_POINT_LIGHTS);
    for (var index = 0u; index < count; index = index + 1u) {
        let offset = camera.point_position[index].xyz - world_position;
        let distance_squared = max(dot(offset, offset), 1.0);
        let direction = offset * inverseSqrt(distance_squared);
        let attenuation = POINT_EXPOSURE / (4.0 * PI * distance_squared);
        let power = camera.point_color[index].a;
        let diffuse = max(dot(normal, direction), 0.0);
        let half_vector = safe_normalize(direction + view_direction);
        let specular = pow(max(dot(normal, half_vector), 0.0), 32.0) * 0.12;
        total += camera.point_color[index].rgb * (diffuse + specular) * power * attenuation;
    }
    return total;
}

struct MeshIn {
    @location(0) normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) color: vec4<f32>,
    @builtin(front_facing) front_facing: bool,
};

// Linear colour of one shaded surface, before the gamma encode the target and egui expect.
fn shade_surface(
    normal_input: vec3<f32>,
    front_facing: bool,
    world_position: vec3<f32>,
    color: vec4<f32>,
) -> vec3<f32> {
    // Panels are open on both faces, so flip the normal for back-facing triangles instead of
    // culling them or lighting the inside of a cutout black.
    let normal = normalize(select(-normal_input, normal_input, front_facing));
    let light_direction = safe_normalize(camera.light_direction.xyz);
    let view_direction = safe_normalize(camera.camera_position.xyz - world_position);
    let half_vector = safe_normalize(light_direction + view_direction);

    let diffuse = max(dot(normal, light_direction), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), 32.0) * 0.12;
    let key = camera.light_color.rgb * diffuse + vec3<f32>(camera.light_color.a);
    let points = point_lighting(normal, world_position, view_direction);
    return color.rgb * (key + points) + vec3<f32>(specular);
}

// The offscreen target is Rgba8Unorm, which is what egui registers as a native texture.
// Encode gamma here so the viewport matches the rest of the interface.
fn gamma_encode(rgb: vec3<f32>) -> vec3<f32> {
    return pow(rgb, vec3<f32>(1.0 / 2.2));
}

@fragment
fn fs_mesh(fragment: MeshIn) -> @location(0) vec4<f32> {
    let rgb = shade_surface(fragment.normal, fragment.front_facing, fragment.world_position, fragment.color);
    return vec4<f32>(gamma_encode(rgb), fragment.color.a);
}

// The translucent pass reduces opacity so blending reveals what lies behind; with alpha 1 the
// blend would copy the surface and the style would be indistinguishable from *shaded*.
const GHOST_ALPHA: f32 = 0.22;

@fragment
fn fs_mesh_translucent(fragment: MeshIn) -> @location(0) vec4<f32> {
    let rgb = shade_surface(fragment.normal, fragment.front_facing, fragment.world_position, fragment.color);
    return vec4<f32>(gamma_encode(rgb), min(fragment.color.a, GHOST_ALPHA));
}

struct LineOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(vertex: VertexIn) -> LineOut {
    var out: LineOut;
    out.clip_position = camera.view_projection * vec4<f32>(vertex.position, 1.0);
    out.color = vertex.color;
    return out;
}

// A constant offset in normalized device depth pulls the edge pass toward the camera so a line
// wins against the face it lies on instead of z-fighting. wgpu rejects DepthBiasState on a line
// topology, so the edge pass applies the bias here rather than on the pipeline.
const EDGE_DEPTH_NUDGE: f32 = 1.0e-4;

@vertex
fn vs_edge(vertex: VertexIn) -> LineOut {
    var out: LineOut;
    out.clip_position = camera.view_projection * vec4<f32>(vertex.position, 1.0);
    out.clip_position.z = out.clip_position.z - EDGE_DEPTH_NUDGE * out.clip_position.w;
    out.color = vertex.color;
    return out;
}

struct LineIn {
    @location(0) color: vec4<f32>,
};

@fragment
fn fs_line(fragment: LineIn) -> @location(0) vec4<f32> {
    // Grid and path line colours are already in gamma space.
    return fragment.color;
}
