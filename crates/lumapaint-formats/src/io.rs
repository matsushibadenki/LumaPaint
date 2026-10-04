//! Operation-level contracts for host file dialogs and independent adapters.
//! Adapters return a new native document; hosts decide whether to open a tab or insert layers.
use crate::{export::FormatId, ConversionReport, DocumentImporter, ImportError, ImportOptions};
use lumapaint_core::document::Document;
use lumapaint_core::tiles::TiledRasterState;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FileOperation {
    Open,
    Import,
    Export,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FormatFamily {
    Native,
    Vector,
    Raster,
    Layered,
    Hdr,
    CameraRaw,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFormat {
    pub format: FormatId,
    pub extension: &'static str,
    pub extensions: &'static [&'static str],
    pub media_type: &'static str,
    pub family: FormatFamily,
    pub open_adapter: bool,
    pub import_adapter: bool,
    pub export_adapter: bool,
    /// Planned operations are not executable capabilities.
    pub planned_open: bool,
    pub planned_import: bool,
    pub planned_export: bool,
    /// Adapter availability does not mean the editor menu is already connected.
    pub editor_open: bool,
    pub editor_import: bool,
    pub editor_export: bool,
    pub partial: bool,
}
const fn entry(
    format: FormatId,
    extensions: &'static [&'static str],
    media_type: &'static str,
    family: FormatFamily,
) -> FileFormat {
    FileFormat {
        format,
        extension: extensions[0],
        extensions,
        media_type,
        family,
        open_adapter: false,
        import_adapter: false,
        export_adapter: false,
        planned_open: true,
        planned_import: true,
        planned_export: true,
        editor_open: false,
        editor_import: false,
        editor_export: false,
        partial: false,
    }
}
pub const FILE_FORMATS: &[FileFormat] = &[
    FileFormat {
        open_adapter: true,
        import_adapter: true,
        export_adapter: true,
        editor_open: cfg!(target_os = "macos"),
        editor_export: cfg!(target_os = "macos"),
        ..entry(
            FormatId::Native,
            &["lumapaint"],
            "application/vnd.lumapaint+json",
            FormatFamily::Native,
        )
    },
    FileFormat {
        open_adapter: true,
        import_adapter: true,
        export_adapter: true,
        editor_open: cfg!(target_os = "macos"),
        editor_import: cfg!(target_os = "macos"),
        editor_export: cfg!(target_os = "macos"),
        partial: true,
        ..entry(
            FormatId::Svg,
            &["svg"],
            "image/svg+xml",
            FormatFamily::Vector,
        )
    },
    FileFormat {
        open_adapter: true,
        import_adapter: true,
        export_adapter: true,
        partial: true,
        editor_open: cfg!(target_os = "macos"),
        editor_import: cfg!(target_os = "macos"),
        editor_export: cfg!(target_os = "macos"),
        ..entry(
            FormatId::Pdf,
            &["pdf"],
            "application/pdf",
            FormatFamily::Vector,
        )
    },
    FileFormat {
        open_adapter: true,
        import_adapter: true,
        partial: true,
        editor_open: cfg!(target_os = "macos"),
        ..entry(
            FormatId::Psd,
            &["psd"],
            "image/vnd.adobe.photoshop",
            FormatFamily::Layered,
        )
    },
    entry(
        FormatId::Ora,
        &["ora"],
        "image/openraster",
        FormatFamily::Layered,
    ),
    entry(
        FormatId::Kra,
        &["kra"],
        "application/x-krita",
        FormatFamily::Layered,
    ),
    entry(FormatId::Exr, &["exr"], "image/x-exr", FormatFamily::Hdr),
    FileFormat {
        editor_open: cfg!(target_os = "macos"),
        editor_import: cfg!(target_os = "macos"),
        ..entry(FormatId::Png, &["png"], "image/png", FormatFamily::Raster)
    },
    FileFormat {
        editor_open: cfg!(target_os = "macos"),
        editor_import: cfg!(target_os = "macos"),
        ..entry(
            FormatId::Jpeg,
            &["jpg", "jpeg"],
            "image/jpeg",
            FormatFamily::Raster,
        )
    },
    FileFormat {
        editor_import: cfg!(target_os = "macos"),
        ..entry(
            FormatId::Webp,
            &["webp"],
            "image/webp",
            FormatFamily::Raster,
        )
    },
    entry(FormatId::Gif, &["gif"], "image/gif", FormatFamily::Raster),
    entry(FormatId::Bmp, &["bmp"], "image/bmp", FormatFamily::Raster),
    entry(
        FormatId::Tiff,
        &["tiff", "tif"],
        "image/tiff",
        FormatFamily::Raster,
    ),
    // Sensor RAW is an import/development workflow; arbitrary raw buffers require a separate schema.
    FileFormat {
        planned_export: false,
        ..entry(
            FormatId::Raw,
            &[
                "raw", "dng", "cr2", "cr3", "nef", "nrw", "arw", "raf", "rw2", "orf", "pef", "srw",
            ],
            "application/octet-stream",
            FormatFamily::CameraRaw,
        )
    },
    entry(
        FormatId::Heif,
        &["heif", "heic"],
        "image/heif",
        FormatFamily::Raster,
    ),
    entry(
        FormatId::Ico,
        &["ico"],
        "image/vnd.microsoft.icon",
        FormatFamily::Raster,
    ),
    entry(
        FormatId::Avif,
        &["avif"],
        "image/avif",
        FormatFamily::Raster,
    ),
    // AI reading may use its PDF-compatible portion. Native AI authoring remains uncommitted.
    FileFormat {
        open_adapter: true,
        import_adapter: true,
        partial: true,
        editor_open: cfg!(target_os = "macos"),
        editor_import: cfg!(target_os = "macos"),
        planned_export: false,
        ..entry(
            FormatId::Illustrator,
            &["ai"],
            "application/vnd.adobe.illustrator",
            FormatFamily::Vector,
        )
    },
];

