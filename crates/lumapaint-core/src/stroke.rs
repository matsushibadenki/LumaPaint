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
choices!(Arrowhead, None, Triangle, Open, Circle);
choices!(
    WidthProfile,
    Uniform,
    TaperBoth,
    TaperStart,
    TaperEnd,
    Bulge
);
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
        Ok(())
    }
    pub fn validate_geometry(&self, data: &str, width: f32) -> Result<(), String> {
        self.validate()?;
        if self.profile == WidthProfile::Uniform
            && self.start_arrow == Arrowhead::None
            && self.end_arrow == Arrowhead::None
            && self.alignment == StrokeAlignment::Center
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
            profile
        );
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
#[derive(Default)]
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
fn factor(profile: WidthProfile, t: f64) -> f64 {
    match profile {
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
            let take = (len - used).min(remain).min((length / 128.).max(0.5));
            let point = |v: f64| {
                [
                    pair[0][0] + (pair[1][0] - pair[0][0]) * v / len,
                    pair[0][1] + (pair[1][1] - pair[0][1]) * v / len,
                ]
            };
            let sample = |v: f64| Sample {
                p: point(v),
                r: width * 0.5 * factor(style.profile, (arc + v) / length),
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
fn arrow(
    out: &mut String,
    contour: &Contour,
    start: bool,
    kind: Arrowhead,
    width: f64,
    scale: f64,
    color: &str,
) {
    if kind == Arrowhead::None || contour.closed || contour.points.len() < 2 {
        return;
    }
    let points = &contour.points;
    let (tip, other) = if start {
        (points[0], points[1])
    } else {
        (points[points.len() - 1], points[points.len() - 2])
    };
    let angle = (tip[1] - other[1]).atan2(tip[0] - other[0]) * 180. / std::f64::consts::PI;
    let size = width * scale;
    let _ = write!(
        out,
        r#"<g transform="translate({} {}) rotate({angle}) scale({size})">"#,
        tip[0], tip[1]
    );
    match kind {
        Arrowhead::Triangle => {
            let _ = write!(out, r#"<path d="M0 0L-4 -2L-4 2Z" fill="{color}"/>"#);
        }
        Arrowhead::Open => {
            let _ = write!(
                out,
                r#"<path d="M-4 -2L0 0L-4 2" fill="none" stroke="{color}" stroke-width="1" stroke-linejoin="round"/>"#
            );
        }
        Arrowhead::Circle => {
            let _ = write!(out, r#"<circle r="1.5" fill="{color}"/>"#);
        }
        Arrowhead::None => {}
    }
    out.push_str("</g>");
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
    if style.validate().is_err() {
        return String::new();
    }
    if width <= 0. || color == "none" {
        return String::new();
    }
    let geometry = if style.alignment != StrokeAlignment::Center
        || style.profile != WidthProfile::Uniform
        || style.start_arrow != Arrowhead::None
        || style.end_arrow != Arrowhead::None
    {
        contours(raw_data)
    } else {
        vec![]
    };
    let closed = !geometry.is_empty() && geometry.iter().all(|contour| contour.closed);
    let alignment = if closed {
        style.alignment
    } else {
        StrokeAlignment::Center
    };
    let weight = if alignment == StrokeAlignment::Center {
        width
    } else {
        width * 2.
    };
    let mut out = String::new();
    let extent = f64::from(weight) * f64::from(style.miter_limit.max(2.));
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
            f64::from(style.arrow_scale),
            color,
        );
        arrow(
            &mut out,
            contour,
            false,
            style.end_arrow,
            f64::from(width),
            f64::from(style.arrow_scale),
            color,
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
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
