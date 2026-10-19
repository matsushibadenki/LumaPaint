# Native rendering and font integration notices

This directory is included in the application bundle. Skia is Copyright (c) 2011
Google Inc. and distributed under the BSD license in Skia-BSD.txt. Rust bindings
are distributed under the MIT license in rust-skia-MIT.txt.

The remaining files reproduce notices for Skia's image codecs, font and text
dependencies. They are copied from the source revision referenced by
skia-bindings 0.153.3. Some optional components may be removed by the linker.
For libjpeg-turbo, this software is based in part on the work of the Independent
JPEG Group. FreeType is used under the FreeType License (FTL), not the alternative
GPL license. Wuffs is used under its Apache-2.0 option.

These notices cover the added Skia integration, not a complete inventory of all
existing LumaPaint dependencies. Review the dependency/license inventory when
preparing a public release or changing Skia features/version.

Sources:
- https://github.com/google/skia/
- https://github.com/rust-skia/rust-skia/tree/0.153.3
- https://github.com/rust-skia/skia-binaries/releases/tag/0.153.3

## Dependency inventory (2026-10-19)

`cargo-license-inventory.json` inventories 627 locked Cargo packages across
platforms/features, including packages that may not be linked into this build.
No package lacks both a license expression and a license file in Cargo metadata.
Regenerate with:

```sh
python3 scripts/dependency-license-inventory.py --output third-party-notices/cargo-license-inventory.json
```

The PDF writer/parser and independent polygon engine license files are included
alongside the Skia notices. Metadata collection is reproducible; it is not a
legal determination or proof that every notice needed by a release is complete.
Intel, Windows and Linux binary dependencies still need validation on their
actual target systems.

Additional fixed source notices:
- iOverlay 9.1.0: https://github.com/iShape-Rust/iOverlay/tree/117637e3a65804973da2bfd7c59f42773f4dcc0d
- iKeySort 0.12.0: https://github.com/iShape-Rust/iKeySort/tree/688424e09b7758d04e62e350160d164c7384f54b
- simplecss 0.2.2: locked crates.io source, local changes documented in vendor/simplecss/LUMAPAINT-PATCHES.md

- ttf-parser 0.25.1: the embedded PDF glyph-outline reader uses this parser directly; MIT and Apache-2.0 source notices are bundled.

## PDF PostScript font reader

freetype-rs 0.38.0 and freetype-sys 0.23.0 are statically bundled for the PDF
font reader, independently of Skia. Their MIT notices are included here. The
bundled native FreeType uses the FTL option; its own FTL and libpng notice are
included as pdf-freetype-FTL.TXT and pdf-freetype-libpng-LICENSE.
Portions of this software are copyright © 2026 The FreeType Project
(www.freetype.org). All rights reserved.

The local freetype-sys build fixes are documented in
vendor/freetype-sys/LUMAPAINT-PATCHES.md; original native source licenses remain
in that directory. Windows zlib headers and linkage come from libz-sys metadata.

The PDF reader bundles FreeType 2.15.0 and libpng 1.6.59 as of 2026-10-19.
Upstream sources and archive SHA-256 values are recorded in
`vendor/freetype-sys/LUMAPAINT-PATCHES.md`. These versions do not replace
Skia’s independently managed codec/font dependencies.

## Timeline motion assets (2026-10-12)

Bundled Roboto Flex and Noto Sans JP variable fonts use SIL OFL 1.1.
Unmodified binaries are loaded only into the Rust process. Their copyright
and license notices are in RobotoFlex-OFL.txt and NotoSansJP-OFL.txt;
variable-fonts-attribution.md records the pinned upstream source.

Kanji stroke paths derive from KanjiVG (Ulrich Apel and contributors),
CC BY-SA 3.0. KanjiVG-attribution.md describes the source revision and
normalization; KanjiVG-CC-BY-SA-3.0.txt includes the license. Stroke-path
data and adaptations retain that license; the application source is separate.
Generated stroke SVG includes attribution metadata.
