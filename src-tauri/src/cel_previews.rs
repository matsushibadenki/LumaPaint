//! Small opaque thumbnail URLs and bounded Rust-owned cel reference rasters.
#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]
use lumapaint_core::document::animation::Cel;
use lumapaint_renderer::animation_cels::Raster;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};

struct Entry {
    cel: std::sync::Weak<Cel>,
    dimensions: (u32, u32),
    side: u32,
    raster: Arc<Raster>,
    png: Option<Vec<u8>>,
    token: u64,
    used: u64,
}
#[derive(Default)]
struct Cache {
    entries: Vec<Entry>,
    clock: u64,
    next: u64,
}
fn cache() -> &'static Mutex<Cache> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
const BUDGET: usize = 48 * 1024 * 1024;
fn charge(entry: &Entry) -> usize {
    entry.raster.pixels.len()
        + entry.png.as_ref().map_or_else(
            || {
                if entry.side == 96 {
                    entry.raster.pixels.len() + 4096
                } else {
                    0
                }
            },
            Vec::len,
        )
}
pub(crate) fn raster(
    cel: Arc<Cel>,
    dimensions: (u32, u32),
    side: u32,
) -> Result<Arc<Raster>, String> {
    if !(32..=1024).contains(&side) {
        return Err("Invalid cel preview size".into());
    }
    {
        let mut cache = cache().lock().map_err(|e| e.to_string())?;
        cache.clock += 1;
        let clock = cache.clock;
        if let Some(entry) = cache.entries.iter_mut().find(|e| {
            std::sync::Weak::ptr_eq(&e.cel, &Arc::downgrade(&cel))
                && e.dimensions == dimensions
                && e.side == side
        }) {
            entry.used = clock;
            return Ok(entry.raster.clone());
        }
    }
    // Rasterization runs on a worker, never while holding the cache mutex.
    let raster = Arc::new(lumapaint_renderer::thumbnails::cel_pixels(
        &cel, dimensions, side,
    )?);
    let mut cache = cache().lock().map_err(|e| e.to_string())?;
    if let Some(entry) = cache.entries.iter().find(|e| {
        std::sync::Weak::ptr_eq(&e.cel, &Arc::downgrade(&cel))
            && e.dimensions == dimensions
            && e.side == side
    }) {
        return Ok(entry.raster.clone());
    }
    while cache.entries.len() >= 192
        || cache.entries.iter().map(charge).sum::<usize>()
            + raster.pixels.len() * if side == 96 { 2 } else { 1 }
            + 4096
            > BUDGET
    {
        let Some(index) = cache
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.used)
            .map(|(i, _)| i)
        else {
            break;
        };
        cache.entries.swap_remove(index);
    }
    cache.next += 1;
    cache.clock += 1;
    let token = cache.next;
    let used = cache.clock;
    cache.entries.push(Entry {
        cel: Arc::downgrade(&cel),
        dimensions,
        side,
        raster: raster.clone(),
        png: None,
        token,
        used,
    });
    Ok(raster)
}
#[derive(Clone)]
pub(crate) struct ThumbnailSource {
    pub layer: String,
    pub frame: u32,
    pub cel: Arc<Cel>,
    pub dimensions: (u32, u32),
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnail {
    pub layer: String,
    pub frame: u32,
    pub token: String,
    pub blank: bool,
}
pub(crate) fn thumbnails(sources: Vec<ThumbnailSource>) -> Result<Vec<Thumbnail>, String> {
    if sources.len() > 64 {
        return Err("Too many cel previews".into());
    }
    let mut result = vec![];
    for source in sources {
        let image = raster(source.cel.clone(), source.dimensions, 96)?;
        let needs_png = cache()
            .lock()
            .map_err(|e| e.to_string())?
            .entries
            .iter()
            .find(|e| Arc::ptr_eq(&e.raster, &image))
            .is_some_and(|e| e.png.is_none());
        let png = if needs_png {
            Some(lumapaint_renderer::vector::document_png_ref(
                image.width,
                image.height,
                &image.pixels,
            )?)
        } else {
            None
        };
        let mut cache = cache().lock().map_err(|e| e.to_string())?;
        let entry = cache
            .entries
            .iter_mut()
            .find(|e| Arc::ptr_eq(&e.raster, &image))
            .ok_or("Cel preview cache was replaced")?;
        if let Some(png) = png {
            entry.png = Some(png);
        }
        result.push(Thumbnail {
            layer: source.layer,
            frame: source.frame,
            token: entry.token.to_string(),
            blank: image.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 0),
        });
    }
    Ok(result)
}
pub fn serve(request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let token = request.uri().path().trim_matches('/').parse::<u64>().ok();
    let png = cache().lock().ok().and_then(|cache| {
        cache
            .entries
            .iter()
            .find(|e| Some(e.token) == token)
            .and_then(|e| e.png.clone())
    });
    let response = tauri::http::Response::builder()
        .status(if png.is_some() { 200 } else { 404 })
        .header("Content-Type", "image/png")
        .header("Cache-Control", "private, max-age=31536000, immutable")
        .header("X-Content-Type-Options", "nosniff")
        .body(png.unwrap_or_default())
        .unwrap();
    responder.respond(response);
}

