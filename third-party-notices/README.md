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

## Dependency inventory (2026-10-04)

`cargo-license-inventory.json` inventories 586 locked Cargo packages across
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
- iOverlay 9.0.0: https://github.com/iShape-Rust/iOverlay/tree/26782e3b93f9b7d2516e7653f3d56eebc31af798
- iKeySort 0.11.0: https://github.com/iShape-Rust/iKeySort/tree/d8c914dc29afd4d4824dacd2c134dbc16168e78c
- simplecss 0.2.2: locked crates.io source, local changes documented in vendor/simplecss/LUMAPAINT-PATCHES.md
