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

## Native stable refresh — 2026-10-19

FreeType 2.14.3 → 2.15.0, from the official release archive:
https://download.savannah.gnu.org/releases/freetype/freetype-2.15.0.tar.xz
SHA-256: `93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5`.
The complete upstream tree is retained, with no local C changes. libpng stays
at 1.6.59, the current stable release checked on the same date.

The new upstream module list includes HVF, so build.rs now compiles hvf/hvf.c.
FT_CONFIG_OPTION_HVF remains disabled by upstream default; this compiles the
unsupported-format stub and introduces no Apple-only framework dependency.
FT_FaceRec, FT_SizeRec and FT_GlyphSlotRec fields were compared with 2.14.3;
their layouts are unchanged. Existing Rust bindings/build fixes are retained.
The PDF-specific FTL notice is copied from the new release.
See docs/dependency-update-2026-10-19.md for executed validation and limitations.
