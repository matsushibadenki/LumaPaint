use super::*;
use crate::{
    document::{vector_svg, SvgLayer},
    vector::{TextRun, VectorObject},
};
use std::{
    f32::consts::{PI, TAU},
    fmt::Write,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug)]
struct Sample {
    x: f32,
    y: f32,
    sx: f32,
    sy: f32,
    rotation: f32,
    skew: f32,
    alpha: f32,
    blur: f32,
    glow: f32,
    hue: f32,
    tint: f32,
    desaturate: f32,
    chromatic: f32,
    noise: f32,
    displace: f32,
    shadow: f32,
    reveal: f32,
    mask: Option<Operation>,
    stroke: Option<(f32, f32)>,
    morph: f32,
    color: Option<[f32; 3]>,
}
impl Default for Sample {
    fn default() -> Self {
        Self {
            x: 0.,
            y: 0.,
            sx: 1.,
            sy: 1.,
            rotation: 0.,
            skew: 0.,
            alpha: 1.,
            blur: 0.,
            glow: 0.,
            hue: 0.,
            tint: 0.,
            desaturate: 0.,
            chromatic: 0.,
            noise: 0.,
            displace: 0.,
            shadow: 0.,
            reveal: 1.,
            mask: None,
            stroke: None,
            morph: 0.,
            color: None,
        }
    }
}

