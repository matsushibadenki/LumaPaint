//! Read-only discovery of desktop font files, including Adobe's hidden LiveType cache.
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::OnceLock,
};

/// Scan only the font directories; encrypted caches and entitlement metadata are excluded.
/// File signatures, rather than extensions or visibility, determine candidates.
pub fn discover_livetype(root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for directory in [".r", ".t", ".w"] {
        let dir = root.join(directory);
        if !fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.take(16384).flatten() {
            let path = entry.path();
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_file() || metadata.len() < 12 || metadata.len() > 64 * 1024 * 1024 {
                continue;
            }
            let mut header = [0; 12];
            if fs::File::open(&path)
                .and_then(|mut f| f.read_exact(&mut header))
                .is_ok()
                && [*b"OTTO", [0, 1, 0, 0], *b"ttcf", *b"true"]
                    .contains(&header[..4].try_into().unwrap())
                && (&header[..4] != b"ttcf"
                    || (1..=64).contains(&u32::from_be_bytes(header[8..12].try_into().unwrap())))
            {
                paths.push(path);
            }
        }
    }
    paths.sort();
    paths
}

pub fn adobe_font_paths() -> &'static [PathBuf] {
    static PATHS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    PATHS.get_or_init(|| {
        #[cfg(target_os = "macos")]
        if let Some(home) = std::env::var_os("HOME") {
            return discover_livetype(
                &PathBuf::from(home)
                    .join("Library/Application Support/Adobe/CoreSync/plugins/livetype"),
            );
        }
        Vec::new()
    })
}

/// Preserve system fonts and add readable cache files without installing or changing them.
pub fn load_system_fonts(db: &mut fontdb::Database) {
    let started = std::time::Instant::now();
    db.load_system_fonts();
    if std::env::var_os("LUMAPAINT_ONION_METRICS").is_some() {
        eprintln!(
            "lumapaint-onion-stage stage=system_font_files elapsed_us={}",
            started.elapsed().as_micros()
        );
    }
    let started = std::time::Instant::now();
    let mut names: std::collections::HashSet<String> =
        db.faces().map(|f| f.post_script_name.clone()).collect();
    let mut additional = fontdb::Database::new();
    for path in adobe_font_paths() {
        let _ = additional.load_font_file(path);
    }
    if std::env::var_os("LUMAPAINT_ONION_METRICS").is_some() {
        eprintln!(
            "lumapaint-onion-stage stage=adobe_font_files elapsed_us={}",
            started.elapsed().as_micros()
        );
    }
    for face in additional.faces() {
        if names.insert(face.post_script_name.clone()) {
            db.push_face_info(face.clone());
        }
    }
}

/// Repair missing generic aliases identically for rendering and document I/O.
/// Keep working OS/fontconfig mappings and use an explicit preference order
/// instead of filesystem discovery order (which differs across hosts).
pub fn resolve_generic_families(db: &mut fontdb::Database) {
    use fontdb::{Family, Query};
    for (family, preferred) in [
        (
            Family::SansSerif,
            &[
                "Arial",
                "Helvetica",
                "DejaVu Sans",
                "Liberation Sans",
                "Noto Sans",
            ][..],
        ),
        (
            Family::Serif,
            &[
                "Times New Roman",
                "Times",
                "DejaVu Serif",
                "Liberation Serif",
                "Noto Serif",
            ][..],
        ),
        (
            Family::Monospace,
            &[
                "Courier New",
                "Courier",
                "DejaVu Sans Mono",
                "Liberation Mono",
                "Noto Sans Mono",
            ][..],
        ),
        (
            Family::Cursive,
            &["Comic Sans MS", "Apple Chancery", "URW Chancery L"][..],
        ),
        (Family::Fantasy, &["Impact", "Papyrus"][..]),
    ] {
        if db
            .query(&Query {
                families: &[family],
                ..Default::default()
            })
            .is_some()
        {
            continue;
        }
        let name = preferred
            .iter()
            .find_map(|wanted| {
                db.faces()
                    .flat_map(|f| &f.families)
                    .find(|(name, _)| name.eq_ignore_ascii_case(wanted))
                    .map(|(name, _)| name.clone())
            })
            .or_else(|| {
                db.faces()
                    .filter(|f| family != Family::Monospace || f.monospaced)
                    .flat_map(|f| &f.families)
                    .map(|(name, _)| name.clone())
                    .min()
            })
            .or_else(|| {
                db.faces()
                    .flat_map(|f| &f.families)
                    .map(|(name, _)| name.clone())
                    .min()
            });
        if let Some(name) = name {
            match family {
                Family::SansSerif => db.set_sans_serif_family(name),
                Family::Serif => db.set_serif_family(name),
                Family::Monospace => db.set_monospace_family(name),
                Family::Cursive => db.set_cursive_family(name),
                Family::Fantasy => db.set_fantasy_family(name),
                Family::Name(_) => unreachable!(),
            }
        }
    }
}

