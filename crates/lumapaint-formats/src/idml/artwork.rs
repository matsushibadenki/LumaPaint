//! Native paths and portable embedded image previews. Geometry stays editable.
use super::*;
use base64::Engine;
use svgtypes::{SimplePathSegment as S, SimplifyingPathParser};

pub(super) fn geometry(data: &str, scale: f32) -> Result<String, ExportError> {
    // Each node stores anchor, incoming handle, outgoing handle.
    let mut contours: Vec<(Vec<[[f64; 2]; 3]>, bool)> = Vec::new();
    for segment in SimplifyingPathParser::from(data) {
        let segment = segment.map_err(|_| unsupported())?;
        match segment {
            S::MoveTo { x, y } => contours.push((vec![[[x, y]; 3]], false)),
            S::ClosePath => {
                let (points, closed) = contours.last_mut().ok_or_else(unsupported)?;
                if points.len() > 1 && points.first().unwrap()[0] == points.last().unwrap()[0] {
                    let last = points.pop().unwrap();
                    points[0][1] = last[1];
                }
                *closed = true;
            }
            s => {
                let (points, _) = contours.last_mut().ok_or_else(unsupported)?;
                let prev = points.last_mut().ok_or_else(unsupported)?;
                let (end, incoming, outgoing) = match s {
                    S::LineTo { x, y } => ([x, y], [x, y], prev[0]),
                    S::CurveTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    } => ([x, y], [x2, y2], [x1, y1]),
                    S::Quadratic { x1, y1, x, y } => (
                        [x, y],
                        [x + (x1 - x) * 2. / 3., y + (y1 - y) * 2. / 3.],
                        [
                            prev[0][0] + (x1 - prev[0][0]) * 2. / 3.,
                            prev[0][1] + (y1 - prev[0][1]) * 2. / 3.,
                        ],
                    ),
                    _ => return Err(unsupported()),
                };
                prev[2] = outgoing;
                points.push([end, incoming, end]);
            }
        }
    }
    let mut result = String::new();
    for (points, closed) in contours {
        let _ = write!(
            result,
            "<GeometryPathType PathOpen=\"{}\"><PathPointArray>",
            !closed
        );
        for [a, l, r] in points {
            let s = scale as f64;
            let _=write!(result,"<PathPointType Anchor=\"{} {}\" LeftDirection=\"{} {}\" RightDirection=\"{} {}\"/>",a[0]*s,a[1]*s,l[0]*s,l[1]*s,r[0]*s,r[1]*s);
        }
        result.push_str("</PathPointArray></GeometryPathType>");
    }
    if result.is_empty() {
        return Err(unsupported());
    }
    Ok(result)
}
pub(super) fn write(
    o: &VectorObject,
    layer: &str,
    id: usize,
    scale: f32,
    resources: &mut Resources,
    locked: bool,
) -> Result<String, ExportError> {
    let geometry = geometry(&o.path.data, scale)?;
    let fill = o.fill.map_or_else(
        || "Swatch/None".into(),
        |p| resources.color([p.color[0], p.color[1], p.color[2]]),
    );
    let stroke = o.stroke.map_or_else(
        || "Swatch/None".into(),
        |p| resources.color([p.color[0], p.color[1], p.color[2]]),
    );
    let [a, b, c, d, e, f] = o.transform;
    let mut image = String::new();
    if let Some(frame) = &o.image_frame {
        if let Some(i) = &frame.image {
            let encoded = i.data_uri.split_once(',').ok_or_else(unsupported)?.1;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(|_| unsupported())?;
            let (width, height, kind) =
                super::read::image_info(&bytes).map_err(|_| unsupported())?;
            if width != i.encoded_width
                || height != i.encoded_height
                || i.width != width
                || i.height != height
                || i.orientation_transform != [1., 0., 0., 1., 0., 0.]
            {
                return Err(unsupported());
            }
            let [a, b, c, d, e, f] = frame.content_transform.ok_or_else(unsupported)?;
            let s = scale;
            image=format!("<Image Self=\"u{:x}\" ImageTypeName=\"$ID/{kind}\" ItemTransform=\"{} {} {} {} {} {}\" ActualPpi=\"72 72\" Space=\"$ID/#Links_RGB\" Visible=\"true\"><Properties><GraphicBounds Left=\"0\" Top=\"0\" Right=\"{width}\" Bottom=\"{height}\"/><Contents><![CDATA[{encoded}]]></Contents></Properties></Image>",0x70000000+id,a*s,b*s,c*s,d*s,e*s,f*s);
            issue(
                &mut resources.report,
                "idml.image_preview_embedded",
                CompatibilityTier::B,
            );
        }
    }
    let cap = match o.stroke_style.cap {
        lumapaint_core::stroke::LineCap::Butt => "ButtEndCap",
        lumapaint_core::stroke::LineCap::Round => "RoundEndCap",
        lumapaint_core::stroke::LineCap::Square => "ProjectingEndCap",
    };
    let join = match o.stroke_style.join {
        lumapaint_core::stroke::LineJoin::Miter => "MiterEndJoin",
        lumapaint_core::stroke::LineJoin::Round => "RoundEndJoin",
        lumapaint_core::stroke::LineJoin::Bevel => "BevelEndJoin",
    };
    Ok(format!("<Polygon Self=\"u{:x}\" Name=\"{}\" ItemLayer=\"{layer}\" Locked=\"{locked}\" ContentType=\"{}\" Visible=\"{}\" ItemTransform=\"{a} {b} {c} {d} {} {}\" FillColor=\"{fill}\" StrokeColor=\"{stroke}\" StrokeWeight=\"{}\" EndCap=\"{cap}\" EndJoin=\"{join}\" MiterLimit=\"{}\"><Properties><PathGeometry>{geometry}</PathGeometry></Properties><TransparencySetting><BlendingSetting Opacity=\"{}\" BlendMode=\"Normal\"/></TransparencySetting>{image}</Polygon>",0x60000000+id,xml(&o.name),if image.is_empty(){"Unassigned"}else{"GraphicType"},o.visible,e*scale,f*scale,o.stroke_width*scale,o.stroke_style.miter_limit,o.opacity*100.))
}
