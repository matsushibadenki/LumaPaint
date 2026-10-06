//! Read-only diagnostic for locally installed Adobe Fonts; no font data is exported.
fn main() {
    let paths = lumapaint_fonts::adobe_font_paths();
    let mut db = fontdb::Database::new();
    lumapaint_fonts::load_system_fonts(&mut db);
    let cache_faces = db
        .faces()
        .filter(|f| match &f.source {
            fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => {
                paths.contains(path)
            }
            _ => false,
        })
        .count();
    #[cfg(target_os = "macos")]
    lumapaint_fonts::register_native_fonts();
    println!(
        "readable cache files: {}; additional cache faces: {}; total faces: {}",
        paths.len(),
        cache_faces,
        db.faces().count()
    );
    assert!(
        paths.is_empty() || cache_faces > 0,
        "No cache fonts resolved"
    );
}
