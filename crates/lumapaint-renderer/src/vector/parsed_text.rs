//! Resolution-independent scene parsing cache. Parsing and drawing never hold its lock.
use super::{font_resolver, system_fonts, usvg};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, RwLock, TryLockError};

const MAX_ENTRIES: usize = 32;
// One shared cache replaces the former two 32 MiB text/workspace caches.
const MAX_BYTES: usize = 64 * 1024 * 1024;

struct Entry {
    identity: u64,
    source: String,
    fingerprint: u64,
    tree: Arc<usvg::Tree>,
    charge: usize,
}

#[derive(Default)]
struct Cache {
    // Oldest first. A small bounded list avoids hash collisions and duplicate key storage.
    entries: Vec<Entry>,
    bytes: usize,
}

fn fingerprint(source: &str) -> u64 {
    // Computed before acquiring the cache lock. Full source equality remains
    // mandatory, so a hash collision can never return the wrong shaped tree.
    let _timer = crate::performance::time("parsed_text_fingerprint");
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let bytes = source.as_bytes();
    bytes.len().hash(&mut hash);
    // Bound the prefilter to 128 source bytes so a warm single-entry lookup
    // does not acquire a new O(source length) hashing cost. Sampling collisions
    // are expected and handled by the mandatory full equality check.
    if bytes.len() <= 128 {
        bytes.hash(&mut hash);
    } else {
        for start in [0, bytes.len() / 3, bytes.len() / 3 * 2, bytes.len() - 32] {
            bytes[start..start + 32].hash(&mut hash);
        }
    }
    hash.finish()
}

impl Entry {
    fn new(source: &str, fingerprint: u64, tree: Arc<usvg::Tree>, charge: usize) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            identity: NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            source: source.into(),
            fingerprint,
            tree,
            charge,
        }
    }
}
impl Cache {
    fn lookup_index(&self, source: &str, fingerprint: u64) -> (Option<usize>, u64) {
        let mut comparisons = 0;
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.fingerprint == fingerprint {
                comparisons += 1;
                if entry.source == source {
                    return (Some(index), comparisons);
                }
            }
        }
        (None, comparisons)
    }
    fn get(&mut self, source: &str, fingerprint: u64) -> Option<Arc<usvg::Tree>> {
        let (index, comparisons) = self.lookup_index(source, fingerprint);
        crate::performance::count("parsed_text_cache_source_comparisons", comparisons);
        let index = index?;
        let entry = self.entries.remove(index);
        let tree = Arc::clone(&entry.tree);
        self.entries.push(entry);
        Some(tree)
    }

    // Entry construction and retired-entry destruction occur outside the lock.
    fn insert(&mut self, entry: Entry) -> Vec<Entry> {
        if entry.charge > MAX_BYTES || self.get(&entry.source, entry.fingerprint).is_some() {
            return vec![entry];
        }
        let mut retired = Vec::new();
        while self.bytes + entry.charge > MAX_BYTES || self.entries.len() >= MAX_ENTRIES {
            let old = self.entries.remove(0);
            self.bytes -= old.charge;
            retired.push(old);
        }
        self.bytes += entry.charge;
        crate::performance::count("parsed_tree_cache_admissions", 1);
        crate::performance::count("parsed_tree_cache_admitted_bytes", entry.charge as u64);
        // Insertion-event samples, not a process RSS gauge. Callers may sum
        // multiple events; the cache's own bound remains MAX_BYTES.
        crate::performance::count(
            "parsed_tree_cache_insertion_retained_bytes",
            self.bytes as u64,
        );
        crate::performance::count("parsed_tree_cache_evictions", retired.len() as u64);
        self.entries.push(entry);
        retired
    }
}

// Never wait behind a reader/inserter. Distinguish contention from an ordinary
// absent key so profiling can attribute otherwise redundant cold shaping.
fn lookup_shared(cache: &RwLock<Cache>, source: &str, fingerprint: u64) -> Option<Arc<usvg::Tree>> {
    let _timer = crate::performance::time("parsed_text_cache_lookup");
    let (tree, identity, newest) = match cache.try_read() {
        Ok(cache) => {
            let (index, comparisons) = cache.lookup_index(source, fingerprint);
            crate::performance::count("parsed_text_cache_source_comparisons", comparisons);
            let index = index?;
            let entry = &cache.entries[index];
            (
                Arc::clone(&entry.tree),
                entry.identity,
                index + 1 == cache.entries.len(),
            )
        }
        Err(error) => {
            crate::performance::count(
                match error {
                    TryLockError::WouldBlock => "parsed_text_cache_lookup_contended",
                    TryLockError::Poisoned(_) => "parsed_text_cache_lookup_poisoned",
                },
                1,
            );
            return None;
        }
    };
    if newest {
        // This hit is already at the MRU position at read time. No exclusive
        // lock or vector mutation is needed for repeated viewport/source reuse.
        crate::performance::count("parsed_text_cache_promotion_unneeded", 1);
        return Some(tree);
    }
    // Promotion is best effort: a concurrent reader must not turn a valid hit
    // into expensive reshaping. Arc ownership also survives concurrent eviction.
    if let Ok(mut cache) = cache.try_write() {
        if let Some(index) = cache
            .entries
            .iter()
            .position(|entry| entry.identity == identity)
        {
            let entry = cache.entries.remove(index);
            cache.entries.push(entry);
        }
    } else {
        crate::performance::count("parsed_text_cache_promotion_skipped", 1);
    }
    Some(tree)
}

fn insert_shared(cache: &RwLock<Cache>, entry: Entry) {
    let retired = {
        let _timer = crate::performance::time("parsed_text_cache_insert");
        match cache.try_write() {
            Ok(mut cache) => cache.insert(entry),
            Err(error) => {
                crate::performance::count(
                    match error {
                        TryLockError::WouldBlock => "parsed_text_cache_insert_contended",
                        TryLockError::Poisoned(_) => "parsed_text_cache_insert_poisoned",
                    },
                    1,
                );
                vec![entry]
            }
        }
    };
    // Both the lock and insertion timer end before large source/tree drops.
    let _timer = crate::performance::time("parsed_text_cache_retirement");
    drop(retired);
}

