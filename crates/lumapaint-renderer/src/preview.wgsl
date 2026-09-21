struct Uniforms {
    viewport: vec4<f32>, // physical width, height, backing scale, relative zoom
    appearance: vec4<f32>,
    document: vec4<f32>,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = u.viewport.xy / u.viewport.z;
    let pixel = position.xy / u.viewport.z;
    let fit = max(0.01, min((size.x - 48.0) / u.document.x, (size.y - 48.0) / u.document.y));
    let point = (pixel - size * 0.5 - u.appearance.yz) / (fit * u.viewport.w) + u.document.xy * 0.5;
    let dark = u.appearance.x > 0.5;
    var color = select(vec3(0.40), vec3(0.014), dark);
    if all(point >= vec2(0.0)) && all(point < u.document.xy) {
        if u.appearance.w > 0.5 {
            let cell = vec2<i32>(floor(point / 12.0));
            color = select(vec3(0.78), vec3(0.92), (cell.x + cell.y) % 2 == 0);
        } else { color = vec3(1.0); }
    }
    return vec4(color, 1.0);
}
