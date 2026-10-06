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

No font parsing or rendering behavior is changed.