fn sample(preset: &Preset, clip: &Clip, p: f32, index: usize, count: usize, fps: u32) -> Sample {
    let seconds = p * clip.duration as f32 / fps as f32;
    let t = clip
        .curve
        .unwrap_or(preset.curve)
        .sample(p, seconds, clip.seed);
    let q = if preset.exit { 1. - t } else { t };
    let wave = (p * TAU * clip.frequency + index as f32 * 0.65).sin();
    let envelope = (PI * p).sin().max(0.);
    let pulse = if preset.cyclic {
        (1. - (TAU * p * clip.frequency).cos()) * 0.5
    } else {
        envelope
    };
    let strength = clip.strength;
    let distance = clip.distance * strength;
    let odd = if index.is_multiple_of(2) { 1. } else { -1. };
    let norm = index as f32 / count.saturating_sub(1).max(1) as f32;
    let mut s = Sample::default();
    for operation in &preset.operations {
        match operation {
            Operation::Fade => s.alpha *= q.clamp(0., 1.),
            Operation::SlideX => {
                s.x += (1. - q)
                    * -distance
                    * if clip.order == Order::Alternating {
                        odd
                    } else {
                        1.
                    }
            }
            Operation::SlideY => {
                s.y += (1. - q) * distance * if preset.id.contains("down") { -1. } else { 1. }
            }
            Operation::Travel => s.x += distance * t,
            Operation::Arc => {
                if clip.path.len() >= 2 {
                    let (position, tangent) = path_point(&clip.path, t.clamp(0., 1.));
                    s.x += (position[0] - clip.path[0][0]) * strength;
                    s.y += (position[1] - clip.path[0][1]) * strength;
                    if preset.id.contains("follow") || preset.id.contains("motion-path") {
                        s.rotation += tangent.to_degrees();
                    }
                } else {
                    s.x += distance * t;
                    s.y -= distance * 0.6 * 4. * t * (1. - t);
                }
            }
            Operation::Orbit => {
                let a = p * TAU * clip.frequency + norm * TAU;
                s.x += distance * (a.cos() - 1.);
                s.y += distance * a.sin();
            }
            Operation::Zigzag => {
                s.x += distance * t;
                s.y += distance * 0.35 * (TAU * 2. * p).sin();
            }
            Operation::Hop => {
                if preset.cyclic {
                    s.y -= distance * pulse;
                } else if preset.id.contains("land")
                    || preset.id.contains("drop")
                    || preset.id.contains("raining")
                {
                    s.y -= distance
                        * (1. - Curve::Bounce { restitution: 0.55 }.sample(p, seconds, clip.seed));
                } else {
                    s.y -= distance * envelope;
                }
            }
            Operation::Anticipate => s.x -= distance * 0.25 * (PI * (p / 0.3).clamp(0., 1.)).sin(),
            Operation::Recoil => {
                s.x += distance * 0.25 * (TAU * clip.frequency * p).sin() * (1. - p).powi(2);
            }
            Operation::Shake => {
                s.x += distance
                    * 0.12
                    * (TAU * clip.frequency * 3. * p + norm).sin()
                    * if preset.cyclic { 1. } else { (1. - p).powi(2) };
                s.y += distance * 0.035 * (TAU * clip.frequency * 5. * p).cos() * envelope;
            }
            Operation::Scale => {
                let value = if preset.cyclic {
                    1. + 0.2 * pulse * strength
                } else {
                    1. - (1. - q) * strength
                };
                s.sx *= value;
                s.sy *= value;
            }
            Operation::ScaleX => {
                s.sx *= if preset.cyclic {
                    1. + 0.25 * wave * strength
                } else {
                    1. - (1. - q) * strength
                }
            }
            Operation::ScaleY => {
                s.sy *= if preset.cyclic {
                    1. + 0.25 * wave * strength
                } else {
                    1. - (1. - q) * strength
                }
            }
            Operation::Pulse => {
                s.sx *= 1. + 0.18 * pulse * strength;
                s.sy *= 1. + 0.18 * pulse * strength;
            }
            Operation::Squash => {
                let impact = if preset.operations.contains(&Operation::Hop) {
                    (-wave).max(0.) * envelope
                } else {
                    envelope
                };
                s.sy *= 1. - 0.35 * impact * strength;
                s.sx *= 1. + 0.25 * impact * strength;
            }
            Operation::Jelly => {
                let v = wave * (1. - p) * 0.3 * strength;
                s.sx *= 1. + v;
                s.sy *= 1. - v;
            }
            Operation::Spin => {
                s.rotation += clip.angle
                    * strength
                    * t
                    * if preset.id.contains("counter") {
                        odd
                    } else {
                        1.
                    }
            }
            Operation::Swing => {
                s.rotation += clip.angle.min(90.)
                    * 0.4
                    * wave
                    * strength
                    * if preset.cyclic { 1. } else { (1. - p).powi(2) }
            }
            Operation::MeshPlane
            | Operation::Cube
            | Operation::DepthStack
            | Operation::Coverflow => {}
            Operation::FlipX => s.sy *= (PI * (1. - q) * strength).cos(),
            Operation::FlipY => s.sx *= (PI * (1. - q) * strength).cos(),
            Operation::Tilt => {
                s.rotation += clip.angle.min(60.) * 0.25 * wave * strength;
                s.skew += 0.2 * wave * strength;
            }
            Operation::Skew => s.skew += 0.4 * wave * envelope * strength,
            Operation::Parallax => {
                s.x += distance * t * (0.2 + 0.8 * norm);
                s.y -= distance * 0.3 * t * norm;
            }
            Operation::Reorder => {
                s.x += distance * (1. - 2. * norm) * t;
                s.y += distance * 0.3 * odd * envelope;
            }
            Operation::Gather => {
                let a = random(clip.seed, index as u32) * TAU;
                let radius = distance * (1. - q);
                s.x += a.cos() * radius;
                s.y += a.sin() * radius;
            }
            Operation::Scatter => {
                let a = random(clip.seed, index as u32) * TAU;
                s.x += a.cos() * distance * t;
                s.y += a.sin() * distance * t;
                s.rotation += odd * clip.angle * strength * t;
            }
            Operation::Follow => {
                s.x += distance * t;
                s.rotation += odd * clip.angle * 0.08 * wave * (1. - p) * strength;
            }
            Operation::Wave => {
                s.y += distance * 0.2 * wave * if preset.cyclic { 1. } else { envelope };
            }
            Operation::Hold => {
                s.x += distance * (p * 6.).floor() / 6.;
            }
            Operation::Cut => s.alpha *= if p < 0.5 { 0. } else { 1. },
            Operation::Camera => {
                let zoom = 1. + 0.65 * t * strength;
                s.sx *= zoom;
                s.sy *= zoom;
            }
            Operation::Crane => {
                s.y -= distance * t;
                s.sx *= 1. + 0.3 * t;
                s.sy *= 1. + 0.3 * t;
            }
            Operation::Wipe
            | Operation::Diagonal
            | Operation::Iris
            | Operation::Split
            | Operation::Blinds
            | Operation::Blocks
            | Operation::Dissolve
            | Operation::ClipWindow => {
                s.mask = Some(*operation);
                s.reveal = q.clamp(0., 1.);
            }
            Operation::Blur => {
                s.blur += clip.blur
                    * strength
                    * if preset.cyclic {
                        pulse
                    } else {
                        1. - q.clamp(0., 1.)
                    };
            }
            Operation::Glow => {
                s.glow += clip.glow * strength * pulse;
            }
            Operation::Sweep => {
                s.tint += 0.5 * envelope * strength;
                s.hue += p * 60. * strength;
            }
            Operation::Hue => s.hue += p * 360. * clip.frequency * strength,
            Operation::Tint => s.tint += envelope * strength,
            Operation::Desaturate => s.desaturate += q.clamp(0., 1.) * strength,
            Operation::Chromatic => s.chromatic += distance * 0.08 * envelope,
            Operation::Noise => {
                s.noise += 0.25 * strength * if preset.cyclic { pulse } else { 1. - p }
            }
            Operation::Displace => s.displace += distance * 0.08 * wave * envelope,
            Operation::Shadow => s.shadow += distance * 0.15 * pulse,
            Operation::StrokeDraw => s.stroke = Some((q.clamp(0., 1.), 0.)),
            Operation::StrokeTrim => s.stroke = Some((0.25, p * 0.75)),
            Operation::Morph => s.morph += 0.2 * strength * wave * envelope,
            Operation::Repeat
            | Operation::Particles
            | Operation::Confetti
            | Operation::Snow
            | Operation::Embers
            | Operation::Fireworks
            | Operation::Coalesce => {}
            Operation::Tracking => {
                s.x += (norm - 0.5) * distance * (1. - q);
            }
            Operation::FontAxes | Operation::KanjiStroke => {},
            Operation::Weight => s.sx *= 1. + 0.2 * pulse * strength,
            Operation::Underline
            | Operation::Highlight
            | Operation::Caption
            | Operation::Lines
            | Operation::Typewriter
            | Operation::Scramble
            | Operation::TextSwap
            | Operation::Counter => {}
            Operation::Marquee => s.x -= distance * t * 2.,
            Operation::Walk => {
                s.x += distance * t;
                s.y -= distance * 0.08 * (TAU * p * clip.frequency * 2.).sin().abs();
                s.rotation += clip.angle * 0.03 * wave;
            }
            Operation::Blink => {
                s.sy *= 1. - 0.9 * ((p - 0.5).abs() * 12.).clamp(0., 1.).mul_add(-1., 1.)
            }
            Operation::Tail => {
                s.rotation += clip.angle * 0.2 * (TAU * p * clip.frequency - norm).sin() * strength
            }
            Operation::AudioPulse | Operation::Beat => {
                s.sy *= 1. + 0.6 * wave.abs() * strength;
                s.glow += clip.glow * wave.abs() * strength;
            }
            Operation::AudioDuck => s.alpha *= 1. - 0.65 * wave.abs() * strength,
        }
    }
    if let Some(actor) = preset.actors.get(index % preset.actors.len().max(1)) {
        // Authored multi-part timing is retained as numeric property tracks.
        // In particular, an input reaction must not start before its press/hold.
        s.x = 0.;
        s.y = 0.;
        s.sx = 1.;
        s.sy = 1.;
        s.rotation = 0.;
        s.skew = 0.;
        s.alpha = 1.;
        let values = actor_values(
            actor,
            if clip.curve.is_some() {
                t.clamp(0., 1.)
            } else {
                p
            },
        );
        for (key, value) in &values {
            match key.as_str() {
                "x" => s.x = value * distance,
                "y" => s.y = value * distance,
                "scaleX" => s.sx = 1. + (value - 1.) * strength,
                "scaleY" => s.sy = 1. + (value - 1.) * strength,
                "rotation" => s.rotation = value * strength * clip.angle / 180.,
                "skew" => s.skew = value * strength,
                "rotateX" => s.sy *= (value * strength * clip.angle / 180.).to_radians().cos(),
                "rotateY" => s.sx *= (value * strength * clip.angle / 180.).to_radians().cos(),
                "alpha" => s.alpha = 1. + (value - 1.) * strength,
                "blur" => s.blur = value * clip.blur / 12. * strength,
                "hue" => s.hue = value * strength,
                "tint" => s.tint = value * strength,
                "reveal" => {
                    s.reveal = *value;
                    if s.mask.is_none() {
                        s.mask = Some(Operation::Wipe);
                    }
                }
                "strokeOffset" => s.stroke = Some(((1. - value).clamp(0., 1.), 0.)),
                _ => {}
            }
        }
        if values.contains_key("red") || values.contains_key("green") || values.contains_key("blue")
        {
            s.color = Some(if clip.color != [100, 140, 255, 255] {
                [
                    clip.color[0] as f32 / 255.,
                    clip.color[1] as f32 / 255.,
                    clip.color[2] as f32 / 255.,
                ]
            } else {
                [
                    *values.get("red").unwrap_or(&0.5),
                    *values.get("green").unwrap_or(&0.5),
                    *values.get("blue").unwrap_or(&0.5),
                ]
            });
            s.tint = strength.clamp(0., 1.);
        }
    }
    s.sx = s.sx.clamp(-10., 10.);
    s.sy = s.sy.clamp(-10., 10.);
    s.alpha = s.alpha.clamp(0., 1.);
    s
}
fn actor_values(actor: &Actor, p: f32) -> BTreeMap<String, f32> {
    let mut result = BTreeMap::new();
    let names: std::collections::BTreeSet<_> = actor
        .keys
        .iter()
        .flat_map(|key| key.values.keys())
        .collect();
    for name in names {
        let keys: Vec<_> = actor
            .keys
            .iter()
            .filter_map(|key| key.values.get(name).map(|value| (key.offset, *value)))
            .collect();
        let i = keys.partition_point(|(offset, _)| *offset <= p);
        let value = if i == 0 {
            keys[0].1
        } else if i == keys.len() {
            keys[i - 1].1
        } else {
            let a = keys[i - 1];
            let b = keys[i];
            let t = ((p - a.0) / (b.0 - a.0)).clamp(0., 1.);
            a.1 + (b.1 - a.1) * t
        };
        result.insert(name.clone(), value);
    }
    result
}

