# LumaPaint patches to usvg 0.48.1

## Explicit variable-font axes during fallback — 2026-10-12

The upstream fallback shaper replaces the entire chunk if a fallback font covers all characters. In mixed Latin/CJK text this discards the requested variable face, including supported Latin glyphs, and makes wdth/slnt animation inert.

Retain an explicit-variation flag from CSS parsing (separate from automatic font-weight mappings). With explicit axes, replace only missing UTF-8 shaping clusters. Handle differing ligature and combining-mark glyph counts by cluster offset rather than vector index. Ordinary text without explicit variation settings keeps the upstream fallback behavior.

Validation lives in lumapaint-renderer detailed_motion_tests and mixed-script real-axis rendering tests. Upstream Apache-2.0/MIT notices remain unchanged.
