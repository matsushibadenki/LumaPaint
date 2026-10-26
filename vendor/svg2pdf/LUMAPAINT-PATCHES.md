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