fn path_point(path: &[[f32; 2]], progress: f32) -> ([f32; 2], f32) {
    let length = path
        .windows(2)
        .map(|w| (w[1][0] - w[0][0]).hypot(w[1][1] - w[0][1]))
        .sum::<f32>();
    let mut remaining = length * progress;
    for pair in path.windows(2) {
        let dx = pair[1][0] - pair[0][0];
        let dy = pair[1][1] - pair[0][1];
        let d = dx.hypot(dy);
        if d > 0. && remaining <= d {
            let u = remaining / d;
            return ([pair[0][0] + dx * u, pair[0][1] + dy * u], dy.atan2(dx));
        }
        remaining -= d;
    }
    (*path.last().unwrap_or(&[0., 0.]), 0.)
}
fn object_bounds(object: &VectorObject) -> [f32; 4] {
    if let Some(b) = object.conservative_drawing_bounds() {
        return b.map(|v| v as f32);
    }
    if let Some(text) = &object.text {
        let [a, b, c, d, x, y] = object.transform;
        let w = text.box_width;
        let h = text.box_height.unwrap_or(
            text.font_size * text.line_height * text.content.lines().count().max(1) as f32,
        );
        let corners = [[0., 0.], [w, 0.], [w, h], [0., h]]
            .map(|[u, v]| [a * u + c * v + x, b * u + d * v + y]);
        let mut result = [
            f32::INFINITY,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
        ];
        for p in corners {
            result[0] = result[0].min(p[0]);
            result[1] = result[1].min(p[1]);
            result[2] = result[2].max(p[0]);
            result[3] = result[3].max(p[1]);
        }
        return result;
    }
    [0., 0., 1., 1.]
}
fn affine(s: &Sample, bounds: [f32; 4], pivot: [f32; 2]) -> [f32; 6] {
    let ax = bounds[0] + (bounds[2] - bounds[0]) * pivot[0];
    let ay = bounds[1] + (bounds[3] - bounds[1]) * pivot[1];
    let (sin, cos) = s.rotation.to_radians().sin_cos();
    let (a, b, c, d) = (
        cos * s.sx,
        sin * s.sx,
        (cos * s.skew - sin) * s.sy,
        (sin * s.skew + cos) * s.sy,
    );
    [
        a,
        b,
        c,
        d,
        s.x + ax - a * ax - c * ay,
        s.y + ay - b * ax - d * ay,
    ]
}
fn transform(object: &mut VectorObject, m: [f32; 6]) {
    object.transform = crate::image_frame::multiply(m, object.transform);
}
fn warp(object: &mut VectorObject, amount: f32) {
    if amount.abs() < 0.0001 || object.text.is_some() || object.image_frame.is_some() {
        return;
    }
    // Straight SVG segments store endpoint controls. Subdivision supplies
    // interior handles so rectangles deform too, rather than staying rigid.
    for _ in 0..2 {
        if object.control_points.len() > 512 {
            break;
        }
        if crate::bezier::subdivide_all(object).is_err() {
            break;
        }
    }
    let Ok(mut contours) = crate::bezier::cubic_contours(&object.path.data) else {
        return;
    };
    if contours.iter().map(|(p, _)| p.len()).sum::<usize>() > 2048 {
        return;
    }
    let bounds = crate::stroke::path_bounds(&object.path.data).unwrap_or([0., 0., 1., 1.]);
    let width = (bounds[2] - bounds[0]).max(1.);
    let height = (bounds[3] - bounds[1]).max(1.);
    let mut source = String::new();
    let mut controls = Vec::new();
    for (points, closed) in &mut contours {
        for p in points.iter_mut() {
            p[0] += width * amount * ((p[1] - bounds[1]) / height * TAU).sin();
            p[1] += height * amount * ((p[0] - bounds[0]) / width * TAU).sin() * 0.5;
        }
        if let Ok(path) = crate::bezier::path_data(points, *closed) {
            source.push_str(&path);
            controls.extend(points.iter().copied());
        }
    }
    object.path.data = source;
    object.control_points = controls;
    object.kind = crate::vector::VectorObjectKind::Compound;
}