/// Zero-based page choice. PDF multi-page policy is explicit rather than silently discarding pages.
#[derive(Clone, Copy, Debug)]
pub struct ReadOptions {
    pub page_index: u32,
    pub frame_index: u32,
    pub raster_dpi: u32,
    pub allow_lossy: bool,
}
impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            page_index: 0,
            frame_index: 0,
            raster_dpi: 144,
            allow_lossy: false,
        }
    }
}
impl ReadOptions {
    pub fn validate(self, format: FormatId) -> Result<(), ImportError> {
        if !(36..=2400).contains(&self.raster_dpi) {
            return Err(ImportError::Malformed("io.raster_dpi"));
        }
        if !matches!(
            format,
            FormatId::Pdf | FormatId::Illustrator | FormatId::Tiff | FormatId::Exr
        ) && self.page_index != 0
        {
            return Err(ImportError::Unsupported("io.page_selection"));
        }
        if !matches!(
            format,
            FormatId::Gif | FormatId::Webp | FormatId::Avif | FormatId::Heif
        ) && self.frame_index != 0
        {
            return Err(ImportError::Unsupported("io.frame_selection"));
        }
        Ok(())
    }
}
/// Model-owned decoded payloads stay in Rust. Different native representations are not flattened.
pub enum ReadContent {
    Vector(Box<Document>),
    Raster(TiledRasterState),
}
pub struct ReadDocument {
    pub content: ReadContent,
    pub report: ConversionReport,
}
pub fn format_from_extension(extension: &str) -> Option<FormatId> {
    let extension = extension.trim_start_matches('.');
    FILE_FORMATS
        .iter()
        .find(|entry| {
            entry
                .extensions
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(extension))
        })
        .map(|entry| entry.format)
}

