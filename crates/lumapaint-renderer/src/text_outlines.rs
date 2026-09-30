//! Convert the same shaped glyph paths used for SVG rendering to portable geometry.
use crate::vector::{font_resolver, system_fonts};
use lumapaint_core::vector::{FillRule, VectorObject, VectorObjectKind, VectorPaint, VectorPath};
#[cfg(test)]
use lumapaint_formats::native::NativeDocumentCodec;
use resvg::{tiny_skia, usvg};
use skia_safe::{Path, PathBuilder, PathFillType, PathOp};

fn geometry(path: &tiny_skia::Path, transform: tiny_skia::Transform) -> Result<Path, String> {
    let path = path
        .clone()
        .transform(transform)
        .ok_or("Invalid glyph transform")?;
    let mut builder = PathBuilder::new();
    for segment in path.segments() {
        match segment {
            tiny_skia::PathSegment::MoveTo(p) => {
                builder.move_to((p.x, p.y));
            }
            tiny_skia::PathSegment::LineTo(p) => {
                builder.line_to((p.x, p.y));
            }
            tiny_skia::PathSegment::QuadTo(a, b) => {
                builder.quad_to((a.x, a.y), (b.x, b.y));
            }
            tiny_skia::PathSegment::CubicTo(a, b, c) => {
                builder.cubic_to((a.x, a.y), (b.x, b.y), (c.x, c.y));
            }
            tiny_skia::PathSegment::Close => {
                builder.close();
            }
        }
    }
    Ok(builder.detach())
}
fn clip_geometry(group: &usvg::Group, transform: tiny_skia::Transform) -> Result<Path, String> {
    let transform = transform.pre_concat(group.transform());
    let mut result = Path::default();
    for node in group.children() {
        let path = match node {
            usvg::Node::Path(p) => geometry(p.data(), transform)?,
            usvg::Node::Group(g) => clip_geometry(g, transform)?,
            _ => return Err("Unsupported text clip".into()),
        };
        result = skia_safe::op(&result, &path, PathOp::Union).ok_or("Text clip union failed")?;
    }
    Ok(result)
}
fn collect(
    group: &usvg::Group,
    transform: tiny_skia::Transform,
    clips: &[Path],
    output: &mut Vec<VectorObject>,
    original: &VectorObject,
) -> Result<(), String> {
    let transform = transform.pre_concat(group.transform());
    if group.opacity().get() != 1. || group.mask().is_some() || !group.filters().is_empty() {
        return Err(
            "Unsupported text effect / この文字効果はアウトライン化できません / 不支持此文字效果"
                .into(),
        );
    }
    let mut clips = clips.to_vec();
    if let Some(clip) = group.clip_path() {
        if clip.clip_path().is_some() {
            return Err("Nested text clipping is unsupported".into());
        }
        clips.push(clip_geometry(
            clip.root(),
            transform.pre_concat(clip.transform()),
        )?);
    }
    for node in group.children() {
        match node {
        usvg::Node::Group(g)=>collect(g,transform,&clips,output,original)?,
        usvg::Node::Text(t)=>collect(t.flattened(),transform,&clips,output,original)?,
        usvg::Node::Image(_)=>return Err("Bitmap glyphs cannot be outlined / 画像形式の字形はアウトライン化できません / 位图字形无法转为轮廓".into()),
        usvg::Node::Path(p)=>{
            if !p.is_visible(){continue;}
            if p.stroke().is_some(){return Err("Unsupported glyph stroke".into());}
            let Some(fill)=p.fill() else {continue;};
            let usvg::Paint::Color(color)=fill.paint() else {return Err("Unsupported glyph paint".into());};
            let mut path=geometry(p.data(),transform)?.with_fill_type(match fill.rule(){usvg::FillRule::NonZero=>PathFillType::Winding,usvg::FillRule::EvenOdd=>PathFillType::EvenOdd});
            for clip in &clips {path=skia_safe::op(&path,clip,PathOp::Intersect).ok_or("Text clip failed")?;}
            if path.is_empty(){continue;}
            let mut object=original.clone();
            object.text=None;object.kind=VectorObjectKind::Compound;object.transform=[1.,0.,0.,1.,0.,0.];
            object.path=VectorPath{data:path.to_svg(),fill_rule:match path.fill_type(){PathFillType::EvenOdd=>FillRule::EvenOdd,_=>FillRule::NonZero}};
            object.control_points=path.points().iter().map(|p|[p.x,p.y]).collect();
            object.fill=Some(VectorPaint{color:[color.red,color.green,color.blue,(fill.opacity().get()*255.).round()as u8]});
            object.stroke=None;object.stroke_width=0.;object.bounds_reset=false;
            object.validate()?;output.push(object);
        }
    }
    }
    Ok(())
}
pub fn outline(source: &str, original: &VectorObject) -> Result<Vec<VectorObject>, String> {
    let options = usvg::Options {
        fontdb: system_fonts(),
        font_resolver: font_resolver(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default().resolve_data,
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(source, &options).map_err(|e| e.to_string())?;
    let mut output = Vec::new();
    collect(
        tree.root(),
        tiny_skia::Transform::identity(),
        &[],
        &mut output,
        original,
    )?;
    if output.is_empty() {
        return Err(
            "No outlineable glyphs / アウトライン化できる文字がありません / 没有可转换的字形"
                .into(),
        );
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumapaint_core::{
        document::{Document, TextSettings},
        vector::VectorText,
    };
    #[test]
    fn outlines_preserve_rendering_and_atomic_history() {
        for framed in [false, true] {
            let mut doc = Document::default();
            doc.set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "Outline O 日本語\nSecond line".into(),
                    font_size: 28.,
                    box_width: 240.,
                    box_height: framed.then_some(38.),
                    ..Default::default()
                },
                position: [40., 40.],
                color: [32, 90, 180],
            })
            .unwrap();
            doc.affine_selected_vectors([0.98, 0.15, -0.15, 0.98, 12., 3.])
                .unwrap();
            let before = doc.encode().unwrap();
            let raster =
                crate::vector::rasterize_svg(&doc.svg_layers().next().unwrap().source, 960, 640)
                    .unwrap();
            doc.outline_selected_text(outline).unwrap();
            assert!(doc.snapshot().text_objects.is_empty());
            let layer = doc.svg_layers().next().unwrap();
            assert!(layer
                .vector_objects
                .iter()
                .all(|o| o.text.is_none() && !o.group_path.is_empty()));
            let after = crate::vector::rasterize_svg(&layer.source, 960, 640).unwrap();
            let error: u64 = raster
                .pixels
                .iter()
                .zip(&after.pixels)
                .map(|(a, b)| a.abs_diff(*b) as u64)
                .sum();
            let ink: u64 = raster.pixels.iter().map(|v| *v as u64).sum();
            assert!(ink > 0);
            assert!(
                error as f64 / (ink as f64) < 0.06,
                "relative raster difference: {}",
                error as f64 / ink as f64
            );
            let saved = doc.encode().unwrap();
            assert!(Document::decode(&saved)
                .unwrap()
                .snapshot()
                .text_objects
                .is_empty());
            doc.undo();
            assert_eq!(doc.encode().unwrap(), before);
            doc.redo();
            assert_eq!(doc.encode().unwrap(), saved);
        }
    }
    #[test]
    fn conversion_failure_keeps_text() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "Keep".into(),
                ..Default::default()
            },
            position: [20., 20.],
            color: [0, 0, 0],
        })
        .unwrap();
        let before = doc.encode().unwrap();
        assert!(doc
            .outline_selected_text(|_, _| Err("unsupported".into()))
            .is_err());
        assert_eq!(doc.encode().unwrap(), before);
        assert!(doc.outline_selected_text(|_, _| Ok(Vec::new())).is_err());
        assert_eq!(doc.encode().unwrap(), before);
    }
}
