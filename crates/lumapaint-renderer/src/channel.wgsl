struct Uniforms { viewport: vec4<f32>, appearance: vec4<f32>, document: vec4<f32> }
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var source_sampler: sampler;
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = array<vec2<f32>, 3>(vec2(-1.0,-1.0), vec2(3.0,-1.0), vec2(-1.0,3.0));
    return vec4(p[i], 0.0, 1.0);
}
@fragment fn fs_main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    var color = textureLoad(source, vec2<i32>(p.xy), 0);
    if u.document.w > 0.5 {
        let gray = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
        color = vec4(vec3(gray), color.a);
    }
    let size = u.viewport.xy / u.viewport.z;
    let fit = max(0.01, min((size.x-48.0)/u.document.x, (size.y-48.0)/u.document.y));
    let point = (p.xy/u.viewport.z-size*0.5-u.appearance.yz)/(fit*u.viewport.w)+u.document.xy*0.5;
    if any(point < vec2(0.0)) || any(point >= u.document.xy) { return color; }
    let mode = u32(floor(u.appearance.x / 2.0));
    if mode == 0u { return color; }
    let straight = select(vec3(0.0), color.rgb / max(color.a, 0.000001), color.a > 0.0);
    let rgb = select(12.92*straight, 1.055*pow(max(straight,vec3(0.0)),vec3(1.0/2.4))-0.055, straight>vec3(0.0031308));
    var value = 0.0;
    if mode == 1u { value = rgb.r; }
    if mode == 2u { value = rgb.g; }
    if mode == 3u { value = rgb.b; }
    if mode == 4u { value = color.a; }
    if mode >= 5u {
        let k = 1.0-max(rgb.r,max(rgb.g,rgb.b));
        let cmy = (vec3(1.0)-rgb-vec3(k))/max(1.0-k,0.00001);
        if mode == 5u { value = 1.0-cmy.r; }
        if mode == 6u { value = 1.0-cmy.g; }
        if mode == 7u { value = 1.0-cmy.b; }
        if mode == 8u { value = 1.0-k; }
    }
    value = clamp(value,0.0,1.0);
    let linear = select(value/12.92,pow((value+0.055)/1.055,2.4),value>0.04045);
    return vec4(vec3(linear),1.0);
}
