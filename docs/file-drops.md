# Finder file drops

The editor accepts `.lumapaint`, `.svg`, `.png`, `.jpg`, `.jpeg`, `.webp`, `.psd`, `.psb`, `.pdf`, and `.ai` files. Extensions are case insensitive; Finder URLs retain Unicode and escaped filename characters.

| Drop location | Result |
| --- | --- |
| Empty document display area | Open each file in a new document tab |
| Empty space to the right of tabs | Open each file in a new document tab |
| Existing document page | Import each file as a separate layer in that document |
| Gray area surrounding the page | Open new document tabs |
| Existing tab, toolbar or inspector | Ignore the drop |

Images retain transparency and original dimensions. SVG remains SVG in legacy documents. Native documents are embedded through their export representation; tiled source documents and PSD are composited when imported as one layer. A tiled destination rasterizes the incoming representation on a Rust worker. Each layer import is independently undoable. Opening imported SVG/images/PSD creates an unsaved LumaPaint document; opening a native file retains its save path and fingerprint.

PDF/AI use the existing page-selection workflow and its compatibility limits. That workflow handles one pending PDF/AI request per editor window. Existing capacity limits still apply: approximately 3 MiB for compressed images, 4 MiB for SVG, 16 imported layers and 6 MiB of embedded SVG data in legacy documents. Raster flattening is bounded to 16 megapixels. WebP is validated and converted to PNG on a Rust worker.

The AppKit `PaintView` accepts file URLs over the native GPU canvas and emits paths plus the Rust document ID to its owning WebView. WebView drop coordinates are physical pixels and are converted to CSS coordinates for tab/empty-canvas hit testing. The bridge serializes the import with canvas commands; the Rust adapter checks the destination again before committing each file. Invalid files report an error without removing successfully imported files or replacing existing document contents. Pixels and document payloads never pass through JavaScript.