// Arc-length correspondence permits paths with different control counts.
fn morph_to(object: &mut VectorObject, target: &str, progress: f32) {
    let (Ok(source), Ok(target)) = (
        crate::bezier::cubic_contours(&object.path.data),
        crate::bezier::cubic_contours(target),
    ) else {
        return;
    };
    if source.len() != target.len() || object.text.is_some() || object.image_frame.is_some() {
        return;
    }
    let mut data = String::new();
    let mut controls = Vec::new();
    for ((a, closed), (b, _)) in source.iter().zip(&target) {
        let a = crate::bezier::flattened(a);
        let b = crate::bezier::flattened(b);
        let samples = a
            .len()
            .max(b.len())
            .clamp(8, (2048 / source.len().max(1) / 3).clamp(8, 128));
        let mut points = Vec::with_capacity(samples * 3 + 1);
        let mut previous = [0., 0.];
        for i in 0..=samples {
            let t = i as f32 / samples as f32;
            let (a, _) = path_point(&a, t);
            let (b, _) = path_point(&b, t);
            let point = [
                a[0] + (b[0] - a[0]) * progress,
                a[1] + (b[1] - a[1]) * progress,
            ];
            if i == 0 {
                points.push(point);
            } else {
                points.extend([previous, point, point]);
            }
            previous = point;
        }
        if let Ok(path) = crate::bezier::path_data(&points, *closed) {
            data.push_str(&path);
            controls.extend(points);
        }
    }
    object.path.data = data;
    object.control_points = controls;
    object.kind = crate::vector::VectorObjectKind::Compound;
}

fn text_motion(object: &mut VectorObject, clip: &Clip, preset: &Preset, frame: u32, fps: u32) {
    if !preset.category.starts_with("tx-")
        && !preset.operations.iter().any(|o| {
            matches!(
                o,
                Operation::Typewriter
                    | Operation::Counter
                    | Operation::TextSwap
                    | Operation::Scramble
                    | Operation::Caption
                    | Operation::Tracking
                    | Operation::Weight
            )
        })
    {
        return;
    }
    let color = object.fill.map_or([32, 32, 32], |fill| {
        [fill.color[0], fill.color[1], fill.color[2]]
    });
    let Some(text) = object.text.as_mut() else {
        return;
    };
    let Some(p) = clip.progress(frame, 0, 1) else {
        return;
    };
    let has = |op| preset.operations.contains(&op);
    let text_phase = clip
        .curve
        .unwrap_or(preset.curve)
        .sample(p, p * clip.duration as f32 / fps as f32, clip.seed)
        .clamp(0., 1.);
    if has(Operation::Counter) {
        let n = clip.number_from + (clip.number_to - clip.number_from) * text_phase as f64;
        text.content = format!("{:.0}", n);
        text.runs.clear();
        text.clear_measured_layout();
    } else if has(Operation::TextSwap) && text_phase >= 0.5 && !clip.alternate_text.is_empty() {
        text.content = clip.alternate_text.clone();
        text.runs.clear();
        text.clear_measured_layout();
    }
    let original = text.clone();
    let graphemes: Vec<_> = original.content.graphemes(true).collect();
    if graphemes.len() > 512 {
        return;
    }
    let mut runs = Vec::with_capacity(graphemes.len());
    let mut at = 0;
    let total = graphemes.len().max(1);
    let mut decoded = String::new();
    for (i, grapheme) in graphemes.iter().enumerate() {
        let end = at + grapheme.encode_utf16().count();
        let mut style = original.style_at(at, color);
        let local = clip.progress(frame, i, total).unwrap_or(0.);
        let value = sample(preset, clip, local, i, total, fps);
        if has(Operation::Typewriter) || has(Operation::Caption) {
            let threshold = (i + 1) as f32 / total as f32;
            if text_phase < threshold {
                style.no_color = true;
            }
        }
        if has(Operation::Scramble) && text_phase < (i + 1) as f32 / total as f32 {
            let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
            let j = (random(clip.seed, frame.wrapping_mul(37).wrapping_add(i as u32))
                * chars.len() as f32)
                .floor() as usize;
            decoded.push(chars.as_bytes()[j.min(chars.len() - 1)] as char);
        } else {
            decoded.push_str(grapheme);
        }
        if !has(Operation::Counter)
            && !has(Operation::Typewriter)
            && !has(Operation::TextSwap)
            && !has(Operation::Scramble)
        {
            style.scale_x = (style.scale_x * value.sx.abs()).max(0.001);
            style.scale_y = (style.scale_y * value.sy.abs()).max(0.001);
            style.baseline_shift -= value.y;
            style.rotation += value.rotation;
            style.tracking += value.x / total as f32 * 20.;
            if has(Operation::Fade) && value.alpha < 0.02 {
                style.no_color = true;
            }
        }
        if has(Operation::Tracking) {
            style.tracking += (1. - local) * clip.distance * 2. * clip.strength;
        }
        if has(Operation::Lines) {
            style.tracking += (1. - local) * clip.distance * clip.strength / total as f32;
            style.baseline_shift += (1. - local) * clip.distance * clip.strength * 0.2;
        }
        if has(Operation::FontAxes) {
            if !clip.variable_font.is_empty() {style.font_family=clip.variable_font.clone();style.bold=false;style.italic=false;}
            let amount=if preset.cyclic || preset.id.contains("wave") || preset.id.contains("breathe") || preset.id.contains("pulse") || preset.id.contains("pressure") || preset.id.contains("stretch") || preset.id.contains("proximity") {(1.-(TAU*local*clip.frequency).cos())*0.5}else{clip.curve.unwrap_or(preset.curve).sample(local,local*clip.duration as f32/fps as f32,clip.seed).clamp(0.,1.)};
            for axis in &clip.font_axes {style.font_axes.insert(axis.tag.clone(),axis.from+(axis.to-axis.from)*amount*clip.strength.min(1.));}
        }
        if has(Operation::Weight) {
            style.bold = local > 0.5;
            style.scale_x *= 1. + 0.25 * (PI * local).sin() * clip.strength;
        }
        if has(Operation::Underline) {
            style.underline = local > 0.;
        }
        if has(Operation::Highlight) {
            let i = i as f32 / total as f32;
            let weight = (1. - (i - p).abs() * 6.).clamp(0., 1.);
            for (channel, &c) in style.color.iter_mut().zip(&clip.color) {
                *channel = (*channel as f32 * (1. - weight) + c as f32 * weight).round() as u8;
            }
        }
        runs.push(TextRun {
            start: at,
            end,
            style,
        });
        at = end;
    }
    if has(Operation::Scramble) && decoded != text.content {
        text.content = decoded;
        text.runs.clear();
        text.clear_measured_layout();
    } else {
        text.runs = runs;
    }
}

