# FreeType build fixes

Source: crates.io `freetype-sys` 0.23.0 (MIT bindings), with the upstream
FreeType and libpng sources and their original licenses retained.
Published crate checksum:
`eab537ce43cab850c64b4cdc390ce7e4f47f877485ddc323208e268280c308ae`.

- Resolve libpng's zlib headers through Cargo's `DEP_Z_INCLUDE` metadata,
  instead of the nonexistent relative `libz-sys/src/zlib` directory. This
  fixes Windows MSVC `C1083: zlib.h` failures with bundled zlib or vcpkg.
- Retain `libz_sys` in the Rust dependency graph so its native link directives
  select the actual platform library; do not assume a library named `z`.
- Generate `pnglibconf.h` in `OUT_DIR`, keeping dependency sources read-only.
- Preserve upstream FFI comparison derives while suppressing their function
  pointer comparison warning, which becomes visible for a local dependency.

The original build fixes do not change font parsing or rendering behavior.

## Native stable refresh — 2026-10-12

The Rust bindings remain freetype-sys 0.23.0. Its bundled C sources are
updated independently: FreeType 2.13.2 → 2.14.3 and libpng 1.6.44 → 1.6.59.
No local C-source patches existed; the complete upstream source trees were
replaced while retaining the Rust and build-script fixes above. Existing FFI
entry points and struct layouts remain compatible; no new native APIs are used.

- FreeType: https://download.savannah.gnu.org/releases/freetype/freetype-2.14.3.tar.xz
  SHA-256: `36bc4f1cc413335368ee656c42afca65c5a3987e8768cc28cf11ba775e785a5f`
- libpng: https://github.com/pnggroup/libpng/archive/refs/tags/v1.6.59.tar.gz
  SHA-256: `2540302a1844ad2b2b501977abecfa850f265f97b78f065a712ab4074a89f5b5`

FreeType's release `docs/CHANGES` and libpng's `CHANGES`/`ANNOUNCE` are retained
with the source. The PDF-specific FTL and libpng license copies are refreshed.
Windows/Linux native execution remains pending until tested on those systems.
