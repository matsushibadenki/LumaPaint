# LumaPaint vertical typography patch

Vendored usvg 0.45.1 (resvg project), retaining its MIT/Apache licenses.

Pass writing mode through shaping including fallback fonts, enable OpenType vert/vrt2 for vertical text, and distinguish Unicode U/Tu/Tr orientation. Tr stays upright when the font supplies a vertical alternate; otherwise it retains the rotated fallback. Horizontal shaping is unchanged.

Remove this patch when upstream supports these vertical substitutions and the renderer regression tests pass.

Vertical substitutions are restricted by Unicode orientation: sideways (R) Latin/digits retain their horizontal glyph because the layout stage already rotates them clockwise. Applying vrt2 to these glyphs would double-rotate them. A raster regression compares F/R/a/2 against a single clockwise rotation and fails with the unrestricted features.
