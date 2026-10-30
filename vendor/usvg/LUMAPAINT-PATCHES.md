# LumaPaint patches to usvg 0.49.0

## Explicit variable-font axes during fallback — 2026-10-12

The upstream fallback shaper replaces the entire chunk if a fallback font covers all characters. In mixed Latin/CJK text this discards the requested variable face, including supported Latin glyphs, and makes wdth/slnt animation inert.

Retain an explicit-variation flag from CSS parsing (separate from automatic font-weight mappings). With explicit axes, replace only missing UTF-8 shaping clusters. Handle differing ligature and combining-mark glyph counts by cluster offset rather than vector index. Ordinary text without explicit variation settings keeps the upstream fallback behavior.

Validation lives in lumapaint-renderer detailed_motion_tests and mixed-script real-axis rendering tests. Upstream Apache-2.0/MIT notices remain unchanged.

## Stable port — 2026-10-26

Rebased all local differences against crates.io usvg 0.48.1 onto 0.49.0 using
three-way merging. Retained source provenance, editable SVG source information,
CSS/custom-property/calc transforms, fragment writing, clip/use behavior,
vertical vert/vrt2 shaping and explicit variable-font fallback cluster handling.
The sole overlapping hunk in parser/converter.rs combines the local reference-box
CSS transform parser with upstream's default transform-origin for the root SVG.
No upstream transform/filter/paint fixes were intentionally removed.

Source: https://static.crates.io/crates/usvg/usvg-0.49.0.crate
SHA-256: `d09b73ee83641d390ad7ffd9000f347b16eb128e437675b6ff277ea5c7fcf22c`.
MIT and Apache-2.0 upstream notices remain in the vendor tree.
See docs/dependency-update-2026-10-26.md for validation and limitations.

## Nested text detection (2026-10-30)

`Tree::has_text_nodes` now descends ordinary `Node::Group` children in addition
to subroots and nested SVG images. Previously a transformed/opacity group could
hide text from capability detection. PDF font-embedding preflight relies on this
detection. Regression coverage is in lumapaint-formats `pdf::text_export` tests.
