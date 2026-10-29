struct Uniforms {
    viewport: vec4<f32>, // physical width, height, backing scale, relative zoom
    appearance: vec4<f32>,
    document: vec4<f32>,
    pasteboard: vec4<f32>,
    paper:vec4<f32>,
    paper_color:vec4<f32>,
    paper_transform:vec4<f32>,
    paper_offset:vec4<f32>,
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
    let dark = (u.appearance.x % 2.0) > 0.5;
    var color = select(vec3(0.40), vec3(0.014), dark);
    if u.pasteboard.w > 0.5 { color = u.pasteboard.xyz; }
    if all(point >=  vec2(0.0)) && all(point < u.document.xy) {
        if u.appearance.w > 1.5 { return vec4(0.0); }
        if u.appearance.w > 0.5 {
            let cell = vec2<i32>(floor(point / 12.0));
            color = select(vec3(0.78), vec3(0.92), (cell.x + cell.y) % 2 == 0);
        } else { color = vec3(1.0); if u.paper.x>0.5 {let shade=1.-u.paper.z*0.14*(1.-paper_relief_filtered(point,u.paper,u.paper_transform,u.paper_offset.xy));let srgb=u.paper_color.rgb*shade;color=select(srgb/12.92,pow((srgb+vec3(0.055))/1.055,vec3(2.4)),srgb>vec3(0.04045));} }
    }
    if u.document.w>0.5 && all(point>=vec2(0.)) && all(point<u.document.xy) {color=vec3(dot(color,vec3(0.2126,0.7152,0.0722)));}
    return vec4(color, 1.0);
}