fn svg_inner(source: &str) -> &str {
    let Some(start) = source
        .find("<svg")
        .and_then(|i| source[i..].find('>').map(|end| i + end + 1))
    else {
        return source;
    };
    let end = source.rfind("</svg>").unwrap_or(source.len());
    &source[start..end]
}
fn mask_markup(op: Operation, p: f32, b: [f32; 4], seed: u32) -> String {
    let [x, y, right, bottom] = b;
    let w = (right - x).max(1.);
    let h = (bottom - y).max(1.);
    let p = p.clamp(0., 1.);
    let mut out = String::new();
    match op {
        Operation::Iris => {
            let r = w.hypot(h) * 0.5 * p;
            let _ = write!(
                out,
                "<circle cx=\"{}\" cy=\"{}\" r=\"{r}\"/>",
                x + w * 0.5,
                y + h * 0.5
            );
        }
        Operation::Split => {
            let _ = write!(
                out,
                "<rect x=\"{}\" y=\"{y}\" width=\"{}\" height=\"{h}\"/>",
                x + w * (1. - p) * 0.5,
                w * p
            );
        }
        Operation::Diagonal => {
            let k = (w + h) * p;
            let _ = write!(out, "<path d=\"M {x} {y} H {} L {x} {} Z\"/>", x + k, y + k);
        }
        Operation::Blinds => {
            for i in 0..8 {
                let _ = write!(
                    out,
                    "<rect x=\"{x}\" y=\"{}\" width=\"{w}\" height=\"{}\"/>",
                    y + h * i as f32 / 8.,
                    h / 8. * p
                );
            }
        }
        Operation::Blocks | Operation::Dissolve => {
            for i in 0..64 {
                let threshold = if op == Operation::Dissolve {
                    random(seed, i)
                } else {
                    i as f32 / 64.
                };
                if p >= threshold {
                    let _ = write!(
                        out,
                        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                        x + w * (i % 8) as f32 / 8.,
                        y + h * (i / 8) as f32 / 8.,
                        w / 8. + 0.5,
                        h / 8. + 0.5
                    );
                }
            }
        }
        Operation::ClipWindow => {
            let _ = write!(
                out,
                "<rect x=\"{}\" y=\"{y}\" width=\"{}\" height=\"{h}\"/>",
                x + w * p * 0.75,
                w * 0.25
            );
        }
        _ => {
            let _ = write!(
                out,
                "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{h}\"/>",
                w * p
            );
        }
    }
    out
}
fn wrap(
    source: String,
    s: &Sample,
    clip: &Clip,
    bounds: [f32; 4],
    tag: &str,
    p: f32,
    preset: &Preset,
) -> String {
    let mut defs = String::new();
    let mut attrs = String::new();
    if let Some(mask) = s.mask {
        let shape = mask_markup(mask, s.reveal, bounds, clip.seed);
        let _ = write!(defs, "<clipPath id=\"{tag}-mask\">{shape}</clipPath>");
        let _ = write!(attrs, " clip-path=\"url(#{tag}-mask)\"");
    }
    let filtered = s.blur > 0.01
        || s.glow > 0.01
        || s.hue.abs() > 0.01
        || s.tint > 0.01
        || s.desaturate > 0.01
        || s.chromatic > 0.01
        || s.noise > 0.01
        || s.displace.abs() > 0.01
        || s.shadow > 0.01;
    if filtered {
        let mut filters = String::new();
        if s.hue.abs() > 0.01 {
            let _ = write!(
                filters,
                "<feColorMatrix type=\"hueRotate\" values=\"{}\"/>",
                s.hue
            );
        }
        if s.desaturate > 0.01 {
            let _ = write!(
                filters,
                "<feColorMatrix type=\"saturate\" values=\"{}\"/>",
                (1. - s.desaturate).clamp(0., 1.)
            );
        }
        if s.blur > 0.01 {
            let _ = write!(
                filters,
                "<feGaussianBlur stdDeviation=\"{}\"/>",
                s.blur.min(64.)
            );
        }
        if s.tint > 0.01 {
            let [r, g, b] = s.color.unwrap_or([
                clip.color[0] as f32 / 255.,
                clip.color[1] as f32 / 255.,
                clip.color[2] as f32 / 255.,
            ]);
            let a = s.tint.clamp(0., 1.);
            let _=write!(filters,"<feColorMatrix type=\"matrix\" values=\"{} 0 0 0 {} 0 {} 0 0 {} 0 0 {} 0 {} 0 0 0 1 0\"/>",1.-a,r*a,1.-a,g*a,1.-a,b*a);
        }
        if s.glow > 0.01 || s.shadow > 0.01 {
            let [r, g, b, _] = clip.color;
            let _=write!(filters,"<feGaussianBlur in=\"SourceAlpha\" stdDeviation=\"{}\" result=\"blur\"/><feOffset in=\"blur\" dx=\"{}\" dy=\"{}\" result=\"offset\"/><feFlood flood-color=\"rgb({r},{g},{b})\"/><feComposite in2=\"offset\" operator=\"in\" result=\"halo\"/><feMerge><feMergeNode in=\"halo\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>",s.glow.max(s.shadow*0.5).min(64.),s.shadow,s.shadow);
        }
        if s.noise > 0.01 || s.displace.abs() > 0.01 {
            let _=write!(filters,"<feTurbulence baseFrequency=\"{}\" numOctaves=\"2\" seed=\"{}\" result=\"noise\"/><feColorMatrix in=\"noise\" type=\"matrix\" values=\"1 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 0 1\" result=\"opaque-map\"/><feComposite in=\"SourceGraphic\" in2=\"opaque-map\" operator=\"in\" result=\"motion-source\"/><feDisplacementMap in=\"motion-source\" in2=\"noise\" scale=\"{}\" xChannelSelector=\"R\" yChannelSelector=\"G\"/>",0.01+s.noise*0.1,clip.seed,s.displace+s.noise*8.);
        }
        if s.chromatic > 0.01 {
            let _=write!(filters,"<feOffset in=\"SourceGraphic\" dx=\"{}\" result=\"red\"/><feColorMatrix in=\"red\" type=\"matrix\" values=\"1 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0\" result=\"r\"/><feOffset in=\"SourceGraphic\" dx=\"{}\" result=\"cyan\"/><feColorMatrix in=\"cyan\" type=\"matrix\" values=\"0 0 0 0 0 0 1 0 0 0 0 0 1 0 0 0 0 0 1 0\" result=\"c\"/><feBlend in=\"r\" in2=\"c\" mode=\"screen\"/>",s.chromatic,-s.chromatic);
        }
        let _=write!(defs,"<filter id=\"{tag}-filter\" x=\"-100%\" y=\"-100%\" width=\"300%\" height=\"300%\" color-interpolation-filters=\"sRGB\">{filters}</filter>");
        let _ = write!(attrs, " filter=\"url(#{tag}-filter)\"");
    }
    let mut content = source;
    if let Some((length, offset)) = s.stroke {
        // Canonical path lengths are measured in local coordinates. Dash values
        // are absolute, avoiding inconsistent pathLength support in SVG backends.
        content = stroke_segments(&content, length, offset);
    }
    if (p < 1. || preset.id == "sh-repeater") && preset.operations.contains(&Operation::Repeat) {
        // Keep one source definition. Inline copies grow exponentially when
        // trails are stacked; animation validation also bounds expansion depth.
        let _ = write!(defs, "<g id=\"{tag}-trail\">{content}</g>");
        let mut repeats = String::new();
        for i in (1..=clip.copies).rev() {
            let lag = i as f32 * 0.07;
            let radius = 1. + (p - lag).max(0.) * clip.strength;
            let [x, y, r, b] = bounds;
            let cx = (x + r) * 0.5;
            let cy = (y + b) * 0.5;
            let radial = preset.id == "sh-repeater";
            let rotation = if radial {
                360. * i as f32 / (clip.copies + 1) as f32 + p * clip.angle * clip.strength
            } else {
                0.
            };
            let radius = if radial { 1. } else { radius };
            let opacity = if radial {
                1.
            } else {
                (1. - p) * 0.12 / i as f32
            };
            let _=write!(repeats,"<use href=\"#{tag}-trail\" opacity=\"{}\" transform=\"translate({cx} {cy}) rotate({rotation}) scale({radius}) translate({} {})\"/>",opacity,-cx,-cy);
        }
        content = format!("{repeats}<use href=\"#{tag}-trail\"/>");
    }
    format!("<defs>{defs}</defs><g{attrs}>{content}</g>")
}

