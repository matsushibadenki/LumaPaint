struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32>, document: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;

fn selected(point: vec2<f32>) -> bool {
    return all(point >= vec2(0.0)) && all(point < u.document.xy) && selection_contains(point);
}

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(positions[index], 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = u.viewport.xy / u.viewport.z;
    let pixel = position.xy / u.viewport.z;
    let scale = max(0.01, min((size.x-48.0)/u.document.x, (size.y-48.0)/u.document.y)) * u.viewport.w;
    let point = (pixel-size*0.5-u.appearance.yz)/scale+u.document.xy*0.5;
    let distance = max(selection_distance(point), rectangle_distance(point,vec4(0.0,0.0,u.document.xy)));
    let dx = abs(dpdx(distance));
    let dy = abs(dpdy(distance));
    let edge = abs(distance)/max(dx+dy,0.0001);
    if edge > 0.9 { discard; }
    // Coincident union edges and a completely subtracted selection are not boundaries.
    let step = 1.0/(scale*u.viewport.z);
    let inside = selected(point);
    if selected(point+vec2(step,0.0)) == inside && selected(point-vec2(step,0.0)) == inside
        && selected(point+vec2(0.0,step)) == inside && selected(point-vec2(0.0,step)) == inside { discard; }
    // Follow the local tangent so diagonal edges do not become solid lines.
    let along = select(pixel.x,pixel.y,dx > dy);
    let stripe = i32(floor(along/4.0)) % 2 == 0;
    return vec4(vec3(select(0.0,1.0,stripe)),1.0-smoothstep(0.55,0.9,edge));
}