enum SharedCache {
    Serial(Mutex<Cache>),
    Parallel(RwLock<Cache>),
}

impl SharedCache {
    fn new() -> Self {
        // Parallel read reuse is qualified for correctness, but its latency
        // benefit is not established. Keep the existing route as the default.
        if std::env::var("LUMAPAINT_PARALLEL_TEXT_CACHE").as_deref() == Ok("1") {
            Self::Parallel(RwLock::default())
        } else {
            Self::Serial(Mutex::default())
        }
    }

    fn lookup(&self, source: &str, fingerprint: u64) -> Option<Arc<usvg::Tree>> {
        match self {
            Self::Parallel(cache) => lookup_shared(cache, source, fingerprint),
            Self::Serial(cache) => {
                let _timer = crate::performance::time("parsed_text_cache_lookup");
                match cache.try_lock() {
                    Ok(mut cache) => cache.get(source, fingerprint),
                    Err(error) => {
                        crate::performance::count(
                            match error {
                                TryLockError::WouldBlock => "parsed_text_cache_lookup_contended",
                                TryLockError::Poisoned(_) => "parsed_text_cache_lookup_poisoned",
                            },
                            1,
                        );
                        None
                    }
                }
            }
        }
    }

    fn insert(&self, entry: Entry) {
        match self {
            Self::Parallel(cache) => insert_shared(cache, entry),
            Self::Serial(cache) => {
                let retired = {
                    let _timer = crate::performance::time("parsed_text_cache_insert");
                    match cache.try_lock() {
                        Ok(mut cache) => cache.insert(entry),
                        Err(error) => {
                            crate::performance::count(
                                match error {
                                    TryLockError::WouldBlock => {
                                        "parsed_text_cache_insert_contended"
                                    }
                                    TryLockError::Poisoned(_) => {
                                        "parsed_text_cache_insert_poisoned"
                                    }
                                },
                                1,
                            );
                            vec![entry]
                        }
                    }
                };
                let _timer = crate::performance::time("parsed_text_cache_retirement");
                drop(retired);
            }
        }
    }
}

type FontLoader = Arc<dyn Fn() -> Arc<usvg::fontdb::Database> + Send + Sync>;

fn options_with_font_loader(load: FontLoader, eager: bool) -> usvg::Options<'static> {
    let metrics = std::env::var_os("LUMAPAINT_ONION_METRICS").is_some();
    let load: FontLoader = Arc::new(move || {
        let started = std::time::Instant::now();
        let fonts = load();
        if metrics {
            eprintln!(
                "lumapaint-onion-stage stage=font_database elapsed_us={}",
                started.elapsed().as_micros()
            );
        }
        fonts
    });
    let resolver = font_resolver();
    let fallback_load = load.clone();
    let select_load = load.clone();
    let mut options = usvg::Options {
        font_resolver: usvg::FontResolver {
            select_font: Box::new(move |font, database| {
                // Replacing an EMPTY database is additive: no existing glyph IDs
                // can be invalidated. Subsequent spans/fallbacks keep the same IDs.
                if database.is_empty() {
                    *database = select_load();
                }
                (resolver.select_font)(font, database)
            }),
            select_fallback: Box::new(move |character, excluded, database| {
                if database.is_empty() {
                    *database = fallback_load();
                }
                (resolver.select_fallback)(character, excluded, database)
            }),
        },
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    if eager {
        options.fontdb = load();
    }
    options
}

pub(super) fn svg_options() -> usvg::Options<'static> {
    // Opt-in same-binary diagnostic baseline. Ordinary rendering loads the
    // existing shared database only when usvg actually resolves a text node.
    options_with_font_loader(
        Arc::new(system_fonts),
        std::env::var("LUMAPAINT_SVG_EAGER_FONTS").as_deref() == Ok("1"),
    )
}

pub(super) fn parse_uncached(source: &str) -> Result<usvg::Tree, String> {
    let _timer = crate::performance::time("svg_parse_including_text_shaping");
    crate::performance::count("svg_parses", 1);
    let options = svg_options();
    let started = std::time::Instant::now();
    let result = usvg::Tree::from_str(source, &options).map_err(|error| error.to_string());
    if std::env::var_os("LUMAPAINT_ONION_METRICS").is_some() {
        eprintln!(
            "lumapaint-onion-stage stage=svg_parse elapsed_us={}",
            started.elapsed().as_micros()
        );
    }
    result
}

pub(super) fn parse(source: &str) -> Result<Arc<usvg::Tree>, String> {
    static CACHE: OnceLock<SharedCache> = OnceLock::new();
    let cache = CACHE.get_or_init(SharedCache::new);
    parse_with_policy(source, cache, true)
}

pub(super) fn parse_workspace(source: &str) -> Result<Arc<usvg::Tree>, String> {
    // Bounds, object rendering and viewport rendering share the exact immutable tree.
    parse(source)
}

#[cfg(test)]
fn parse_with_cache(source: &str, cache: &SharedCache) -> Result<Arc<usvg::Tree>, String> {
    parse_with_policy(source, cache, false)
}
fn parse_with_policy(
    source: &str,
    cache: &SharedCache,
    workspace: bool,
) -> Result<Arc<usvg::Tree>, String> {
    // Embedded image bytes and gradient stops are charged before admission. A hit avoids
    // parsing/base64 decoding during pan, zoom, bounds queries and repeated redraws.
    let candidate = if workspace {
        source.len() <= 4 * 1024 * 1024
    } else {
        source.len() <= 512 * 1024 && source.contains("<text")
    };
    let fingerprint = if candidate { fingerprint(source) } else { 0 };
    if candidate {
        if let Some(tree) = cache.lookup(source, fingerprint) {
            crate::performance::count("parsed_text_cache_hits", 1);
            return Ok(tree);
        }
    }
    crate::performance::count(
        if candidate {
            "parsed_text_cache_misses"
        } else {
            "parsed_text_cache_bypasses"
        },
        1,
    );
    let tree = Arc::new(parse_uncached(source)?);
    if candidate {
        if let Some(charge) = retained_charge_policy(source, &tree, workspace) {
            let entry = Entry::new(source, fingerprint, Arc::clone(&tree), charge);
            cache.insert(entry);
        }
    }
    Ok(tree)
}

