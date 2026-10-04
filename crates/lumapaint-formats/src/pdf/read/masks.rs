//! Graphics-state soft masks converted to bounded SVG mask definitions.
use super::*;
impl Interpreter<'_> {
    pub(super) fn soft_mask(
        &mut self,
        value: &Object,
        resources: &Dictionary,
        state: &State,
        depth: usize,
    ) -> Result<Option<String>, ImportError> {
        let body_start = self.body.len();
        let defs_start = self.defs.len();
        let result = self.soft_mask_inner(value, resources, state, depth);
        if result.is_err() {
            self.body.truncate(body_start);
            self.defs.truncate(defs_start);
            self.image_cache.clear();
        }
        match result {
            Err(ImportError::Unsupported(code)) => {
                self.unsupported(code)?;
                Ok(None)
            }
            other => other,
        }
    }
    fn soft_mask_inner(
        &mut self,
        value: &Object,
        resources: &Dictionary,
        state: &State,
        depth: usize,
    ) -> Result<Option<String>, ImportError> {
        let object = resolve(self.doc, value)?;
        if object.as_name().ok() == Some(b"None") {
            return Ok(None);
        }
        let mask = object.as_dict().map_err(|_| malformed())?;
        let kind = match mask
            .get(b"S")
            .and_then(Object::as_name)
            .map_err(|_| malformed())?
        {
            b"Luminosity" => "luminance",
            b"Alpha" => "alpha",
            _ => return Err(ImportError::Unsupported("pdf.soft_mask")),
        };
        if let Ok(tr) = mask.get(b"TR") {
            if resolve(self.doc, tr)?.as_name().ok() != Some(b"Identity") {
                return Err(ImportError::Unsupported("pdf.soft_mask"));
            }
        }
        if let Ok(bc) = mask.get(b"BC") {
            if resolve(self.doc, bc)?
                .as_array()
                .map_err(|_| malformed())?
                .iter()
                .any(|v| num(v).ok() != Some(0.))
            {
                return Err(ImportError::Unsupported("pdf.soft_mask"));
            }
        }
        let form = resolve(self.doc, mask.get(b"G").map_err(|_| malformed())?)?
            .as_stream()
            .map_err(|_| malformed())?;
        if form.dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(b"Form") {
            return Err(malformed());
        }
        let group = resolve(self.doc, form.dict.get(b"Group").map_err(|_| malformed())?)?
            .as_dict()
            .map_err(|_| malformed())?;
        if group.get(b"S").and_then(Object::as_name).ok() != Some(b"Transparency")
            || group.get(b"K").and_then(Object::as_bool).unwrap_or(false)
        {
            return Err(ImportError::Unsupported("pdf.soft_mask"));
        }
        if kind == "luminance" && group.get(b"CS").is_err() {
            return Err(ImportError::Unsupported("pdf.soft_mask"));
        }
        if let Ok(cs) = group.get(b"CS") {
            if ![1, 3].contains(&self.space(cs)?) {
                return Err(ImportError::Unsupported("pdf.soft_mask"));
            }
        }
        let mut ctm = state.ctm;
        if let Ok(m) = form.dict.get(b"Matrix") {
            ctm = matrix(
                ctm,
                nums(
                    resolve(self.doc, m)?.as_array().map_err(|_| malformed())?,
                    6,
                )?
                .try_into()
                .unwrap(),
            );
        }
        let bbox = nums(
            resolve(self.doc, form.dict.get(b"BBox").map_err(|_| malformed())?)?
                .as_array()
                .map_err(|_| malformed())?,
            4,
        )?;
        if bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
            return Err(malformed());
        }
        let clip = format!("pdf-mask-clip-{}", self.serial);
        self.serial += 1;
        let [a, b, c, d, e, f] = ctm;
        let _=write!(self.defs,"<clipPath id=\"{clip}\" clipPathUnits=\"userSpaceOnUse\"><path d=\"M {} {} H {} V {} H {} Z\" transform=\"matrix({a} {b} {c} {d} {e} {f})\"/></clipPath>",bbox[0],bbox[1],bbox[2],bbox[3],bbox[0]);
        let nested_resources = match form.dict.get(b"Resources") {
            Ok(v) => resolve(self.doc, v)?.as_dict().map_err(|_| malformed())?,
            Err(_) => resources,
        };
        let data = form
            .get_plain_content_with_limit(self.remaining)
            .map_err(|_| ImportError::LimitExceeded("pdf.stream"))?;
        let start = self.body.len();
        self.content(
            &data,
            nested_resources,
            State {
                ctm,
                pattern_base: ctm,
                clips: vec![clip],
                ..Default::default()
            },
            depth + 1,
        )?;
        let body = self.body.split_off(start);
        let id = format!("pdf-mask-{}", self.serial);
        self.serial += 1;
        let [x, y, right, top] = self.page_bounds;
        let _=write!(self.defs,"<mask id=\"{id}\" maskUnits=\"userSpaceOnUse\" maskContentUnits=\"userSpaceOnUse\" x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" style=\"mask-type:{kind}\">{body}</mask>",right-x,top-y);
        Ok(Some(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};
    fn parser(doc: &lopdf::Document) -> Interpreter<'_> {
        Interpreter {
            doc,
            body: "existing".into(),
            defs: "definitions".into(),
            issues: vec![],
            remaining: LIMIT,
            operations: 0,
            serial: 0,
            allow_lossy: true,
            page_bounds: [0., 0., 100., 80.],
            font_cache: Default::default(),
            text_glyphs: 0,
            image_remaining: 64 * 1024 * 1024,
            image_cache: Default::default(),
        }
    }
    fn mask(doc: &mut lopdf::Document, content: &[u8], kind: &str) -> Object {
        let g=doc.add_object(Stream::new(dictionary! {"Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),100.into(),80.into()],"Group"=>dictionary! {"S"=>"Transparency","CS"=>"DeviceRGB"}},content.to_vec()));
        dictionary! {"S"=>Object::Name(kind.as_bytes().to_vec()),"G"=>g}.into()
    }
    #[test]
    fn unsupported_mask_never_leaks_partial_drawing_or_definitions() {
        let mut doc = lopdf::Document::new();
        let mask = mask(
            &mut doc,
            b"0 0 10 10 re f 0 0 m 1 0 0 1 0 0 cm",
            "Luminosity",
        );
        let mut p = parser(&doc);
        assert!(p
            .soft_mask(&mask, &Dictionary::new(), &State::default(), 0)
            .unwrap()
            .is_none());
        assert_eq!(p.body, "existing");
        assert_eq!(p.defs, "definitions");
        assert_eq!(p.issues[0].code, "pdf.ctm_inside_path");
    }
    #[test]
    fn alpha_and_luminosity_masks_capture_content_and_none_clears_mask() {
        for kind in ["Alpha", "Luminosity"] {
            let mut doc = lopdf::Document::new();
            let mask = mask(&mut doc, b"0 0 10 10 re f", kind);
            let mut p = parser(&doc);
            assert!(p
                .soft_mask(&mask, &Dictionary::new(), &State::default(), 0)
                .unwrap()
                .is_some());
            assert_eq!(p.body, "existing");
            assert!(p.defs.contains(if kind == "Alpha" {
                "mask-type:alpha"
            } else {
                "mask-type:luminance"
            }));
            assert!(p
                .soft_mask(
                    &Object::Name(b"None".to_vec()),
                    &Dictionary::new(),
                    &State::default(),
                    0
                )
                .unwrap()
                .is_none());
        }
    }
    #[test]
    fn unsupported_transfer_functions_require_consent() {
        let mut doc = lopdf::Document::new();
        let mut mask = mask(&mut doc, b"0 0 10 10 re f", "Alpha");
        mask.as_dict_mut().unwrap().set("TR", "Unsupported");
        let mut p = parser(&doc);
        p.allow_lossy = false;
        assert!(matches!(
            p.soft_mask(&mask, &Dictionary::new(), &State::default(), 0),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
        assert_eq!(p.body, "existing");
    }
}
