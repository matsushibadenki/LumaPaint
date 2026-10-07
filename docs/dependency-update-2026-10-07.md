# Dependency update — 2026-10-07

Checked npm's `latest` tags and crates.io's `max_stable_version` for every external workspace dependency. Prerelease channels were excluded. Existing versions of Tauri 2.12.1, Skia 0.153.3, FreeType bindings, PNG, PDF and Pathfinder libraries were already current.

## JavaScript

React / React DOM 19.3.0, Tauri API / CLI 2.12.1, React types 19.3.0, TypeScript 7.0.2, Vite 8.3.3, React plugin 6.1.2. Both npm and pnpm lockfiles were synchronized. `npm outdated --json` returns `{}` and npm reports no known vulnerabilities.

TypeScript 7's package no longer provides the old `transpileModule` API used by two regression scripts. These now use Node's `stripTypeScriptTypes`; minimum Node is 22.13.0. Vite's client declarations supply CSS import types. The existing Node 26.10.0 environment was retained.

## Rust direct dependencies

| Dependency | Latest stable checked |
| --- | --- |
| `base64` | 0.23.1 |
| `bytemuck` | 1.25.2 |
| `crc32fast` | 1.5.2 |
| `flate2` | 1.1.10 |
| `fontdb` | 0.24.0 |
| `freetype-rs` | 0.38.0 |
| `i_overlay` | 9.0.0 |
| `image` | 0.25.10 |
| `lopdf` | 0.45.0 |
| `metal` | 0.33.0 |
| `objc2` | 0.6.5 |
| `objc2-app-kit` | 0.3.2 |
| `objc2-foundation` | 0.3.2 |
| `png` | 0.18.1 |
| `pollster` | 1.0.1 |
| `raw-window-handle` | 0.6.2 |
| `reqwest` | 0.13.5 |
| `resvg` | 0.48.1 |
| `rfd` | 0.17.2 |
| `roxmltree` | 0.21.1 |
| `rustybuzz` | 0.20.1 |
| `security-framework` | 3.7.0 |
| `serde` | 1.0.229 |
| `serde_json` | 1.0.151 |
| `sha2` | 0.11.0 |
| `skia-safe` | 0.153.3 |
| `svg2pdf` | 0.13.0 |
| `svgtypes` | 0.16.1 |
| `tauri` | 2.12.1 |
| `tauri-build` | 2.7.1 |
| `tempfile` | 3.27.0 |
| `tiny-skia-path` | 0.12.0 |
| `ttf-parser` | 0.25.1 |
| `unicode-segmentation` | 1.13.3 |
| `usvg` | 0.48.1 |
| `wgpu` | 30.0.1 |

Version requirements now explicitly select these stable releases. Cargo.lock also updates compatible transitive dependencies. `cargo update --dry-run --verbose` reports zero compatible updates.

Cargo reports four older versions within compatible release series, constrained by upstream packages: `generic-array` 0.14.7 is pinned by `crypto-common` 0.1.7 in Tauri's SHA-256 chain; Linux GTK's `proc-macro-crate` 2.0.2 pins `toml_datetime` 0.6.3, keeping `toml` 0.8.2 / `toml_edit` 0.20.2. An explicit attempt to upgrade TOML to 0.8.23 was rejected by Cargo's resolver because of that exact pin. Upstream chains may also retain older major versions (for example Tauri’s SHA-256 chain uses sha2 0.10 while the app’s direct dependency uses 0.11). Those upstream-controlled versions are not claimed to be the newest releases across all majors.

## Compatibility changes

- wgpu 27 → 30.0.1: optional binding / vertex slots, immediate-size declarations, multiview masks, scoped error guards, fallible mapped ranges, adapter limit buckets, owned instance descriptors and queue presentation.
- GPU mapping errors in image processing return to the existing CPU fallback. Successful mapping retains the original readback/unmap lifecycle.
- Metal HAL now exposes objc2 protocol objects. Owned references bridge to the existing metal / Skia wrappers; imported textures retain ownership and explicitly declare their initial render-target state. No additional CPU pixel transfer is introduced.
- usvg / resvg 0.45 → 0.48.1, fontdb 0.24.0, tiny-skia-path 0.12.0 and svgtypes 0.16.1. Existing source provenance, CSS parsing, fragment writing and vertical glyph patches were merged into the upstream source. Vertical OpenType features were ported to upstream's harfrust shaper while preserving variable-font and optical-size arguments.
- svg2pdf 0.13.0 is already latest but depends on usvg 0.45 upstream. A licensed local copy changes its usvg dependency to 0.48.1 (and raster fallback to tiny-skia 0.12) so PDF export shares the current SVG tree type.
- Existing block 0.1.6, simplecss 0.2.2 and freetype-sys 0.23.0 patches remain at their latest upstream stable versions.

References: [wgpu releases](https://github.com/gfx-rs/wgpu/releases), [resvg releases](https://github.com/linebender/resvg/releases), [crates.io](https://crates.io/), [npm registry](https://registry.npmjs.org/).

## Validation

Full frontend build, measurement / color regression scripts and architecture checks pass. Final `cargo test --workspace --locked` passed 772 tests (41 existing manual/GPU tests ignored). Workspace Clippy with warnings denied, all-target compilation, formatting, diff checks and the workspace/macOS debug binary build passed. Explicit Metal/Skia runs passed 22 GPU pipeline tests and 4 shared-texture tests/benchmark (one shared-object test is included in both filters). No running-app restart or full GUI interaction test was performed. Linux and Windows native execution is not available on this macOS host; all platform dependencies remain resolved in Cargo.lock.

The baseline-shift pixel regression originally failed because the preceding frame-overflow change now hides an entire line if a shifted glyph crosses its frame. Its fixture now provides safe measured baselines so it tests glyph-local movement without frame overflow. The equality assertion remains unchanged and passes in both writing directions.
