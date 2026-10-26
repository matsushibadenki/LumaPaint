# Dependency update — 2026-10-26

## Versions and scope

Checked 50 external workspace/vendor packages against official npm latest tags
and crates.io max_stable_version. Prereleases are excluded; Tauri remains 2.x.
FreeType 2.15.0 and libpng 1.6.59 remain current stable native source releases.
The source/state ownership boundary, Rust/wgpu rendering, and English/Japanese/
Simplified Chinese UI are retained.

| Registry | Dependency | Before | After |
| --- | --- | --- | --- |
| cargo | `tauri-build` | =2.7.3 | 2.7.4 |
| cargo | `tauri` | =2.12.4 | 2.12.5 |
| cargo | `usvg` | 0.48.1 | 0.49.0 |
| cargo | `roxmltree` | =0.21.1 | 0.21.2 |
| cargo | `resvg` | =0.48.1 | 0.49.0 |
| cargo | `lopdf` | =0.45.0 | 0.46.0 |
| cargo | `crc32fast` | =1.5.2 | 1.5.3 |
| npm | `@tauri-apps/cli` | ^2.12.3 | 2.12.4 |
| npm | `vite` | ^8.3.4 | 8.3.5 |

npm/pnpm/Cargo lockfiles are synchronized. Cargo reports 39 updated packages
and removes older base64/png entries. The license inventory covers 625 packages;
none lack license metadata. Existing source changes in canvas_macos.rs,
roadmap and docs/pen-close-qa.md are retained.

## Compatibility work

- Rebased the complete usvg local delta onto 0.49 using published 0.48.1 as the
  common ancestor. All 14 locally modified/additional files were retained.
  One overlapping CSS transform hunk was resolved by preserving the local
  reference-box transform parser and adopting upstream's root transform-origin.
  Source provenance/editing, CSS variables/calc, fragments, clip/use behavior,
  vertical alternates and explicit variable-font fallback patches remain.
  A new pixel regression checks percentage CSS translation together with the
  root SVG default transform origin.
- usvg/resvg 0.49 introduce PlusLighter/Lighter, improve clip/filter bounding boxes,
  rotated/skewed filter handling, root SVG transforms, baseline inheritance,
  RTL positioning and referenced paints. The normal renderer's unsupported-feature
  predicate routes non-normal blending and filters to resvg.
- svg2pdf stays 0.13.0 with its resvg/usvg dependencies aligned to 0.49.
  PDF has no equivalent PlusLighter blend operator, so the adapter returns an
  explicit UnsupportedBlendMode error. It never substitutes Normal. Existing
  Multiply output is covered by a new regression. Exact additive PDF appearance
  would require a backdrop-aware raster strategy and remains planned work.
- Added pixel-level renderer regressions for plus-lighter against a sibling
  backdrop and feComposite lighter inputs. These guard against silent downgrades.
- lopdf 0.46 declares Rust 1.88 as its minimum; the current Rust 1.99 host and
  CI stable toolchain satisfy that requirement. Its bundled license text is
  unchanged from the existing notice copy.
- Tauri stays on the stable 2.x chain; API 2.12.4 has no newer stable release.
  CLI 2.12.4 includes upstream ZIP dependency fixes. Vite stays in the 8.3 series.

## Retained upstream constraints

Cargo update --dry-run --verbose reports zero compatible updates. Four older
packages remain pinned by the upstream GTK/crypto chains: generic-array 0.14.7,
toml 0.8.2, toml_datetime 0.6.3 and toml_edit 0.20.2. The earlier brotli constraint
is resolved by upstream brotli 9.1.0 / brotli-decompressor 6.2.0. No forced
cross-major overrides are applied.

FreeType's private modified zlib 1.3.1 and Skia's internal native dependency pins
remain upstream-managed, as documented in the previous update. They are not
claimed to be the newest standalone native libraries.

## Validation

- 🟢 [Done] Frontend build, architecture boundary, measurements, color modes,
  workspace notifications, timeline view and CI document-layout regressions passed.
- 🟢 [Done] npm install reports zero known vulnerabilities in the installed tree.
- 🟢 [Done] cargo check --workspace --all-targets --locked passed after the
  svg2pdf blend-enum adapter. Initial compilation detected the new enum variant;
  it was handled explicitly instead of adding a wildcard fallback.
- 🟢 [Done] Rust fmt and workspace/all-target Clippy with warnings denied passed.
- 🟢 [Done] cargo test --workspace --locked: 1101 passed, 0 failed,
  74 ignored. Both new PDF blend tests and all three SVG pixel regressions passed.
  Existing SVG editing/provenance, vertical/mixed-script variable text and PDF/AI
  round-trip checks also passed.
- 🟢 [Done] macOS Blocks runtime test: 1 passed.
- 🟢 [Done] Explicit gpu_tests:: ignored suite: 35 passed on Apple M4 / Metal.
- 🟢 [Done] Explicit gpu_cache::tests:: ignored suite: 5 passed, including shared
  textures/queues and separate-window contexts.
- 🟢 [Done] Repeated one brush-crossing test for image export; passed with alpha
  reference difference at most 2/255. This is one repeat of the 35-test suite,
  not another unique test. Inspected one-stroke and six-pass PNGs: smooth edges,
  expected intersections and no missing geometry/tiles. This is renderer-image
  inspection, not full GUI acceptance.
- ⭕️ [Pending] macOS debug bundle validation is in progress.
- ⭕️ [Pending] Full interactive GUI acceptance and Windows/Linux native execution.
- 🔴 [Later] Backdrop-aware PDF flattening for the newly introduced plus-lighter mode.

## Sources

- [Tauri 2.12.5](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.5)
- [resvg 0.49 changelog](https://github.com/linebender/resvg/blob/v0.49.0/CHANGELOG.md)
- [usvg 0.49 crate](https://crates.io/crates/usvg/0.49.0)
- [lopdf 0.46 crate](https://crates.io/crates/lopdf/0.46.0)
- [Vite releases](https://github.com/vitejs/vite/releases)
- [FreeType stable news](https://freetype.org/index.html)
- [libpng stable information](https://www.libpng.org/pub/png/libpng.html)
