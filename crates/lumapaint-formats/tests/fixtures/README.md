# Synthetic PDF test font

`lp-pdf-test.ttf` was generated for LumaPaint tests with FontBuilder. It contains
simple triangle/rectangle/curve outlines for A, B, C, space and a synthetic U+3042 glyph.
It is not copied from a third-party font and is not intended for artwork.
The 1000-unit em, 600/800-unit font metrics and separate PDF widths exercise
embedded font decoding, CID mappings and PDF-controlled advances independently
of system fonts. No third-party font data is included in the fixture.

`lp-psd-alpha.psd` is an artificial PSD v1 RGB8 with one RGBA layer and a white-matted merged preview. Its four pixels are half-transparent red, quarter-transparent blue, opaque green, and transparent. It contains no third-party artwork.

`lp-psd-layers.psd` and `lp-psd-layers-rle.psd` are artificial three-layer RGB8 PSDs. They contain an opaque red base, a half-opacity blue pixel with a Japanese Unicode name, and a hidden green layer with a Simplified Chinese name. They test normal blending, position, order, names and protection flags without third-party artwork.

`lp-psd-layers-zip.psd` contains the same synthetic pixels and metadata as `lp-psd-layers.psd`, with zlib/ZIP without prediction for all layer channels and the merged image. It tests compression-independent native state and contains no third-party artwork.

`lp-psd-layers-prediction.psd` stores the same synthetic layers and merged image using ZIP with 8bit row prediction. It verifies unsigned wraparound, row/channel boundaries and compression-independent native state.

`lp-japanese-cid.otf` is an original synthetic CID-keyed OpenType CFF font.
Its U+3042/U+3044 cmap uses GID 1/CID 42 and GID 2/CID 7, deliberately different
orders. The outlines are a triangle and rectangle, not Japanese letter artwork.
`lp-japanese-cid-horizontal.pdf` and `lp-japanese-cid-vertical.pdf` embed this font
with Identity-H/Identity-V, a ToUnicode map, and PDF-controlled positions. They
verify CID decoding and placement independently of installed fonts. They do not
prove Japanese typography, shaping or editable Adobe text compatibility.
Illustrator 2026 opened the horizontal PDF but requested missing-font substitution
and displayed replacement Japanese letters; that is not a matching visual oracle.

`generate_psb.py` converts our four small layer PSD fixtures to PSB v2: 64bit
layer section/channel lengths and 32bit PackBits row counts. Generated
`lp-psb-layers*.psb` files test all four RGB8 compression modes and Unicode names.
No external packages or third-party artwork are used.

`lp-photoshop-2026-roundtrip.psb` was made by opening our generated PackBits PSB
in Adobe Photoshop 2026 (27.10, Macintosh) and saving a separate local PSB copy
on 2026-10-05. Its Japanese/Chinese names, visibility and protection were checked
in the Photoshop Layers panel. Photoshop adds metadata not yet retained by LP;
this fixture checks explicit metadata-loss consent, retention of three Unicode
layers, conservative lock projection, native save, and pixel agreement with the
original composite (maximum 1/255 channel difference). The importer now verifies
its retained layer composite against the Adobe merged preview before accepting
known additional settings. A changed preview forces explicit flattening.
This is real Adobe resaving of synthetic artwork, not a representative production
Photoshop text/smart-object/CMYK document or proof of complete PSB compatibility.

## PSD／PSB writer verification (2026-10-05)

`cargo run --offline --locked -p lumapaint-formats --example psd_roundtrip -- crates/lumapaint-formats/tests/fixtures/lp-psd-layers.psd /private/tmp/lp-written-layers.psd`

The same command with `.psb` generates version 2. Both own outputs were opened
in Photoshop 2026; Japanese and Simplified Chinese layer names, layer order,
visibility and locks appeared correctly. Automated tests compare native raster
state after write/read for both versions and composite pixels with quantized
opacity/mask density. These tiny RGB8 fixtures do not establish compatibility
for production PSD type layers, effects, profiles or smart objects.

## Resolution resource verification (2026-10-05)

The writer tests use independently specified resource 1005 bytes with 300.5 ppi
(horizontal) and 150.25 ppi (vertical), mixed ppi/pixels-per-cm display units,
and different physical-size display units. They check PSD/PSB and native
container round trips, physical size, consent for 16.16 quantization, duplicate
resources, invalid densities/units and truncation. Old manifests omit this field.
A temporary own PSD with that resource was converted by `psd_roundtrip` to PSB
and opened in Photoshop 2026. Image Size displayed `118.307 pixel/cm`, matching
300.5 / 2.54. This verifies that density is stored in ppi even with cm display;
it is not a guarantee of complete production-file interoperability.

## Separable blend verification (2026-10-05)

Core tests specify expected opaque RGB8 output for Multiply, Screen, Darken,
Lighten, Difference and Exclusion, plus a partially transparent backdrop and
transparent backdrop. Writer tests exercise all six modes with opacity and a
mask through native containers and PSD/PSB, and reject a mismatched merged
preview without loss consent. The importer comparison is bounded to 4 million
allocated preview pixels; larger non-normal documents use the merged fallback.
A temporary own RGB8 fixture's Japanese layer was set to Multiply with 128/255
opacity and its merged blue component corrected to zero. `psd_roundtrip`
exported it to PSB. Photoshop 2026 opened the output and showed the Japanese
layer as Multiply (`乗算`) with 50% opacity. This verifies one own fixture,
not arbitrary Photoshop blend settings, color profiles or every blend mode.
