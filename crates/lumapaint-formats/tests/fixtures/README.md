# Synthetic PDF test font

`lp-pdf-test.ttf` was generated for LumaPaint tests with FontBuilder. It contains
simple triangle/rectangle/curve outlines for A, B, C, space and a synthetic U+3042 glyph.
It is not copied from a third-party font and is not intended for artwork.
The 1000-unit em, 600/800-unit font metrics and separate PDF widths exercise
embedded font decoding, CID mappings and PDF-controlled advances independently
of system fonts. No third-party font data is included in the fixture.

`lp-psd-alpha.psd` is an artificial PSD v1 RGB8 with one RGBA layer and a white-matted merged preview. Its four pixels are half-transparent red, quarter-transparent blue, opaque green, and transparent. It contains no third-party artwork.

`lp-psd-layers.psd` and `lp-psd-layers-rle.psd` are artificial three-layer RGB8 PSDs. They contain an opaque red base, a half-opacity blue pixel with a Japanese Unicode name, and a hidden green layer with a Simplified Chinese name. They test normal blending, position, order, names and protection flags without third-party artwork.
