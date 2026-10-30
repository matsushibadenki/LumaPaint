# LumaPaint svg2pdf compatibility patches

Upstream svg2pdf 0.13.0 retains its MIT/Apache-2.0 notices and existing
LumaPaint raster/SVG compatibility changes. The direct usvg/resvg dependencies
now use 0.49.0, matching the application's shared SVG tree.

## New blend-mode handling — 2026-10-26

usvg 0.49 adds PlusLighter, which has no equivalent PDF blend operator.
BlendModeExt now returns Result; PlusLighter produces the explicit
UnsupportedBlendMode conversion error instead of incorrectly exporting Normal.
Existing supported modes keep their mappings. PDF consumers receive the existing
conversion-error path. Regression coverage lives in
crates/lumapaint-formats/tests/svg_blend_compatibility.rs.
Raster/PDF flattening of the entire affected backdrop is future work; a group-only
raster fallback would not preserve additive blending against sibling content.

## Positioned PDF text (2026-10-30)

- Align the optional fontdb dependency with the vendored usvg 0.49 fontdb 0.24.
- Replace `resvg/text` with the weak `resvg?/text` feature: text encoding uses
  usvg layout without activating a rasterizer. When a caller opts into `filters`,
  the optional resvg dependency still receives its text support.
- LumaPaint enables text encoding but keeps filter rasterization disabled. It
  preflights font formats and embedding/subsetting flags before selecting the
  text route; unsupported pages keep the explicit outline report.
- Convert usvg 0.49 u32 glyph IDs to the subsetter's u16 IDs with checked errors.
- Keep a single text object for a styled span while retaining individual glyph
  matrices; avoid artificial per-glyph line breaks during text extraction.
- For filled-and-stroked text, emit the stroke counterpart as vector outlines in
  the requested paint order so search/copy contains only one copy of the text.
- Group compatible horizontal glyphs into TJ runs with explicit advance
  adjustments, instead of splitting every character with a new text matrix.
- Keep separate character codes/ToUnicode strings for glyph aliases. Use a custom
  Encoding CMap only when codes differ from subset CIDs; ordinary fonts retain
  Identity-H. This also works for CID-keyed CFF without duplicating outlines.
- Bound each positioned text array to 4,096 glyphs; long lines continue at their
  exact next glyph matrix without allocating one unbounded TJ array.
