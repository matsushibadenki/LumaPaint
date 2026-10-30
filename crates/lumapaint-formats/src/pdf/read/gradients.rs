//! Vector-only axial/radial shading conversion; unsupported functions require consent.
use super::*;
#[derive(Clone)]
pub(super) struct GradientPaint {
    id: String,
    tag: &'static str,
    matrix: [f32; 6],
}
fn inverse(m: [f32; 6]) -> Result<[f32; 6], ImportError> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return Err(ImportError::Unsupported("pdf.gradient_transform"));
    }
    Ok([
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ])
}
// Bound the interpolation error of x^N using its analytic chord extremum.
// Subdivision is bounded even for exponents with a near-singular endpoint.
fn exponential_samples(n: f32) -> Result<Vec<(f32, f32)>, ImportError> {
    if !n.is_finite() || n <= 0. || n > 1e7 {
        return Err(ImportError::Unsupported("pdf.gradient_function"));
    }
    let n = f64::from(n);
    let mut samples = vec![(0., 0.)];
    let mut pending = vec![(0_f64, 1_f64, 0_f64, 1_f64)];
    while let Some((a, b, ya, yb)) = pending.pop() {
        let slope = (yb - ya) / (b - a);
        let x = if n == 1. {
            a
        } else {
            (slope / n).powf(1. / (n - 1.)).clamp(a, b)
        };
        let error = (x.powf(n) - (ya + slope * (x - a))).abs();
        if error > 1. / 1024. {
            let mid = (a + b) * 0.5;
            // SVG offsets are f32: refuse unrepresentable subdivisions.
            if samples.len() + pending.len() + 2 > 256
                || mid as f32 <= a as f32
                || mid as f32 >= b as f32
            {
                return Err(ImportError::Unsupported("pdf.gradient_function"));
            }
            let y = mid.powf(n);
            pending.push((mid, b, y, yb));
            pending.push((a, mid, ya, y));
        } else {
            samples.push((b as f32, yb as f32));
        }
    }
    Ok(samples)
}
impl Interpreter<'_> {
    fn gradient_stops(
        &mut self,
        function: &Object,
        channels: usize,
        depth: usize,
    ) -> Result<Vec<(f32, String)>, ImportError> {
        if depth > 8 {
            return Err(ImportError::LimitExceeded("pdf.gradient_function_depth"));
        }
        let dict = resolve(self.doc, function)?
            .as_dict()
            .map_err(|_| malformed())?;
        if nums(
            resolve(self.doc, dict.get(b"Domain").map_err(|_| malformed())?)?
                .as_array()
                .map_err(|_| malformed())?,
            2,
        )? != [0., 1.]
        {
            return Err(ImportError::Unsupported("pdf.gradient_function"));
        }
        if let Ok(range) = dict.get(b"Range") {
            let r = nums(
                resolve(self.doc, range)?
                    .as_array()
                    .map_err(|_| malformed())?,
                channels * 2,
            )?;
            if r.as_chunks::<2>().0.iter().any(|r| *r != [0., 1.]) {
                return Err(ImportError::Unsupported("pdf.gradient_function"));
            }
        }
        match num(dict.get(b"FunctionType").map_err(|_| malformed())?)? {
            2. => {
                let samples =
                    exponential_samples(num(dict.get(b"N").map_err(|_| malformed())?)?)?;
                let mut endpoints = Vec::new();
                for (key, offset) in [(b"C0".as_slice(), 0.), (b"C1".as_slice(), 1.)] {
                    let default = vec![Object::Real(offset); channels];
                    let values = match dict.get(key) {
                        Ok(v) => resolve(self.doc, v)?.as_array().map_err(|_| malformed())?,
                        Err(_) => &default,
                    };
                    // PDF defaults are one-channel values, not arbitrary RGB colors.
                    if dict.get(key).is_err() && channels != 1 {
                        return Err(ImportError::Unsupported("pdf.gradient_function"));
                    }
                    let numbers = nums(values, channels)?;
                    if numbers.iter().any(|v| !(0. ..=1.).contains(v)) {
                        return Err(ImportError::Unsupported("pdf.gradient_function"));
                    }
                    endpoints.push(numbers);
                }
                samples
                    .into_iter()
                    .map(|(t, weight)| {
                        let values: Vec<Object> = endpoints[0]
                            .iter()
                            .zip(&endpoints[1])
                            .map(|(a, b)| Object::Real(a + weight * (b - a)))
                            .collect();
                        Ok((t, self.color(&values, channels)?))
                    })
                    .collect()
            }
            3. => {
                let functions =
                    resolve(self.doc, dict.get(b"Functions").map_err(|_| malformed())?)?
                        .as_array()
                        .map_err(|_| malformed())?;
                if functions.is_empty() || functions.len() > 128 {
                    return Err(ImportError::LimitExceeded("pdf.gradient_stops"));
                }
                let bounds = resolve(self.doc, dict.get(b"Bounds").map_err(|_| malformed())?)?
                    .as_array()
                    .map_err(|_| malformed())?;
                let mut points = vec![0.];
                points.extend(nums(bounds, functions.len() - 1)?);
                points.push(1.);
                if points
                    .windows(2)
                    .any(|p| p[0] > p[1] || !(0. ..=1.).contains(&p[1]))
                {
                    return Err(malformed());
                }
                let encode = nums(
                    resolve(self.doc, dict.get(b"Encode").map_err(|_| malformed())?)?
                        .as_array()
                        .map_err(|_| malformed())?,
                    functions.len() * 2,
                )?;
                if encode.as_chunks::<2>().0.iter().any(|p| *p != [0., 1.]) {
                    return Err(ImportError::Unsupported("pdf.gradient_function"));
                }
                let mut stops = Vec::new();
                for (i, function) in functions.iter().enumerate() {
                    for (t, color) in self.gradient_stops(function, channels, depth + 1)? {
                        stops.push((points[i] + t * (points[i + 1] - points[i]), color));
                    }
                    if stops.len() > 256 {
                        return Err(ImportError::LimitExceeded("pdf.gradient_stops"));
                    }
                }
                Ok(stops)
            }
            _ => Err(ImportError::Unsupported("pdf.gradient_function")),
        }
    }
    fn shading(
        &mut self,
        object: &Object,
        resources: &Dictionary,
        transform: [f32; 6],
    ) -> Result<GradientPaint, ImportError> {
        let object = resolve(self.doc, object)?;
        let dict = match object {
            Object::Dictionary(dict) => dict,
            Object::Stream(stream) => &stream.dict,
            _ => return Err(malformed()),
        };
        let shading_type = num(dict.get(b"ShadingType").map_err(|_| malformed())?)?;
        if shading_type != 2. && shading_type != 3. {
            return Err(ImportError::Unsupported("pdf.shading_conversion"));
        }
        let extend = dict
            .get(b"Extend")
            .ok()
            .map(|v| resolve(self.doc, v))
            .transpose()?
            .and_then(|v| v.as_array().ok());
        if !extend.is_some_and(|v| v.len() == 2 && v.iter().all(|v| v.as_bool().ok() == Some(true)))
        {
            return Err(ImportError::Unsupported("pdf.gradient_extent"));
        }
        if let Ok(domain) = dict.get(b"Domain") {
            if nums(
                resolve(self.doc, domain)?
                    .as_array()
                    .map_err(|_| malformed())?,
                2,
            )? != [0., 1.]
            {
                return Err(ImportError::Unsupported("pdf.gradient_function"));
            }
        }
        let mut space = resolve(self.doc, dict.get(b"ColorSpace").map_err(|_| malformed())?)?;
        if let Ok(name) = space.as_name() {
            if ![b"DeviceRGB".as_slice(), b"DeviceGray", b"DeviceCMYK"].contains(&name) {
                space = resource(self.doc, resources, b"ColorSpace", name)?;
            }
        }
        let channels = self.space(space)?;
        let stops =
            self.gradient_stops(dict.get(b"Function").map_err(|_| malformed())?, channels, 0)?;
        let coords = resolve(self.doc, dict.get(b"Coords").map_err(|_| malformed())?)?
            .as_array()
            .map_err(|_| malformed())?;
        let (tag, geometry) = match shading_type {
            2. => {
                let p = nums(coords, 4)?;
                if p[0] == p[2] && p[1] == p[3] {
                    return Err(ImportError::Unsupported("pdf.gradient_geometry"));
                }
                (
                    "linearGradient",
                    format!(
                        "x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
                        p[0], p[1], p[2], p[3]
                    ),
                )
            }
            3. => {
                let p = nums(coords, 6)?;
                if p[2] != 0. || p[5] <= 0. || (p[0] - p[3]).hypot(p[1] - p[4]) >= p[5] {
                    return Err(ImportError::Unsupported("pdf.gradient_geometry"));
                }
                (
                    "radialGradient",
                    format!(
                        "fx=\"{}\" fy=\"{}\" cx=\"{}\" cy=\"{}\" r=\"{}\"",
                        p[0], p[1], p[3], p[4], p[5]
                    ),
                )
            }
            _ => return Err(ImportError::Unsupported("pdf.shading_conversion")),
        };
        // A shading BBox is an additional clip; do not paint beyond it silently.
        if dict.get(b"BBox").is_ok() {
            return Err(ImportError::Unsupported("pdf.gradient_extent"));
        }
        let id = format!("pdf-gradient-{}", self.serial);
        self.serial += 1;
        let _ = write!(
            self.defs,
            "<{tag} id=\"{id}\" gradientUnits=\"userSpaceOnUse\" {geometry}>"
        );
        for (offset, color) in stops {
            let _ = write!(
                self.defs,
                "<stop offset=\"{offset}\" stop-color=\"{color}\"/>"
            );
        }
        let _ = write!(self.defs, "</{tag}>");
        Ok(GradientPaint {
            id,
            tag,
            matrix: transform,
        })
    }
    pub(super) fn pattern(
        &mut self,
        name: &[u8],
        resources: &Dictionary,
        base: [f32; 6],
    ) -> Result<Option<GradientPaint>, ImportError> {
        let result = (|| {
            let object = resource(self.doc, resources, b"Pattern", name)?;
            let dict = match object {
                Object::Dictionary(dict) => dict,
                Object::Stream(stream) => &stream.dict,
                _ => return Err(malformed()),
            };
            if num(dict.get(b"PatternType").map_err(|_| malformed())?)? != 2.
                || dict.get(b"ExtGState").is_ok()
            {
                return Err(ImportError::Unsupported("pdf.pattern_conversion"));
            }
            let m = match dict.get(b"Matrix") {
                Ok(m) => nums(
                    resolve(self.doc, m)?.as_array().map_err(|_| malformed())?,
                    6,
                )?
                .try_into()
                .unwrap(),
                Err(_) => [1., 0., 0., 1., 0., 0.],
            };
            self.shading(
                dict.get(b"Shading").map_err(|_| malformed())?,
                resources,
                matrix(base, m),
            )
        })();
        match result {
            Err(ImportError::Unsupported(code)) => {
                self.unsupported(code)?;
                Ok(None)
            }
            other => other.map(Some),
        }
    }
    pub(super) fn shading_draw(
        &mut self,
        name: &[u8],
        resources: &Dictionary,
        state: &State,
    ) -> Result<(), ImportError> {
        let result = self.shading(
            resource(self.doc, resources, b"Shading", name)?,
            resources,
            state.ctm,
        );
        let paint = match result {
            Err(ImportError::Unsupported(code)) => {
                self.unsupported(code)?;
                return Ok(());
            }
            other => other?,
        };
        let paint = self.gradient_paint(&Some(paint), "none", [1., 0., 0., 1., 0., 0.])?;
        let [x, y, right, top] = self.page_bounds;
        self.draw(format!("<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" fill=\"{paint}\" opacity=\"{}\" style=\"mix-blend-mode:{}\"/>",right-x,top-y,state.fill_alpha,state.blend),state);
        Ok(())
    }
    pub(super) fn gradient_paint(
        &mut self,
        paint: &Option<GradientPaint>,
        fallback: &str,
        path_matrix: [f32; 6],
    ) -> Result<String, ImportError> {
        let Some(paint) = paint else {
            return Ok(fallback.into());
        };
        let inverse = match inverse(path_matrix) {
            Err(ImportError::Unsupported(code)) => {
                self.unsupported(code)?;
                return Ok("none".into());
            }
            other => other?,
        };
        let [a, b, c, d, e, f] = matrix(inverse, paint.matrix);
        let id = format!("pdf-paint-{}", self.serial);
        self.serial += 1;
        let _ = write!(
            self.defs,
            "<{} id=\"{id}\" href=\"#{}\" gradientTransform=\"matrix({a} {b} {c} {d} {e} {f})\"/>",
            paint.tag, paint.id
        );
        Ok(format!("url(#{id})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    fn parser(doc: &lopdf::Document) -> Interpreter<'_> {
        Interpreter {
            doc,
            body: String::new(),
            defs: String::new(),
            issues: vec![],
            remaining: LIMIT,
            operations: 0,
            serial: 0,
            allow_lossy: false,
            page_bounds: [0., 0., 100., 80.],
            font_cache: Default::default(),
            text_glyphs: 0,
            glyph_depth: 0,
            image_remaining: 64 * 1024 * 1024,
            image_cache: Default::default(),
            glyph_uses: Default::default(),
        }
    }
    fn function() -> Object {
        dictionary! {"FunctionType"=>2,"Domain"=>vec![0.into(),1.into()],"N"=>1,"C0"=>vec![1.into(),0.into(),0.into()],"C1"=>vec![0.into(),0.into(),1.into()]}.into()
    }
    fn shading() -> Object {
        dictionary! {"ShadingType"=>2,"ColorSpace"=>"DeviceRGB","Coords"=>vec![0.into(),0.into(),100.into(),0.into()],"Extend"=>vec![true.into(),true.into()],"Function"=>function()}.into()
    }
    #[test]
    fn exponential_samples_bound_error_and_work() {
        for n in [0.1_f32, 0.5, 1., 2., 8., 100.] {
            let samples = exponential_samples(n).unwrap();
            assert!(samples.len() <= 256);
            assert_eq!(samples.first(), Some(&(0., 0.)));
            assert_eq!(samples.last(), Some(&(1., 1.)));
            for pair in samples.windows(2) {
                let [(a, ya), (b, yb)] = [pair[0], pair[1]];
                assert!(a < b);
                for i in 0..=100 {
                    let f = i as f32 / 100.;
                    let x = a + f * (b - a);
                    assert!((x.powf(n) - (ya + f * (yb - ya))).abs() < 0.00098);
                }
            }
        }
        assert!(exponential_samples(0.).is_err());
        assert!(exponential_samples(-1.).is_err());
        assert!(exponential_samples(f32::MAX).is_err());
        assert!(exponential_samples(f32::NAN).is_err());
        assert!(exponential_samples(1e7).is_err());
    }
    #[test]
    fn nonlinear_rgb_shading_preserves_midpoint_and_stitched_functions() {
        let doc = lopdf::Document::new();
        let mut f = function().as_dict().unwrap().clone();
        f.set("N", 2);
        let mut p = parser(&doc);
        let stops = p.gradient_stops(&f.clone().into(), 3, 0).unwrap();
        assert!(stops.contains(&(0.5, "#bf0040".into())));
        let stitched: Object = dictionary! {"FunctionType"=>3,"Domain"=>vec![0.into(),1.into()],
        "Functions"=>vec![Object::Dictionary(f.clone()), Object::Dictionary(f.clone())],
        "Bounds"=>vec![0.5.into()],"Encode"=>vec![0.into(),1.into(),0.into(),1.into()]}
        .into();
        let stops = p.gradient_stops(&stitched, 3, 0).unwrap();
        assert!(stops.contains(&(0.25, "#bf0040".into())));
        assert!(stops.contains(&(0.75, "#bf0040".into())));
        let mut s = shading().as_dict().unwrap().clone();
        s.set("Function", f);
        for kind in [2, 3] {
            s.set("ShadingType", kind);
            if kind == 3 {
                s.set(
                    "Coords",
                    vec![0.into(), 0.into(), 0.into(), 0.into(), 0.into(), 100.into()],
                );
            }
            let resources = dictionary! {"Shading"=>dictionary! {"S"=>s.clone()}};
            p.shading_draw(b"S", &resources, &State::default()).unwrap();
        }
        assert!(p.defs.contains("<linearGradient"));
        assert!(p.defs.contains("<radialGradient"));
        assert!(p.issues.is_empty());
    }
    #[test]
    fn direct_shading_obeys_current_transform_clip_and_opacity() {
        let doc = lopdf::Document::new();
        let resources = dictionary! {"Shading"=>dictionary! {"S"=>shading()}};
        let mut p = parser(&doc);
        p.content(
            b"0 0 50 30 re W n 1 0 0 1 7 9 cm /S sh",
            &resources,
            State {
                fill_alpha: 0.4,
                ..Default::default()
            },
            0,
        )
        .unwrap();
        assert!(p.defs.contains("matrix(1 0 0 1 7 9)"));
        assert!(p.body.contains("clip-path="));
        assert!(p.body.contains("opacity=\"0.4\""));
        assert!(p.body.contains("width=\"100\" height=\"80\""));
    }
    #[test]
    fn invalid_exponents_and_nonextended_shadings_require_consent() {
        let doc = lopdf::Document::new();
        for nonlinear in [true, false] {
            let mut s = shading().as_dict().unwrap().clone();
            if nonlinear {
                let mut f = function().as_dict().unwrap().clone();
                f.set("N", -2);
                s.set("Function", f);
            } else {
                s.set("Extend", vec![false.into(), true.into()]);
            }
            let resources = dictionary! {"Pattern"=>dictionary! {"P"=>dictionary! {"PatternType"=>2,"Shading"=>s}}};
            assert!(matches!(
                parser(&doc).pattern(b"P", &resources, [1., 0., 0., 1., 0., 0.]),
                Err(ImportError::LossyConversionRequiresConsent(_))
            ));
            let mut p = parser(&doc);
            p.allow_lossy = true;
            assert!(p
                .pattern(b"P", &resources, [1., 0., 0., 1., 0., 0.])
                .unwrap()
                .is_none());
            assert_eq!(p.issues.len(), 1);
        }
    }
    #[test]
    fn tiling_pattern_stream_is_reported_as_unsupported_not_malformed() {
        let doc = lopdf::Document::new();
        let resources = dictionary! {"Pattern"=>dictionary! {"P"=>lopdf::Stream::new(dictionary! {"PatternType"=>1},vec![])}};
        assert!(matches!(
            parser(&doc).pattern(b"P", &resources, [1., 0., 0., 1., 0., 0.]),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
    }
    #[test]
    fn mesh_shading_stream_requires_consent_without_missing_function_errors() {
        let doc = lopdf::Document::new();
        let resources = dictionary! {"Shading"=>dictionary! {"S"=>lopdf::Stream::new(dictionary! {"ShadingType"=>4},vec![])}};
        let mut p = parser(&doc);
        assert!(matches!(
            p.shading_draw(b"S", &resources, &State::default()),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
    }
    #[test]
    fn recursive_functions_and_excessive_stop_counts_are_bounded() {
        let doc = lopdf::Document::new();
        let mut f = function();
        for _ in 0..10 {
            f=dictionary! {"FunctionType"=>3,"Domain"=>vec![0.into(),1.into()],"Functions"=>vec![f],"Bounds"=>Vec::<Object>::new(),"Encode"=>vec![0.into(),1.into()]}.into();
        }
        assert!(matches!(
            parser(&doc).gradient_stops(&f, 3, 0),
            Err(ImportError::LimitExceeded("pdf.gradient_function_depth"))
        ));
        let f:Object=dictionary! {"FunctionType"=>3,"Domain"=>vec![0.into(),1.into()],"Functions"=>vec![function();129]}.into();
        assert!(matches!(
            parser(&doc).gradient_stops(&f, 3, 0),
            Err(ImportError::LimitExceeded("pdf.gradient_stops"))
        ));
    }
}
