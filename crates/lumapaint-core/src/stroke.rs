//! Portable stroke appearance. Geometry remains in Rust.
use serde::{Deserialize, Serialize};
use std::fmt::Write;
macro_rules! choices {
    ($name:ident, $default:ident, $($variant:ident),+) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        pub enum $name { #[default] $default, $($variant),+ }
    };
}
choices!(LineCap, Butt, Round, Square);
choices!(LineJoin, Miter, Round, Bevel);
choices!(StrokeAlignment, Center, Inside, Outside);
choices!(Arrowhead, None, Triangle, Open, Circle, Diamond, Square, Bar, Stealth);
choices!(
    WidthProfile,
    Uniform,
    TaperBoth,
    TaperStart,
    TaperEnd,
    Bulge,
    Custom
);
/// Normalized arc position, width multiplier, and Hermite slope (width / position).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WidthStop {
    pub position: f32,
    pub width: f32,
    pub slope: f32,
}
fn default_curve() -> Vec<WidthStop> {
    vec![
        WidthStop {
            position: 0.,
            width: 1.,
            slope: 0.,
        },
        WidthStop {
            position: 1.,
            width: 1.,
            slope: 0.,
        },
    ]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct StrokeStyle {
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
    pub alignment: StrokeAlignment,
    pub dash_array: Vec<f32>,
    pub dash_offset: f32,
    pub start_arrow: Arrowhead,
    pub end_arrow: Arrowhead,
    pub arrow_scale: f32,
    pub profile: WidthProfile,
    pub width_curve: Vec<WidthStop>,
    pub start_arrow_scale: Option<f32>,
    pub end_arrow_scale: Option<f32>,
    pub contour_alignments: Vec<StrokeAlignment>,
}
impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.,
            alignment: StrokeAlignment::Center,
            dash_array: vec![],
            dash_offset: 0.,
            start_arrow: Arrowhead::None,
            end_arrow: Arrowhead::None,
            arrow_scale: 1.,
            profile: WidthProfile::Uniform,
            width_curve: default_curve(),
            start_arrow_scale: None,
            end_arrow_scale: None,
            contour_alignments: vec![],
        }
    }
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StrokeStylePatch {
    pub cap: Option<LineCap>,
    pub join: Option<LineJoin>,
    pub miter_limit: Option<f32>,
    pub alignment: Option<StrokeAlignment>,
    pub dash_array: Option<Vec<f32>>,
    pub dash_offset: Option<f32>,
    pub start_arrow: Option<Arrowhead>,
    pub end_arrow: Option<Arrowhead>,
    pub arrow_scale: Option<f32>,
    pub profile: Option<WidthProfile>,
    pub width_curve: Option<Vec<WidthStop>>,
    pub start_arrow_scale: Option<f32>,
    pub end_arrow_scale: Option<f32>,
    pub contour_alignments: Option<Vec<StrokeAlignment>>,
}
impl StrokeStyle {
    pub fn validate(&self) -> Result<(), String> {
        if !self.miter_limit.is_finite()
            || !(1.0..=100.).contains(&self.miter_limit)
            || !self.arrow_scale.is_finite()
            || !(0.1..=10.).contains(&self.arrow_scale)
            || !self.dash_offset.is_finite()
            || self.dash_offset.abs() > 100_000.
            || self.dash_array.len() > 12
            || self
                .dash_array
                .iter()
                .any(|v| !v.is_finite() || !(0.1..=100_000.).contains(v))
        {
            return Err("Invalid stroke settings / 線の設定値が不正です / 描边设置无效".into());
        }
        if self.width_curve.len() < 2
            || self.width_curve.len() > 32
            || self.width_curve.first().unwrap().position != 0.
            || self.width_curve.last().unwrap().position != 1.
            || self
                .width_curve
                .windows(2)
                .any(|p| p[1].position - p[0].position < 0.001)
            || self.width_curve.iter().any(|p| {
                !p.position.is_finite()
                    || !p.width.is_finite()
                    || !(0.0..=4.).contains(&p.width)
                    || !p.slope.is_finite()
                    || p.slope.abs() > 20.
            })
            || self.contour_alignments.len() > 4096
            || [self.start_arrow_scale, self.end_arrow_scale]
                .into_iter()
                .flatten()
                .any(|v| !v.is_finite() || !(0.1..=10.).contains(&v))
        {
            return Err(
                "Invalid width curve / 幅カーブの設定値が不正です / 宽度曲线设置无效".into(),
            );
        }
        Ok(())
    }
    pub fn validate_geometry(&self, data: &str, width: f32) -> Result<(), String> {
        self.validate()?;
        if self.profile == WidthProfile::Uniform
            && self.start_arrow == Arrowhead::None
            && self.end_arrow == Arrowhead::None
            && self.alignment == StrokeAlignment::Center
            && self.contour_alignments.is_empty()
        {
            return Ok(());
        }
        let geometry = contours(data);
        if geometry.is_empty()
            || geometry
                .iter()
                .flat_map(|c| &c.points)
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
        {
            return Err("Stroke path is invalid or too complex / パスが不正か複雑すぎます / 描边路径无效或过于复杂".into());
        }
        if self.profile != WidthProfile::Uniform {
            let mut bytes = 0;
            for contour in &geometry {
                let outline = variable_outline(contour, f64::from(width), self)
                    .ok_or("Stroke profile exceeds the geometry budget / 線プロファイルの描画量が上限を超えました / 描边轮廓超出绘制预算")?;
                bytes += outline.len();
                if bytes > 3 * 1024 * 1024 {
                    return Err("Stroke profile exceeds the geometry budget / 線プロファイルの描画量が上限を超えました / 描边轮廓超出绘制预算".into());
                }
            }
        }
        Ok(())
    }
    pub fn apply(&mut self, patch: &StrokeStylePatch) {
        macro_rules! apply { ($($field:ident),+) => { $(if let Some(value) = &patch.$field { self.$field = value.clone(); })+ }; }
        apply!(
            cap,
            join,
            miter_limit,
            alignment,
            dash_array,
            dash_offset,
            start_arrow,
            end_arrow,
            arrow_scale,
            profile,
            width_curve,
            contour_alignments
        );
        if patch.alignment.is_some() && patch.contour_alignments.is_none() {
            self.contour_alignments.clear();
        }
        if patch.arrow_scale.is_some() {
            self.start_arrow_scale = None;
            self.end_arrow_scale = None;
        }
        if let Some(v) = patch.start_arrow_scale {
            self.start_arrow_scale = Some(v);
        }
        if let Some(v) = patch.end_arrow_scale {
            self.end_arrow_scale = Some(v);
        }
    }
    pub fn attributes(&self) -> String {
        let cap = match self.cap {
            LineCap::Butt => "butt",
            LineCap::Round => "round",
            LineCap::Square => "square",
        };
        let join = match self.join {
            LineJoin::Miter => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        };
        let dash = self
            .dash_array
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            r#"stroke-linecap="{cap}" stroke-linejoin="{join}" stroke-miterlimit="{}" stroke-dasharray="{}" stroke-dashoffset="{}""#,
            self.miter_limit,
            if dash.is_empty() { "none" } else { &dash },
            self.dash_offset
        )
    }
}
type Point = [f64; 2];
#[derive(Clone, Default)]
struct Contour {
    points: Vec<Point>,
    closed: bool,
}
fn distance(a: Point, b: Point) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}
fn midpoint(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn cubic(out: &mut Vec<Point>, p: [Point; 4], depth: u8) {
    if out.len() >= 65_536 {
        out.push(p[3]);
        return;
    }
    if depth >= 12
        || distance(p[0], p[1]) + distance(p[1], p[2]) + distance(p[2], p[3]) - distance(p[0], p[3])
            < 0.02
    {
        out.push(p[3]);
        return;
    }
    let a = midpoint(p[0], p[1]);
    let b = midpoint(p[1], p[2]);
    let c = midpoint(p[2], p[3]);
    let d = midpoint(a, b);
    let e = midpoint(b, c);
    let m = midpoint(d, e);
    cubic(out, [p[0], a, d, m], depth + 1);
    cubic(out, [m, e, c, p[3]], depth + 1);
}
fn contours(data: &str) -> Vec<Contour> {
    use svgtypes::SimplePathSegment::*;
    let mut result = Vec::new();
    let mut current = Contour::default();
    let mut completed_points = 0;
    let mut at = [0., 0.];
    for segment in svgtypes::SimplifyingPathParser::from(data) {
        let Ok(segment) = segment else {
            return vec![];
        };
        match segment {
            MoveTo { x, y } => {
                if !current.points.is_empty() {
                    completed_points += current.points.len();
                    result.push(current);
                }
                current = Contour {
                    points: vec![[x, y]],
                    closed: false,
                };
                at = [x, y];
            }
            LineTo { x, y } => {
                current.points.push([x, y]);
                at = [x, y];
            }
            CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                cubic(&mut current.points, [at, [x1, y1], [x2, y2], [x, y]], 0);
                at = [x, y];
            }
            Quadratic { x1, y1, x, y } => {
                let q = [x1, y1];
                let end = [x, y];
                cubic(
                    &mut current.points,
                    [
                        at,
                        [(at[0] + 2. * q[0]) / 3., (at[1] + 2. * q[1]) / 3.],
                        [(end[0] + 2. * q[0]) / 3., (end[1] + 2. * q[1]) / 3.],
                        end,
                    ],
                    0,
                );
                at = end;
            }
            ClosePath => {
                current.closed = true;
                if let Some(&first) = current.points.first() {
                    current.points.push(first);
                    at = first;
                }
            }
        }
        if completed_points + current.points.len() > 65_536 || result.len() > 4096 {
            return vec![];
        }
    }
    if !current.points.is_empty() {
        result.push(current);
    }
    for contour in &mut result {
        contour.points.dedup_by(|a, b| distance(*a, *b) < 1e-8);
    }
    result
}
fn polygon(out: &mut String, points: &[Point]) {
    if points.len() < 3 {
        return;
    }
    let area: f64 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum();
    if area.abs() < 1e-16 {
        return;
    }
    let ordered: Vec<_> = if area < 0. {
        points.iter().rev().collect()
    } else {
        points.iter().collect()
    };
    let _ = write!(out, "M{:.5} {:.5}", ordered[0][0], ordered[0][1]);
    for p in &ordered[1..] {
        let _ = write!(out, "L{:.5} {:.5}", p[0], p[1]);
    }
    out.push('Z');
}
fn circle(out: &mut String, p: Point, r: f64) {
    if r <= 0. {
        return;
    }
    let [x, y] = p;
    let _ = write!(
        out,
        "M{} {}a{r} {r} 0 1 1 {} 0a{r} {r} 0 1 1 {} 0Z",
        x - r,
        y,
        2. * r,
        -2. * r
    );
}
fn factor(style: &StrokeStyle, t: f64) -> f64 {
    match style.profile {
        WidthProfile::Custom => {
            let t = t.clamp(0., 1.);
            let pair = style
                .width_curve
                .windows(2)
                .find(|p| t <= f64::from(p[1].position))
                .unwrap_or(&style.width_curve[style.width_curve.len() - 2..]);
            let span = f64::from(pair[1].position - pair[0].position);
            let u = (t - f64::from(pair[0].position)) / span;
            ((2. * u.powi(3) - 3. * u * u + 1.) * f64::from(pair[0].width)
                + (u.powi(3) - 2. * u * u + u) * span * f64::from(pair[0].slope)
                + (-2. * u.powi(3) + 3. * u * u) * f64::from(pair[1].width)
                + (u.powi(3) - u * u) * span * f64::from(pair[1].slope))
            .clamp(0., 4.)
        }
        WidthProfile::Uniform => 1.,
        WidthProfile::TaperBoth => (std::f64::consts::PI * t).sin().max(0.),
        WidthProfile::TaperStart => t,
        WidthProfile::TaperEnd => 1. - t,
        WidthProfile::Bulge => 0.3 + 0.7 * (std::f64::consts::PI * t).sin(),
    }
}
#[derive(Clone, Copy)]
struct Sample {
    p: Point,
    r: f64,
}
fn normal(a: Point, b: Point) -> Point {
    let len = distance(a, b);
    if len < 1e-9 {
        [0., 0.]
    } else {
        [-(b[1] - a[1]) / len, (b[0] - a[0]) / len]
    }
}
fn offset(p: Point, n: Point, r: f64) -> Point {
    [p[0] + n[0] * r, p[1] + n[1] * r]
}
fn ribbon(out: &mut String, points: &[Sample], style: &StrokeStyle, closed: bool) {
    if points.len() < 2 {
        return;
    }
    for (i, pair) in points.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let n = normal(a.p, b.p);
        let tangent = [n[1], -n[0]];
        let mut ap = a.p;
        let mut bp = b.p;
        if !closed && style.cap == LineCap::Square {
            if i == 0 {
                ap = offset(ap, tangent, -a.r);
            }
            if i == points.len() - 2 {
                bp = offset(bp, tangent, b.r);
            }
        }
        polygon(
            out,
            &[
                offset(ap, n, a.r),
                offset(bp, n, b.r),
                offset(bp, n, -b.r),
                offset(ap, n, -a.r),
            ],
        );
    }
    let count = points.len() - 1;
    for i in 0..points.len() {
        if (i == 0 || i == count) && !closed {
            if style.cap == LineCap::Round {
                circle(out, points[i].p, points[i].r);
            }
            continue;
        }
        if closed && i == count {
            continue;
        }
        let p = points[i];
        let prev = points[if i == 0 { count - 1 } else { i - 1 }].p;
        let next = points[i + 1].p;
        let n1 = normal(prev, p.p);
        let n2 = normal(p.p, next);
        if style.join == LineJoin::Round {
            circle(out, p.p, p.r);
            continue;
        }
        for sign in [-1., 1.] {
            let a = offset(p.p, n1, p.r * sign);
            let b = offset(p.p, n2, p.r * sign);
            let denom = 1. + n1[0] * n2[0] + n1[1] * n2[1];
            if style.join == LineJoin::Miter && denom > 1e-8 {
                let m = offset(p.p, [n1[0] + n2[0], n1[1] + n2[1]], p.r * sign / denom);
                if distance(m, p.p) <= p.r * f64::from(style.miter_limit) {
                    polygon(out, &[p.p, a, m, b]);
                    continue;
                }
            }
            polygon(out, &[p.p, a, b]);
        }
    }
}
fn variable_outline(contour: &Contour, width: f64, style: &StrokeStyle) -> Option<String> {
    let mut out = String::new();
    let length: f64 = contour
        .points
        .windows(2)
        .map(|p| distance(p[0], p[1]))
        .sum();
    if length <= 1e-8 {
        return Some(out);
    }
    let mut dashes: Vec<f64> = style.dash_array.iter().copied().map(f64::from).collect();
    if dashes.len() % 2 == 1 {
        dashes.extend(dashes.clone());
    }
    let mut index = 0;
    let mut remain = f64::INFINITY;
    if !dashes.is_empty() {
        let cycle: f64 = dashes.iter().sum();
        let mut phase = f64::from(style.dash_offset).rem_euclid(cycle);
        while phase >= dashes[index] {
            phase -= dashes[index];
            index = (index + 1) % dashes.len();
        }
        remain = dashes[index] - phase;
    }
    let curve_step = if style.profile == WidthProfile::Custom {
        length
            * style
                .width_curve
                .windows(2)
                .map(|p| f64::from(p[1].position - p[0].position))
                .fold(1., f64::min)
            / 8.
    } else {
        f64::INFINITY
    };
    let mut arc = 0.;
    let mut run = Vec::new();
    let mut runs = Vec::new();
    let mut steps = 0;
    for pair in contour.points.windows(2) {
        let len = distance(pair[0], pair[1]);
        if len < 1e-8 {
            continue;
        }
        let mut used = 0.;
        while used < len - 1e-8 {
            steps += 1;
            if steps > 65_536 {
                return None;
            }
            let take = (len - used)
                .min(remain)
                .min(if style.profile == WidthProfile::Uniform {
                    f64::INFINITY
                } else {
                    (length / 128.).max(0.5)
                })
                .min(curve_step);
            let point = |v: f64| {
                [
                    pair[0][0] + (pair[1][0] - pair[0][0]) * v / len,
                    pair[0][1] + (pair[1][1] - pair[0][1]) * v / len,
                ]
            };
            let sample = |v: f64| Sample {
                p: point(v),
                r: width * 0.5 * factor(style, (arc + v) / length),
            };
            if index % 2 == 0 {
                if run.is_empty() {
                    run.push(sample(used));
                }
                run.push(sample(used + take));
            }
            used += take;
            remain -= take;
            if remain <= 1e-8 {
                if !run.is_empty() {
                    runs.push(std::mem::take(&mut run));
                }
                index = (index + 1) % dashes.len();
                remain = dashes[index];
            }
        }
        arc += len;
    }
    if !run.is_empty() {
        runs.push(run);
    }
    // A dash crossing the closing anchor has a join there, rather than two caps.
    if contour.closed && runs.len() > 1 {
        let first = runs[0][0].p;
        let last = runs.last().unwrap().last().unwrap().p;
        if distance(first, last) < 1e-8 {
            let first_run = runs.remove(0);
            runs.last_mut()
                .unwrap()
                .extend(first_run.into_iter().skip(1));
        }
    }
    for run in runs {
        let closed = contour.closed && distance(run[0].p, run.last().unwrap().p) < 1e-8;
        ribbon(&mut out, &run, style, closed);
    }
    Some(out)
}
fn arrow_path(contour: &Contour, start: bool, kind: Arrowhead, width: f64, scale: f64) -> String {
    if kind == Arrowhead::None || contour.closed || contour.points.len() < 2 {
        return String::new();
    }
    let points = &contour.points;
    let (tip, other) = if start {
        (points[0], points[1])
    } else {
        (points[points.len() - 1], points[points.len() - 2])
    };
    let n = normal(other, tip);
    let size = width * scale;
    let map = |p: Point| {
        [
            tip[0] + (n[1] * p[0] + n[0] * p[1]) * size,
            tip[1] + (-n[0] * p[0] + n[1] * p[1]) * size,
        ]
    };
    let mut out = String::new();
    let shape: &[Point] = match kind {
        Arrowhead::Triangle => &[[0., 0.], [-4., -2.], [-4., 2.]],
        Arrowhead::Diamond => &[[0., 0.], [-2., -1.5], [-4., 0.], [-2., 1.5]],
        Arrowhead::Square => &[[-1.5, -1.5], [1.5, -1.5], [1.5, 1.5], [-1.5, 1.5]],
        Arrowhead::Bar => &[[-0.5, -2.], [0.5, -2.], [0.5, 2.], [-0.5, 2.]],
        Arrowhead::Stealth => &[[0., 0.], [-4., -2.], [-3., 0.], [-4., 2.]],
        _ => &[],
    };
    match kind {
        Arrowhead::Circle => circle(&mut out, tip, size * 1.5),
        Arrowhead::Open => {
            let samples: Vec<_> = [[-4., -2.], [0., 0.], [-4., 2.]]
                .into_iter()
                .map(|p| Sample {
                    p: map(p),
                    r: size * 0.5,
                })
                .collect();
            ribbon(
                &mut out,
                &samples,
                &StrokeStyle {
                    join: LineJoin::Round,
                    ..Default::default()
                },
                false,
            );
        }
        _ => polygon(
            &mut out,
            &shape.iter().copied().map(map).collect::<Vec<_>>(),
        ),
    }
    out
}
fn arrow(
    out: &mut String,
    contour: &Contour,
    start: bool,
    kind: Arrowhead,
    width: f64,
    scale: f64,
    color: &str,
) {
    let path = arrow_path(contour, start, kind, width, scale);
    if !path.is_empty() {
        let _ = write!(out, r#"<path d="{path}" fill="{color}"/>"#);
    }
}
pub fn contour_closed(data: &str) -> Vec<bool> {
    contours(data).iter().map(|c| c.closed).collect()
}
fn contour_data(contour: &Contour) -> String {
    let mut out = String::new();
    for (i, p) in contour.points.iter().enumerate() {
        let _ = write!(out, "{}{} {}", if i == 0 { "M" } else { "L" }, p[0], p[1]);
    }
    if contour.closed {
        out.push('Z');
    }
    out
}
/// Alignment clips only the stroke, never the fill. Data is XML-escaped by the caller.
pub fn svg_stroke(
    data: &str,
    raw_data: &str,
    rule: &str,
    width: f32,
    style: &StrokeStyle,
    color: &str,
    id: usize,
) -> String {
    svg_stroke_scoped(data, raw_data, rule, width, style, color, &id.to_string())
}
fn svg_stroke_scoped(
    data: &str,
    raw_data: &str,
    rule: &str,
    width: f32,
    style: &StrokeStyle,
    color: &str,
    id: &str,
) -> String {
    if style.validate().is_err() {
        return String::new();
    }
    if width <= 0. || color == "none" {
        return String::new();
    }
    let geometry = if style.alignment != StrokeAlignment::Center
        || !style.contour_alignments.is_empty()
        || style.profile != WidthProfile::Uniform
        || style.start_arrow != Arrowhead::None
        || style.end_arrow != Arrowhead::None
    {
        contours(raw_data)
    } else {
        vec![]
    };
    if geometry.len() > 1
        && (!style.contour_alignments.is_empty() || style.alignment != StrokeAlignment::Center)
    {
        let exact: Vec<String> = crate::bezier::cubic_contours(raw_data)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(points, closed)| crate::bezier::path_data(&points, closed).ok())
            .collect();
        let parts: Vec<String> = geometry
            .iter()
            .enumerate()
            .map(|(i, c)| exact.get(i).cloned().unwrap_or_else(|| contour_data(c)))
            .collect();
        let domain: String = geometry
            .iter()
            .enumerate()
            .filter(|(_, c)| c.closed)
            .map(|(i, _)| parts[i].clone())
            .collect();
        let mut out = String::new();
        for (index, contour) in geometry.iter().enumerate() {
            let part = parts[index].clone();
            let mut local = style.clone();
            local.alignment = if contour.closed {
                style
                    .contour_alignments
                    .get(index)
                    .copied()
                    .unwrap_or(style.alignment)
            } else {
                StrokeAlignment::Center
            };
            local.contour_alignments.clear();
            let svg = svg_stroke_scoped(
                &part,
                &part,
                rule,
                width,
                &local,
                color,
                &format!("{id}-contour-{index}"),
            );
            // Clip against the complete closed fill domain, preserving compound holes.
            out.push_str(
                &svg.replace(
                    &format!(r#"d="{part}" clip-rule"#),
                    &format!(r#"d="{domain}" clip-rule"#),
                )
                .replace(
                    &format!(r#"d="{part}" fill="black""#),
                    &format!(r#"d="{domain}" fill="black""#),
                ),
            );
        }
        return out;
    }
    let closed = !geometry.is_empty() && geometry.iter().all(|contour| contour.closed);
    let alignment = if closed {
        style
            .contour_alignments
            .first()
            .copied()
            .unwrap_or(style.alignment)
    } else {
        StrokeAlignment::Center
    };
    let weight = if alignment == StrokeAlignment::Center {
        width
    } else {
        width * 2.
    };
    let mut out = String::new();
    let extent = f64::from(weight)
        * f64::from(style.miter_limit.max(2.))
        * if style.profile == WidthProfile::Custom {
            4.
        } else {
            1.
        };
    let points: Vec<_> = geometry
        .iter()
        .flat_map(|contour| &contour.points)
        .collect();
    let left = points.iter().map(|p| p[0]).fold(0., f64::min) - extent;
    let top = points.iter().map(|p| p[1]).fold(0., f64::min) - extent;
    let mask_width = points.iter().map(|p| p[0]).fold(0., f64::max) + extent - left;
    let mask_height = points.iter().map(|p| p[1]).fold(0., f64::max) + extent - top;
    match alignment {
        StrokeAlignment::Inside => {
            let _ = write!(
                out,
                r#"<defs><clipPath id="stroke-in-{id}"><path d="{data}" clip-rule="{rule}"/></clipPath></defs><g clip-path="url(#stroke-in-{id})">"#
            );
        }
        StrokeAlignment::Outside => {
            let _ = write!(
                out,
                r#"<defs><mask id="stroke-out-{id}" maskUnits="userSpaceOnUse" x="{left}" y="{top}" width="{mask_width}" height="{mask_height}"><rect x="{left}" y="{top}" width="{mask_width}" height="{mask_height}" fill="white"/><path d="{data}" fill="black" fill-rule="{rule}"/></mask></defs><g mask="url(#stroke-out-{id})">"#
            );
        }
        StrokeAlignment::Center => {}
    }
    if style.profile == WidthProfile::Uniform {
        let _ = write!(
            out,
            r#"<path d="{data}" fill="none" stroke="{color}" stroke-width="{weight}" {}/>"#,
            style.attributes()
        );
    } else {
        let mut outline = String::new();
        for contour in &geometry {
            if let Some(part) = variable_outline(contour, f64::from(weight), style) {
                outline.push_str(&part);
            }
        }
        let _ = write!(
            out,
            r#"<path d="{outline}" fill="{color}" fill-rule="nonzero"/>"#
        );
    }
    if alignment != StrokeAlignment::Center {
        out.push_str("</g>");
    }
    for contour in &geometry {
        arrow(
            &mut out,
            contour,
            true,
            style.start_arrow,
            f64::from(width),
            f64::from(style.start_arrow_scale.unwrap_or(style.arrow_scale)),
            color,
        );
        arrow(
            &mut out,
            contour,
            false,
            style.end_arrow,
            f64::from(width),
            f64::from(style.end_arrow_scale.unwrap_or(style.arrow_scale)),
            color,
        );
    }
    out
}

/// Drawn stroke geometry shared by picking, marquee selection and selection bounds.
/// Coordinates are transformed before measuring pointer tolerance.
pub struct StrokeGeometry {
    regions: Vec<(Vec<Contour>, StrokeAlignment)>,
    domain: Vec<Contour>,
    even_odd: bool,
    pub edges: Vec<[Point; 2]>,
}
fn winding(contours: &[Contour], p: Point, even_odd: bool) -> bool {
    let mut count = 0i64;
    for c in contours {
        for (a, b) in c
            .points
            .iter()
            .zip(c.points.iter().cycle().skip(1))
            .take(c.points.len())
        {
            let side = (b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1]);
            if a[1] <= p[1] && b[1] > p[1] && side > 0. {
                count += 1;
            }
            if a[1] > p[1] && b[1] <= p[1] && side < 0. {
                count -= 1;
            }
        }
    }
    if even_odd {
        count % 2 != 0
    } else {
        count != 0
    }
}
fn edge_distance(p: Point, edge: [Point; 2]) -> f64 {
    let [a, b] = edge;
    let d = [b[0] - a[0], b[1] - a[1]];
    let length = d[0] * d[0] + d[1] * d[1];
    let t = if length == 0. {
        0.
    } else {
        ((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length
    }
    .clamp(0., 1.);
    distance(p, [a[0] + d[0] * t, a[1] + d[1] * t])
}
fn crossing(a: [Point; 2], b: [Point; 2]) -> Option<f64> {
    let r = [a[1][0] - a[0][0], a[1][1] - a[0][1]];
    let s = [b[1][0] - b[0][0], b[1][1] - b[0][1]];
    let d = r[0] * s[1] - r[1] * s[0];
    if d.abs() < 1e-12 {
        return None;
    }
    let q = [b[0][0] - a[0][0], b[0][1] - a[0][1]];
    let t = (q[0] * s[1] - q[1] * s[0]) / d;
    let u = (q[0] * r[1] - q[1] * r[0]) / d;
    ((0.0..=1.).contains(&t) && (0.0..=1.).contains(&u)).then_some(t)
}
fn edges(contours: &[Contour]) -> Vec<[Point; 2]> {
    contours
        .iter()
        .flat_map(|c| c.points.windows(2).map(|p| [p[0], p[1]]))
        .collect()
}
impl StrokeGeometry {
    fn accepted(&self, p: Point, alignment: StrokeAlignment) -> bool {
        match alignment {
            StrokeAlignment::Center => true,
            StrokeAlignment::Inside => winding(&self.domain, p, self.even_odd),
            StrokeAlignment::Outside => !winding(&self.domain, p, self.even_odd),
        }
    }
    pub fn contains(&self, p: [f32; 2], tolerance: f32) -> bool {
        let p = p.map(f64::from);
        self.regions
            .iter()
            .any(|(c, a)| self.accepted(p, *a) && winding(c, p, false))
            || self
                .edges
                .iter()
                .any(|e| edge_distance(p, *e) <= f64::from(tolerance))
    }
    pub fn intersects(&self, bounds: [f32; 4], ellipse: bool) -> bool {
        let [x, y, w, h] = bounds.map(f64::from);
        let normalized = |p: Point| {
            [
                (p[0] - x - w / 2.) / (w / 2.),
                (p[1] - y - h / 2.) / (h / 2.),
            ]
        };
        let inside = |p: Point| {
            if ellipse {
                let q = normalized(p);
                q[0] * q[0] + q[1] * q[1] <= 1.
            } else {
                p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h
            }
        };
        let box_edges = [
            [[x, y], [x + w, y]],
            [[x + w, y], [x + w, y + h]],
            [[x + w, y + h], [x, y + h]],
            [[x, y + h], [x, y]],
        ];
        self.contains([(x + w / 2.) as f32, (y + h / 2.) as f32], 0.)
            || self.edges.iter().any(|e| {
                inside(e[0])
                    || inside(e[1])
                    || if ellipse {
                        edge_distance([0., 0.], [normalized(e[0]), normalized(e[1])]) <= 1.
                    } else {
                        box_edges.iter().any(|b| crossing(*e, *b).is_some())
                    }
            })
    }
}
pub fn fill_geometry(data: &str, transform: [f32; 6], even_odd: bool) -> StrokeGeometry {
    let mut source = contours(data);
    let [a, b, c, d, e, f] = transform.map(f64::from);
    for contour in &mut source {
        for p in &mut contour.points {
            *p = [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f];
        }
        if let Some(first) = contour.points.first().copied() {
            if contour.points.last() != Some(&first) {
                contour.points.push(first);
            }
        }
    }
    let boundary = edges(&source);
    // Fill membership uses its own winding rule, including open contours implicitly closed by SVG.
    StrokeGeometry {
        regions: vec![(source.clone(), StrokeAlignment::Inside)],
        domain: source,
        even_odd,
        edges: boundary,
    }
}
pub fn selection_geometry(
    data: &str,
    width: f32,
    style: &StrokeStyle,
    transform: [f32; 6],
    even_odd: bool,
) -> StrokeGeometry {
    let source = contours(data);
    let [a, b, c, d, e, f] = transform.map(f64::from);
    let map = |p: Point| [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f];
    let mapped = |mut cs: Vec<Contour>| {
        for contour in &mut cs {
            for p in &mut contour.points {
                *p = map(*p);
            }
        }
        cs
    };
    let domain = mapped(source.iter().filter(|c| c.closed).cloned().collect());
    let mut result = StrokeGeometry {
        regions: vec![],
        domain,
        even_odd,
        edges: vec![],
    };
    for (i, contour) in source.iter().enumerate() {
        let alignment = if contour.closed {
            style
                .contour_alignments
                .get(i)
                .copied()
                .unwrap_or(style.alignment)
        } else {
            StrokeAlignment::Center
        };
        let weight = if alignment == StrokeAlignment::Center {
            width
        } else {
            width * 2.
        };
        if let Some(path) = variable_outline(contour, f64::from(weight), style) {
            result.regions.push((mapped(contours(&path)), alignment));
        }
        for (start, kind, scale) in [
            (true, style.start_arrow, style.start_arrow_scale),
            (false, style.end_arrow, style.end_arrow_scale),
        ] {
            result.regions.push((
                mapped(contours(&arrow_path(
                    contour,
                    start,
                    kind,
                    f64::from(width),
                    f64::from(scale.unwrap_or(style.arrow_scale)),
                ))),
                StrokeAlignment::Center,
            ));
        }
    }
    let domain_edges = edges(&result.domain);
    for (polygons, alignment) in &result.regions {
        let region_edges = edges(polygons);
        if *alignment == StrokeAlignment::Center {
            result.edges.extend(region_edges);
            continue;
        }
        // Split at clipping boundaries. Include the visible portion of both outlines.
        for (candidates, cutters, is_domain) in [
            (&region_edges, &domain_edges, false),
            (&domain_edges, &region_edges, true),
        ] {
            for edge in candidates {
                let mut stops = vec![0., 1.];
                stops.extend(cutters.iter().filter_map(|other| crossing(*edge, *other)));
                stops.sort_by(f64::total_cmp);
                stops.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
                let at = |t: f64| {
                    [
                        edge[0][0] + (edge[1][0] - edge[0][0]) * t,
                        edge[0][1] + (edge[1][1] - edge[0][1]) * t,
                    ]
                };
                for pair in stops.windows(2) {
                    let p = at((pair[0] + pair[1]) * 0.5);
                    if if is_domain {
                        winding(polygons, p, false)
                    } else {
                        result.accepted(p, *alignment)
                    } {
                        result.edges.push([at(pair[0]), at(pair[1])]);
                    }
                }
            }
        }
    }
    result
}

/// Untransformed path bounds for object-local appearance coordinates.
pub fn path_bounds(data: &str) -> Option<[f32; 4]> {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in contours(data).into_iter().flat_map(|c| c.points) {
        b[0] = b[0].min(p[0]);
        b[1] = b[1].min(p[1]);
        b[2] = b[2].max(p[0]);
        b[3] = b[3].max(p[1]);
    }
    b.iter().all(|v| v.is_finite()).then(|| b.map(|v| v as f32))
}

/// Flattened portable centerline edges, including closed contours.
pub fn path_edges(data: &str, transform: [f32; 6]) -> Vec<[[f32; 2]; 2]> {
    let [a, b, c, d, e, f] = transform;
    let map = |p: Point| {
        [
            a * p[0] as f32 + c * p[1] as f32 + e,
            b * p[0] as f32 + d * p[1] as f32 + f,
        ]
    };
    contours(data)
        .into_iter()
        .flat_map(|contour| {
            let mut edges: Vec<_> = contour
                .points
                .windows(2)
                .map(|p| [map(p[0]), map(p[1])])
                .collect();
            if contour.closed && contour.points.len() > 1 {
                edges.push([map(*contour.points.last().unwrap()), map(contour.points[0])]);
            }
            edges
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_curve_validation_interpolation_and_legacy_defaults() {
        let mut style = StrokeStyle {
            profile: WidthProfile::Custom,
            width_curve: vec![
                WidthStop {
                    position: 0.,
                    width: 0.,
                    slope: 2.,
                },
                WidthStop {
                    position: 1.,
                    width: 2.,
                    slope: 2.,
                },
            ],
            ..Default::default()
        };
        style.validate().unwrap();
        assert!((factor(&style, 0.25) - 0.5).abs() < 1e-6);
        style.width_curve[0].slope = 20.;
        assert!((0.0..=4.).contains(&factor(&style, 0.4)));
        for value in [f32::NAN, 21.] {
            style.width_curve[0].slope = value;
            assert!(style.validate().is_err());
        }
        style.width_curve = default_curve();
        style.width_curve[1].position = 0.;
        assert!(style.validate().is_err());
        let legacy: StrokeStyle = serde_json::from_str(r#"{"arrowScale":2}"#).unwrap();
        assert_eq!(legacy.start_arrow_scale, None);
        assert_eq!(legacy.width_curve, default_curve());
    }
    #[test]
    fn arrow_extents_independent_scales_transforms_and_marquee() {
        let style = StrokeStyle {
            start_arrow: Arrowhead::Square,
            end_arrow: Arrowhead::Diamond,
            start_arrow_scale: Some(0.5),
            end_arrow_scale: Some(2.),
            ..Default::default()
        };
        let geometry =
            selection_geometry("M30 50H130", 10., &style, [1., 0., 0., 1., 0., 0.], false);
        assert!(geometry.contains([90., 75.], 0.));
        assert!(!geometry.contains([30., 65.], 0.));
        assert!(geometry.intersects([88., 73., 4., 4.], false));
        assert!(geometry.intersects([88., 73., 4., 4.], true));
        assert!(!geometry.intersects([130., 75., 4., 4.], false));
        let rotated = selection_geometry(
            "M30 50H130",
            10.,
            &style,
            [0., 2., -2., 0., 300., 0.],
            false,
        );
        assert!(rotated.contains([150., 180.], 0.));
        for shape in [
            Arrowhead::Triangle,
            Arrowhead::Open,
            Arrowhead::Circle,
            Arrowhead::Diamond,
            Arrowhead::Square,
            Arrowhead::Bar,
            Arrowhead::Stealth,
        ] {
            let s = StrokeStyle {
                end_arrow: shape,
                ..Default::default()
            };
            assert!(
                selection_geometry("M30 50H130", 10., &s, [1., 0., 0., 1., 0., 0.], false)
                    .contains([129., 50.], 0.),
                "{shape:?}"
            );
        }
    }
    #[test]
    fn mixed_contours_clip_separately_and_keep_compound_holes() {
        let path = "M20 20H100V80H20Z M40 40H80V60H40Z M120 20V80";
        let style = StrokeStyle {
            alignment: StrokeAlignment::Inside,
            contour_alignments: vec![
                StrokeAlignment::Inside,
                StrokeAlignment::Outside,
                StrokeAlignment::Outside,
            ],
            ..Default::default()
        };
        let geometry = selection_geometry(path, 10., &style, [1., 0., 0., 1., 0., 0.], true);
        assert!(geometry.contains([50., 22.], 0.));
        assert!(!geometry.contains([50., 18.], 0.));
        assert!(geometry.contains([50., 42.], 0.));
        assert!(!geometry.contains([50., 38.], 0.));
        assert!(geometry.contains([123., 50.], 0.));
        assert!(!geometry.contains([127., 50.], 0.));
        let svg = svg_stroke(path, path, "evenodd", 10., &style, "red", 0);
        assert!(svg.contains("clip-path"));
        assert!(svg.contains("mask="));
        assert_eq!(contour_closed(path), vec![true, true, false]);
        assert!(svg.contains("stroke-in-0-contour-0"));
        let neighbor = svg_stroke(
            "M0 0H10V10H0Z",
            "M0 0H10V10H0Z",
            "nonzero",
            1.,
            &StrokeStyle {
                alignment: StrokeAlignment::Inside,
                ..Default::default()
            },
            "red",
            1,
        );
        assert!(neighbor.contains("stroke-in-1"));
        assert!(!svg.contains(r#"id="stroke-in-1""#));
    }
    #[test]
    fn custom_width_and_dash_gaps_are_used_for_picking() {
        let style = StrokeStyle {
            profile: WidthProfile::Custom,
            width_curve: vec![
                WidthStop {
                    position: 0.,
                    width: 0.,
                    slope: 0.,
                },
                WidthStop {
                    position: 0.5,
                    width: 3.,
                    slope: 0.,
                },
                WidthStop {
                    position: 1.,
                    width: 0.,
                    slope: 0.,
                },
            ],
            ..Default::default()
        };
        let geometry =
            selection_geometry("M10 50H150", 10., &style, [1., 0., 0., 1., 0., 0.], false);
        assert!(geometry.contains([80., 63.], 0.));
        assert!(!geometry.contains([12., 55.], 0.));
        let zero = StrokeStyle {
            profile: WidthProfile::Custom,
            width_curve: vec![
                WidthStop {
                    position: 0.,
                    width: 0.,
                    slope: 0.,
                },
                WidthStop {
                    position: 1.,
                    width: 0.,
                    slope: 0.,
                },
            ],
            ..Default::default()
        };
        assert!(
            !selection_geometry("M10 50H150", 10., &zero, [1., 0., 0., 1., 0., 0.], false)
                .contains([80., 50.], 0.)
        );
        let dashed = StrokeStyle {
            dash_array: vec![10., 10.],
            ..Default::default()
        };
        let geometry =
            selection_geometry("M10 50H150", 10., &dashed, [1., 0., 0., 1., 0., 0.], false);
        assert!(geometry.contains([15., 50.], 0.));
        assert!(!geometry.contains([25., 50.], 0.));
    }
    #[test]
    fn rejects_invalid_styles_unknown_fields_and_excessive_geometry() {
        for dash_array in [vec![0.], vec![-1.], vec![f32::NAN], vec![1.; 13]] {
            assert!(StrokeStyle {
                dash_array,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        for dash_offset in [f32::NAN, f32::INFINITY, 100001.] {
            assert!(StrokeStyle {
                dash_offset,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(serde_json::from_str::<StrokeStyle>(r#"{"cap":"invalid"}"#).is_err());
        assert!(serde_json::from_str::<StrokeStylePatch>(r#"{"unexpected":1}"#).is_err());
        assert_eq!(
            serde_json::from_str::<StrokeStyle>("{}").unwrap(),
            StrokeStyle::default()
        );
        let style = StrokeStyle {
            profile: WidthProfile::TaperBoth,
            dash_array: vec![0.1, 0.1],
            ..Default::default()
        };
        assert!(style.validate_geometry("M0 0H100000", 10.).is_err());
        assert!(style.validate_geometry("not a path", 10.).is_err());
        let gap_only = StrokeStyle {
            profile: WidthProfile::Bulge,
            dash_array: vec![10., 100.],
            dash_offset: 20.,
            ..Default::default()
        };
        gap_only.validate_geometry("M0 0H5", 10.).unwrap();
        let style = StrokeStyle {
            profile: WidthProfile::Bulge,
            ..Default::default()
        };
        style
            .validate_geometry("M10 10q20 -10 40 0t40 0a20 20 0 0 1 20 20Z", 10.)
            .unwrap();
    }
}