fn particles(clip: &Clip, preset: &Preset, p: f32, bounds: [f32; 4]) -> String {
    let Some(kind) = preset.operations.iter().find(|op| {
        matches!(
            op,
            Operation::Particles
                | Operation::Confetti
                | Operation::Snow
                | Operation::Embers
                | Operation::Fireworks
                | Operation::Coalesce
        )
    }) else {
        return String::new();
    };
    let [x, y, right, bottom] = bounds;
    let w = (right - x).max(32.);
    let h = (bottom - y).max(32.);
    let cx = (x + right) * 0.5;
    let cy = (y + bottom) * 0.5;
    let mut out = String::new();
    for i in 0..clip.particle_count {
        let a = random(clip.seed, i) * TAU;
        let speed = 0.4 + random(clip.seed + 1, i) * 0.6;
        let travel = clip.distance * clip.strength * (1. - (1. - p).powi(3)) * speed;
        let mut px = cx + a.cos() * travel;
        let mut py = cy + a.sin() * travel;
        let mut alpha = 1. - p;
        let radius = 1.5 + random(clip.seed + 3, i) * 3.;
        match kind {
            Operation::Snow => {
                px = x + random(clip.seed, i) * w + (p * TAU + speed * 4.).sin() * 16.;
                py = y + (random(clip.seed + 2, i) + p * speed).fract() * h;
                alpha = 0.75;
            }
            Operation::Embers => {
                px = x + random(clip.seed, i) * w + wave_sway(p, i) * 12.;
                py = bottom - p * clip.distance * speed;
                alpha = (PI * p).sin();
            }
            Operation::Coalesce => {
                px = cx + a.cos() * clip.distance * (1. - p) * speed;
                py = cy + a.sin() * clip.distance * (1. - p) * speed;
                alpha = p;
            }
            Operation::Confetti => py += clip.distance * p * p,
            Operation::Fireworks => py += clip.distance * p * p * 0.5,
            _ => {}
        }
        let [r, g, b, _] = clip.color;
        if *kind == Operation::Confetti {
            let _=write!(out,"<rect x=\"{px}\" y=\"{py}\" width=\"{}\" height=\"{}\" fill=\"rgb({r},{g},{b})\" opacity=\"{alpha}\" transform=\"rotate({} {px} {py})\"/>",radius*2.,radius,p*720.*speed);
        } else {
            let _=write!(out,"<circle cx=\"{px}\" cy=\"{py}\" r=\"{radius}\" fill=\"rgb({r},{g},{b})\" opacity=\"{alpha}\"/>");
        }
    }
    out
}
fn wave_sway(p: f32, i: u32) -> f32 {
    (p * TAU * 2. + i as f32).sin()
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn stroke_segments(source: &str, length: f32, offset: f32) -> String {
    let wrapped = format!("<svg>{source}</svg>");
    let Ok(doc) = roxmltree::Document::parse(&wrapped) else {
        return source.into();
    };
    let mut replacements = Vec::new();
    for node in doc.descendants().filter(|node| {
        node.has_tag_name("path") && !node.ancestors().any(|n| n.has_tag_name("defs"))
    }) {
        let Some(data) = node.attribute("d") else {
            continue;
        };
        let Ok(contours) = crate::bezier::cubic_contours(data) else {
            continue;
        };
        let total = contours
            .iter()
            .map(|(points, _)| {
                crate::bezier::flattened(points)
                    .windows(2)
                    .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
                    .sum::<f32>()
            })
            .sum::<f32>()
            .max(0.001);
        let mut path = String::from("<path");
        for attr in node.attributes().filter(|a| {
            !matches!(
                a.name(),
                "stroke-dasharray" | "stroke-dashoffset" | "pathLength"
            )
        }) {
            let _ = write!(path, " {}=\"{}\"", attr.name(), escape(attr.value()));
        }
        let _ = write!(
            path,
            " stroke-dasharray=\"{} {}\" stroke-dashoffset=\"{}\"/>",
            total * length,
            total * (1. - length),
            -total * offset
        );
        replacements.push((node.range(), path));
    }
    let mut result = wrapped;
    for (range, path) in replacements.into_iter().rev() {
        result.replace_range(range, &path);
    }
    svg_inner(&result).into()
}

pub(crate) fn apply(
    layer: &mut SvgLayer,
    clips: &[Clip],
    frame: u32,
    fps: u32,
    dimensions: [u32; 2],
    outlines: Option<&Outline<'_>>,
) {
    layer.motion_scene = None;
    const SOURCE_LIMIT: usize = 4 * 1024 * 1024;
    if layer.source.len() > SOURCE_LIMIT {
        return;
    }
    let mut expanded_clips = clips.to_vec();
    if let Some(resolve) = outlines.filter(|_| layer.vector_layer) {
        let mut expanded = Vec::new();
        for object in &layer.vector_objects {
            let needs_outline = object.text.is_some()
                && clips.iter().any(|clip| {
                    clip.enabled
                        && clip.strength > 0.
                        && clip.progress(frame, 0, 1).is_some()
                        && (clip.objects.is_empty() || clip.objects.contains(&object.id))
                        && preset(&clip.preset).is_some_and(|p| {
                            p.operations.iter().any(|o| {
                                matches!(
                                    o,
                                    Operation::StrokeDraw
                                        | Operation::StrokeTrim
                                        | Operation::Morph
                                )
                            })
                        })
                });
            if needs_outline {
                let mut local = object.clone();
                local.transform = [1., 0., 0., 1., 0., 0.];
                local.opacity = 1.;
                let source = vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(&local));
                if let Ok(mut glyphs) = resolve(&source, &local) {
                    for (i, glyph) in glyphs.iter_mut().enumerate() {
                        glyph.id = format!("{}-motion-glyph-{i}", object.id);
                        glyph.name = object.name.clone();
                        glyph.group_path = object.group_path.clone();
                        glyph.opacity = object.opacity;
                        transform(glyph, object.transform);
                    }
                    for clip in &mut expanded_clips {
                        if let Some(index) = clip.objects.iter().position(|id| id == &object.id) {
                            clip.objects
                                .splice(index..=index, glyphs.iter().map(|g| g.id.clone()));
                        }
                    }
                    expanded.extend(glyphs);
                    continue;
                }
            }
            expanded.push(object.clone());
        }
        layer.vector_objects = expanded;
    }
    let clips = &expanded_clips;
    let active: Vec<_> = clips
        .iter()
        .filter(|clip| clip.strength > 0.)
        .filter_map(|clip| {
            clip.progress(frame, 0, 1)
                .and_then(|p| preset(&clip.preset).map(|preset| (clip, preset, p)))
        })
        .collect();
    if active.is_empty() {
        return;
    }
    let has_clipping = layer
        .vector_objects
        .iter()
        .any(|o| o.clipping_group.is_some());
    if !layer.vector_layer || has_clipping || layer.vector_objects.is_empty() {
        let bounds = [0., 0., dimensions[0] as f32, dimensions[1] as f32];
        let mut source = layer.source.clone();
        for (i, (clip, preset, p)) in active.iter().enumerate() {
            let s = sample(preset, clip, *p, 0, 1, fps);
            let [a, b, c, d, e, f] = affine(&s, bounds, clip.pivot);
            source = format!(
                "<g transform=\"matrix({a} {b} {c} {d} {e} {f})\" opacity=\"{}\">{source}</g>",
                s.alpha
            );
            source = wrap(source, &s, clip, bounds, &format!("lm-{i}"), *p, preset);
            source.push_str(&particles(clip, preset, *p, bounds));
            if source.len() > SOURCE_LIMIT {
                layer.vector_layer = false;
                return;
            }
        }
        layer.source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{source}</svg>",
            dimensions[0], dimensions[1]
        );
        layer.vector_layer = false;
        layer.vector_objects.clear();
        return;
    }
    let object_count = layer.vector_objects.len();
    let mut fragments = Vec::with_capacity(object_count);
    let mut noncanonical = false;
    let mut source_bytes = 0usize;
    let mut scene = mesh::Scene::default();
    let has_mesh = active.iter().any(|(_, preset, _)| mesh::enabled(preset));
    for (object_index, object) in layer.vector_objects.iter_mut().enumerate() {
        let mut wrappers = Vec::new();
        for (clip_index, (clip, preset, _)) in active.iter().enumerate() {
            if !clip.objects.is_empty() && !clip.objects.contains(&object.id) {
                continue;
            }
            let rank = if clip.objects.is_empty() {
                object_index
            } else {
                clip.objects
                    .iter()
                    .position(|id| id == &object.id)
                    .unwrap_or(0)
            };
            let count = if clip.objects.is_empty() {
                object_count
            } else {
                clip.objects.len()
            };
            let p = clip.progress(frame, rank, count).unwrap_or(0.);
            let mut s = sample(preset, clip, p, rank, count, fps);
            let bounds = object_bounds(object);
            text_motion(object, clip, preset, frame, fps);
            let glyph_motion = object.text.is_some() && preset.category.starts_with("tx-");
            if !glyph_motion {
                transform(object, affine(&s, bounds, clip.pivot));
                object.opacity *= s.alpha;
            }
            if preset.operations.contains(&Operation::Morph) && !clip.morph_target.is_empty() {
                let progress = clip
                    .curve
                    .unwrap_or(preset.curve)
                    .sample(p, p * clip.duration as f32 / fps as f32, clip.seed)
                    .clamp(0., 1.)
                    * clip.strength.min(1.);
                morph_to(object, &clip.morph_target, progress);
            } else {
                warp(object, s.morph);
            }
            if s.stroke.is_some() {
                if p == 1. && preset.operations.contains(&Operation::StrokeDraw) {
                    s.stroke = None;
                } else {
                    if object.stroke.is_none() {
                        object.stroke = object.fill.or(Some(crate::vector::VectorPaint {
                            registration: false,
                            color: clip.color,
                        }));
                        object.stroke_gradient = object.fill_gradient.clone();
                        object.stroke_width = object.stroke_width.max(2.);
                    }
                    object.fill = None;
                    object.fill_gradient = None;
                    if p == 0. && preset.operations.contains(&Operation::StrokeDraw) {
                        object.opacity = 0.;
                    }
                }
            }
            wrappers.push((clip_index, *clip, *preset, p, s, object_bounds(object)));
        }
        let glyph_motion = object.text.is_some()
            && active.iter().any(|(clip, preset, _)| {
                preset.category.starts_with("tx-")
                    && (clip.objects.is_empty() || clip.objects.contains(&object.id))
            });
        let source = if glyph_motion {
            noncanonical = true;
            glyph_source(object, &active, frame, fps, dimensions)
        } else {
            vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(object))
        };
        let mut source = svg_inner(&source)
            .to_string()
            .replace("lp-gradient-", &format!("lm-{object_index}-gradient-"))
            .replace("lp-image-frame-", &format!("lm-{object_index}-image-"));
        for (clip_index, clip, preset, p, s, bounds) in wrappers {
            let complex = s.mask.is_some()
                || s.stroke.is_some()
                || s.blur > 0.01
                || s.glow > 0.01
                || s.hue.abs() > 0.01
                || s.tint > 0.01
                || s.desaturate > 0.01
                || s.chromatic > 0.01
                || s.noise > 0.01
                || s.displace.abs() > 0.01
                || s.shadow > 0.01
                || preset.operations.iter().any(|o| {
                    matches!(
                        o,
                        Operation::Repeat
                            | Operation::Particles
                            | Operation::Confetti
                            | Operation::Snow
                            | Operation::Embers
                            | Operation::Fireworks
                            | Operation::Coalesce
                    )
                });
            noncanonical |= complex;
            source = wrap(
                source,
                &s,
                clip,
                bounds,
                &format!("lm-{object_index}-{clip_index}"),
                p,
                preset,
            );
            source.push_str(&particles(clip, preset, p, bounds));
        }
        if has_mesh {
            let chosen = active.iter().rev().find(|(clip, preset, _)| {
                mesh::enabled(preset)
                    && (clip.objects.is_empty() || clip.objects.contains(&object.id))
            });
            let surface = if let Some((clip, preset, _)) = chosen {
                let rank = if clip.objects.is_empty() {
                    object_index
                } else {
                    clip.objects
                        .iter()
                        .position(|id| id == &object.id)
                        .unwrap_or(0)
                };
                let count = if clip.objects.is_empty() {
                    object_count
                } else {
                    clip.objects.len()
                };
                let p = clip.progress(frame, rank, count).unwrap_or(0.);
                mesh::surface(
                    source,
                    object_bounds(object),
                    clip,
                    preset,
                    p,
                    rank,
                    count,
                    fps,
                )
            } else {
                let bounds = object_bounds(object);
                let width = (bounds[2] - bounds[0]).max(1.);
                let height = (bounds[3] - bounds[1]).max(1.);
                mesh::Surface{size:[width,height],source:format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"{} {} {width} {height}\">{source}</svg>",bounds[0],bounds[1]),triangles:mesh::Mesh::plane(width,height).project([0.;3],[0.;3],[(bounds[0]+bounds[2])*0.5,(bounds[1]+bounds[3])*0.5],600.)}
            };
            source = mesh::svg(&surface, &format!("lm-mesh-{object_index}"));
            scene.surfaces.push(surface);
            noncanonical = true;
        }
        source_bytes = source_bytes.saturating_add(source.len());
        if source_bytes > SOURCE_LIMIT {
            // Retain the original SVG for oversized legacy files. Metadata is
            // preview-only and must not bypass that retained source.
            layer.vector_layer = false;
            return;
        }
        fragments.push(source);
    }
    if has_mesh {
        layer.motion_scene = Some(std::sync::Arc::new(scene));
    }
    if noncanonical {
        layer.source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{}</svg>",
            dimensions[0],
            dimensions[1],
            fragments.join("")
        );
        layer.vector_layer = false;
    } else {
        layer.source = vector_svg(dimensions[0], dimensions[1], &layer.vector_objects);
    }
}