/// Register only in this process so AppKit's native editor uses the same fonts.
#[cfg(target_os = "macos")]
pub fn register_native_fonts() {
    use std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: *const c_void,
            bytes: *const u8,
            len: isize,
            is_directory: u8,
        ) -> *const c_void;
        fn CFRelease(value: *const c_void);
    }
    #[link(name = "CoreText", kind = "framework")]
    unsafe extern "C" {
        fn CTFontManagerRegisterFontsForURL(
            url: *const c_void,
            scope: u32,
            error: *mut *const c_void,
        ) -> u8;
    }
    static REGISTERED: OnceLock<()> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        use std::os::unix::ffi::OsStrExt;
        for path in adobe_font_paths() {
            let bytes = path.as_os_str().as_bytes();
            // The URL and any returned CFError are owned references, released after registration.
            unsafe {
                let url = CFURLCreateFromFileSystemRepresentation(
                    std::ptr::null(),
                    bytes.as_ptr(),
                    bytes.len() as isize,
                    0,
                );
                if url.is_null() {
                    continue;
                }
                let mut error = std::ptr::null();
                CTFontManagerRegisterFontsForURL(url, 1, &mut error);
                if !error.is_null() {
                    CFRelease(error);
                }
                CFRelease(url);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_repair_is_independent_of_font_discovery_order() {
        use fontdb::{Database, Family, Query};
        let mut fixture = Database::new();
        fixture.load_font_data(
            include_bytes!("../../lumapaint-formats/tests/fixtures/lp-pdf-test.ttf").to_vec(),
        );
        let face = fixture.faces().next().unwrap().clone();
        let families = [
            "Noto Sans CJK JP",
            "Liberation Sans",
            "DejaVu Sans",
            "Noto Serif CJK JP",
            "DejaVu Serif",
            "Liberation Mono",
            "DejaVu Sans Mono",
        ];
        for reverse in [false, true] {
            let mut db = Database::new();
            let mut ordered = families.to_vec();
            if reverse {
                ordered.reverse();
            }
            for name in ordered {
                let mut aliased = face.clone();
                aliased.families = vec![(name.into(), face.families[0].1)];
                aliased.post_script_name = name.replace(' ', "");
                aliased.monospaced = name.contains("Mono");
                db.push_face_info(aliased);
            }
            // Simulate Linux without Arial/Times New Roman/Courier New.
            // Picking the first family containing "sans" previously chose CJK
            // or Mono in the renderer, unlike the PDF exporter's DejaVu Sans.
            db.set_sans_serif_family("Missing Sans");
            db.set_serif_family("Missing Serif");
            db.set_monospace_family("Missing Mono");
            resolve_generic_families(&mut db);
            for (generic, expected) in [
                (Family::SansSerif, "DejaVu Sans"),
                (Family::Serif, "DejaVu Serif"),
                (Family::Monospace, "DejaVu Sans Mono"),
            ] {
                assert_eq!(db.family_name(&generic), expected);
                assert!(db
                    .query(&Query {
                        families: &[generic],
                        ..Default::default()
                    })
                    .is_some());
            }
            // A valid user/fontconfig selection must not be overridden.
            db.set_sans_serif_family("Liberation Sans");
            resolve_generic_families(&mut db);
            assert_eq!(db.family_name(&Family::SansSerif), "Liberation Sans");
        }
    }

    #[test]
    fn hidden_fonts_are_detected_by_signature() {
        let root = tempfile::tempdir().unwrap();
        for dir in [".r", ".t", ".w", ".e"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        for (dir, name, magic) in [
            (".r", ".123.otf", *b"OTTO"),
            (".t", ".456", [0, 1, 0, 0]),
            (".w", ".789", *b"ttcf"),
            (".e", ".encrypted", *b"OTTO"),
            (".r", ".invalid.otf", *b"xxxx"),
        ] {
            let mut data = magic.to_vec();
            data.extend_from_slice(&[0; 12]);
            if magic == *b"ttcf" {
                data[8..12].copy_from_slice(&1u32.to_be_bytes());
            }
            fs::write(root.path().join(dir).join(name), data).unwrap();
        }
        assert_eq!(discover_livetype(root.path()).len(), 3);
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_are_excluded() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".r")).unwrap();
        fs::write(root.path().join("font"), b"OTTO000000000000").unwrap();
        std::os::unix::fs::symlink(root.path().join("font"), root.path().join(".r/.font")).unwrap();
        assert!(discover_livetype(root.path()).is_empty());
    }
}