pub fn adapter_available(format: FormatId, operation: FileOperation) -> bool {
    FILE_FORMATS
        .iter()
        .find(|entry| entry.format == format)
        .is_some_and(|entry| match operation {
            FileOperation::Open => entry.open_adapter,
            FileOperation::Import => entry.import_adapter,
            FileOperation::Export => entry.export_adapter,
        })
}
/// Parsing is completed before the caller changes its active document, layers, or saved path.
pub fn read_document(
    format: FormatId,
    name: String,
    bytes: &[u8],
    options: ReadOptions,
) -> Result<ReadDocument, ImportError> {
    options.validate(format)?;
    match format {
        FormatId::Svg => {
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(ImportError::LimitExceeded("svg.source_bytes"));
            }
            let source =
                std::str::from_utf8(bytes).map_err(|_| ImportError::Malformed("svg.utf8"))?;
            let document = crate::svg::import(name, source.to_owned())
                .map_err(|_| ImportError::Malformed("svg.document"))?;
            Ok(ReadDocument {
                content: ReadContent::Vector(Box::new(document)),
                report: ConversionReport::default(),
            })
        }
        FormatId::Native => {
            let content = if crate::tile_container::is_container(bytes) {
                ReadContent::Raster(
                    crate::tile_container::decode(bytes)
                        .map_err(|_| ImportError::Malformed("native.tiles"))?,
                )
            } else {
                ReadContent::Vector(Box::new(
                    crate::native::decode(bytes)
                        .map_err(|_| ImportError::Malformed("native.document"))?,
                ))
            };
            Ok(ReadDocument {
                content,
                report: ConversionReport::default(),
            })
        }
        FormatId::Psd => {
            let imported = crate::psd::PsdImporter.import(
                bytes,
                ImportOptions {
                    allow_lossy: options.allow_lossy,
                },
            )?;
            Ok(ReadDocument {
                content: ReadContent::Raster(imported.raster),
                report: imported.report,
            })
        }
        FormatId::Pdf => crate::pdf::read(bytes, options),
        FormatId::Illustrator => {
            if !bytes.starts_with(b"%PDF-") {
                return Err(ImportError::Unsupported("ai.private_format"));
            }
            let mut decoded = crate::pdf::read(bytes, options)?;
            decoded.report.issues.push(crate::ConversionIssue {
                code: "ai.pdf_compatible_only",
                tier: crate::CompatibilityTier::B,
            });
            if !options.allow_lossy {
                return Err(ImportError::LossyConversionRequiresConsent(decoded.report));
            }
            Ok(decoded)
        }
        _ => Err(ImportError::Unsupported("io.read_format")),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_requested_extension_has_one_route_and_unimplemented_adapters_stay_disabled() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for entry in FILE_FORMATS {
            for extension in entry.extensions {
                assert!(seen.insert(*extension));
                assert_eq!(
                    format_from_extension(&extension.to_ascii_uppercase()),
                    Some(entry.format)
                );
            }
            if !entry.open_adapter {
                assert!(matches!(
                    read_document(
                        entry.format,
                        "sample".into(),
                        b"placeholder",
                        ReadOptions::default()
                    ),
                    Err(ImportError::Unsupported(_))
                ));
            }
            let capabilities = crate::export::capabilities();
            let capability = capabilities
                .iter()
                .find(|c| c.format == entry.format)
                .unwrap();
            assert_eq!(capability.import, entry.import_adapter);
            assert_eq!(capability.export, entry.export_adapter);
            if !entry.export_adapter {
                let snapshot = crate::export::ExportSnapshot::capture(&Document::default());
                assert!(matches!(
                    crate::export::export(
                        entry.format,
                        &snapshot,
                        crate::export::ExportOptions::default()
                    ),
                    Err(crate::export::ExportError::Unsupported(_))
                ));
            }
        }
        for ext in [
            "ora", "psd", "exr", "kra", "png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "raw",
            "heif", "ico", "avif", "ai",
        ] {
            assert!(format_from_extension(ext).is_some());
        }
        for format in [FormatId::Raw, FormatId::Illustrator] {
            assert!(
                !FILE_FORMATS
                    .iter()
                    .find(|entry| entry.format == format)
                    .unwrap()
                    .planned_export
            );
        }
    }
    #[test]
    fn psd_dispatch_returns_native_tiles_and_keeps_lossy_consent() {
        let mut bytes = b"8BPS".to_vec();
        bytes.extend(1u16.to_be_bytes());
        bytes.extend([0; 6]);
        bytes.extend(3u16.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(1u32.to_be_bytes());
        bytes.extend(8u16.to_be_bytes());
        bytes.extend(3u16.to_be_bytes());
        bytes.extend([0; 8]); // Color data and resources.
        bytes.extend(8u32.to_be_bytes()); // Nonempty layer metadata cannot be silently discarded.
        bytes.extend([0; 8]);
        bytes.extend(0u16.to_be_bytes());
        bytes.extend([20, 21, 22]);
        assert!(matches!(
            read_document(
                FormatId::Psd,
                "sample".into(),
                &bytes,
                ReadOptions::default()
            ),
            Err(ImportError::LossyConversionRequiresConsent(_))
        ));
        let imported = read_document(
            FormatId::Psd,
            "sample".into(),
            &bytes,
            ReadOptions {
                allow_lossy: true,
                ..ReadOptions::default()
            },
        )
        .unwrap();
        let ReadContent::Raster(raster) = imported.content else {
            panic!("PSD should return tiles");
        };
        assert_eq!(&raster.layers[0].tiles[0].pixels[..4], &[20, 21, 22, 255]);
        assert!(!imported.report.issues.is_empty());
        let encoded = crate::tile_container::encode(&raster).unwrap();
        let native = read_document(
            FormatId::Native,
            "sample".into(),
            &encoded,
            ReadOptions::default(),
        )
        .unwrap();
        let ReadContent::Raster(restored) = native.content else {
            panic!("Native tiles should remain tiles");
        };
        assert_eq!(raster, restored);
    }
    #[test]
    fn dialogs_can_distinguish_adapter_readiness_from_menu_readiness() {
        assert_eq!(format_from_extension(".SVG"), Some(FormatId::Svg));
        assert_eq!(format_from_extension("PDF"), Some(FormatId::Pdf));
        assert_eq!(format_from_extension("ai"), Some(FormatId::Illustrator));
        for operation in [
            FileOperation::Open,
            FileOperation::Import,
            FileOperation::Export,
        ] {
            assert!(adapter_available(FormatId::Svg, operation));
            assert!(adapter_available(FormatId::Pdf, operation));
        }
        let svg = FILE_FORMATS
            .iter()
            .find(|entry| entry.format == FormatId::Svg)
            .unwrap();
        assert_eq!(svg.editor_open, cfg!(target_os = "macos"));
        assert_eq!(svg.editor_export, cfg!(target_os = "macos"));
    }
    #[test]
    fn source_validation_precedes_host_document_mutation() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect width="20" height="20"/></svg>"#;
        let result = read_document(
            FormatId::Svg,
            "sample.svg".into(),
            svg,
            ReadOptions::default(),
        )
        .unwrap();
        let ReadContent::Vector(document) = result.content else {
            panic!("SVG should remain vector content");
        };
        assert_eq!(document.svg_layers().count(), 1);
        assert!(
            read_document(FormatId::Svg, "bad".into(), &[255], ReadOptions::default()).is_err()
        );
        assert!(read_document(
            FormatId::Svg,
            "bad".into(),
            &vec![b' '; 4 * 1024 * 1024 + 1],
            ReadOptions::default()
        )
        .is_err());
        assert!(ReadOptions {
            page_index: 1,
            ..ReadOptions::default()
        }
        .validate(FormatId::Svg)
        .is_err());
        assert!(ReadOptions {
            raster_dpi: 0,
            ..ReadOptions::default()
        }
        .validate(FormatId::Pdf)
        .is_err());
    }
}
