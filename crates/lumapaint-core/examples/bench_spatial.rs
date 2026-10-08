//! Scene-index microbenchmark, not application object capacity or input-to-present latency.
//! cargo run --release --offline --locked -p lumapaint-core --example bench_spatial -- --samples 1000
use lumapaint_core::scene::spatial::SpatialIndex;
use std::time::Instant;

fn report(count: usize, phase: &str, mut samples: Vec<f64>, visited: usize) {
    samples.sort_by(f64::total_cmp);
    let p = |percent: usize| samples[((samples.len() - 1) * percent).div_ceil(100)];
    println!(
        "{count},{phase},{},{:.3},{:.3},{:.3},{visited}",
        samples.len(),
        p(50),
        p(95),
        p(99)
    );
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let samples: usize = args
        .windows(2)
        .find(|p| p[0] == "--samples")
        .map(|p| p[1].parse().unwrap())
        .unwrap_or(1000);
    assert!(samples > 0);
    println!("objects,phase,samples,p50_us,p95_us,p99_us,visited_nodes");
    for count in [1000, 10_000, 100_000, 1_000_000] {
        let mut tree = SpatialIndex::build((0..count).map(|i| {
            let x = i as f64 * 10.;
            (i, Some([x, 0., x + 5., 5.]))
        }));
        let query = [0., 0., 5115., 5.];
        let mut items = Vec::with_capacity(512);
        let mut stack = Vec::with_capacity(64);
        for _ in 0..64 {
            tree.query_into(query, &mut items, &mut stack);
        }
        let mut timings = Vec::with_capacity(samples);
        let mut visited = 0;
        for _ in 0..samples {
            let start = Instant::now();
            visited = tree.query_into(query, &mut items, &mut stack);
            timings.push(start.elapsed().as_secs_f64() * 1e6);
            assert_eq!(items.len(), 512);
            std::hint::black_box(&items);
        }
        report(count, "query_reused_buffers", timings, visited);
        let mut timings = Vec::with_capacity(samples);
        for _ in 0..samples {
            let start = Instant::now();
            let result = tree.query(query);
            timings.push(start.elapsed().as_secs_f64() * 1e6);
            assert_eq!(result.items, items);
            drop(result);
        }
        report(count, "query_allocated_buffers", timings, visited);
        let mut timings = Vec::with_capacity(samples);
        for i in 0..samples {
            let x = count as f64 * 10. + (i % 2) as f64 * 10.;
            let start = Instant::now();
            assert!(tree.refit(count - 1, Some([x, 0., x + 5., 5.])));
            timings.push(start.elapsed().as_secs_f64() * 1e6);
        }
        report(count, "single_refit", timings, 0);
        let mut old_x = count as f64 * 10. + ((samples - 1) % 2) as f64 * 10.;
        let mut timings = Vec::with_capacity(samples);
        for i in 0..samples {
            let snapshot = tree.clone();
            let x = count as f64 * 10. + (i % 2) as f64 * 10.;
            let start = Instant::now();
            assert!(tree.refit(count - 1, Some([x, 0., x + 5., 5.])));
            timings.push(start.elapsed().as_secs_f64() * 1e6);
            assert_eq!(
                snapshot.query([old_x, 0., old_x + 5., 5.]).items,
                [count - 1]
            );
            old_x = x;
            drop(snapshot);
        }
        report(count, "single_refit_shared_snapshot", timings, 0);
        assert_eq!(tree.query(query).items.len(), 512);
    }
}
