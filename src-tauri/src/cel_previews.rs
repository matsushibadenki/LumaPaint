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

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub(crate) struct OnionJob {
    pub generation: u64,
    pub label: String,
    pub dimensions: (u32, u32),
    pub references: Vec<lumapaint_core::document::animation::OnionReference>,
    pub mode: lumapaint_core::document::animation::OnionColor,
}
#[cfg(target_os = "macos")]
pub(crate) fn prepare_onion(job: &OnionJob) -> Result<Option<Raster>, String> {
    let mut output: Option<Raster> = None;
    for reference in &job.references {
        let source = raster(reference.cel.clone(), job.dimensions, 1024)?;
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
    type Work = HashMap<String, (tauri::WebviewWindow, OnionJob)>;
    static WORK: OnceLock<Arc<(Mutex<Work>, Condvar)>> = OnceLock::new();
    let work = WORK.get_or_init(|| {
        let work: Arc<(Mutex<Work>, Condvar)> =
            Arc::new((Mutex::new(HashMap::new()), Condvar::new()));
        let worker = work.clone();
        std::thread::Builder::new()
            .name("cel-onion-preview".into())
            .spawn(move || loop {
                let (window, job) = {
                    let (lock, signal) = &*worker;
                    let mut pending = lock.lock().unwrap();
                    while pending.is_empty() {
                        pending = signal.wait(pending).unwrap();
                    }
                    let key = pending.keys().next().unwrap().clone();
                    pending.remove(&key).unwrap()
                };
                let result = prepare_onion(&job);
                let label = job.label.clone();
                let _ = window.run_on_main_thread(move || {
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
        .insert(job.label.clone(), (window, job));
    work.1.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;
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