// Accounted estimate, not RSS: source-owned metadata, nodes, paths and positioned glyphs
// with allocation headroom. The immutable system font database is shared, not charged per
// entry. Raster image kinds retain encoded bytes, not decoded RGBA. Nested SVG images,
// patterns and complex filters bypass retention. In-flight Arc users may outlive eviction.
#[cfg(test)]
fn retained_charge(source: &str, tree: &usvg::Tree) -> Option<usize> {
    retained_charge_policy(source, tree, false)
}
fn retained_charge_policy(source: &str, tree: &usvg::Tree, workspace: bool) -> Option<usize> {
    // Only fixed-size blur primitives are qualified for scene retention.
    let simple_filters = workspace
        && tree.filters().iter().all(|filter| {
            filter
                .primitives()
                .iter()
                .all(|p| matches!(p.kind(), usvg::filter::Kind::GaussianBlur(_)))
        });
    if (!tree.filters().is_empty() && !simple_filters)
        || !tree.patterns().is_empty()
        || (!workspace && !tree.masks().is_empty())
        || (!workspace
            && (!tree.linear_gradients().is_empty() || !tree.radial_gradients().is_empty()))
    {
        return None;
    }
    fn visit(
        group: &usvg::Group,
        bytes: &mut usize,
        depth: usize,
        has_text: &mut bool,
        workspace: bool,
    ) -> Option<()> {
        if depth > 64 || *bytes > MAX_BYTES {
            return None;
        }
        *bytes = bytes.saturating_add(1024 + group.id().len());
        for node in group.children() {
            *bytes = bytes.saturating_add(1024 + node.id().len());
            match node {
                usvg::Node::Image(image) => {
                    if !workspace {
                        return None;
                    }
                    let data = match image.kind() {
                        usvg::ImageKind::JPEG(data)
                        | usvg::ImageKind::PNG(data)
                        | usvg::ImageKind::GIF(data)
                        | usvg::ImageKind::WEBP(data) => data,
                        usvg::ImageKind::SVG(_) => return None,
                    };
                    *bytes = bytes.saturating_add(data.len().saturating_mul(2));
                }
                usvg::Node::Group(child) => visit(child, bytes, depth + 1, has_text, workspace)?,
                usvg::Node::Path(path) => {
                    *bytes = bytes.saturating_add(path.data().points().len().saturating_mul(32));
                }
                usvg::Node::Text(text) => {
                    *has_text = true;
                    for span in text.layouted() {
                        *bytes = bytes.saturating_add(1024);
                        for glyph in &span.positioned_glyphs {
                            *bytes = bytes.saturating_add(256 + glyph.text.len() * 2);
                        }
                    }
                }
            }
            let mut valid = true;
            node.subroots(|root| {
                valid &= visit(root, bytes, depth + 1, has_text, workspace).is_some();
            });
            if !valid || *bytes > MAX_BYTES {
                return None;
            }
        }
        Some(())
    }
    let mut bytes = source.len().saturating_mul(8).saturating_add(4096);
    for gradient in tree.linear_gradients() {
        bytes = bytes.saturating_add(1024 + gradient.stops().len().saturating_mul(256));
    }
    for gradient in tree.radial_gradients() {
        bytes = bytes.saturating_add(1024 + gradient.stops().len().saturating_mul(256));
    }
    for filter in tree.filters() {
        bytes = bytes.saturating_add(1024 + filter.primitives().len().saturating_mul(1024));
    }
    // usvg Tree::has_text_nodes checks subroots, but misses ordinary nested groups.
    // Detect text in the same complete traversal used for retained-memory accounting.
    let mut has_text = false;
    visit(tree.root(), &mut bytes, 0, &mut has_text, workspace)?;
    ((workspace || has_text) && bytes <= MAX_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200"><text x="10" y="40" font-size="24">English 日本語 简体中文</text></svg>"#;

    use base64::Engine;

    fn image_source() -> String {
        let mut png = Vec::new();
        let mut encoder = png::Encoder::new(&mut png, 64, 64);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let pixels: Vec<u8> = (0..64 * 64)
            .flat_map(|i| {
                [
                    (i % 64 * 4) as u8,
                    (i / 64 * 4) as u8,
                    170,
                    (i % 7 * 35) as u8,
                ]
            })
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
        format!(
            r#"<svg width="320" height="240" xmlns="http://www.w3.org/2000/svg"><g opacity=".65" transform="rotate(13 160 120)"><image x="-30" y="-20" width="380" height="280" href="data:image/png;base64,{}"/></g></svg>"#,
            base64::engine::general_purpose::STANDARD.encode(png)
        )
    }

    #[test]
    fn scenes_reuse_images_and_gradients_with_exact_pixels_and_invalidation() {
        let cache = SharedCache::new();
        let gradient = r##"<svg width="320" height="240"><defs><linearGradient id="g"><stop stop-color="#f80" stop-opacity=".3"/><stop offset="1" stop-color="#08f"/></linearGradient><radialGradient id="r"><stop stop-color="#f08"/><stop offset="1" stop-color="#0f8" stop-opacity="0"/></radialGradient></defs><path d="M-20 50 C180 -90 400 40 380 240 L-50 270Z" fill="url(#g)"/><circle cx="130" cy="130" r="80" fill="url(#r)" opacity=".5"/></svg>"##;
        for source in [image_source(), gradient.into()] {
            let first = parse_with_policy(&source, &cache, true).unwrap();
            let retained = parse_with_policy(&source, &cache, true).unwrap();
            assert!(Arc::ptr_eq(&first, &retained));
            let fresh = parse_uncached(&source).unwrap();
            for transform in [[1., 0., 0.], [2., -37., -19.], [0.75, 17.3, -4.7]] {
                let draw = |tree: &usvg::Tree| {
                    super::super::rasterize_tree_region(
                        tree,
                        [320, 240],
                        transform,
                        false,
                        false,
                        true,
                    )
                    .unwrap()
                    .raster
                    .pixels
                };
                assert_eq!(draw(&fresh), draw(&retained));
            }
            let edited = source.replace(".65", ".35").replace("#f80", "#080");
            assert_eq!(source.len(), edited.len());
            let changed = parse_with_policy(&edited, &cache, true).unwrap();
            assert!(!Arc::ptr_eq(&retained, &changed));
            assert!(Arc::ptr_eq(
                &retained,
                &parse_with_policy(&source, &cache, true).unwrap()
            ));
            let charge = retained_charge_policy(&source, &retained, true).unwrap();
            assert!(charge >= source.len() * 8);
            assert!(charge <= MAX_BYTES);
        }
        let tree = parse_uncached(gradient).unwrap();
        assert!(retained_charge_policy(&"x".repeat(MAX_BYTES / 8), &tree, true).is_none());
    }

    /// User artwork remains external; no licensed artwork is embedded in this test.
    #[test]
    #[ignore = "requires LUMAPAINT_COMPLEX_SVG; manual real-artwork benchmark"]
    fn complex_artwork_redraw_timing() {
        let path = std::env::var("LUMAPAINT_COMPLEX_SVG").expect("set LUMAPAINT_COMPLEX_SVG");
        let source = std::fs::read_to_string(path).unwrap();
        let start = std::time::Instant::now();
        let document =
            lumapaint_formats::svg::import("Complex artwork".into(), source.clone()).unwrap();
        eprintln!(
            "complex import: ms={:.3}, dimensions={:?}, preserved_svg_layers={}, native_objects={}",
            start.elapsed().as_secs_f64() * 1000.,
            document.dimensions(),
            document.svg_layers().count(),
            document
                .svg_layers()
                .map(|l| l.vector_objects.len())
                .sum::<usize>()
        );
        assert_eq!(document.svg_layers().next().unwrap().source, source);
        let cache = SharedCache::new();
        let start = std::time::Instant::now();
        let tree = parse_with_policy(&source, &cache, true).unwrap();
        let cold_ms = start.elapsed().as_secs_f64() * 1000.;
        let charge = retained_charge_policy(&source, &tree, true);
        assert!(
            Arc::ptr_eq(&tree, &parse_with_policy(&source, &cache, true).unwrap()),
            "real artwork must be admitted before reporting reuse"
        );
        eprintln!(
            "complex parse: cold_ms={cold_ms:.3}, charged_bytes={charge:?}, source_bytes={}",
            source.len()
        );
        let size = tree.size();
        let fit = (752. / size.width()).min(506. / size.height());
        for (name, zoom) in [("fit", fit), ("200", 2.), ("400", 4.)] {
            let mut baseline = Vec::new();
            let mut retained = Vec::new();
            for sample in 0..7 {
                let pan = if sample == 0 {
                    [0., 0.]
                } else {
                    [sample as f32 * 13.25, sample as f32 * -7.5]
                };
                let transform = [
                    zoom,
                    (800. - size.width() * zoom) * 0.5 + pan[0],
                    (554. - size.height() * zoom) * 0.5 + pan[1],
                ];
                let mut outputs = Vec::new();
                for fast in if sample % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let start = std::time::Instant::now();
                    let parsed = if fast {
                        parse_with_policy(&source, &cache, true).unwrap()
                    } else {
                        Arc::new(parse_uncached(&source).unwrap())
                    };
                    let raster = super::super::rasterize_tree_region(
                        &parsed,
                        [800, 554],
                        transform,
                        false,
                        false,
                        true,
                    )
                    .unwrap();
                    let ms = start.elapsed().as_secs_f64() * 1000.;
                    if sample != 0 {
                        if fast { &mut retained } else { &mut baseline }.push(ms);
                    }
                    outputs.push(raster.raster.pixels);
                }
                assert_eq!(outputs[0], outputs[1], "zoom={name}, pan sample={sample}");
                if sample == 0 {
                    if let Ok(dir) = std::env::var("LUMAPAINT_COMPLEX_OUTPUT") {
                        let mut data = Vec::new();
                        let mut encoder = png::Encoder::new(&mut data, 800, 554);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder
                            .write_header()
                            .unwrap()
                            .write_image_data(&outputs[0])
                            .unwrap();
                        std::fs::write(
                            std::path::Path::new(&dir).join(format!("complex-{name}.png")),
                            data,
                        )
                        .unwrap();
                    }
                }
            }
            baseline.sort_by(f64::total_cmp);
            retained.sort_by(f64::total_cmp);
            eprintln!("complex redraw zoom={name}: baseline_median_ms={:.3}, retained_median_ms={:.3}, baseline_max_ms={:.3}, retained_max_ms={:.3}, exact_pixel_pairs=7",
                (baseline[2]+baseline[3])*0.5,(retained[2]+retained[3])*0.5,baseline[5],retained[5]);
        }
    }

    #[test]
    #[ignore = "manual same-process scene parsing/redraw benchmark"]
    fn scene_redraw_timing() {
        let image = std::env::var("LUMAPAINT_PAN_IMAGE").map_or_else(|_| image_source(), |path| {
            format!(r#"<svg width="1672" height="941" xmlns="http://www.w3.org/2000/svg"><image width="1672" height="941" href="data:image/png;base64,{}"/></svg>"#,
                base64::engine::general_purpose::STANDARD.encode(std::fs::read(path).unwrap()))
        });
        let shapes = (0..600).map(|i| format!(r##"<ellipse cx="{}" cy="{}" rx="160" ry="110" fill="url(#g)" opacity=".18"/>"##, i * 47 % 1672, i * 31 % 941)).collect::<String>();
        let vector = format!(
            r##"<svg width="1672" height="941"><defs><linearGradient id="g"><stop stop-color="#08f"/><stop offset="1" stop-color="#f80" stop-opacity=".4"/></linearGradient></defs>{shapes}</svg>"##
        );
        for (name, source) in [("pixel", image), ("vector", vector)] {
            let cache = SharedCache::new();
            let retained = parse_with_policy(&source, &cache, true).unwrap();
            assert!(Arc::ptr_eq(
                &retained,
                &parse_with_policy(&source, &cache, true).unwrap()
            ));
            for draw in [false, true] {
                let mut baseline = Vec::new();
                let mut reused = Vec::new();
                for i in 0..10 {
                    for fast in if i % 2 == 0 {
                        [false, true]
                    } else {
                        [true, false]
                    } {
                        let start = std::time::Instant::now();
                        let tree = if fast {
                            parse_with_policy(&source, &cache, true).unwrap()
                        } else {
                            Arc::new(parse_uncached(&source).unwrap())
                        };
                        if draw {
                            std::hint::black_box(
                                super::super::rasterize_tree_region(
                                    &tree,
                                    [800, 554],
                                    [2., -800. - i as f32 * 13., -400.],
                                    false,
                                    false,
                                    true,
                                )
                                .unwrap(),
                            );
                        }
                        std::hint::black_box(tree);
                        if i > 0 {
                            if fast { &mut reused } else { &mut baseline }
                                .push(start.elapsed().as_secs_f64() * 1000.);
                        }
                    }
                }
                baseline.sort_by(f64::total_cmp);
                reused.sort_by(f64::total_cmp);
                eprintln!(
                    "scene benchmark {name} draw={draw}: baseline_ms={:.3} retained_ms={:.3}",
                    baseline[4], reused[4]
                );
            }
        }
    }

    #[test]
    fn workspace_reuses_shapes_and_masked_blur_but_not_complex_filters() {
        use resvg::tiny_skia;
        let cache = SharedCache::new();
        for body in ["<rect width='16' height='16'/>", "<defs><mask id='m'><rect width='32' height='32' fill='white'/></mask><filter id='b'><feGaussianBlur stdDeviation='1'/></filter></defs><g mask='url(#m)'><circle cx='16' cy='16' r='8' filter='url(#b)'/></g>"] {
            let svg = format!("<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'>{body}</svg>");
            let first = parse_with_policy(&svg,&cache,true).unwrap();
            let second = parse_with_policy(&svg,&cache,true).unwrap();
            assert!(Arc::ptr_eq(&first,&second));
            let fresh = parse_uncached(&svg).unwrap();
            let draw = |tree: &usvg::Tree| { let mut p=tiny_skia::Pixmap::new(32,32).unwrap(); resvg::render(tree,tiny_skia::Transform::identity(),&mut p.as_mut());p.take() };
            assert_eq!(draw(&first),draw(&fresh));
        }
        let source = "<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'><defs><filter id='b'><feTurbulence baseFrequency='.1'/></filter></defs><rect width='32' height='32' filter='url(#b)'/></svg>";
        let tree = parse_uncached(source).unwrap();
        assert!(retained_charge_policy(source, &tree, true).is_none());
    }

    #[test]
    fn shapes_comments_and_masks_do_not_load_fonts() {
        let options = options_with_font_loader(
            Arc::new(|| panic!("Shape SVG must not load or wait for fonts")),
            false,
        );
        for body in ["<rect width='16' height='16'/>","<!-- <text>not a node</text> --><circle r='8' cx='8' cy='8'/>","<defs><mask id='m'><rect width='16' height='16' fill='white'/></mask></defs><rect width='16' height='16' mask='url(#m)'/>"] {
            let tree=usvg::Tree::from_str(&format!("<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'>{body}</svg>"),&options).unwrap();
            assert!(tree.fontdb().is_empty());
        }
    }
    #[test]
    fn lazy_fonts_preserve_english_japanese_chinese_namespaces_uses_and_nested_svg_pixels() {
        use base64::Engine;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let mut db = usvg::fontdb::Database::new();
        db.load_font_data(include_bytes!("../../assets/fonts/RobotoFlex.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/fonts/NotoSansJP.ttf").to_vec());
        let db = Arc::new(db);
        let calls = Arc::new(AtomicUsize::new(0));
        let loader: FontLoader = {
            let db = db.clone();
            let calls = calls.clone();
            Arc::new(move || {
                calls.fetch_add(1, Ordering::Relaxed);
                db.clone()
            })
        };
        let eager = options_with_font_loader(loader.clone(), true);
        let lazy = options_with_font_loader(loader, false);
        let render = |tree: &usvg::Tree| {
            let mut pixmap = resvg::tiny_skia::Pixmap::new(200, 64).unwrap();
            resvg::render(
                tree,
                resvg::tiny_skia::Transform::identity(),
                &mut pixmap.as_mut(),
            );
            pixmap.take()
        };
        let text="<text x='4' y='32' font-family='Roboto Flex, Noto Sans JP' font-size='22'>English 日本語 简体中文</text>";
        let nested =
            format!("<svg xmlns='http://www.w3.org/2000/svg' width='200' height='64'>{text}</svg>");
        let encoded = base64::engine::general_purpose::STANDARD.encode(nested.as_bytes());
        let language_sources=["English", "日本語", "简体中文"].into_iter().map(|content|format!("<svg xmlns='http://www.w3.org/2000/svg' width='200' height='64'><text x='4' y='32' font-family='Roboto Flex, Noto Sans JP' font-size='22'>{content}</text></svg>"));
        for source in language_sources.chain([nested,format!("<s:svg xmlns:s='http://www.w3.org/2000/svg' width='200' height='64'>{}</s:svg>",text.replace("<text","<s:text").replace("</text>","</s:text>")),format!("<svg xmlns='http://www.w3.org/2000/svg' width='200' height='64'><defs><g id='t'>{text}</g></defs><use href='#t'/></svg>"),format!("<svg xmlns='http://www.w3.org/2000/svg' width='200' height='64'><image width='200' height='64' href='data:image/svg+xml;base64,{encoded}'/></svg>")]) {
            let a=usvg::Tree::from_str(&source,&eager).unwrap();let b=usvg::Tree::from_str(&source,&lazy).unwrap();
            let expected=render(&a);assert!(expected.as_chunks::<4>().0.iter().any(|p|p[3]>0));assert_eq!(expected,render(&b));
        }
        assert!(calls.load(Ordering::Relaxed) > 1);
    }
    #[test]
    #[ignore = "manual Release concurrent parsing benchmark, not GUI latency"]
    fn benchmark_shared_parsed_cache_workers() {
        if cfg!(debug_assertions) {
            panic!("run with --release");
        }
        assert!(
            crate::performance::enabled(),
            "enable LUMAPAINT_RENDER_METRICS"
        );
        const OPERATIONS: usize = 200;
        for bytes in [1024, 65536] {
            let sources: Arc<Vec<String>> = Arc::new(
                (0..8)
                    .map(|id| format!("{SOURCE}<!--{}-{id:04}-->", "x".repeat(bytes)))
                    .collect(),
            );
            let caches = [
                Arc::new(SharedCache::Serial(Mutex::default())),
                Arc::new(SharedCache::Parallel(RwLock::default())),
            ];
            // Identical prewarmed sources and independent, bounded caches.
            for cache in &caches {
                for source in sources.iter() {
                    parse_with_cache(source, cache).unwrap();
                }
            }
            for source_count in [1, 8] {
                for workers in [1, 4, 8] {
                    let ready = Arc::new(std::sync::Barrier::new(workers + 1));
                    let (results_tx, results_rx) = std::sync::mpsc::channel();
                    let mut senders = Vec::new();
                    let mut handles = Vec::new();
                    for worker in 0..workers {
                        let (jobs_tx, jobs_rx) =
                            std::sync::mpsc::channel::<(usize, std::time::Instant)>();
                        senders.push(jobs_tx);
                        let sources = Arc::clone(&sources);
                        let caches = caches.clone();
                        let ready = Arc::clone(&ready);
                        let results_tx = results_tx.clone();
                        handles.push(std::thread::spawn(move || {
                            crate::performance::take();
                            ready.wait();
                            while let Ok((kind, dispatched)) = jobs_rx.recv() {
                                let dispatch_delay = dispatched.elapsed();
                                let start = std::time::Instant::now();
                                for operation in 0..OPERATIONS {
                                    let source = &sources[(operation + worker) % source_count];
                                    std::hint::black_box(
                                        parse_with_cache(source, &caches[kind]).unwrap(),
                                    );
                                }
                                let elapsed = start.elapsed();
                                results_tx
                                    .send((crate::performance::take(), elapsed, dispatch_delay))
                                    .unwrap();
                            }
                        }));
                    }
                    drop(results_tx);
                    // Every worker has started before any measured dispatch.
                    ready.wait();
                    // Round zero warms dispatch/worker caches and is excluded from summaries.
                    for round in 0..8 {
                        for kind in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                            let start = std::time::Instant::now();
                            for sender in &senders {
                                sender.send((kind, std::time::Instant::now())).unwrap();
                            }
                            let results: Vec<_> =
                                (0..workers).map(|_| results_rx.recv().unwrap()).collect();
                            let wall_ms = start.elapsed().as_secs_f64() * 1000.0;
                            let mut total = crate::performance::Snapshot::default();
                            let mut worker_max_ms = 0.0_f64;
                            let mut dispatch_max_ms = 0.0_f64;
                            for (stats, elapsed, delay) in results {
                                total.merge(&stats);
                                worker_max_ms = worker_max_ms.max(elapsed.as_secs_f64() * 1000.0);
                                dispatch_max_ms = dispatch_max_ms.max(delay.as_secs_f64() * 1000.0);
                            }
                            let hits = total
                                .counts
                                .get("parsed_text_cache_hits")
                                .copied()
                                .unwrap_or(0);
                            let misses = total
                                .counts
                                .get("parsed_text_cache_misses")
                                .copied()
                                .unwrap_or(0);
                            assert_eq!(hits + misses, (workers * OPERATIONS) as u64);
                            let mode = ["serial", "parallel"][kind];
                            eprintln!("shared_cache mode={mode} sources={source_count} workers={workers} bytes={} round={round} ops={} wall_ms={wall_ms:.3} worker_max_ms={worker_max_ms:.3} dispatch_max_ms={dispatch_max_ms:.3} counts={:?} cpu_ns={:?}",
                            sources[0].len(), workers * OPERATIONS, total.counts, total.cpu_nanoseconds);
                        }
                    }
                    drop(senders);
                    for handle in handles {
                        handle.join().unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn contended_shared_cache_never_waits_or_changes_retained_entries() {
        crate::performance::take();
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let cache = RwLock::new(Cache::default());
        let key = fingerprint(SOURCE);
        insert_shared(&cache, Entry::new(SOURCE, key, Arc::clone(&tree), 100));
        let guard = cache.write().unwrap();
        // Same-thread ownership makes an accidental blocking lock reproducible.
        assert!(lookup_shared(&cache, SOURCE, key).is_none());
        let rejected_tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let rejected = Arc::downgrade(&rejected_tree);
        insert_shared(&cache, Entry::new(SOURCE, key, rejected_tree, 100));
        assert!(rejected.upgrade().is_none());
        assert_eq!(guard.bytes, 100);
        assert_eq!(guard.entries.len(), 1);
        drop(guard);
        assert!(Arc::ptr_eq(
            &tree,
            &lookup_shared(&cache, SOURCE, key).unwrap()
        ));
        if crate::performance::enabled() {
            let stats = crate::performance::take();
            assert_eq!(stats.counts["parsed_text_cache_lookup_contended"], 1);
            assert_eq!(stats.counts["parsed_text_cache_insert_contended"], 1);
            for timer in [
                "parsed_text_cache_lookup",
                "parsed_text_cache_insert",
                "parsed_text_cache_retirement",
            ] {
                assert!(stats.cpu_nanoseconds.contains_key(timer));
            }
        }
    }

    #[test]
    fn shared_readers_reuse_hits_when_lru_promotion_is_contended() {
        crate::performance::take();
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let cache = RwLock::new(Cache::default());
        let key = fingerprint(SOURCE);
        insert_shared(&cache, Entry::new(SOURCE, key, Arc::clone(&tree), 100));
        insert_shared(&cache, Entry::new("other", 0, Arc::clone(&tree), 100));
        let reader = cache.read().unwrap();
        assert!(Arc::ptr_eq(
            &tree,
            &lookup_shared(&cache, SOURCE, key).unwrap()
        ));
        assert_eq!(reader.entries.len(), 2);
        assert_eq!(reader.bytes, 200);
        drop(reader);
        assert!(Arc::ptr_eq(
            &tree,
            &lookup_shared(&cache, SOURCE, key).unwrap()
        ));
        if crate::performance::enabled() {
            let stats = crate::performance::take();
            assert_eq!(stats.counts["parsed_text_cache_promotion_skipped"], 1);
            assert!(!stats
                .counts
                .contains_key("parsed_text_cache_lookup_contended"));
        }
    }

    #[test]
    fn newest_shared_hit_needs_no_exclusive_promotion() {
        crate::performance::take();
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let cache = RwLock::new(Cache::default());
        let key = fingerprint(SOURCE);
        insert_shared(&cache, Entry::new(SOURCE, key, Arc::clone(&tree), 100));
        let reader = cache.read().unwrap();
        for _ in 0..200 {
            assert!(Arc::ptr_eq(
                &tree,
                &lookup_shared(&cache, SOURCE, key).unwrap()
            ));
        }
        assert_eq!(reader.entries.len(), 1);
        assert_eq!(reader.bytes, 100);
        if crate::performance::enabled() {
            let stats = crate::performance::take();
            assert_eq!(stats.counts["parsed_text_cache_promotion_unneeded"], 200);
            assert!(!stats
                .counts
                .contains_key("parsed_text_cache_promotion_skipped"));
            assert!(!stats
                .counts
                .contains_key("parsed_text_cache_lookup_contended"));
        }
    }

    #[test]
    fn shared_promotion_uses_entry_identity_even_with_collision_and_shared_tree() {
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let cache = RwLock::new(Cache::default());
        insert_shared(&cache, Entry::new("first", 7, Arc::clone(&tree), 100));
        insert_shared(&cache, Entry::new("second", 7, Arc::clone(&tree), 100));
        assert!(Arc::ptr_eq(
            &tree,
            &lookup_shared(&cache, "second", 7).unwrap()
        ));
        assert_eq!(
            cache.read().unwrap().entries.last().unwrap().source,
            "second"
        );
        lookup_shared(&cache, "first", 7).unwrap();
        assert_eq!(
            cache.read().unwrap().entries.last().unwrap().source,
            "first"
        );
    }

    #[test]
    #[ignore = "manual Release lookup benchmark, not application latency"]
    fn benchmark_parsed_cache_lookup() {
        if cfg!(debug_assertions) {
            panic!("run with --release");
        }
        assert!(
            !crate::performance::enabled(),
            "disable LUMAPAINT_RENDER_METRICS"
        );
        fn legacy_get(cache: &mut Cache, source: &str) -> Option<Arc<usvg::Tree>> {
            let index = cache
                .entries
                .iter()
                .position(|entry| entry.source == source)?;
            let entry = cache.entries.remove(index);
            let tree = Arc::clone(&entry.tree);
            cache.entries.push(entry);
            Some(tree)
        }
        const ITERATIONS: usize = 5000;
        const ROUNDS: usize = 7;
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        for (entries, size) in [(1, 1024), (8, 8192), (32, 65536), (4, 511000)] {
            let padding = "x".repeat(size - SOURCE.len() - 12);
            let source = |id: usize| format!("{SOURCE}<!--{padding}{id:04}-->");
            let mut legacy = Cache::default();
            let mut current = Cache::default();
            for i in 0..entries {
                let text = source(i);
                let charge = retained_charge(&text, &tree).unwrap();
                legacy.insert(Entry::new(
                    &text,
                    fingerprint(&text),
                    Arc::clone(&tree),
                    charge,
                ));
                current.insert(Entry::new(
                    &text,
                    fingerprint(&text),
                    Arc::clone(&tree),
                    charge,
                ));
            }
            assert_eq!(current.entries.len(), entries);
            for (mode, query) in [
                ("latest_hit", source(entries - 1)),
                ("same_length_miss", source(9999)),
            ] {
                let mut samples = [Vec::new(), Vec::new()];
                for round in 0..ROUNDS {
                    // Alternate ordering to avoid always warming one implementation first.
                    for kind in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                        let start = std::time::Instant::now();
                        for _ in 0..ITERATIONS {
                            let query = std::hint::black_box(query.as_str());
                            let result = if kind == 0 {
                                legacy_get(std::hint::black_box(&mut legacy), query)
                            } else {
                                let key = fingerprint(query);
                                std::hint::black_box(&mut current).get(query, key)
                            };
                            assert_eq!(result.is_some(), mode == "latest_hit");
                            std::hint::black_box(result);
                        }
                        samples[kind].push(start.elapsed().as_secs_f64() * 1e9 / ITERATIONS as f64);
                    }
                }
                for sample in &mut samples {
                    sample.sort_by(f64::total_cmp);
                }
                eprintln!("cache_lookup entries={entries} bytes={} mode={mode} rounds={ROUNDS} ops={ITERATIONS} legacy_median_ns={:.1} current_median_ns={:.1} legacy_batch_max_ns={:.1} current_batch_max_ns={:.1}",
                    query.len(), samples[0][3], samples[1][3], samples[0][6], samples[1][6]);
            }
        }
    }
    #[test]
    fn fingerprints_filter_long_shared_prefixes_and_collisions_remain_exact() {
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let prefix = "x".repeat(128 * 1024);
        let mut cache = Cache::default();
        for i in 0..MAX_ENTRIES {
            let source = format!("{prefix}{i}");
            cache.insert(Entry::new(
                &source,
                fingerprint(&source),
                Arc::clone(&tree),
                1,
            ));
        }
        let miss = format!("{prefix}99");
        assert_eq!(cache.lookup_index(&miss, fingerprint(&miss)), (None, 0));
        let hit = format!("{prefix}31");
        assert_eq!(cache.lookup_index(&hit, fingerprint(&hit)), (Some(31), 1));
        assert!(Arc::ptr_eq(
            &tree,
            &cache.get(&hit, fingerprint(&hit)).unwrap()
        ));
        let sample_a = "x".repeat(256);
        let mut sample_b = sample_a.clone();
        sample_b.replace_range(60..61, "y");
        assert_eq!(fingerprint(&sample_a), fingerprint(&sample_b));
        let mut collision = Cache::default();
        let other = Arc::new(parse_uncached(&SOURCE.replace("English", "Other")).unwrap());
        collision.insert(Entry::new("a", 7, Arc::clone(&tree), 1));
        collision.insert(Entry::new("b", 7, Arc::clone(&other), 1));
        assert_eq!(collision.lookup_index("b", 7), (Some(1), 2));
        assert!(Arc::ptr_eq(&other, &collision.get("b", 7).unwrap()));
        assert!(Arc::ptr_eq(&tree, &collision.get("a", 7).unwrap()));
        assert!(collision.get("c", 7).is_none());
        collision.insert(Entry::new(
            &sample_a,
            fingerprint(&sample_a),
            Arc::clone(&tree),
            1,
        ));
        collision.insert(Entry::new(
            &sample_b,
            fingerprint(&sample_b),
            Arc::clone(&other),
            1,
        ));
        assert!(Arc::ptr_eq(
            &other,
            &collision.get(&sample_b, fingerprint(&sample_b)).unwrap()
        ));
        assert!(Arc::ptr_eq(
            &tree,
            &collision.get(&sample_a, fingerprint(&sample_a)).unwrap()
        ));
    }
    #[test]
    fn retired_tree_ownership_outlives_lock_and_is_released_afterwards() {
        let old = Arc::new(parse_uncached(SOURCE).unwrap());
        let weak = Arc::downgrade(&old);
        let cache = Mutex::new(Cache::default());
        let entry = Entry::new("old", fingerprint("old"), Arc::clone(&old), 1);
        let retired = { cache.lock().unwrap().insert(entry) };
        drop(retired);
        drop(old);
        let next = Entry::new(
            "new",
            fingerprint("new"),
            Arc::new(parse_uncached(SOURCE).unwrap()),
            MAX_BYTES,
        );
        let retired = { cache.lock().unwrap().insert(next) };
        assert!(
            cache.try_lock().is_ok(),
            "evicted resources must outlive the lock"
        );
        assert!(weak.upgrade().is_some());
        drop(retired);
        assert!(weak.upgrade().is_none());
    }
    #[test]
    fn nested_text_frames_are_retained_without_repeated_shaping() {
        let source = SOURCE
            .replace("<text", "<g><g><text")
            .replace("</text>", "</text></g></g>");
        let tree = Arc::new(parse_uncached(&source).unwrap());
        let charge = retained_charge(&source, &tree).expect("nested text must be cacheable");
        let mut cache = Cache::default();
        cache.insert(Entry::new(
            &source,
            fingerprint(&source),
            Arc::clone(&tree),
            charge,
        ));
        assert!(Arc::ptr_eq(
            &tree,
            &cache.get(&source, fingerprint(&source)).unwrap()
        ));
        assert!(retained_charge("<svg/>", &parse_uncached(r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#).unwrap()).is_none());
    }

    #[test]
    fn reuse_is_exact_and_eviction_obeys_both_limits() {
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let mut cache = Cache::default();
        for i in 0..MAX_ENTRIES {
            cache.insert(Entry::new(
                &i.to_string(),
                fingerprint(&i.to_string()),
                Arc::clone(&tree),
                1,
            ));
        }
        assert!(Arc::ptr_eq(
            &tree,
            &cache.get("0", fingerprint("0")).unwrap()
        ));
        cache.insert(Entry::new(
            "next",
            fingerprint("next"),
            Arc::clone(&tree),
            1,
        ));
        assert!(cache.get("1", fingerprint("1")).is_none());
        assert!(cache.get("0", fingerprint("0")).is_some());
        cache.insert(Entry::new(
            "large",
            fingerprint("large"),
            Arc::clone(&tree),
            MAX_BYTES,
        ));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, MAX_BYTES);
        cache.insert(Entry::new(
            "too-large",
            fingerprint("too-large"),
            Arc::clone(&tree),
            MAX_BYTES + 1,
        ));
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.get("large ", fingerprint("large ")).is_none());
        assert!(parse_uncached("invalid").is_err());
    }

    #[test]
    fn cached_tree_matches_fresh_at_multiple_sizes_and_text_versions() {
        for content in [
            "English 日本語 简体中文",
            "ffi e\u{301} 日本語",
            "GPU 123",
            "",
        ] {
            for attributes in [
                "",
                "writing-mode=\"vertical-rl\"",
                "transform=\"rotate(12 100 80)\" opacity=\".5\"",
            ] {
                let source = format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200"><defs><clipPath id="c"><rect width="250" height="180"/></clipPath></defs><g clip-path="url(#c)"><text x="30" y="40" font-size="24" {attributes}>{content}</text></g></svg>"#
                );
                let fresh = parse_uncached(&source).unwrap();
                let cached = parse(&source).unwrap();
                // Other tests/workers may evict this entry; correctness cannot depend on a hit.
                for size in [64, 256] {
                    let transform = [size as f32 / 300., 0., 0.];
                    let a = super::super::rasterize_tree_region(
                        &fresh,
                        [size, size],
                        transform,
                        false,
                        false,
                        false,
                    )
                    .unwrap();
                    let b = super::super::rasterize_tree_region(
                        &cached,
                        [size, size],
                        transform,
                        false,
                        false,
                        false,
                    )
                    .unwrap();
                    assert_eq!(a.raster.pixels, b.raster.pixels);
                }
            }
        }
    }

    #[test]
    fn resource_heavy_text_bypasses_retention() {
        let source = SOURCE.replace("<text", "<defs><filter id=\"b\"><feGaussianBlur stdDeviation=\"2\"/></filter></defs><text filter=\"url(#b)\"");
        assert!(retained_charge(&source, &parse_uncached(&source).unwrap()).is_none());
        assert!(retained_charge(SOURCE, &parse_uncached(SOURCE).unwrap()).is_some());
    }

    #[test]
    fn concurrent_readers_keep_identical_output() {
        let tree = parse_uncached(SOURCE).unwrap();
        let render = |tree: &usvg::Tree| {
            let mut pixels = super::super::tiny_skia::Pixmap::new(300, 200).unwrap();
            resvg::render(tree, Default::default(), &mut pixels.as_mut());
            pixels.take()
        };
        let expected = render(&tree);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let expected = &expected;
                let render = &render;
                scope.spawn(move || {
                    for _ in 0..8 {
                        assert_eq!(&render(&parse(SOURCE).unwrap()), expected);
                    }
                });
            }
        });
    }
}
