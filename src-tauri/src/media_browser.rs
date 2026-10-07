//! Read-only media catalogue. Pixels stay out of IPC: bounded workers serve disk proxies
//! through a private protocol; the webview receives metadata and opaque catalogue URLs.
use serde::Serialize;
use std::{
    collections::{hash_map::DefaultHasher, HashMap, VecDeque},
    fs,
    hash::{Hash, Hasher},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, OnceLock},
    time::UNIX_EPOCH,
};
use tauri::Manager;
const MAX_FILES: usize = 50_000;
const CACHE_BUDGET: u64 = 512 * 1024 * 1024;
const PAGE_SIZE: usize = 256;
const FAILURE: &str = "Preview unavailable / プレビューを作成できません / 无法生成预览";
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    id: usize,
    name: String,
    kind: &'static str,
    extension: String,
    bytes: u64,
    modified: u64,
    #[serde(skip)]
    path: PathBuf,
}
#[derive(Clone, Serialize)]
pub struct Folder {
    name: String,
    path: String,
}
struct Catalogue {
    id: u64,
    entries: Vec<Entry>,
    order: Mutex<Option<Index>>,
}
struct Index {
    key: (String, String, String),
    ids: Vec<usize>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scan {
    id: u64,
    path: String,
    parent: Option<String>,
    folders: Vec<Folder>,
    places: Vec<Folder>,
    truncated: bool,
}
#[derive(Serialize)]
pub struct Page {
    entries: Vec<Entry>,
    total: usize,
}
struct Job {
    session: u64,
    index: usize,
    size: u32,
    responder: tauri::UriSchemeResponder,
}
struct Browser {
    catalogues: Mutex<VecDeque<Arc<Catalogue>>>,
    queue: Mutex<VecDeque<Job>>,
    wake: Condvar,
    cache: PathBuf,
    config: PathBuf,
    inventory: Mutex<Option<CacheInventory>>,
    scans: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicU64>>>,
}
static BROWSER: OnceLock<Arc<Browser>> = OnceLock::new();
fn browser() -> Result<&'static Arc<Browser>, String> {
    BROWSER
        .get()
        .ok_or_else(|| "Media browser is not ready".into())
}
fn kind(extension: &str) -> Option<&'static str> {
    match extension {
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" | "tif" | "tiff" | "heic" | "heif"
        | "avif" | "ico" | "psd" | "psb" | "exr" | "dng" | "cr2" | "cr3" | "nef" | "arw"
        | "orf" | "raf" | "rw2" => Some("image"),
        "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" | "mpeg" | "mpg" => Some("video"),
        _ => None,
    }
}
fn stamp(metadata: &fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |t| t.as_secs())
}
fn folder(path: PathBuf) -> Folder {
    Folder {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: path.to_string_lossy().into_owned(),
    }
}
pub fn initialize(app: &tauri::AppHandle) -> Result<(), String> {
    let state = Arc::new(Browser {
        inventory: Mutex::new(None),
        scans: Mutex::new(HashMap::new()),
        catalogues: Mutex::new(VecDeque::new()),
        queue: Mutex::new(VecDeque::new()),
        wake: Condvar::new(),
        cache: app
            .path()
            .app_cache_dir()
            .map_err(|e| e.to_string())?
            .join("media-proxies-v1"),
        config: app
            .path()
            .app_config_dir()
            .map_err(|e| e.to_string())?
            .join("media-browser-folder.json"),
    });
    fs::create_dir_all(&state.cache).map_err(|e| e.to_string())?;
    BROWSER
        .set(state.clone())
        .map_err(|_| "Media browser already initialized")?;
    for _ in 0..2 {
        let state = state.clone();
        std::thread::spawn(move || loop {
            let job = {
                let mut queue = state.queue.lock().unwrap();
                while queue.is_empty() {
                    queue = state.wake.wait(queue).unwrap();
                }
                queue.pop_front().unwrap()
            };
            let result = entry(&state, job.session, job.index)
                .and_then(|file| proxy(&state, &file, job.size));
            let response = match result {
                Ok(bytes) => response(200, "image/jpeg", bytes),
                Err(_) => response(422, "text/plain", FAILURE.as_bytes().to_vec()),
            };
            job.responder.respond(response);
        });
    }
    Ok(())
}
#[tauri::command]
pub async fn media_scan(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    path: Option<String>,
) -> Result<Scan, String> {
    let generation = browser()?
        .scans
        .lock()
        .unwrap()
        .entry(window.label().to_owned())
        .or_insert_with(|| Arc::new(std::sync::atomic::AtomicU64::new(0)))
        .clone();
    let request = generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let state = browser()?;
        let home = app.path().home_dir().map_err(|e| e.to_string())?;
        if let Some(ref path) = path {
            if !Path::new(path).is_dir() {
                return Err("Folder not found / フォルダーが見つかりません / 找不到文件夹".into());
            }
        }
        let path = path
            .map(PathBuf::from)
            .or_else(|| {
                fs::read(&state.config)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<PathBuf>(&b).ok())
            })
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| {
                let p = home.join("Pictures");
                if p.is_dir() {
                    p
                } else {
                    home.clone()
                }
            });
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        let mut folders = Vec::new();
        let mut truncated = false;
        for item in fs::read_dir(&path).map_err(|e| e.to_string())? {
            if generation.load(std::sync::atomic::Ordering::Relaxed) != request {
                return Err("Folder scan cancelled".into());
            }
            let Ok(item) = item else { continue };
            if item.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(meta) = item.metadata() else { continue };
            if meta.is_dir() {
                if folders.len() < 2000 {
                    folders.push(folder(item.path()));
                } else {
                    truncated = true;
                }
                continue;
            }
            if !meta.is_file() {
                continue;
            }
            let extension = item
                .path()
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let Some(kind) = kind(&extension) else {
                continue;
            };
            if entries.len() >= MAX_FILES {
                truncated = true;
                break;
            }
            entries.push(Entry {
                id: entries.len(),
                name: item.file_name().to_string_lossy().into_owned(),
                kind,
                extension,
                bytes: meta.len(),
                modified: stamp(&meta),
                path: item.path(),
            });
        }
        folders.sort_by_cached_key(|f| f.name.to_lowercase());
        let id = {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        };
        {
            let mut catalogues = state.catalogues.lock().unwrap();
            catalogues.push_back(Arc::new(Catalogue {
                id,
                entries,
                order: Mutex::new(None),
            }));
            if catalogues.len() > 16 {
                catalogues.pop_front();
            }
        }
        if let Some(parent) = state.config.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&state.config, serde_json::to_vec(&path).unwrap());
        let places = [
            home.clone(),
            home.join("Desktop"),
            home.join("Pictures"),
            home.join("Movies"),
            home.join("Downloads"),
        ]
        .into_iter()
        .filter(|p| p.is_dir())
        .map(folder)
        .collect();
        Ok(Scan {
            id,
            path: path.to_string_lossy().into_owned(),
            parent: path.parent().map(|p| p.to_string_lossy().into_owned()),
            folders,
            places,
            truncated,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn media_pick_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            let _ = tx.send(
                rfd::FileDialog::new()
                    .pick_folder()
                    .map(|p| p.to_string_lossy().into_owned()),
            );
        })
        .map_err(|e| e.to_string())?;
        tauri::async_runtime::spawn_blocking(move || rx.recv().map_err(|e| e.to_string()))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
        Err("Enter a folder path / フォルダーパスを入力してください / 请输入文件夹路径".into())
    }
}
fn page(catalogue: &Catalogue, query: &str, filter: &str, sort: &str, offset: usize) -> Page {
    let key = (query.to_lowercase(), filter.to_owned(), sort.to_owned());
    let mut order = catalogue.order.lock().unwrap();
    if order.as_ref().is_none_or(|index| index.key != key) {
        let mut entries: Vec<_> = catalogue
            .entries
            .iter()
            .filter(|e| {
                (filter == "all" || filter == e.kind) && e.name.to_lowercase().contains(&key.0)
            })
            .collect();
        match sort {
            "date" => entries.sort_by_key(|e| std::cmp::Reverse((e.modified, e.id))),
            "size" => entries.sort_by_key(|e| std::cmp::Reverse((e.bytes, e.id))),
            _ => entries.sort_by_cached_key(|e| (e.name.to_lowercase(), e.id)),
        }
        *order = Some(Index {
            key,
            ids: entries.into_iter().map(|e| e.id).collect(),
        });
    }
    let ids = &order.as_ref().unwrap().ids;
    Page {
        total: ids.len(),
        entries: ids
            .iter()
            .skip(offset)
            .take(PAGE_SIZE)
            .map(|&id| catalogue.entries[id].clone())
            .collect(),
    }
}
#[tauri::command]
pub async fn media_page(
    session: u64,
    query: String,
    filter: String,
    sort: String,
    offset: usize,
) -> Result<Page, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = browser()?;
        let catalogue = state
            .catalogues
            .lock()
            .unwrap()
            .iter()
            .find(|c| c.id == session)
            .cloned()
            .ok_or("Folder expired; refresh / フォルダーを再読み込みしてください / 请刷新文件夹")?;
        Ok(page(&catalogue, &query, &filter, &sort, offset))
    })
    .await
    .map_err(|e| e.to_string())?
}
fn entry(state: &Browser, session: u64, index: usize) -> Result<Entry, String> {
    state
        .catalogues
        .lock()
        .unwrap()
        .iter()
        .find(|c| c.id == session)
        .and_then(|c| c.entries.get(index))
        .cloned()
        .ok_or_else(|| "Unknown media".into())
}
fn cache_key(file: &Entry, size: u32) -> Result<String, String> {
    let metadata = fs::metadata(&file.path).map_err(|e| e.to_string())?;
    let mut hash = DefaultHasher::new();
    file.path.hash(&mut hash);
    metadata.len().hash(&mut hash);
    metadata.modified().ok().hash(&mut hash);
    size.hash(&mut hash);
    Ok(format!("{:016x}.jpg", hash.finish()))
}
#[derive(Default)]
struct CacheInventory {
    files: HashMap<PathBuf, (u64, u64)>,
    bytes: u64,
    clock: u64,
}
impl CacheInventory {
    fn record(&mut self, path: PathBuf, bytes: u64) {
        self.clock += 1;
        if let Some((old, _)) = self.files.insert(path, (bytes, self.clock)) {
            self.bytes = self.bytes.saturating_sub(old);
        }
        self.bytes += bytes;
    }
    fn trim(&mut self, budget: u64) {
        while self.bytes > budget {
            let Some(path) = self
                .files
                .iter()
                .min_by_key(|(_, (_, used))| used)
                .map(|(p, _)| p.clone())
            else {
                break;
            };
            if fs::remove_file(&path).is_err() && path.exists() {
                break;
            }
            if let Some((bytes, _)) = self.files.remove(&path) {
                self.bytes = self.bytes.saturating_sub(bytes);
            }
        }
    }
}
fn record_cache(state: &Browser, path: PathBuf, bytes: u64) {
    let mut inventory = state.inventory.lock().unwrap();
    let book = inventory.get_or_insert_with(|| {
        let mut book = CacheInventory::default();
        if let Ok(files) = fs::read_dir(&state.cache) {
            let mut files: Vec<_> = files
                .flatten()
                .filter(|f| f.path().extension().is_some_and(|e| e == "jpg"))
                .filter_map(|f| {
                    let meta = f.metadata().ok()?;
                    Some((meta.modified().unwrap_or(UNIX_EPOCH), f.path(), meta.len()))
                })
                .collect();
            files.sort_by_key(|f| f.0);
            for (_, path, bytes) in files {
                book.record(path, bytes);
            }
        }
        book
    });
    book.record(path, bytes);
    book.trim(CACHE_BUDGET);
}
#[cfg(target_os = "macos")]
fn run(mut command: std::process::Command) -> Result<(), String> {
    let mut child = command
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(FAILURE.into())
            };
        }
        if start.elapsed() > std::time::Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FAILURE.into());
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
fn decode_proxy(file: &Entry, size: u32) -> Result<image::DynamicImage, String> {
    let size = image::image_dimensions(&file.path)
        .ok()
        .map_or(size, |(w, h)| size.min(w.max(h).max(1)));
    // Native macOS decoders handle orientation, RAW/HEIF and video poster frames.
    #[cfg(target_os = "macos")]
    {
        let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
        if file.kind == "video" {
            let mut cmd = std::process::Command::new("/usr/bin/qlmanage");
            cmd.args(["-t", "-s", &size.to_string(), "-o"])
                .arg(dir.path())
                .arg(&file.path);
            if run(cmd).is_ok() {
                if let Some(path) = fs::read_dir(dir.path())
                    .map_err(|e| e.to_string())?
                    .flatten()
                    .map(|f| f.path())
                    .find(|p| p.extension().is_some_and(|e| e == "png"))
                {
                    return image::open(path).map_err(|e| e.to_string());
                }
            }
        } else {
            let output = dir.path().join("proxy.png");
            let mut cmd = std::process::Command::new("/usr/bin/sips");
            cmd.args([
                "-s",
                "format",
                "png",
                "-m",
                "/System/Library/ColorSync/Profiles/sRGB Profile.icc",
                "-Z",
                &size.to_string(),
            ])
            .arg(&file.path)
            .arg("--out")
            .arg(&output);
            if run(cmd).is_ok() {
                return image::open(output).map_err(|e| e.to_string());
            }
        }
    }
    if file.kind == "video" {
        return Err(FAILURE.into());
    }
    let mut reader = image::ImageReader::open(&file.path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(256 * 1024 * 1024);
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = image::ImageDecoder::orientation(&mut decoder).map_err(|e| e.to_string())?;
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    Ok(fit_proxy(image, size))
}
fn fit_proxy(image: image::DynamicImage, size: u32) -> image::DynamicImage {
    if image.width() > size || image.height() > size {
        image.thumbnail(size, size)
    } else {
        image
    }
}
fn proxy(state: &Browser, file: &Entry, size: u32) -> Result<Vec<u8>, String> {
    let output = state.cache.join(cache_key(file, size)?);
    if let Ok(bytes) = fs::read(&output) {
        record_cache(state, output, bytes.len() as u64);
        return Ok(bytes);
    }
    let image = fit_proxy(decode_proxy(file, size)?, size);
    // Composite transparent pixels over a neutral checker, rather than black JPEG alpha.
    let rgba = image.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, p) in rgba.enumerate_pixels() {
        let bg = if (x / 12 + y / 12) % 2 == 0 {
            62u32
        } else {
            72
        };
        let a = u32::from(p[3]);
        rgb.put_pixel(
            x,
            y,
            image::Rgb(
                [0, 1, 2].map(|i| ((u32::from(p[i]) * a + bg * (255 - a) + 127) / 255) as u8),
            ),
        );
    }
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 85)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(&state.cache).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut temp, &bytes).map_err(|e| e.to_string())?;
    temp.persist(&output).map_err(|e| e.to_string())?;
    record_cache(state, output, bytes.len() as u64);
    Ok(bytes)
}
fn response(status: u16, mime: &str, bytes: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(bytes)
        .unwrap()
}
fn byte_range(header: Option<&str>, len: u64) -> Result<(u64, u64), String> {
    if len == 0 {
        return Err("Empty media".into());
    }
    let (start, end) = if let Some(header) = header {
        let range = header.strip_prefix("bytes=").ok_or("Invalid range")?;
        let (a, b) = range.split_once('-').ok_or("Invalid range")?;
        if a.is_empty() {
            let count = b.parse::<u64>().map_err(|_| "Invalid range")?;
            if count == 0 {
                return Err("Invalid range".into());
            }
            (len.saturating_sub(count), len - 1)
        } else {
            (
                a.parse().map_err(|_| "Invalid range")?,
                if b.is_empty() {
                    len - 1
                } else {
                    b.parse::<u64>().map_err(|_| "Invalid range")?.min(len - 1)
                },
            )
        }
    } else {
        (0, len - 1)
    };
    if start >= len || end < start {
        return Err("Invalid range".into());
    }
    Ok((start, end.min(start.saturating_add(4 * 1024 * 1024 - 1))))
}
fn video(
    file: &Entry,
    request: &tauri::http::Request<Vec<u8>>,
) -> Result<tauri::http::Response<Vec<u8>>, String> {
    if file.kind != "video" {
        return Err("Not a video".into());
    }
    let mut source = fs::File::open(&file.path).map_err(|e| e.to_string())?;
    let len = source.metadata().map_err(|e| e.to_string())?.len();
    let Ok((start, end)) = byte_range(
        request.headers().get("Range").and_then(|h| h.to_str().ok()),
        len,
    ) else {
        return Ok(tauri::http::Response::builder()
            .status(416)
            .header("Content-Range", format!("bytes */{len}"))
            .body(vec![])
            .unwrap());
    };
    source
        .seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = vec![0; (end - start + 1) as usize];
    source.read_exact(&mut bytes).map_err(|e| e.to_string())?;
    let mime = match file.extension.as_str() {
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => "video/mp4",
    };
    Ok(tauri::http::Response::builder()
        .status(206)
        .header("Content-Type", mime)
        .header("Access-Control-Allow-Origin", "*")
        .header("Accept-Ranges", "bytes")
        .header("Cache-Control", "no-store")
        .header("Content-Range", format!("bytes {start}-{end}/{len}"))
        .header("Content-Length", bytes.len())
        .body(bytes)
        .unwrap())
}
pub fn serve(request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let parts: Vec<_> = request.uri().path().trim_matches('/').split('/').collect();
    let parsed = (|| {
        Some((
            parts.first()?.parse::<u64>().ok()?,
            parts.get(1)?.parse::<usize>().ok()?,
            *parts.get(2)?,
        ))
    })();
    let Ok(state) = browser() else {
        responder.respond(response(503, "text/plain", vec![]));
        return;
    };
    let Some((session, index, variant)) = parsed else {
        responder.respond(response(404, "text/plain", vec![]));
        return;
    };
    if variant == "video" {
        let state = state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = entry(&state, session, index).and_then(|file| video(&file, &request));
            responder.respond(result.unwrap_or_else(|_| response(404, "text/plain", vec![])));
        });
        return;
    }
    let size = match variant {
        "thumb" => 320,
        "preview" => 1600,
        _ => {
            responder.respond(response(404, "text/plain", vec![]));
            return;
        }
    };
    let mut queue = state.queue.lock().unwrap();
    if queue.len() >= 64 {
        responder.respond(response(503, "text/plain", vec![]));
        return;
    }
    let job = Job {
        session,
        index,
        size,
        responder,
    };
    if size == 1600 {
        queue.push_front(job);
    } else {
        queue.push_back(job);
    }
    state.wake.notify_one();
}
#[cfg(test)]
mod tests {
    use super::*;
    fn test_browser(cache: PathBuf) -> Browser {
        Browser {
            catalogues: Mutex::new(VecDeque::new()),
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            config: cache.join("settings"),
            cache,
            inventory: Mutex::new(None),
            scans: Mutex::new(HashMap::new()),
        }
    }
    #[test]
    fn proxies_are_bounded_reused_and_refreshed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.png");
        image::RgbImage::from_pixel(1200, 800, image::Rgb([31, 112, 201]))
            .save(&source)
            .unwrap();
        let cache = dir.path().join("cache");
        fs::create_dir(&cache).unwrap();
        let state = test_browser(cache);
        let file = Entry {
            id: 0,
            name: "source.png".into(),
            kind: "image",
            extension: "png".into(),
            bytes: 0,
            modified: 0,
            path: source.clone(),
        };
        let first = proxy(&state, &file, 320).unwrap();
        let decoded = image::load_from_memory(&first).unwrap();
        assert_eq!(decoded.width(), 320);
        assert!(decoded.height() <= 320);
        let key = state.cache.join(cache_key(&file, 320).unwrap());
        let modified = fs::metadata(&key).unwrap().modified().unwrap();
        assert_eq!(proxy(&state, &file, 320).unwrap(), first);
        assert_eq!(fs::metadata(&key).unwrap().modified().unwrap(), modified);
        let preview = proxy(&state, &file, 1600).unwrap();
        assert_eq!(image::load_from_memory(&preview).unwrap().width(), 1200);
        image::RgbImage::from_pixel(640, 480, image::Rgb([220, 55, 20]))
            .save(source)
            .unwrap();
        let changed = proxy(&state, &file, 320).unwrap();
        assert_ne!(changed, first);
    }
    #[test]
    fn cache_evicts_least_recently_used_and_counts_replacements() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let c = dir.path().join("c");
        for p in [&a, &b, &c] {
            fs::write(p, [0u8; 10]).unwrap();
        }
        let mut book = CacheInventory::default();
        book.record(a.clone(), 10);
        book.record(b.clone(), 10);
        book.record(a.clone(), 10);
        book.record(c.clone(), 10);
        assert_eq!(book.bytes, 30);
        book.trim(20);
        assert!(a.exists());
        assert!(!b.exists());
        assert!(c.exists());
        assert_eq!(book.bytes, 20);
    }
    #[test]
    fn video_serves_only_requested_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clip.mp4");
        fs::write(&path, (0..100u8).collect::<Vec<_>>()).unwrap();
        let file = Entry {
            id: 0,
            name: "clip.mp4".into(),
            kind: "video",
            extension: "mp4".into(),
            bytes: 100,
            modified: 0,
            path,
        };
        let request = tauri::http::Request::builder()
            .header("Range", "bytes=20-29")
            .body(vec![])
            .unwrap();
        let result = video(&file, &request).unwrap();
        assert_eq!(result.status(), 206);
        assert_eq!(result.headers()["Content-Range"], "bytes 20-29/100");
        assert_eq!(result.body(), &(20..30u8).collect::<Vec<_>>());
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a local movie fixture and macOS Quick Look"]
    fn native_movie_poster() {
        let path = PathBuf::from(std::env::var("LUMAPAINT_MEDIA_VIDEO_FIXTURE").unwrap());
        let file = Entry {
            id: 0,
            name: "clip.mp4".into(),
            kind: "video",
            extension: "mp4".into(),
            bytes: 0,
            modified: 0,
            path,
        };
        let image = decode_proxy(&file, 320).unwrap();
        assert!(image.width() > 0 && image.width() <= 320);
        assert!(image.height() > 0 && image.height() <= 320);
    }
    #[test]
    fn ranges_are_bounded_and_validate_suffixes() {
        assert_eq!(byte_range(Some("bytes=10-19"), 100).unwrap(), (10, 19));
        assert_eq!(byte_range(Some("bytes=-10"), 100).unwrap(), (90, 99));
        assert_eq!(
            byte_range(None, 10_000_000).unwrap(),
            (0, 4 * 1024 * 1024 - 1)
        );
        assert!(byte_range(Some("bytes=100-"), 100).is_err());
        assert!(byte_range(Some("bytes=-0"), 100).is_err());
        assert!(byte_range(Some("bytes=0-1,3-4"), 100).is_err());
    }
    #[test]
    fn filtering_precedes_paging() {
        let entries = (0..600)
            .map(|id| Entry {
                id,
                name: format!("image-{id:04}.jpg"),
                kind: if id % 2 == 0 { "image" } else { "video" },
                extension: "jpg".into(),
                bytes: id as u64,
                modified: 0,
                path: PathBuf::new(),
            })
            .collect();
        let c = Catalogue {
            id: 1,
            entries,
            order: Mutex::new(None),
        };
        let result = page(&c, "image-", "image", "size", 256);
        assert_eq!(result.total, 300);
        assert_eq!(result.entries.len(), 44);
        assert_eq!(result.entries[0].id, 86);
    }
    #[test]
    fn cache_changes_with_file_contents_and_proxy_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        fs::write(&path, b"one").unwrap();
        let mut file = Entry {
            id: 0,
            name: "a".into(),
            kind: "image",
            extension: "png".into(),
            bytes: 3,
            modified: 0,
            path,
        };
        let a = cache_key(&file, 320).unwrap();
        assert_ne!(a, cache_key(&file, 1600).unwrap());
        fs::write(&file.path, b"changed length").unwrap();
        assert_ne!(a, cache_key(&file, 320).unwrap());
        file.path = dir.path().join("missing");
        assert!(cache_key(&file, 320).is_err());
    }
}

/// Discard queued thumbnails outside the current viewport, or all jobs when closing.
#[tauri::command]
pub fn media_interest(session: u64, ids: Vec<usize>, preview: Option<usize>) -> Result<(), String> {
    let state = browser()?;
    let mut queue = state.queue.lock().unwrap();
    let mut keep = VecDeque::new();
    while let Some(job) = queue.pop_front() {
        if job.session != session
            || (job.size == 1600 && preview == Some(job.index))
            || (job.size == 320 && ids.contains(&job.index))
        {
            keep.push_back(job);
        } else {
            job.responder.respond(response(410, "text/plain", vec![]));
        }
    }
    *queue = keep;
    Ok(())
}

#[tauri::command]
pub fn media_cancel_scan(window: tauri::WebviewWindow) -> Result<(), String> {
    if let Some(generation) = browser()?.scans.lock().unwrap().get(window.label()) {
        generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    Ok(())
}
