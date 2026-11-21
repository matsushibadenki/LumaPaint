use edfx::{Document, Paragraph};
use std::{env, fs, path::PathBuf};

#[test]
#[ignore = "requires an editable CJK OpenType font; set EDFX_TEST_FONT"]
fn three_language_roundtrip_and_edit() {
    let font_path = env::var_os("EDFX_TEST_FONT")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/Library/Fonts/NotoSansCJKjp-VF.otf".into());
    let font = fs::read(&font_path).unwrap_or_else(|error| {
        panic!(
            "{}: {error}; set EDFX_TEST_FONT to an editable CJK OpenType font",
            font_path.display()
        )
    });
    let paragraphs: Vec<Paragraph> =
        serde_json::from_str(include_str!("../fixtures/paragraphs.json")).unwrap();
    let document = Document::new(paragraphs.clone(), &font, 0).unwrap();
    for index in 0..3 {
        let lines: Vec<_> = document
            .snapshot()
            .lines
            .iter()
            .filter(|line| line.paragraph == index)
            .collect();
        assert!(lines.len() >= 2, "paragraph {index} must exercise wrapping");
        assert!(lines.iter().all(|line| !line.glyphs.is_empty()));
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.edfx");
    document.save(&path).unwrap();
    let mut reopened = Document::load(&path).unwrap();
    assert_eq!(reopened.paragraphs(), document.paragraphs());
    assert_eq!(reopened.snapshot(), document.snapshot());

    let original_bytes = fs::read(&path).unwrap();
    assert!(
        reopened
            .replace_text(1, "固定ページ。".repeat(300))
            .is_err()
    );
    assert_eq!(reopened.paragraphs(), document.paragraphs());
    assert_eq!(reopened.snapshot(), document.snapshot());
    assert_eq!(fs::read(&path).unwrap(), original_bytes);

    reopened.replace_text(1, "短い文章です。".into()).unwrap();
    let edited = reopened.snapshot();
    assert_eq!(
        edited
            .lines
            .iter()
            .filter(|line| line.paragraph == 1)
            .count(),
        1
    );
    let first_chinese_line = |doc: &Document| {
        doc.snapshot()
            .lines
            .iter()
            .find(|line| line.paragraph == 2)
            .unwrap()
            .glyphs[0]
            .y
    };
    assert!(first_chinese_line(&reopened) < first_chinese_line(&document));
    reopened.save(&path).unwrap();
    let edited_reload = Document::load(&path).unwrap();
    assert_eq!(edited_reload.paragraphs()[1].text, "短い文章です。");
    assert_eq!(edited_reload.snapshot(), reopened.snapshot());

    let mut saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    saved["snapshot"]["lines"][0]["glyphs"][0]["x"] = 123.into();
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(
        Document::load(&path)
            .unwrap_err()
            .to_string()
            .contains("integrity check")
    );
}

#[test]
fn rejects_outline_overflow_without_committing_the_edit() {
    use edfx::Locale;

    // Both glyphs advance 600 units. B alone has ink 36 pt left of its origin,
    // which is 30 pt from the page edge. The line advance and baseline both fit.
    let font = include_bytes!("../fixtures/boundary-test.ttf");
    let paragraph = |text: &str| Paragraph {
        locale: Locale::En,
        text: text.into(),
    };
    let creation_error = Document::new(vec![paragraph("B")], font, 0).unwrap_err();
    assert!(
        creation_error
            .to_string()
            .contains("extends outside the page")
    );

    let original = Document::new(vec![paragraph("A")], font, 0).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("page.edfx");
    original.save(&path).unwrap();
    let original_bytes = fs::read(&path).unwrap();

    let mut reopened = Document::load(&path).unwrap();
    let error = reopened.replace_text(0, "B".into()).unwrap_err();
    assert!(error.to_string().contains("extends outside the page"));
    assert_eq!(reopened.paragraphs(), original.paragraphs());
    assert_eq!(reopened.snapshot(), original.snapshot());
    reopened.save(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original_bytes);

    reopened.replace_text(0, "AA".into()).unwrap();
    assert_eq!(reopened.paragraphs()[0].text, "AA");
    assert_eq!(reopened.snapshot().lines[0].glyphs.len(), 2);
}
