struct Params { size: vec4<f32>, matrix: vec4<f32>, origin: vec4<f32> }
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read> ramp: array<u32>;
@group(0) @binding(2) var<storage, read_write> pixels: array<u32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let w = u32(p.size.x); let h = u32(p.size.y); let i = id.x;
    if i >= w*h { return; }
    let point = vec2<f32>(f32(i%w)+0.5, f32(i/w)+0.5)-p.origin.xy;
    let det = p.matrix.x*p.matrix.w-p.matrix.y*p.matrix.z;
    let u = (p.matrix.w*point.x-p.matrix.z*point.y)/det;
    let v = (-p.matrix.y*point.x+p.matrix.x*point.y)/det;
    var t: f32;
    if p.size.z == 0. { t = atan2(v,u)/6.28318530718; t = t-floor(t); }
    else if p.size.z == 1. { t = abs(u); }
    else { t = abs(u)+abs(v); }
    if p.size.w != 0. { t += (f32((i*1664525u+1013904223u)>>24u)/255.-0.5)/255.; }
    let c = ramp[u32(round(clamp(t,0.,1.)*4096.))];
    let a = c>>24u;
    pixels[i] = ((c&255u)*a/255u) | ((((c>>8u)&255u)*a/255u)<<8u) |
        ((((c>>16u)&255u)*a/255u)<<16u) | (a<<24u);
}
