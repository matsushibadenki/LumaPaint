//! Portable, flattened AE exchange: full-resolution PNG sequence plus ExtendScript.
//! Rendering is supplied by the Rust host; no frame pixels cross the WebView bridge.
use std::io::{Cursor, Write};

const MAX_PACKAGE: usize = 256 * 1024 * 1024;

pub fn package(
    width: u32,
    height: u32,
    fps: u32,
    frames: u32,
    mut render: impl FnMut(u32) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    if width == 0
        || height == 0
        || width > 30000
        || height > 30000
        || u64::from(width) * u64::from(height) > 16_777_216
        || !(1..=99).contains(&fps)
        || !(1..=10000).contains(&frames)
    {
        return Err("AE export: invalid dimensions, fps (1–99), or frames (1–10000) / AE書き出しのサイズ・fps・フレーム数が範囲外です / AE导出的尺寸、帧率或帧数超出范围".into());
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut total = 0usize;
    for frame in 0..frames {
        let png = render(frame)?;
        total = total
            .checked_add(png.len())
            .ok_or("AE package size overflow")?;
        if total > MAX_PACKAGE {
            return Err(
                "AE package exceeds 256 MiB / AEパッケージは256MiBまでです / AE包上限为256MiB"
                    .into(),
            );
        }
        let mut decoder = png::Decoder::new(Cursor::new(&png));
        decoder.set_limits(png::Limits {
            bytes: 128 * 1024 * 1024,
        });
        let reader = decoder.read_info().map_err(|e| e.to_string())?;
        let info = reader.info();
        if (info.width, info.height) != (width, height)
            || info.animation_control.is_some()
            || info.color_type != png::ColorType::Rgba
            || info.bit_depth != png::BitDepth::Eight
        {
            return Err("AE export requires full-size RGBA8 PNG frames".into());
        }
        zip.start_file(format!("frames/frame_{frame:06}.png"), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(&png).map_err(|e| e.to_string())?;
    }
    let script = format!(
        r#"// Extract the ZIP before running File > Scripts > Run Script File.
(function () {{
    var first = new File(new File($.fileName).parent.fsName + "/frames/frame_000000.png");
    if (!first.exists) {{ alert("Extract ZIP first / ZIPを展開してください / 请先解压ZIP"); return; }}
    app.beginUndoGroup("LumaPaint animation");
    try {{
        if (!app.project) app.newProject();
        var options = new ImportOptions(first);
        options.sequence = {sequence};
        options.forceAlphabetical = false;
        var footage = app.project.importFile(options);
        {conform}
        footage.mainSource.alphaMode = AlphaMode.STRAIGHT;
        var comp = app.project.items.addComp("LumaPaint", {width}, {height}, 1, {frames}/{fps}, {fps});
        var layer = comp.layers.add(footage);
        layer.startTime = 0;
        layer.inPoint = 0;
        layer.outPoint = {frames}/{fps};
        comp.workAreaStart = 0;
        comp.workAreaDuration = {frames}/{fps};
        comp.openInViewer();
    }} finally {{ app.endUndoGroup(); }}
}})();
"#,
        sequence = if frames > 1 { "true" } else { "false" },
        conform = if frames > 1 {
            format!("footage.mainSource.conformFrameRate = {fps};")
        } else {
            String::new()
        }
    );
    for (name, content) in [("Import-LumaPaint.jsx", script.as_str()), ("README.txt", "Extract this ZIP, then run Import-LumaPaint.jsx via After Effects File > Scripts > Run Script File.\nZIPを展開し、After Effectsのファイル > スクリプトからImport-LumaPaint.jsxを実行してください。\n解压ZIP，然后通过After Effects的文件 > 脚本运行Import-LumaPaint.jsx。\n\nFlattened RGBA8 frames; layers, editable keys, audio and color profiles are not transferred. Active page only.\n統合RGBA8連番。レイヤー・編集可能なキー・音声・ICCは転送しません。現在のページのみ。\n合并RGBA8序列，不传输图层、可编辑关键帧、音频或ICC。仅当前页面。\n")] {
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(content.as_bytes()).map_err(|e| e.to_string())?;
    }
    zip.finish()
        .map(|v| v.into_inner())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn image() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[255, 0, 0, 128])
                .unwrap();
        }
        bytes
    }
    #[test]
    fn ordered_frames_and_portable_script_preserve_timing_and_alpha() {
        let mut called = vec![];
        let bytes = package(1, 1, 24, 3, |f| {
            called.push(f);
            Ok(image())
        })
        .unwrap();
        assert_eq!(called, [0, 1, 2]);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 5);
        assert_eq!(
            zip.by_name("frames/frame_000002.png").unwrap().size(),
            image().len() as u64
        );
        let mut script = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("Import-LumaPaint.jsx").unwrap(),
            &mut script,
        )
        .unwrap();
        for expected in [
            "$.fileName",
            "options.sequence = true",
            "conformFrameRate = 24",
            "AlphaMode.STRAIGHT",
            "3/24",
            "finally",
        ] {
            assert!(script.contains(expected), "{expected}");
        }
    }
    #[test]
    fn limits_and_bad_frames_fail_without_rendering_invalid_requests() {
        assert!(package(1, 1, 0, 3, |_| panic!()).is_err());
        assert!(package(1, 1, 24, 10001, |_| panic!()).is_err());
        assert!(package(2, 1, 24, 1, |_| Ok(image())).is_err());
        assert!(package(1, 1, 24, 1, |_| Err("render failed".into())).is_err());
    }
    #[test]
    fn single_frame_is_imported_as_still() {
        let bytes = package(1, 1, 24, 1, |_| Ok(image())).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut script = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("Import-LumaPaint.jsx").unwrap(),
            &mut script,
        )
        .unwrap();
        assert!(script.contains("options.sequence = false"));
        assert!(!script.contains("conformFrameRate"));
    }
}
