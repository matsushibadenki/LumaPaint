# Dependency update — 2026-10-19

## Scope and versions

Checked 50 external workspace/vendor packages using npm latest tags and
crates.io max_stable_version. Prereleases are excluded, including Tauri 3 alpha.
Tauri stays on 2.x; Rust Document/State/Renderer ownership and GPU data boundaries
remain intact. No UI strings or supported languages were changed.

| Registry | Dependency | Before | After |
| --- | --- | --- | --- |
| cargo | `tauri-build` | =2.7.2 | 2.7.3 |
| cargo | `libc` | =0.2.190 | 0.2.191 |
| cargo | `tauri` | =2.12.3 | 2.12.4 |
| cargo | `i_overlay` | =9.0.1 | 9.1.0 |
| cargo | `sha2` | =0.11.0 | 0.11.1 |
| npm | `@tauri-apps/api` | ^2.12.3 | 2.12.4 |
| npm | `react` | ^19.3.0 | 19.3.1 |
| npm | `react-dom` | ^19.3.0 | 19.3.1 |
| npm | `@tauri-apps/cli` | ^2.12.2 | 2.12.3 |

The PDF font reader's bundled FreeType was additionally updated from 2.14.3
to 2.15.0. libpng 1.6.59 and the remaining vendor package versions are current.
Skia stays at 0.153.3; its independently pinned native dependencies are managed
by that upstream release, not replaced by the PDF-reader source update.

## Compatibility and provenance

- Tauri 2.12.4 fixes empty-path IPC/asset URI handling and updates its 2.x runtime
  chain. API and CLI stable packages have independent version numbers.
- React and React DOM stay paired at 19.3.1; no application API migration is needed.
- iOverlay 9.1.0 improves tree-solver performance and fragment/intersection buffer
  preallocation. Existing path-operation APIs are retained.
- FreeType 2.15.0 adds font-format support and rendering fixes. Its public face,
  size and glyph-slot structure fields match 2.14.3. The upstream HVF module now
  requires its C translation unit in the build list; the experimental feature
  remains disabled by default, compiling only its unsupported-format stub.
- Vendor C source has no local patches; the complete official release tree replaces
  the previous release. Existing Rust/build fixes remain. Archive URL/checksum are
  recorded in vendor/freetype-sys/LUMAPAINT-PATCHES.md.
- npm/pnpm/Cargo lockfiles are synchronized. Cargo resolves 35 changed/new package
  entries, including the static_vcruntime Windows runtime helper added through libz-sys.
  No new prerelease Cargo package was introduced.
- The license inventory now covers 627 Cargo packages; none lack license metadata.
  FreeType's PDF-specific FTL and the iOverlay/iKeySort source references are updated.
- Existing user changes in src-tauri/src/recovery.rs and docs/roadmap.md are retained.

## Upstream constraints

Cargo update --dry-run --verbose reports zero compatible updates. Five older
versions are retained by upstream requirements: brotli-decompressor 6.0.1
(brotli's ~6.0), generic-array 0.14.7 (crypto-common's exact pin), toml 0.8.2,
toml_datetime 0.6.3 and toml_edit 0.20.2 (the GTK/proc-macro-crate chain's exact
TOML date pin). Recheck after upstream relaxes those requirements; no forced
cross-major substitutions are applied.

FreeType 2.15.0 still contains its upstream-adapted gzip/zlib 1.3.1 sources,
while standalone zlib stable is 1.3.2. This run retains FreeType's private,
modified embedded copy rather than replacing it with incompatible raw zlib
files. libz-sys is updated to 1.1.30 for the separate PNG link; host system
libraries and Skia's pinned internal codec sources are not claimed to be the
latest standalone upstream builds. Reconsider the embedded copy when FreeType
updates it, or after a separate tested integration of a shared zlib provider.


## Validation

- 🟢 [Done] Frontend production build, architecture checks, measurement/color-mode,
  workspace-notification and timeline-view regression scripts passed.
- 🟢 [Done] npm install reports zero known vulnerabilities for the installed tree.
- 🟢 [Done] Rust formatting and workspace/all-target Clippy with warnings denied.
- 🟢 [Done] Native linkage smoke check reports FreeType 2.15.0 and libpng 1.6.59;
  exact RGBA and partial-alpha PNG decoding passed.
- 🟢 [Done] cargo test --workspace --locked: 1003 passed, 0 failed,
  66 ignored (ordinary ignored tests are not counted as GPU validation).
  Includes PDF Type1/CFF outline, multilingual/vertical text, iOverlay path-operation,
  undo/redo and PDF/AI integration checks.
- 🟢 [Done] macOS Blocks runtime compatibility: 1 passed.
- 🟢 [Done] Explicit gpu_tests:: ignored suite: 27 passed on Apple M4 / Metal.
- 🟢 [Done] Explicit Metal gpu_cache::tests:: ignored suite: 5 passed, including
  shared-queue ordering, distinct-window contexts, cache eviction and premultiplied text.
  An earlier filter matched zero tests; it is excluded from validation counts.
- 🟢 [Done] Repeated the brush-crossing GPU test to export visual fixtures; it passed
  (this repeats one of the 27 tests, not an additional unique regression).
  Inspected the single-stroke and six-pass PNGs: soft edges and intersections are
  present without missing geometry or broken tiles. The pixel test reports a maximum
  alpha reference difference of 2/255. This is renderer-image inspection, not GUI QA.
- 🟢 [Done] npm run tauri build -- --debug --bundles app completed and produced
  target/debug/bundle/macos/LumaPaint.app. This verifies a debug macOS bundle,
  not a signed/notarized release build.
- 🟢 [Done] First-party diff whitespace checks passed; upstream vendor source is
  kept as published. No update was rolled back for an observed compatibility failure.

Saved evidence: [validation snapshots](dependency-validation-2026-10-19/results.json),
[one stroke](dependency-validation-2026-10-19/crossing-one-stroke.png),
[six passes](dependency-validation-2026-10-19/crossing-six-passes.png).

- ⭕️ [Pending] Windows/Linux native execution and full interactive GUI acceptance.
  No successful current GUI inspection is claimed; the previous CUA attempt was
  interrupted. Ordinary ignored tests are not counted as executed GPU checks.

The earlier 2026-10-12 changes exist in the current baseline but have no complete
saved validation report. This run validates the current combined dependency state;
it does not reconstruct missing historical test results.

## Sources

- [Tauri stable releases](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.4)
- [React 19.3.1](https://github.com/react/react/releases/tag/v19.3.1)
- [iOverlay published crate](https://crates.io/crates/i_overlay/9.1.0)
- [FreeType release news](https://freetype.org/index.html)
- [zlib stable release](https://zlib.net/)
- [libpng release information](https://www.libpng.org/pub/png/libpng.html)
- [libc 0.2.191](https://crates.io/crates/libc/0.2.191)
- [sha2 0.11.1](https://crates.io/crates/sha2/0.11.1)
