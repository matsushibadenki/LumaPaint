//! Editable motion programs. Evaluation is a pure function of saved parameters
//! and frame time; reference-site code, HTML and demo artwork are not embedded.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    pub reference: String,
    pub categories: Vec<Category>,
    pub presets: Vec<Preset>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Category {
    pub id: String,
    pub names: BTreeMap<String, String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub category: String,
    pub names: BTreeMap<String, String>,
    pub duration_ms: u32,
    pub curve: Curve,
    pub operations: Vec<Operation>,
    pub stagger: Order,
    pub exit: bool,
    pub cyclic: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actors: Vec<Actor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Actor {
    pub keys: Vec<ActorKey>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorKey {
    pub offset: f32,
    pub values: BTreeMap<String, f32>,
}
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("motion/catalog.json"))
            .expect("validated built-in motion catalog")
    })
}
pub fn preset(id: &str) -> Option<&'static Preset> {
    static INDEX: OnceLock<BTreeMap<&'static str, usize>> = OnceLock::new();
    let index = INDEX.get_or_init(|| {
        catalog()
            .presets
            .iter()
            .enumerate()
            .map(|(i, p)| (p.id.as_str(), i))
            .collect()
    });
    index.get(id).map(|i| &catalog().presets[*i])
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Curve {
    Linear,
    Power {
        exponent: f32,
        mode: EaseMode,
    },
    Sine {
        mode: EaseMode,
    },
    Expo {
        mode: EaseMode,
    },
    Circular {
        mode: EaseMode,
    },
    Back {
        overshoot: f32,
        mode: EaseMode,
    },
    Elastic {
        amplitude: f32,
        period: f32,
    },
    Bounce {
        restitution: f32,
    },
    Bezier {
        points: [f32; 4],
    },
    Spring {
        stiffness: f32,
        damping: f32,
        mass: f32,
    },
    Steps {
        count: u32,
    },
    Rough {
        strength: f32,
        points: u32,
    },
    SlowMo {
        plateau: f32,
    },
    ExpoScale {
        ratio: f32,
    },
    Wiggle {
        cycles: f32,
    },
    Gravity,
    Inertia {
        decay: f32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EaseMode {
    In,
    Out,
    InOut,
}
impl Default for Curve {
    fn default() -> Self {
        Self::Power {
            exponent: 3.,
            mode: EaseMode::Out,
        }
    }
}
impl Curve {
    pub fn validate(self) -> bool {
        match self {
            Self::Linear | Self::Gravity => true,
            Self::Power { exponent, .. } => exponent.is_finite() && (1. ..=8.).contains(&exponent),
            Self::Sine { .. } | Self::Expo { .. } | Self::Circular { .. } => true,
            Self::Back { overshoot, .. } => {
                overshoot.is_finite() && (0. ..=8.).contains(&overshoot)
            }
            Self::Elastic { amplitude, period } => {
                amplitude.is_finite()
                    && (1. ..=5.).contains(&amplitude)
                    && period.is_finite()
                    && (0.05..=2.).contains(&period)
            }
            Self::Bounce { restitution } => {
                restitution.is_finite() && (0.1..=0.95).contains(&restitution)
            }
            Self::Bezier {
                points: [x1, y1, x2, y2],
            } => {
                [x1, y1, x2, y2].iter().all(|x| x.is_finite())
                    && (0. ..=1.).contains(&x1)
                    && (0. ..=1.).contains(&x2)
                    && y1.abs() <= 8.
                    && y2.abs() <= 8.
            }
            Self::Spring {
                stiffness,
                damping,
                mass,
            } => {
                stiffness.is_finite()
                    && (1. ..=2000.).contains(&stiffness)
                    && damping.is_finite()
                    && (0.1..=200.).contains(&damping)
                    && mass.is_finite()
                    && (0.1..=20.).contains(&mass)
            }
            Self::Steps { count } => (1..=120).contains(&count),
            Self::Rough { strength, points } => {
                strength.is_finite()
                    && (0. ..=2.).contains(&strength)
                    && (2..=120).contains(&points)
            }
            Self::SlowMo { plateau } => plateau.is_finite() && (0.05..=0.95).contains(&plateau),
            Self::ExpoScale { ratio } => ratio.is_finite() && (1.01..=100.).contains(&ratio),
            Self::Wiggle { cycles } => cycles.is_finite() && (1. ..=30.).contains(&cycles),
            Self::Inertia { decay } => decay.is_finite() && (0.1..=30.).contains(&decay),
        }
    }
    pub fn sample(self, progress: f32, seconds: f32, seed: u32) -> f32 {
        let t = progress.clamp(0., 1.);
        if t == 0. {
            return 0.;
        }
        if t == 1. {
            return 1.;
        }
        let mode = |f: &dyn Fn(f32) -> f32, m: EaseMode| match m {
            EaseMode::In => f(t),
            EaseMode::Out => 1. - f(1. - t),
            EaseMode::InOut => {
                if t < 0.5 {
                    f(2. * t) * 0.5
                } else {
                    1. - f(2. - 2. * t) * 0.5
                }
            }
        };
        match self {
            Self::Linear => t,
            Self::Power { exponent, mode: m } => mode(&|p| p.powf(exponent), m),
            Self::Sine { mode: m } => mode(&|p| 1. - (p * std::f32::consts::FRAC_PI_2).cos(), m),
            Self::Expo { mode: m } => mode(&|p| 2_f32.powf(10. * (p - 1.)), m),
            Self::Circular { mode: m } => mode(&|p| 1. - (1. - p * p).max(0.).sqrt(), m),
            Self::Back {
                overshoot: s,
                mode: m,
            } => mode(&|p| p * p * ((s + 1.) * p - s), m),
            Self::Elastic {
                amplitude: a,
                period: p,
            } => {
                let phase = p / std::f32::consts::TAU * (1. / a).asin();
                a * 2_f32.powf(-10. * t) * ((t - phase) * std::f32::consts::TAU / p).sin() + 1.
            }
            Self::Bounce { restitution: r } => {
                // Successive ballistic flights have geometrically decreasing height.
                let mut spans = Vec::with_capacity(8);
                let mut span = 1.;
                let mut total = span;
                for _ in 0..6 {
                    span *= r;
                    total += 2. * span;
                    spans.push(span);
                }
                let mut x = t * total;
                if x <= 1. {
                    return x * x;
                }
                x -= 1.;
                for span in spans {
                    if x <= 2. * span {
                        let u = x / (2. * span);
                        return 1. - 4. * span * span * u * (1. - u);
                    }
                    x -= 2. * span;
                }
                1.
            }
            Self::Bezier {
                points: [x1, y1, x2, y2],
            } => {
                let b = |p: f32, a: f32, b: f32| {
                    3. * (1. - p).powi(2) * p * a + 3. * (1. - p) * p * p * b + p.powi(3)
                };
                let (mut lo, mut hi) = (0., 1.);
                for _ in 0..24 {
                    let m = (lo + hi) * 0.5;
                    if b(m, x1, x2) < t {
                        lo = m
                    } else {
                        hi = m
                    }
                }
                b((lo + hi) * 0.5, y1, y2)
            }
            Self::Spring {
                stiffness: k,
                damping: c,
                mass: m,
            } => {
                let w = (k / m).sqrt();
                let z = c / (2. * (k * m).sqrt());
                let time = seconds.max(0.);
                if (z - 1.).abs() < 0.0001 {
                    1. - (-w * time).exp() * (1. + w * time)
                } else if z < 1. {
                    let wd = w * (1. - z * z).sqrt();
                    1. - (-z * w * time).exp()
                        * ((wd * time).cos() + z * w / wd * (wd * time).sin())
                } else {
                    let root = (z * z - 1.).sqrt();
                    let r1 = -w * (z - root);
                    let r2 = -w * (z + root);
                    1. - (r2 * (r1 * time).exp() - r1 * (r2 * time).exp()) / (r2 - r1)
                }
            }
            Self::Steps { count } => (t * count as f32).floor() / count as f32,
            Self::Rough { strength, points } => {
                let p = t * points as f32;
                let i = p.floor() as u32;
                let a = random(seed, i);
                let b = random(seed, i + 1);
                t + strength * (a + (b - a) * p.fract() - 0.5) * t * (1. - t)
            }
            Self::SlowMo { plateau } => {
                let edge = (1. - plateau) * 0.5;
                if t < edge {
                    0.45 * (t / edge).powi(3)
                } else if t > 1. - edge {
                    1. - 0.45 * ((1. - t) / edge).powi(3)
                } else {
                    0.45 + 0.1 * (t - edge) / plateau
                }
            }
            Self::ExpoScale { ratio } => (ratio.powf(t) - 1.) / (ratio - 1.),
            Self::Wiggle { cycles } => {
                t + (t * std::f32::consts::TAU * cycles).sin() * (1. - t) * 0.15
            }
            Self::Gravity => t * t,
            Self::Inertia { decay } => (1. - (-decay * t).exp()) / (1. - (-decay).exp()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Operation {
    Fade,
    SlideX,
    SlideY,
    Travel,
    Arc,
    Orbit,
    Zigzag,
    Hop,
    Anticipate,
    Recoil,
    Shake,
    Scale,
    ScaleX,
    ScaleY,
    Pulse,
    Squash,
    Jelly,
    Spin,
    Swing,
    MeshPlane,
    Cube,
    DepthStack,
    Coverflow,
    FlipX,
    FlipY,
    Tilt,
    Skew,
    Parallax,
    Reorder,
    Gather,
    Scatter,
    Follow,
    Wave,
    Hold,
    Cut,
    Camera,
    Crane,
    Wipe,
    Diagonal,
    Iris,
    Split,
    Blinds,
    Blocks,
    Dissolve,
    ClipWindow,
    Blur,
    Glow,
    Sweep,
    Hue,
    Tint,
    Desaturate,
    Chromatic,
    Noise,
    Displace,
    Shadow,
    StrokeDraw,
    StrokeTrim,
    Morph,
    Repeat,
    Particles,
    Confetti,
    Snow,
    Embers,
    Fireworks,
    Coalesce,
    Typewriter,
    Scramble,
    TextSwap,
    Counter,
    Tracking,
    FontAxes,
    KanjiStroke,
    Weight,
    Underline,
    Highlight,
    Caption,
    Lines,
    Marquee,
    Walk,
    Blink,
    Tail,
    AudioPulse,
    AudioDuck,
    Beat,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Order {
    #[default]
    Forward,
    Reverse,
    Center,
    Edges,
    Random,
    Grid,
    Alternating,
    Spiral,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AxisMotion {
    pub tag: String,
    pub from: f32,
    pub to: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[serde(default)]
pub struct Clip {
    pub id: String,
    pub preset: String,
    pub enabled: bool,
    pub start: u32,
    pub duration: u32,
    pub repeats: u32,
    pub reverse: bool,
    pub yoyo: bool,
    pub strength: f32,
    pub distance: f32,
    pub angle: f32,
    pub frequency: f32,
    pub stagger: f32,
    pub order: Order,
    pub pivot: [f32; 2],
    pub seed: u32,
    pub curve: Option<Curve>,
    pub objects: Vec<String>,
    pub particle_count: u32,
    pub copies: u32,
    pub blur: f32,
    pub glow: f32,
    pub color: [u8; 4],
    pub alternate_text: String,
    pub number_from: f64,
    pub number_to: f64,
    pub path: Vec<[f32; 2]>,
    pub morph_target: String,
    pub variable_font: String,
    pub font_axes: Vec<AxisMotion>,
    pub stroke_gap_ms: f32,
    pub stroke_width: f32,
    pub mesh_depth: f32,
    pub camera_distance: f32,
    pub rotation_x: f32,
    pub rotation_y: f32,
    pub rotation_z: f32,
}
impl Default for Clip {
    fn default() -> Self {
        Self {
            id: String::new(),
            preset: "fade-slide".into(),
            enabled: true,
            start: 0,
            duration: 24,
            repeats: 1,
            reverse: false,
            yoyo: false,
            strength: 1.,
            distance: 80.,
            angle: 180.,
            frequency: 2.,
            stagger: 0.08,
            order: Order::Forward,
            pivot: [0.5, 0.5],
            seed: 1,
            curve: None,
            objects: vec![],
            particle_count: 24,
            copies: 4,
            blur: 12.,
            glow: 12.,
            color: [100, 140, 255, 255],
            alternate_text: String::new(),
            number_from: 0.,
            number_to: 100.,
            path: vec![],
            morph_target: String::new(),
            variable_font: String::new(),
            font_axes: vec![],
            stroke_gap_ms: 100.,
            stroke_width: 4.,
            mesh_depth: 80.,
            camera_distance: 600.,
            rotation_x: -20.,
            rotation_y: 25.,
            rotation_z: 0.,
        }
    }
}
impl Clip {
    pub fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.id.len()
            + self.preset.len()
            + self.alternate_text.len()
            + self.morph_target.len()
            + self.variable_font.len()
            + self
                .font_axes
                .iter()
                .map(|axis| std::mem::size_of::<AxisMotion>() + axis.tag.len())
                .sum::<usize>()
            + self.path.len() * std::mem::size_of::<[f32; 2]>()
            + self
                .objects
                .iter()
                .map(|id| std::mem::size_of::<String>() + id.len())
                .sum::<usize>()
    }
    pub fn validate(&self, timeline_duration: u32) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 128
            || preset(&self.preset).is_none()
            || !(1..=18000).contains(&self.duration)
            || !(1..=1000).contains(&self.repeats)
            || self
                .start
                .checked_add(self.duration.saturating_mul(self.repeats))
                .is_none_or(|end| end > timeline_duration)
            || !self.strength.is_finite()
            || !(0. ..=4.).contains(&self.strength)
            || !self.distance.is_finite()
            || self.distance.abs() > 8192.
            || !self.angle.is_finite()
            || self.angle.abs() > 36000.
            || !self.frequency.is_finite()
            || !(0.1..=30.).contains(&self.frequency)
            || !self.stagger.is_finite()
            || !(0. ..=0.95).contains(&self.stagger)
            || self
                .pivot
                .iter()
                .any(|v| !v.is_finite() || !(-2. ..=3.).contains(v))
            || self.particle_count > 128
            || !(1..=24).contains(&self.copies)
            || !self.blur.is_finite()
            || !(0. ..=64.).contains(&self.blur)
            || !self.glow.is_finite()
            || !(0. ..=64.).contains(&self.glow)
            || self.curve.is_some_and(|c| !c.validate())
            || self.objects.len() > 512
            || self
                .objects
                .iter()
                .any(|id| id.is_empty() || id.len() > 256)
            || self
                .objects
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.objects.len()
            || self.alternate_text.len() > 8192
            || self.morph_target.len() > 32768
            || self.variable_font.len() > 200
            || self.variable_font.chars().any(char::is_control)
            || self.font_axes.len() > 16
            || self.font_axes.iter().any(|a| {
                !crate::vector::valid_font_axes(&std::collections::BTreeMap::from([(
                    a.tag.clone(),
                    a.from,
                )])) || !a.to.is_finite()
                    || a.to.abs() > 100000.
            })
            || self
                .font_axes
                .iter()
                .map(|a| &a.tag)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.font_axes.len()
            || !self.stroke_gap_ms.is_finite()
            || !(0. ..=1000.).contains(&self.stroke_gap_ms)
            || !self.stroke_width.is_finite()
            || !(0.5..=20.).contains(&self.stroke_width)
            || ![
                self.mesh_depth,
                self.camera_distance,
                self.rotation_x,
                self.rotation_y,
                self.rotation_z,
            ]
            .iter()
            .all(|v| v.is_finite())
            || !(0. ..=4096.).contains(&self.mesh_depth)
            || !(64. ..=8192.).contains(&self.camera_distance)
            || [self.rotation_x, self.rotation_y, self.rotation_z]
                .iter()
                .any(|v| v.abs() > 36000.)
            || !self.number_from.is_finite()
            || !self.number_to.is_finite()
            || self.number_from.abs() > 1e12
            || self.number_to.abs() > 1e12
            || self.path.len() > 256
            || self
                .path
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 65536.)
        {
            return Err(
                "Invalid motion parameters / モーションのパラメータが無効です / 动效参数无效"
                    .into(),
            );
        }
        if !self.morph_target.is_empty() {
            let contours = crate::bezier::cubic_contours(&self.morph_target).map_err(|_| {
                "Invalid morph target / モーフ先のパスが無効です / 变形目标路径无效"
            })?;
            if contours.is_empty()
                || contours.len() > 32
                || contours
                    .iter()
                    .map(|(points, _)| points.len())
                    .sum::<usize>()
                    > 2048
                || contours
                    .iter()
                    .flat_map(|(points, _)| points.iter().flatten())
                    .any(|v| !v.is_finite() || v.abs() > 65536.)
            {
                return Err("Morph target exceeds the path budget / モーフ先のパスが上限を超えます / 变形目标路径超过预算".into());
            }
        }
        Ok(())
    }
    pub fn progress(&self, frame: u32, index: usize, count: usize) -> Option<f32> {
        if !self.enabled || frame < self.start {
            return None;
        }
        let elapsed = frame - self.start;
        let finished = elapsed >= self.duration * self.repeats - 1;
        let iteration = if finished {
            self.repeats - 1
        } else {
            elapsed / self.duration
        };
        let cycle = if finished || self.duration == 1 {
            1.
        } else {
            (elapsed % self.duration) as f32 / (self.duration - 1) as f32
        };
        let n = count.saturating_sub(1).max(1) as f32;
        let i = index as f32;
        let rank = match self.order {
            Order::Forward => i / n,
            Order::Reverse => 1. - i / n,
            Order::Center => (i / n - 0.5).abs() * 2.,
            Order::Edges => 1. - (i / n - 0.5).abs() * 2.,
            Order::Random => random(self.seed, index as u32),
            Order::Grid => {
                let cols = (count as f32).sqrt().ceil().max(1.) as usize;
                ((index / cols + index % cols) as f32 / (cols as f32 * 2. - 2.).max(1.))
                    .clamp(0., 1.)
            }
            Order::Alternating => {
                if index.is_multiple_of(2) {
                    i / n * 0.5
                } else {
                    0.5 + i / n * 0.5
                }
            }
            Order::Spiral => (i / n).sqrt(),
        };
        let stagger = if count > 1 { self.stagger } else { 0. };
        let p = ((cycle - rank * stagger) / (1. - stagger)).clamp(0., 1.);
        Some(if self.reverse ^ (self.yoyo && iteration % 2 == 1) {
            1. - p
        } else {
            p
        })
    }
}

pub fn default_axes(id: &str) -> Vec<AxisMotion> {
    let make = |tag: &str, from, to| AxisMotion {
        tag: tag.into(),
        from,
        to,
    };
    if id.contains("width-slant") {
        vec![make("wdth", 75., 125.), make("slnt", 0., -10.)]
    } else if id.contains("width") || id.contains("pressure") {
        vec![make("wdth", 75., 125.)]
    } else if id.contains("slant") {
        vec![make("slnt", 0., -10.)]
    } else {
        vec![make("wght", 200., 900.)]
    }
}

pub(crate) fn random(seed: u32, index: u32) -> f32 {
    let mut n = seed.wrapping_add(index.wrapping_mul(0x9e3779b9));
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846ca68b);
    n ^= n >> 16;
    n as f64 as f32 / u32::MAX as f32
}

#[path = "motion/kanji.rs"]
pub mod kanji;

#[path = "motion/mesh.rs"]
pub mod mesh;

#[path = "motion/evaluate.rs"]
mod evaluate;
pub(crate) use evaluate::apply;
pub type Outline<'a> = dyn Fn(&str, &crate::vector::VectorObject) -> Result<Vec<crate::vector::VectorObject>, String>
    + 'a;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Document, TextSettings},
        vector::{VectorObject, VectorText},
    };
    fn artwork() -> (Document, String) {
        let mut doc = Document::default();
        let layer = doc.add_vector_layer().unwrap();
        for (i, x) in [20., 100.].into_iter().enumerate() {
            let object:VectorObject=serde_json::from_value(serde_json::json!({"id":format!("shape-{i}"),"name":"Shape","path":{"data":"M0 0H40V40H0Z","fillRule":"nonZero"},"kind":"rectangle","controlPoints":[[0,0],[40,40]],"transform":[1,0,0,1,x,20],"fill":{"color":[200,40,60,255]},"stroke":null,"strokeWidth":0,"visible":true})).unwrap();
            doc.upsert_vector_object(&layer, object).unwrap();
        }
        (doc, layer)
    }
    #[test]
    fn target_path_morph_preserves_endpoints_and_rejects_bad_topology_atomically() {
        let (mut doc, layer) = artwork();
        let clip = Clip {
            id: "target".into(),
            preset: "svg-path-morph".into(),
            duration: 60,
            objects: vec!["shape-0".into()],
            morph_target: "M0 0H80V20H0Z".into(),
            curve: Some(Curve::Linear),
            ..Default::default()
        };
        doc.set_motion_clip(&layer, clip.clone()).unwrap();
        let mut preview = doc.clone_for_rendering();
        preview.evaluate_animation(&doc, 59);
        let object = &preview
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .vector_objects[0];
        let bounds = crate::stroke::path_bounds(&object.path.data).unwrap();
        assert!((bounds[2] - 80.).abs() < 0.01 && (bounds[3] - 20.).abs() < 0.01);
        let saved = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc
            .set_motion_clip(
                &layer,
                Clip {
                    morph_target: "M0 0H5V5Z M10 10H15V15Z".into(),
                    ..clip
                }
            )
            .is_err());
        assert_eq!(saved, serde_json::to_value(doc.document_state()).unwrap());
    }
    #[test]
    fn trail_expansion_budget_rejects_atomically_and_end_frame_keeps_original() {
        let (mut doc, layer) = artwork();
        for i in 0..3 {
            doc.set_motion_clip(
                &layer,
                Clip {
                    id: format!("trail-{i}"),
                    preset: "sh-repeater".into(),
                    duration: 60,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let saved = serde_json::to_value(doc.document_state()).unwrap();
        assert!(doc
            .set_motion_clip(
                &layer,
                Clip {
                    id: "overflow".into(),
                    preset: "sh-repeater".into(),
                    duration: 60,
                    ..Default::default()
                }
            )
            .is_err());
        assert_eq!(saved, serde_json::to_value(doc.document_state()).unwrap());
        let mut preview = doc.clone_for_rendering();
        preview.evaluate_animation(&doc, 20);
        let source = &preview.svg_layers().find(|l| l.id == layer).unwrap().source;
        assert!(source.len() < 32_768);
        assert!(source.contains("<use href="));
        roxmltree::Document::parse(source).unwrap();
    }
    #[test]
    fn all_reference_types_are_unique_localized_valid_and_numerically_bounded() {
        let catalog = catalog();
        assert_eq!(catalog.presets.len(), 782);
        assert_eq!(catalog.categories.len(), 64);
        let mut ids = std::collections::BTreeSet::new();
        for preset in &catalog.presets {
            assert!(ids.insert(&preset.id));
            assert!(!preset.operations.is_empty());
            assert!(preset.curve.validate(), "{}: {:?}", preset.id, preset.curve);
            assert!(catalog.categories.iter().any(|c| c.id == preset.category));
            for locale in ["ja", "en", "zh-CN"] {
                assert!(!preset.names[locale].is_empty());
            }
            for i in 0..=100 {
                let p = i as f32 / 100.;
                let sample = preset.curve.sample(p, p * 3., 42);
                assert!(
                    sample.is_finite() && sample.abs() < 32.,
                    "{} {sample}",
                    preset.id
                );
            }
            for actor in &preset.actors {
                assert!(actor.keys.len() >= 2);
                assert!(actor.keys.windows(2).all(|w| w[0].offset < w[1].offset));
                for key in &actor.keys {
                    assert!((0. ..=1.).contains(&key.offset));
                    assert!(key.values.values().all(|v| v.is_finite() && v.abs() < 1e6));
                }
            }
        }
    }
    #[test]
    fn every_type_produces_deterministic_valid_svg_without_editing_the_source() {
        let (mut source, shape_layer) = artwork();
        source
            .set_text_object(TextSettings {
                id: None,
                text: VectorText {
                    content: "English 日本語 中文 👩‍👩‍👦 é".into(),
                    ..Default::default()
                },
                position: [10., 100.],
                color: [32, 32, 32],
            })
            .unwrap();
        let text_layer = source
            .svg_layers()
            .find(|l| l.vector_objects.iter().any(|o| o.text.is_some()))
            .unwrap()
            .id
            .clone();
        let saved = serde_json::to_value(source.document_state()).unwrap();
        for preset in &catalog().presets {
            let mut doc = source.clone();
            let layer = if preset.category.starts_with("tx-")
                || preset.operations.iter().any(|op| {
                    matches!(
                        op,
                        Operation::Counter
                            | Operation::TextSwap
                            | Operation::Typewriter
                            | Operation::Scramble
                    )
                }) {
                &text_layer
            } else {
                &shape_layer
            };
            let clip = Clip {
                id: "motion".into(),
                preset: preset.id.clone(),
                duration: 60,
                alternate_text: "次の文字 Next 简体中文".into(),
                ..Default::default()
            };
            doc.set_motion_clip(layer, clip).unwrap();
            let mut a = doc.clone_for_rendering();
            let mut b = doc.clone_for_rendering();
            a.evaluate_animation(&doc, 23);
            b.evaluate_animation(&doc, 23);
            let aa = a.svg_layers().find(|l| &l.id == layer).unwrap();
            let bb = b.svg_layers().find(|l| &l.id == layer).unwrap();
            assert_eq!(aa.source, bb.source, "{}", preset.id);
            assert!(
                !aa.source.contains("NaN") && !aa.source.contains("inf"),
                "{}",
                preset.id
            );
            roxmltree::Document::parse(&aa.source).unwrap_or_else(|e| panic!("{}: {e}", preset.id));
            assert!(aa.source.len() < 4 * 1024 * 1024);
        }
        assert_eq!(
            serde_json::to_value(source.document_state()).unwrap(),
            saved
        );
    }
    #[test]
    fn clip_edit_reorder_remove_expand_and_undo_are_atomic() {
        let (mut doc, layer) = artwork();
        let before = serde_json::to_value(doc.document_state()).unwrap();
        doc.set_motion_clip(
            &layer,
            Clip {
                id: "a".into(),
                duration: 180,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(doc.animation().duration, 180);
        assert_eq!(doc.animation().tracks[&layer].motions.len(), 1);
        doc.undo();
        assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), before);
        doc.redo();
        doc.set_motion_clip(
            &layer,
            Clip {
                id: "b".into(),
                preset: "orbit".into(),
                ..Default::default()
            },
        )
        .unwrap();
        doc.move_motion_clip(&layer, "b", 0).unwrap();
        assert_eq!(doc.animation().tracks[&layer].motions[0].id, "b");
        doc.undo();
        assert_eq!(doc.animation().tracks[&layer].motions[0].id, "a");
        let valid = serde_json::to_value(doc.document_state()).unwrap();
        let revision = doc.revision();
        for clip in [
            Clip {
                id: "bad".into(),
                distance: f32::NAN,
                ..Default::default()
            },
            Clip {
                id: "bad".into(),
                preset: "unknown".into(),
                ..Default::default()
            },
            Clip {
                id: "bad".into(),
                start: 17999,
                duration: 3,
                ..Default::default()
            },
            Clip {
                id: "bad".into(),
                objects: vec!["missing".into()],
                ..Default::default()
            },
            Clip {
                id: "bad".into(),
                particle_count: 129,
                ..Default::default()
            },
        ] {
            assert!(doc.set_motion_clip(&layer, clip).is_err());
            assert_eq!(serde_json::to_value(doc.document_state()).unwrap(), valid);
            assert_eq!(doc.revision(), revision);
        }
        assert!(doc.set_animation_settings(24, 120, true).is_err());
        doc.remove_motion_clip(&layer, "a").unwrap();
        assert_eq!(doc.animation().tracks[&layer].motions.len(), 1);
        doc.undo();
        assert_eq!(doc.animation().tracks[&layer].motions.len(), 2);
    }
    #[test]
    fn clip_time_endpoints_repeat_reverse_and_stagger_are_exact() {
        let mut c = Clip {
            id: "test".into(),
            start: 10,
            duration: 10,
            repeats: 2,
            yoyo: true,
            stagger: 0.4,
            ..Default::default()
        };
        assert_eq!(c.progress(9, 0, 4), None);
        assert_eq!(c.progress(10, 0, 4), Some(0.));
        assert_eq!(c.progress(19, 3, 4), Some(1.));
        assert_eq!(c.progress(20, 0, 4), Some(1.));
        assert_eq!(c.progress(29, 3, 4), Some(0.));
        assert_eq!(c.progress(100, 3, 4), Some(0.));
        assert!(c.progress(14, 0, 4) > c.progress(14, 3, 4));
        c.duration = 1;
        c.repeats = 1;
        c.yoyo = false;
        assert_eq!(c.progress(10, 0, 1), Some(1.));
        c.reverse = true;
        assert_eq!(c.progress(10, 0, 1), Some(0.));
    }
    #[test]
    fn selected_objects_pivot_and_seed_change_only_the_requested_artwork() {
        let (mut doc, layer) = artwork();
        doc.set_motion_clip(
            &layer,
            Clip {
                id: "test".into(),
                preset: "linear-travel".into(),
                duration: 10,
                distance: 60.,
                objects: vec!["shape-0".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let mut view = doc.clone_for_rendering();
        view.evaluate_animation(&doc, 9);
        let objects = &view
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .vector_objects;
        assert_eq!(objects[0].transform[4], 80.);
        assert_eq!(objects[1].transform[4], 100.);
        let original = doc.svg_layers().find(|l| l.id == layer).unwrap();
        assert_eq!(original.vector_objects[0].transform[4], 20.);
        let c = Clip {
            id: "r".into(),
            preset: "particle-burst".into(),
            duration: 10,
            ..Default::default()
        };
        doc.set_motion_clip(&layer, c.clone()).unwrap();
        view.evaluate_animation(&doc, 5);
        let a = view
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .source
            .clone();
        doc.set_motion_clip(&layer, Clip { seed: 2, ..c }).unwrap();
        view.evaluate_animation(&doc, 5);
        assert_ne!(view.svg_layers().find(|l| l.id == layer).unwrap().source, a);
    }
    #[test]
    fn text_typing_respects_emoji_combining_and_utf16_boundaries() {
        let mut doc = Document::default();
        doc.set_text_object(TextSettings {
            id: None,
            text: VectorText {
                content: "👩‍👩‍👦é日本語简体中文".into(),
                ..Default::default()
            },
            position: [10., 10.],
            color: [32, 32, 32],
        })
        .unwrap();
        let layer = doc.svg_layers().next().unwrap().id.clone();
        doc.set_motion_clip(
            &layer,
            Clip {
                id: "typing".into(),
                preset: "tx-typewriter".into(),
                duration: 10,
                ..Default::default()
            },
        )
        .unwrap();
        let mut view = doc.clone_for_rendering();
        view.evaluate_animation(&doc, 4);
        let text = view
            .svg_layers()
            .find(|l| l.id == layer)
            .unwrap()
            .vector_objects[0]
            .text
            .as_ref()
            .unwrap();
        for run in &text.runs {
            assert!(text.utf16_boundary(run.start) && text.utf16_boundary(run.end));
        }
        assert!(text.runs.iter().any(|run| run.style.no_color));
        assert_eq!(text.runs[0].end, "👩‍👩‍👦".encode_utf16().count());
    }
}
