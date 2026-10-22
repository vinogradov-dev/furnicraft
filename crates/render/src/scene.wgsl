struct Camera {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    light_direction: vec4<f32>,
    // rgb = key colour, a = ambient fraction
    light_color: vec4<f32>,
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

struct MeshIn {
    @location(0) normal: vec3<f32>,
    @location(1) world_position: vec3<f32>,
    @location(2) color: vec4<f32>,
    @builtin(front_facing) front_facing: bool,
};

@fragment
fn fs_mesh(fragment: MeshIn) -> @location(0) vec4<f32> {
    // Panels are open on both faces, so flip the normal for back-facing triangles instead of
    // culling them or lighting the inside of a cutout black.
    let normal = normalize(select(-fragment.normal, fragment.normal, fragment.front_facing));
    let light_direction = normalize(camera.light_direction.xyz);
    let view_direction = normalize(camera.camera_position.xyz - fragment.world_position);
    let half_vector = normalize(light_direction + view_direction);

    let diffuse = max(dot(normal, light_direction), 0.0);
    let specular = pow(max(dot(normal, half_vector), 0.0), 32.0) * 0.12;
    let light = camera.light_color.rgb * diffuse + vec3<f32>(camera.light_color.a);
    let rgb = fragment.color.rgb * light + vec3<f32>(specular);

    // The offscreen target is Rgba8Unorm, which is what egui registers as a native texture.
    // Encode gamma here so the viewport matches the rest of the interface.
    return vec4<f32>(pow(rgb, vec3<f32>(1.0 / 2.2)), fragment.color.a);
}

struct GridOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_grid(vertex: VertexIn) -> GridOut {
    var out: GridOut;
    out.clip_position = camera.view_projection * vec4<f32>(vertex.position, 1.0);
    out.color = vertex.color;
    return out;
}

struct GridIn {
    @location(0) color: vec4<f32>,
};

@fragment
fn fs_grid(fragment: GridIn) -> @location(0) vec4<f32> {
    // Grid colours are already in gamma space.
    return fragment.color;
}
