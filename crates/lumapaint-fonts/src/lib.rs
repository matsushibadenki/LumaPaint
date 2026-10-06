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
    db.load_system_fonts();
    let mut names: std::collections::HashSet<String> =
        db.faces().map(|f| f.post_script_name.clone()).collect();
    let mut additional = fontdb::Database::new();
    for path in adobe_font_paths() {
        let _ = additional.load_font_file(path);
    }
    for face in additional.faces() {
        if names.insert(face.post_script_name.clone()) {
            db.push_face_info(face.clone());
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
