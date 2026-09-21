# Skia integration notices

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
