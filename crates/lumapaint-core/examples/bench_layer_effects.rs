//! Release microbenchmark; includes plan construction, excludes rendering/I/O.
use lumapaint_core::layer_effects::LayerEffects;
use std::{hint::black_box, time::Instant};
fn main() {
    let pixels: Vec<[u8; 4]> = (0..262144u32)
        .map(|i| {
            let v = i.wrapping_mul(2654435761);
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
        })
        .collect();
    for name in ["tone", "curves", "mixed"] {
        let mut e = LayerEffects {
            enabled: true,
            values: [1.2, 25., -35., 20., 10., -15., 30., -20., 0., 0.],
            ..Default::default()
        };
        if name != "tone" {
            e.curves = std::array::from_fn(|_| {
                vec![[0., 0.], [0.12, 0.22], [0.4, 0.33], [0.75, 0.88], [1., 1.]]
            });
        }
        if name == "mixed" {
            e.values[8] = 25.;
            e.values[9] = -15.;
            e.mixer[0] = [30., 25., -15.];
            e.grading = [[30., 20., -5.], [210., 10., 5.], [300., 15., 0.]];
        }
        e.validate().unwrap();
        let run = |prepared: bool| {
            let started = Instant::now();
            let plan = prepared.then(|| black_box(&e).prepare());
            let sum: u64 = pixels
                .iter()
                .map(|p| {
                    let rgba = if let Some(plan) = &plan {
                        plan.apply(black_box(*p))
                    } else {
                        black_box(&e).apply(black_box(*p))
                    };
                    rgba.into_iter().map(u64::from).sum::<u64>()
                })
                .sum();
            (started.elapsed().as_secs_f64() * 1000., black_box(sum))
        };
        assert_eq!(run(false).1, run(true).1);
        let mut original = Vec::new();
        let mut prepared = Vec::new();
        for _ in 0..5 {
            let a = run(false);
            let b = run(true);
            assert_eq!(a.1, b.1);
            original.push(a.0);
            prepared.push(b.0);
        }
        original.sort_by(f64::total_cmp);
        prepared.sort_by(f64::total_cmp);
        println!(
            "{name}: pixels={} reference_median_ms={:.3} prepared_median_ms={:.3} speedup={:.2}x",
            pixels.len(),
            original[2],
            prepared[2],
            original[2] / prepared[2]
        );
    }
}
