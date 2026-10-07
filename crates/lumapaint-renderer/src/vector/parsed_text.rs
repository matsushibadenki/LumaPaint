//! Resolution-independent text shaping cache. Parsing and drawing never hold its lock.
use super::{font_resolver, system_fonts, usvg};
use std::sync::{Arc, Mutex, OnceLock};

const MAX_ENTRIES: usize = 32;
const MAX_BYTES: usize = 32 * 1024 * 1024;

struct Entry {
    source: String,
    tree: Arc<usvg::Tree>,
    charge: usize,
}

#[derive(Default)]
struct Cache {
    // Oldest first. A small bounded list avoids hash collisions and duplicate key storage.
    entries: Vec<Entry>,
    bytes: usize,
}

impl Cache {
    fn get(&mut self, source: &str) -> Option<Arc<usvg::Tree>> {
        let index = self
            .entries
            .iter()
            .position(|entry| entry.source == source)?;
        let entry = self.entries.remove(index);
        let tree = Arc::clone(&entry.tree);
        self.entries.push(entry);
        Some(tree)
    }

    fn insert(&mut self, source: &str, tree: Arc<usvg::Tree>, charge: usize) {
        if charge > MAX_BYTES || self.get(source).is_some() {
            return;
        }
        while self.bytes + charge > MAX_BYTES || self.entries.len() >= MAX_ENTRIES {
            let old = self.entries.remove(0);
            self.bytes -= old.charge;
        }
        self.bytes += charge;
        self.entries.push(Entry {
            source: source.into(),
            tree,
            charge,
        });
    }
}

pub(super) fn parse_uncached(source: &str) -> Result<usvg::Tree, String> {
    let options = usvg::Options {
        fontdb: system_fonts(),
        font_resolver: font_resolver(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    usvg::Tree::from_str(source, &options).map_err(|error| error.to_string())
}

pub(super) fn parse(source: &str) -> Result<Arc<usvg::Tree>, String> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    // Ordinary vectors/images retain their existing path. Namespaced text remains correct,
    // but bypasses this optimization. Bound source retention before doing any extra traversal.
    let candidate = source.len() <= 512 * 1024 && source.contains("<text");
    if candidate {
        if let Ok(mut cache) = cache.try_lock() {
            if let Some(tree) = cache.get(source) {
                return Ok(tree);
            }
        }
    }
    let tree = Arc::new(parse_uncached(source)?);
    if candidate {
        if let Some(charge) = retained_charge(source, &tree) {
            if let Ok(mut cache) = cache.try_lock() {
                cache.insert(source, Arc::clone(&tree), charge);
            }
        }
    }
    Ok(tree)
}

// Accounted estimate, not RSS: source-owned metadata, nodes, paths and positioned glyphs
// with allocation headroom. The immutable system font database is shared, not charged per
// entry. Images (including bitmap emoji), filters and paint servers bypass retention to
// avoid hiding large decoded resources. In-flight Arc users may outlive cache eviction.
fn retained_charge(source: &str, tree: &usvg::Tree) -> Option<usize> {
    if !tree.has_text_nodes()
        || !tree.filters().is_empty()
        || !tree.patterns().is_empty()
        || !tree.masks().is_empty()
        || !tree.linear_gradients().is_empty()
        || !tree.radial_gradients().is_empty()
    {
        return None;
    }
    fn visit(group: &usvg::Group, bytes: &mut usize, depth: usize) -> Option<()> {
        if depth > 64 || *bytes > MAX_BYTES {
            return None;
        }
        *bytes = bytes.saturating_add(1024 + group.id().len());
        for node in group.children() {
            *bytes = bytes.saturating_add(1024 + node.id().len());
            match node {
                usvg::Node::Image(_) => return None,
                usvg::Node::Group(child) => visit(child, bytes, depth + 1)?,
                usvg::Node::Path(path) => {
                    *bytes = bytes.saturating_add(path.data().points().len().saturating_mul(32));
                }
                usvg::Node::Text(text) => {
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
                valid &= visit(root, bytes, depth + 1).is_some();
            });
            if !valid || *bytes > MAX_BYTES {
                return None;
            }
        }
        Some(())
    }
    let mut bytes = source.len().saturating_mul(8).saturating_add(4096);
    visit(tree.root(), &mut bytes, 0)?;
    (bytes <= MAX_BYTES).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="300" height="200"><text x="10" y="40" font-size="24">English 日本語 简体中文</text></svg>"#;

    #[test]
    fn reuse_is_exact_and_eviction_obeys_both_limits() {
        let tree = Arc::new(parse_uncached(SOURCE).unwrap());
        let mut cache = Cache::default();
        for i in 0..MAX_ENTRIES {
            cache.insert(&i.to_string(), Arc::clone(&tree), 1);
        }
        assert!(Arc::ptr_eq(&tree, &cache.get("0").unwrap()));
        cache.insert("next", Arc::clone(&tree), 1);
        assert!(cache.get("1").is_none());
        assert!(cache.get("0").is_some());
        cache.insert("large", Arc::clone(&tree), MAX_BYTES);
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.bytes, MAX_BYTES);
        cache.insert("too-large", Arc::clone(&tree), MAX_BYTES + 1);
        assert_eq!(cache.entries.len(), 1);
        assert!(cache.get("large ").is_none());
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
                    )
                    .unwrap();
                    let b = super::super::rasterize_tree_region(
                        &cached,
                        [size, size],
                        transform,
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