/// Quantized physical-pixel demand avoids a new texture for every zoom tick.
/// The existing overlay/shader contract remains capped at 1,024 pixels.
pub(crate) fn reference_side(dimensions: (u32, u32), pixels_per_document_pixel: f32) -> u32 {
    if !pixels_per_document_pixel.is_finite() || pixels_per_document_pixel <= 0. {
        return 1024;
    }
    let demand = (dimensions.0.max(dimensions.1) as f32 * pixels_per_document_pixel.min(1.))
        .ceil()
        .clamp(64., 1024.) as u32;
    demand.next_power_of_two().min(1024)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SceneKey {
    document: u64,
    revision: u64,
    frame: u32,
    dimensions: (u32, u32),
    side: u32,
}
/// Keeps pixels only: never holds documents, SVG sources, strokes or history.
#[derive(Default)]
struct SceneCache {
    entries: std::collections::VecDeque<(SceneKey, Arc<Raster>)>,
    bytes: usize,
}
const SCENE_BUDGET: usize = 32 * 1024 * 1024;
impl SceneCache {
    fn get(&mut self, key: SceneKey) -> Option<Arc<Raster>> {
        let index = self.entries.iter().position(|(k, _)| *k == key)?;
        let entry = self.entries.remove(index)?;
        let image = entry.1.clone();
        self.entries.push_back(entry);
        Some(image)
    }
    fn insert(&mut self, key: SceneKey, image: Arc<Raster>) {
        self.entries.retain(|(k, _)| {
            *k != key && (k.document != key.document || k.revision == key.revision)
        });
        self.bytes = self.entries.iter().map(|(_, r)| r.pixels.len()).sum();
        if image.pixels.len() > SCENE_BUDGET {
            return;
        }
        while self.bytes + image.pixels.len() > SCENE_BUDGET || self.entries.len() >= 64 {
            let Some((_, oldest)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= oldest.pixels.len();
        }
        self.bytes += image.pixels.len();
        self.entries.push_back((key, image));
    }
}
fn scene_cache() -> &'static Mutex<SceneCache> {
    static CACHE: OnceLock<Mutex<SceneCache>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}
#[cfg(target_os = "macos")]
pub(crate) struct OnionJob {
    pub generation: u64,
    pub document_id: u64,
    pub revision: u64,
    pub side: u32,
    pub label: String,
    pub dimensions: (u32, u32),
    pub references: Vec<lumapaint_core::document::animation::OnionReference>,
    pub mode: lumapaint_core::document::animation::OnionColor,
    pub scene: Option<lumapaint_core::document::Document>,
    pub scene_references: Vec<lumapaint_core::document::animation::SceneReference>,
}
#[cfg(target_os = "macos")]
pub(crate) fn prepare_onion(job: &OnionJob) -> Result<Option<Raster>, String> {
    let started = std::time::Instant::now();
    let metrics = std::env::var_os("LUMAPAINT_ONION_METRICS").is_some();
    let stage = |name: &str, start: std::time::Instant| {
        if metrics {
            eprintln!(
                "lumapaint-onion-stage stage={name} elapsed_us={}",
                start.elapsed().as_micros()
            );
        }
    };
    let mut hits = 0;
    let mut misses = 0;
    let mut output: Option<Raster> = None;
    if let Some(doc) = &job.scene {
        let mut preview = None;
        for reference in &job.scene_references {
            let key = SceneKey {
                document: job.document_id,
                revision: job.revision,
                frame: reference.frame,
                dimensions: job.dimensions,
                side: job.side,
            };
            let cached = scene_cache().lock().map_err(|e| e.to_string())?.get(key);
            let source = if let Some(image) = cached {
                hits += 1;
                image
            } else {
                misses += 1;
                let start = std::time::Instant::now();
                let preview = preview.get_or_insert_with(|| doc.clone_for_rendering());
                stage("preview_clone", start);
                let start = std::time::Instant::now();
                preview.evaluate_animation_with_outlines(
                    doc,
                    reference.frame,
                    Some(&lumapaint_renderer::text_outlines::outline),
                );
                stage("frame_evaluation", start);
                let start = std::time::Instant::now();
                let image = Arc::new(lumapaint_renderer::thumbnails::scene_pixels(
                    preview, job.side,
                )?);
                stage("scene_raster", start);
                scene_cache()
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(key, image.clone());
                image
            };
            let output = output.get_or_insert_with(|| Raster {
                width: source.width,
                height: source.height,
                pixels: vec![0; source.pixels.len()],
            });
            let start = std::time::Instant::now();
            lumapaint_renderer::animation_cels::composite(
                output,
                &source,
                reference.color,
                reference.opacity,
                job.mode,
            )?;
            stage("reference_composite", start);
        }
        if std::env::var_os("LUMAPAINT_ONION_METRICS").is_some() {
            eprintln!("lumapaint-onion scope=scene document={} revision={} side={} hits={} misses={} elapsed_us={}",job.document_id,job.revision,job.side,hits,misses,started.elapsed().as_micros());
        }
        return Ok(output);
    }
    for reference in &job.references {
        let source = raster(reference.cel.clone(), job.dimensions, job.side)?;
        let output = output.get_or_insert_with(|| Raster {
            width: source.width,
            height: source.height,
            pixels: vec![0; source.pixels.len()],
        });
        lumapaint_renderer::animation_cels::composite(
            output,
            &source,
            reference.color,
            reference.opacity,
            job.mode,
        )?;
    }
    Ok(output)
}
/// One worker, one latest pending job per editor. Rapid scrubbing does not fan
/// out tasks or retain a history of full-size reference rasters.
#[cfg(target_os = "macos")]
pub(crate) fn schedule(window: tauri::WebviewWindow, job: OnionJob) {
    use std::sync::Condvar;
    type Work = HashMap<String, (tauri::WebviewWindow, OnionJob, std::time::Instant)>;
    static WORK: OnceLock<Arc<(Mutex<Work>, Condvar)>> = OnceLock::new();
    let work = WORK.get_or_init(|| {
        let work: Arc<(Mutex<Work>, Condvar)> =
            Arc::new((Mutex::new(HashMap::new()), Condvar::new()));
        let worker = work.clone();
        std::thread::Builder::new()
            .name("cel-onion-preview".into())
            .spawn(move || loop {
                let (window, job, queued) = {
                    let (lock, signal) = &*worker;
                    let mut pending = lock.lock().unwrap();
                    while pending.is_empty() {
                        pending = signal.wait(pending).unwrap();
                    }
                    let key = pending.keys().next().unwrap().clone();
                    pending.remove(&key).unwrap()
                };
                let metrics = std::env::var_os("LUMAPAINT_ONION_METRICS").is_some();
                if metrics {
                    eprintln!(
                        "lumapaint-onion-host generation={} stage=worker_queue elapsed_us={}",
                        job.generation,
                        queued.elapsed().as_micros()
                    );
                }
                let result = prepare_onion(&job);
                let completed = std::time::Instant::now();
                let label = job.label.clone();
                let _ = window.run_on_main_thread(move || {
                    if metrics {
                        eprintln!(
                            "lumapaint-onion-host generation={} stage=main_queue elapsed_us={}",
                            job.generation,
                            completed.elapsed().as_micros()
                        );
                    }
                    crate::canvas::platform::timeline::install_onion_for_window(
                        &label,
                        job.generation,
                        result,
                    );
                });
            })
            .expect("cel reference worker");
        work
    });
    work.0
        .lock()
        .unwrap()
        .insert(job.label.clone(), (window, job, std::time::Instant::now()));
    work.1.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_resolution_tracks_physical_zoom_without_exceeding_overlay_budget() {
        assert_eq!(reference_side((4000, 2000), 0.05), 256);
        assert_eq!(reference_side((4000, 2000), 0.1), 512);
        assert_eq!(reference_side((4000, 2000), 0.2), 1024);
        assert_eq!(reference_side((4000, 2000), 20.), 1024);
        assert_eq!(reference_side((100, 50), 0.01), 64);
        assert_eq!(reference_side((100, 50), 20.), 128);
        for invalid in [0., -1., f32::NAN, f32::INFINITY] {
            assert_eq!(reference_side((4000, 2000), invalid), 1024);
        }
        // Retina backing scale is included by the native caller.
        assert_eq!(reference_side((4000, 2000), 0.05 * 2.), 512);
        assert_eq!(reference_side((4000, 2000), 0.051), 256);
    }
    #[test]
    fn scene_cache_is_lru_bounded_and_isolates_documents_revisions_frames_and_sizes() {
        let mut cache = SceneCache::default();
        let key = SceneKey {
            document: 1,
            revision: 1,
            frame: 4,
            dimensions: (32, 32),
            side: 64,
        };
        let image = Arc::new(Raster {
            width: 32,
            height: 32,
            pixels: vec![0; 4096],
        });
        cache.insert(key, image.clone());
        assert!(Arc::ptr_eq(&cache.get(key).unwrap(), &image));
        for other in [
            SceneKey { document: 2, ..key },
            SceneKey { revision: 2, ..key },
            SceneKey { frame: 5, ..key },
            SceneKey { side: 128, ..key },
            SceneKey {
                dimensions: (64, 32),
                ..key
            },
        ] {
            assert!(cache.get(other).is_none());
        }
        cache.insert(SceneKey { frame: 5, ..key }, image.clone());
        cache.get(key);
        assert_eq!(cache.entries.front().unwrap().0.frame, 5);
        cache.insert(SceneKey { revision: 2, ..key }, image.clone());
        assert!(cache.get(key).is_none());
        assert_eq!(cache.entries.len(), 1);
        for frame in 0..70 {
            cache.insert(
                SceneKey {
                    revision: 2,
                    frame,
                    ..key
                },
                image.clone(),
            );
        }
        assert_eq!(cache.entries.len(), 64);
        assert!(cache
            .get(SceneKey {
                revision: 2,
                frame: 0,
                ..key
            })
            .is_none());
        let large = Arc::new(Raster {
            width: 1024,
            height: 1024,
            pixels: vec![0; 4 * 1024 * 1024],
        });
        for frame in 0..12 {
            cache.insert(
                SceneKey {
                    document: 3,
                    frame,
                    ..key
                },
                large.clone(),
            );
        }
        assert!(cache.bytes <= SCENE_BUDGET);
        assert_eq!(
            cache.bytes,
            cache
                .entries
                .iter()
                .map(|(_, r)| r.pixels.len())
                .sum::<usize>()
        );
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn scene_worker_reuses_pixels_and_rejects_stale_cache_keys() {
        use lumapaint_core::document::animation::{OnionColor, SceneReference};
        use lumapaint_core::document::{Brush, Document, Point};
        let mut doc = Document::default();
        doc.begin(Point { x: 30., y: 30. }, Brush::default())
            .unwrap();
        doc.finish();
        let mut job = OnionJob {
            generation: 1,
            document_id: u64::MAX,
            revision: doc.revision(),
            side: 64,
            label: "cache-test".into(),
            dimensions: doc.dimensions(),
            references: vec![],
            mode: OnionColor::Tint,
            scene: Some(doc.clone_for_rendering()),
            scene_references: vec![SceneReference {
                frame: 0,
                opacity: 0.5,
                color: [255, 0, 0],
            }],
        };
        let before = serde_json::to_value(doc.document_state()).unwrap();
        let first = prepare_onion(&job).unwrap().unwrap();
        let key = SceneKey {
            document: job.document_id,
            revision: job.revision,
            frame: 0,
            dimensions: job.dimensions,
            side: job.side,
        };
        let cached = scene_cache().lock().unwrap().get(key).unwrap();
        let again = prepare_onion(&job).unwrap().unwrap();
        assert_eq!(again.pixels, first.pixels);
        assert!(Arc::ptr_eq(
            &cached,
            &scene_cache().lock().unwrap().get(key).unwrap()
        ));
        job.mode = OnionColor::Original;
        let original = prepare_onion(&job).unwrap().unwrap();
        assert_ne!(original.pixels, first.pixels);
        assert!(Arc::ptr_eq(
            &cached,
            &scene_cache().lock().unwrap().get(key).unwrap()
        ));
        job.mode = OnionColor::Tint;
        job.side = 128;
        let larger = prepare_onion(&job).unwrap().unwrap();
        assert!(larger.width > first.width);
        doc.begin(Point { x: 250., y: 250. }, Brush::default())
            .unwrap();
        doc.finish();
        job.side = 64;
        job.revision = doc.revision();
        job.scene = Some(doc.clone_for_rendering());
        assert_ne!(prepare_onion(&job).unwrap().unwrap().pixels, first.pixels);
        assert!(scene_cache().lock().unwrap().get(key).is_none());
        assert_ne!(serde_json::to_value(doc.document_state()).unwrap(), before);
        assert!(cached.pixels.iter().any(|p| *p > 0));
    }
    /// Worker-only timing, explicitly excludes UI input, queue wait and GPU upload.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "opt-in scene reference worker benchmark"]
    fn scene_onion_benchmark() {
        use lumapaint_core::document::{
            animation::{OnionColor, SceneReference},
            Document,
        };
        let mut doc = Document::default();
        let shapes: String = (0..600)
            .map(|i| {
                format!(
                    "<rect x='{}' y='{}' width='19' height='19' rx='3' fill='rgb({},{},{})'/>",
                    (i % 40) * 24,
                    (i / 40) * 30,
                    i % 255,
                    (i * 7) % 255,
                    (i * 13) % 255
                )
            })
            .collect();
        doc.import_svg(
            "Benchmark".into(),
            format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='960' height='640'>{shapes}</svg>"
            ),
        )
        .unwrap();
        let mut job = OnionJob {
            generation: 1,
            document_id: u64::MAX - 1,
            revision: doc.revision(),
            side: 256,
            label: "benchmark".into(),
            dimensions: doc.dimensions(),
            references: vec![],
            mode: OnionColor::Tint,
            scene: Some(doc.clone_for_rendering()),
            scene_references: (0..4)
                .map(|frame| SceneReference {
                    frame,
                    opacity: 0.2,
                    color: [65, 145, 255],
                })
                .collect(),
        };
        let mut results = std::collections::BTreeMap::<u32, (Vec<u128>, Vec<u128>)>::new();
        for trial in 0..5 {
            for side in if trial % 2 == 0 {
                [256, 1024]
            } else {
                [1024, 256]
            } {
                job.side = side;
                *scene_cache().lock().unwrap() = SceneCache::default();
                let start = std::time::Instant::now();
                let cold = prepare_onion(&job).unwrap().unwrap();
                let cold_us = start.elapsed().as_micros();
                let start = std::time::Instant::now();
                let warm = prepare_onion(&job).unwrap().unwrap();
                let warm_us = start.elapsed().as_micros();
                assert_eq!(cold.pixels, warm.pixels);
                let entry = results.entry(side).or_default();
                entry.0.push(cold_us);
                entry.1.push(warm_us);
            }
        }
        for (side, (mut cold, mut warm)) in results {
            cold.sort();
            warm.sort();
            eprintln!("scene-worker-benchmark side={side} trials=5 cold_median_us={} warm_median_us={} cold_range_us={}..{} warm_range_us={}..{}",cold[2],warm[2],cold[0],cold[4],warm[0],warm[4]);
        }
    }
    #[test]
    fn thumbnails_share_immutable_cels_and_do_not_retain_old_artwork() {
        let cel = Arc::new(Cel {
            source: None,
            strokes: vec![],
        });
        let weak = Arc::downgrade(&cel);
        let first = raster(cel.clone(), (64, 64), 96).unwrap();
        let second = raster(cel.clone(), (64, 64), 96).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let sources = vec![ThumbnailSource {
            layer: "layer-1".into(),
            frame: 0,
            cel: cel.clone(),
            dimensions: (64, 64),
        }];
        let a = thumbnails(sources.clone()).unwrap();
        let b = thumbnails(sources).unwrap();
        assert_eq!(a[0].token, b[0].token);
        assert!(a[0].blank);
        let png = cache()
            .lock()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.token.to_string() == a[0].token)
            .unwrap()
            .png
            .clone()
            .unwrap();
        let image = image::load_from_memory(&png).unwrap();
        assert_eq!((image.width(), image.height()), (64, 64));
        drop(cel);
        assert!(
            weak.upgrade().is_none(),
            "The reference cache must not retain old cel source/stroke data"
        );
        assert!(
            cache()
                .lock()
                .unwrap()
                .entries
                .iter()
                .map(charge)
                .sum::<usize>()
                <= BUDGET
        );
    }
}
