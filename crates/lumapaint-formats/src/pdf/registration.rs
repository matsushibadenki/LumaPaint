//! PDF /All separation for solid, opaque registration paths. LP stays renderer-independent.
use crate::export::{ExportError, ExportSnapshot};
use lopdf::{
    content::{Content, Operation},
    dictionary, Object, Stream,
};
pub(super) fn apply(
    bytes: Vec<u8>,
    snapshot: &ExportSnapshot,
    layout: Option<&super::print_layout::PrintLayout>,
) -> Result<Vec<u8>, ExportError> {
    let invalid = |e: lopdf::Error| ExportError::InvalidDocument(e.to_string());
    let document =
        lumapaint_core::document::Document::from_document_state(snapshot.state().clone())
            .map_err(ExportError::InvalidDocument)?;
    let mut objects = Vec::new();
    for layer in document.visible_svg_layers() {
        for object in layer.vector_objects.iter().filter(|o| o.visible) {
            if ![object.fill, object.stroke]
                .into_iter()
                .flatten()
                .any(|p| p.registration)
            {
                continue;
            }
            if layer.effective_opacity() != 1.
                || object.opacity != 1.
                || object.blend_mode != "normal"
                || object.text.is_some()
                || object.clipping_group.is_some()
                || !object.group_path.is_empty()
                || object.stroke_style != Default::default()
                || object.live_corners.is_some()
                || object.rectangle_radii.is_some()
                || document.has_layer_effects(&layer.id)
                || document.has_document_clipping()
            {
                return Err(ExportError::UnsupportedFeature(
                    "pdf.registration_requires_plain_opaque_paths",
                ));
            }
            objects.push(object);
        }
    }
    if objects.is_empty() {
        return Ok(bytes);
    }
    let mut pdf = lopdf::Document::load_mem(&bytes).map_err(invalid)?;
    let page = *pdf
        .get_pages()
        .values()
        .next()
        .ok_or_else(|| ExportError::InvalidDocument("Missing PDF page".into()))?;
    let scale = 72. / snapshot.state().resolution.unwrap_or(72) as f32;
    let tint=pdf.add_object(dictionary!{"FunctionType"=>2,"Domain"=>vec![0.into(),1.into()],"C0"=>vec![0.into(),0.into(),0.into(),0.into()],"C1"=>vec![1.into(),1.into(),1.into(),1.into()],"N"=>1});
    let mut resources = pdf
        .get_page_resources(page)
        .map_err(invalid)?
        .0
        .cloned()
        .unwrap_or_default();
    let mut spaces = resources
        .get(b"ColorSpace")
        .ok()
        .and_then(|o| pdf.dereference(o).ok())
        .and_then(|(_, o)| o.as_dict().ok())
        .cloned()
        .unwrap_or_default();
    spaces.set(
        "LPAll",
        vec![
            Object::Name(b"Separation".to_vec()),
            Object::Name(b"All".to_vec()),
            Object::Name(b"DeviceCMYK".to_vec()),
            Object::Reference(tint),
        ],
    );
    resources.set("ColorSpace", spaces);
    pdf.get_object_mut(page)
        .map_err(invalid)?
        .as_dict_mut()
        .map_err(invalid)?
        .set("Resources", resources);
    if let Some(layout) = layout {
        layout.boxes(&mut pdf, snapshot)?;
    }
    let [origin_x, _, _, bottom] = layout.map(|l| l.bounds).unwrap_or([
        0.,
        0.,
        snapshot.state().width as f32,
        snapshot.state().height as f32,
    ]);
    let real = |values: &[f32]| values.iter().copied().map(Object::Real).collect::<Vec<_>>();
    let mut ops = vec![
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            real(&[scale, 0., 0., -scale, -origin_x * scale, bottom * scale]),
        ),
    ];
    for o in objects {
        ops.push(Operation::new("q", vec![]));
        ops.push(Operation::new("cm", real(&o.transform)));
        ops.push(Operation::new("cs", vec![Object::Name(b"LPAll".to_vec())]));
        ops.push(Operation::new("CS", vec![Object::Name(b"LPAll".to_vec())]));
        ops.push(Operation::new("scn", vec![1.into()]));
        ops.push(Operation::new("SCN", vec![1.into()]));
        ops.push(Operation::new("w", real(&[o.stroke_width])));
        ops.push(Operation::new("J", vec![0.into()]));
        ops.push(Operation::new("j", vec![0.into()]));
        ops.push(Operation::new("M", real(&[o.stroke_style.miter_limit])));
        ops.push(Operation::new("d", vec![Object::Array(vec![]), 0.into()]));
        for (points, closed) in lumapaint_core::bezier::cubic_contours(&o.path.data)
            .map_err(ExportError::InvalidDocument)?
        {
            if points.is_empty() {
                continue;
            }
            ops.push(Operation::new("m", real(&points[0])));
            for curve in points[1..].as_chunks::<3>().0 {
                ops.push(Operation::new(
                    "c",
                    real(&[
                        curve[0][0],
                        curve[0][1],
                        curve[1][0],
                        curve[1][1],
                        curve[2][0],
                        curve[2][1],
                    ]),
                ));
            }
            if closed {
                ops.push(Operation::new("h", vec![]));
            }
        }
        let fill = o.fill.is_some_and(|p| p.registration);
        let stroke = o.stroke.is_some_and(|p| p.registration);
        let even = o.path.fill_rule == lumapaint_core::vector::FillRule::EvenOdd;
        ops.push(Operation::new(
            match (fill, stroke, even) {
                (true, true, true) => "B*",
                (true, true, false) => "B",
                (true, false, true) => "f*",
                (true, false, false) => "f",
                _ => "S",
            },
            vec![],
        ));
        ops.push(Operation::new("Q", vec![]));
    }
    ops.push(Operation::new("Q", vec![]));
    // Isolate original graphics state before appending the registration overlay.
    let old = pdf.get_page_content(page);
    let mut content = b"q\n".to_vec();
    content.extend(old);
    content.extend(b"\nQ\n");
    content.extend(Content { operations: ops }.encode().map_err(invalid)?);
    let stream = pdf.add_object(Stream::new(dictionary! {}, content));
    pdf.get_object_mut(page)
        .map_err(invalid)?
        .as_dict_mut()
        .map_err(invalid)?
        .set("Contents", stream);
    let mut result = Vec::new();
    pdf.save_to(&mut result)
        .map_err(|e| ExportError::InvalidDocument(e.to_string()))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{DocumentExporter, ExportOptions};
    #[test]
    fn pdf_registration_is_all_separation_and_not_process_black() {
        let mut d = lumapaint_core::document::Document::default();
        let layer = d.add_vector_layer().unwrap();
        let object=serde_json::from_value(serde_json::json!({"id":"marks","name":"Trim marks","path":{"data":"M100 100L120 100","fillRule":"nonZero"},"transform":[1,0,0,1,0,0],"fill":null,"stroke":{"color":[0,0,0,255],"registration":true},"strokeWidth":0.25,"visible":true})).unwrap();
        d.upsert_vector_object(&layer, object).unwrap();
        let pdf = crate::pdf::PdfExporter
            .export(
                &ExportSnapshot::capture(&d),
                ExportOptions { allow_lossy: true },
            )
            .unwrap();
        let parsed = lopdf::Document::load_mem(&pdf.bytes).unwrap();
        let page = *parsed.get_pages().values().next().unwrap();
        assert!(parsed
            .get_dictionary(page)
            .unwrap()
            .get(b"TrimBox")
            .is_err());
        let spaces = parsed
            .get_page_resources(page)
            .unwrap()
            .0
            .unwrap()
            .get(b"ColorSpace")
            .unwrap()
            .as_dict()
            .unwrap();
        let all = spaces.get(b"LPAll").unwrap().as_array().unwrap();
        assert_eq!(all[0].as_name().unwrap(), b"Separation");
        assert_eq!(all[1].as_name().unwrap(), b"All");
        let tint_id = all[3].as_reference().unwrap();
        let content = Content::decode(&parsed.get_page_content(page)).unwrap();
        assert!(content
            .operations
            .iter()
            .any(|o| o.operator == "SCN" && o.operands == vec![1.into()]));
        assert!(content
            .operations
            .iter()
            .any(|o| o.operator == "M"
                && o.operands.first().and_then(|v| v.as_float().ok()) == Some(4.)));
        assert!(content
            .operations
            .iter()
            .any(|o| o.operator == "w" && o.operands == vec![Object::Real(0.25)]));
        let imported = crate::pdf::read(
            &pdf.bytes,
            crate::io::ReadOptions {
                allow_lossy: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(imported
            .report
            .issues
            .iter()
            .any(|i| i.code == "pdf.registration_preview_only"));
        let mut nonlinear = parsed;
        nonlinear.get_dictionary_mut(tint_id).unwrap().set("N", 2);
        let mut bytes = Vec::new();
        nonlinear.save_to(&mut bytes).unwrap();
        assert!(matches!(
            crate::pdf::read(
                &bytes,
                crate::io::ReadOptions {
                    allow_lossy: true,
                    ..Default::default()
                }
            ),
            Err(crate::ImportError::Unsupported("pdf.color_space"))
        ));
    }
    #[test]
    fn print_boxes_survive_publication_and_preserve_physical_size() {
        let mut publication = crate::pdf::PdfPublication::default();
        for dpi in [72, 300] {
            let scale = dpi as f32 / 72.;
            let mut state = lumapaint_core::document::Document::default().document_state();
            state.width = dpi * 2;
            state.height = dpi;
            state.resolution = Some(dpi);
            let mut d = lumapaint_core::document::Document::from_document_state(state).unwrap();
            let layer = d.add_vector_layer().unwrap();
            let object = serde_json::from_value(serde_json::json!({
                "id":"marks", "name":"Marks", "path":{"data":"M-20 -10L164 -10M-20 82L164 82","fillRule":"nonZero"},
                "transform":[scale,0,0,scale,0,0],"fill":null,
                "stroke":{"color":[0,0,0,255],"registration":true},"strokeWidth":0.25,"visible":true
            })).unwrap();
            d.upsert_vector_object(&layer, object).unwrap();
            let exported = crate::pdf::PdfExporter
                .export(
                    &ExportSnapshot::capture(&d),
                    ExportOptions { allow_lossy: true },
                )
                .unwrap();
            publication.push(exported).unwrap();
        }
        let exported = publication
            .finish(ExportOptions { allow_lossy: true })
            .unwrap();
        let pdf = lopdf::Document::load_mem(&exported.bytes).unwrap();
        let mut first: Option<Vec<f32>> = None;
        for id in pdf.get_pages().values() {
            let page = pdf.get_dictionary(*id).unwrap();
            let values = |key: &[u8]| {
                page.get(key)
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_float().unwrap())
                    .collect::<Vec<_>>()
            };
            let trim = values(b"TrimBox");
            let bleed = values(b"BleedBox");
            let media = values(b"MediaBox");
            assert!((trim[2] - trim[0] - 144.).abs() < 0.001);
            assert!((trim[3] - trim[1] - 72.).abs() < 0.001);
            assert!((trim[0] - bleed[0] - 3. * 72. / 25.4).abs() < 0.001);
            assert_eq!(values(b"CropBox"), media);
            for axis in 0..2 {
                assert!(media[axis] < bleed[axis]);
                assert!(media[axis + 2] > bleed[axis + 2]);
            }
            if let Some(previous) = &first {
                for (a, b) in media.iter().zip(previous) {
                    assert!((a - b).abs() < 0.001);
                }
            } else {
                first = Some(media);
            }
            assert!(pdf
                .get_page_resources(*id)
                .unwrap()
                .0
                .unwrap()
                .get(b"ColorSpace")
                .is_ok());
        }
    }
    #[test]
    fn print_export_refuses_page_sized_effect_fallback() {
        let mut d = crate::svg::import("Filter".into(), r##"<svg width="120" height="80"><defs><filter id="blur"><feGaussianBlur stdDeviation="2"/></filter></defs><rect x="-5" width="120" height="80" fill="red" filter="url(#blur)"/></svg>"##.into()).unwrap();
        let layer = d.add_vector_layer().unwrap();
        let object = serde_json::from_value(serde_json::json!({
            "id":"marks", "name":"Marks", "path":{"data":"M-20 -10L140 -10","fillRule":"nonZero"},
            "transform":[1,0,0,1,0,0],"fill":null,
            "stroke":{"color":[0,0,0,255],"registration":true},"strokeWidth":0.25,"visible":true
        }))
        .unwrap();
        d.upsert_vector_object(&layer, object).unwrap();
        assert!(matches!(
            crate::pdf::PdfExporter.export(
                &ExportSnapshot::capture(&d),
                ExportOptions { allow_lossy: true }
            ),
            Err(ExportError::UnsupportedFeature(
                "pdf.print_bleed_requires_vector_content"
            ))
        ));
        d.set_layer_effects(
            "layer-1",
            lumapaint_core::layer_effects::LayerEffects {
                enabled: true,
                ..Default::default()
            },
        )
        .unwrap();
        // Reject before SVG flattening can hide the fact that its image stops at the trim.
        assert!(matches!(
            crate::pdf::PdfExporter.export(
                &ExportSnapshot::capture(&d),
                ExportOptions { allow_lossy: true }
            ),
            Err(ExportError::UnsupportedFeature(
                "pdf.print_bleed_requires_vector_content"
            ))
        ));
    }
}
