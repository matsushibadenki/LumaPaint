# Illustrator 2026 geometry reference

Locally exported on macOS on 2026-10-08 from `fixture.svg` with Illustrator 2026:
RGB document, 256 × 256 point artboard, PNG, artboard bounds, Screen (72 ppi),
Art Optimized (Supersampling), transparent background, no interlacing.
The fixture contains an opaque white background, integer rectangle, round diagonal
stroke, cubic fill, clipped linear gradient, and a half-transparent rectangle.
No external images, fonts, or user artwork are included.

Illustrator displayed a general SVG clipping import warning. The exported image
visibly retains the circular clipping region; the warning is recorded rather than
silently interpreting this as a fully validated clipping import.

Reproduce the LumaPaint compatibility reference and compare without resampling:

```sh
cargo run --offline --locked -p lumapaint-renderer --example adobe_compare -- generate /tmp/adobe-check
cargo run --offline --locked -p lumapaint-renderer --example adobe_compare -- compare /tmp/adobe-check/reference.png crates/lumapaint-renderer/tests/fixtures/adobe-2026/adobe.png
```

The comparison writes an amplified RGB difference image beside its second input.
This is a diagnostic, not an assertion that the renderers match. With the Resvg
compatibility backend, the recorded full image has 5,592 changed pixels out of
65,536, maximum channel difference 77/255, and mean channel difference 0.213341/255
(premultiplied RGBA). Interior opaque fill and half-transparent fill samples match
exactly. Differences occur on curved/stroked/clipped edges and gradient rounding.
The cubic region maximum is 77/255; diagonal stroke 28/255; clip/gradient 68/255.

This does not validate the experimental native Bézier renderer, font layout,
color-managed Adobe workflows, masks/effects, all zoom levels, or other operating
systems. Do not loosen native-versus-compatibility quality gates based on it.
