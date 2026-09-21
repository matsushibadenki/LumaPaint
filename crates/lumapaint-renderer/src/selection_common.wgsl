struct SelectionRegion { bounds: vec4<f32>, info: vec4<f32> }
@group(1) @binding(0) var<storage, read> selection_regions: array<SelectionRegion>;

fn rectangle_distance(point: vec2<f32>, bounds: vec4<f32>) -> f32 {
    let q = abs(point-bounds.xy-bounds.zw*0.5)-bounds.zw*0.5;
    return max(q.x,q.y);
}
fn selection_contains(point: vec2<f32>) -> bool {
    var inside = false;
    for (var index = 0u; index < arrayLength(&selection_regions); index += 1u) {
        let region = selection_regions[index];
        let operation = i32(region.info.x);
        if operation == 4 { return true; }
        let local = point-region.bounds.xy;
        var hit = all(local >= vec2(0.0)) && all(local < region.bounds.zw);
        if region.info.y > 0.5 {
            let normalized = local/region.bounds.zw*2.0-vec2(1.0);
            hit = dot(normalized,normalized) <= 1.0;
        }
        switch operation {
            case 0: { inside = hit; }
            case 1: { inside = inside || hit; }
            case 2: { inside = inside && !hit; }
            case 3: { inside = hit && !inside; }
            default: {}
        }
    }
    return inside;
}
fn selection_distance(point: vec2<f32>) -> f32 {
    var result = -100000.0;
    for (var index = 0u; index < arrayLength(&selection_regions); index += 1u) {
        let region = selection_regions[index];
        let operation = i32(region.info.x);
        if operation == 4 { return -100000.0; }
        var distance = rectangle_distance(point,region.bounds);
        if region.info.y > 0.5 {
            let radius = region.bounds.zw*0.5;
            distance = (length((point-region.bounds.xy-radius)/radius)-1.0)*min(radius.x,radius.y);
        }
        switch operation {
            case 0: { result = distance; }
            case 1: { result = min(result,distance); }
            case 2: { result = max(result,-distance); }
            case 3: { result = max(-result,distance); }
            default: {}
        }
    }
    return result;
}