fn glyph_source(
    object: &VectorObject,
    active: &[(&Clip, &Preset, f32)],
    frame: u32,
    fps: u32,
    dimensions: [u32; 2],
) -> String {
    let Some(text) = &object.text else {
        return vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(object));
    };
    let glyphs: Vec<_> = text.content.graphemes(true).collect();
    // Dense paragraphs retain the style-run path rather than multiplying their
    // complete shaped source quadratically. The editable artwork is untouched.
    if glyphs.len() > 128
        || text
            .content
            .len()
            .saturating_mul(glyphs.len())
            .saturating_mul(512)
            > 1024 * 1024
    {
        return vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(object));
    }
    let color = object
        .fill
        .map_or([32, 32, 32], |p| [p.color[0], p.color[1], p.color[2]]);
    let mut offsets = Vec::with_capacity(glyphs.len());
    let mut at = 0;
    for glyph in &glyphs {
        let end = at + glyph.encode_utf16().count();
        offsets.push((at, end));
        at = end;
    }
    let mut parts = String::new();
    for (i, &(start, end)) in offsets.iter().enumerate() {
        let mut glyph = object.clone();
        let target = glyph.text.as_mut().unwrap();
        target.runs.clear();
        for &(a, b) in &offsets {
            let mut style = text.style_at(a, color);
            if a != start {
                style.no_color = true;
            }
            target.runs.push(TextRun {
                start: a,
                end: b,
                style,
            });
        }
        let mut alpha = 1.;
        for (clip, preset, _) in active {
            if preset.category.starts_with("tx-")
                && (clip.objects.is_empty() || clip.objects.contains(&object.id))
            {
                let p = clip.progress(frame, i, glyphs.len()).unwrap_or(0.);
                alpha *= sample(preset, clip, p, i, glyphs.len(), fps).alpha;
            }
        }
        let source = vector_svg(dimensions[0], dimensions[1], std::slice::from_ref(&glyph));
        let _ = write!(parts, "<g opacity=\"{alpha}\">{}</g>", svg_inner(&source));
        let _ = end;
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\">{parts}</svg>",
        dimensions[0], dimensions[1]
    )
}
