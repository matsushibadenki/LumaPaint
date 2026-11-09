# Dependency update — 2026-11-09

## Adopted stable versions

| Dependency | Before | After |
| --- | --- | --- |
| lopdf | 0.46.0 | 0.47.0 |
| svgtypes | 0.16.1 | 0.16.2 |
| flate2 | 1.1.10 | 1.1.11 |
| zip | 9.2.1 | 9.5.0 |
| rfd | 0.17.2 | 0.18.0 |
| @vitejs/plugin-react | 6.1.2 | 6.2.1 |
| vite | 8.3.5 | 8.3.6 |

Checked 79 declared external dependency names/registry entries and all six vendored packages against official registries. Final Cargo changes contain 26 added/new-version records and 27 removed records; all added versions were checked non-yanked and without prerelease suffix (SemVer build metadata is retained). npm changes are the two direct development packages above. All 81 npm/pnpm package/version pairs match after importing package-lock into pnpm, and frozen lock verification passes. Tauri remains 2.x, app version 0.0.2 remains unchanged, and Rust Document/State/Renderer ownership, wgpu rendering and command/state-only JS IPC are preserved. No UI strings were added; existing English/Japanese/Simplified Chinese behavior is retained.

[Registry evidence](dependency-validation-2026-11-09/registry.json), [vendor audit](dependency-validation-2026-11-09/vendor-registry.json), [Cargo delta](dependency-validation-2026-11-09/cargo-changes.json), [updated registry versions](dependency-validation-2026-11-09/updated-cargo-registry.json), [npm delta](dependency-validation-2026-11-09/npm-changes.json), [lock consistency](dependency-validation-2026-11-09/lock-consistency.json).

## Compatibility and upstream notes

- lopdf fixes encryption-filter defaults, decompression-limit enforcement, inline image masks and PDF xref-version handling. Existing PDF/AI round-trip and bounded/malformed input tests were executed. No application PDF API migration was required.
- svgtypes adds CSS color/identifier handling and returns errors for invalid transform-origin rather than panicking. Existing SVG editing/provenance, mask/filter and mixed-language text regressions were retained.
- flate2 fixes sync flush and gzip header/input-error progress handling. Existing native containers, PSD and ZIP round-trips exercise the adopted codec. ZIP 9.5 adds an extraction root filter; LumaPaint keeps bounded in-memory import/export and does not enable the new extraction API.
- rfd adds hidden-file options and a macOS named-filter save picker, and fixes fallback starting directories. Its Rust MSRV is 1.92; host Rust 1.99.1 and the CI stable toolchain satisfy it. Existing call sites compile unchanged. Interactive native picker acceptance remains pending.
- Vite fixes CSS dependency collection and overlapping container-close handling. The React plugin repairs refresh include/exclude options. Production frontend compilation and existing model regressions passed.
- Vendored resvg/usvg/svg2pdf/simplecss/block/FreeType sources and local patches were not changed. The resvg offscreen-layer coordinate fix remains intact. FreeType 2.15.0 and libpng 1.6.59 remain the upstream standalone stable releases checked directly. FreeType's private modified zlib and Skia's native pins remain upstream-managed, not forcibly replaced.
- Cargo dry-run reports zero compatible updates. Older generic-array/toml chains and vendor constraints for harfrust/skrifa/pdf-writer/miniz_oxide are retained instead of cross-major overrides.

## Deferred candidates

- 🔴 [Later] Skia 0.157.0 is the registry max stable version but is explicitly marked prerelease by its official GitHub release. Retain 0.153.3 under the previous audit policy. Reconsider an upstream-confirmed stable release or explicit acceptance of that release convention. [Upstream metadata](dependency-validation-2026-11-09/skia-release.json).
- 🔴 [Later] moxcms 0.9.2 was tried because it includes P3/gamma/NEON fixes. The same 16,384-pixel Photoshop half-transparent P3 test still gives maximum premultiplied RGB difference 10/255 against the unchanged limit 9/255. Reverting only moxcms to 0.8.1 passes at 9/255. The update was removed; no rendering tolerance was relaxed. Reconsider a newer stable version passing that reference guard. [A/B result](dependency-validation-2026-11-09/moxcms-compatibility.json).

## Test repair and validation

The first workspace run exposed an existing thumbnail assertion comparing a 262,144-byte padded GPU tile with a 4,096-byte 32×32 image. The baseline and working `paper_uploads` implementations are identical. The test now validates origin/extent and compares every logical image byte using bytes_per_row, retaining exact equality. No paint-cache production code or numerical threshold was changed by this repair.

- 🟢 [Done] Frontend build; architecture; measurement/color-mode; workspace notification; timeline view/clock/graph; document layout regressions.
- 🟢 [Done] Rust fmt and workspace/all-target Clippy with warnings denied.
- 🟢 [Done] Final workspace unit/integration run: 1,371 passed, zero failed, 98 ignored. The repaired thumbnail test and retained P3 test pass. Ignored tests are not counted as executed.
- 🟢 [Done] A rustdoc E0463 lookup failure appeared during native/test feature builds. Serial package doc rebuilding followed by workspace doc tests succeeds with no source changes or disabled tests. The initial workspace command's nonzero exit is retained in the evidence rather than reported as a clean one-command pass.
- 🟢 [Done] Blocks runtime: 1 passed. Shared Metal/cache suite: 5 passed.
- 🟠 [Next] GPU suite: 42 passed, 2 failed out of 44. They are the same failures documented and reproduced with the original dependency graph on 2026-11-02: crossing formula error 2/255 but pen-lift difference 7/255 against limit 4/255; DryBrush rotation material=false at (739,240), 255 versus 203. Neither assertion nor shader was changed. These remain known brush-rendering work, not passed checks.
- 🟢 [Done] Inspected [one stroke](dependency-validation-2026-11-09/crossing-one-stroke.png) and [six passes](dependency-validation-2026-11-09/crossing-six-passes.png): stroke paths, soft edges and intersections are retained. Both complete PNG files are byte-identical to the prior original-dependency baseline. This does not negate the failed numerical guards or constitute full GUI acceptance. [Image comparison](dependency-validation-2026-11-09/image-comparison.json).
- 🟢 [Done] macOS local debug app build succeeds (229.13MiB) using `npm run tauri build -- --debug --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'`. The override is invocation-only; release configuration and updater public key were not changed.
- ⭕️ [Pending] The normal bundle command builds the app but exits at updater signing because this environment lacks TAURI_SIGNING_PRIVATE_KEY. Signed updater artifacts, signed/notarized Release, full interactive GUI/native picker acceptance and Windows/Linux native execution are not verified. Existing user-deferred application input-latency comparisons remain deferred. No release was published.

[Exact commands and results](dependency-validation-2026-11-09/results.json).

## Preserved work and licenses

The existing layer_mask.rs / paint_cache.rs mask-bounds work, roadmap edits and untracked mask-bounds measurement file were preserved. Maintenance changed dependency definitions/locks, notices, this report/roadmap and the independent thumbnail assertion only. [User snapshot](dependency-validation-2026-11-09/preserved-user-snapshot.json).

Cargo license inventory covers 638 packages, none lacking license metadata. Updated dependency notice originals were rechecked/copied for lopdf/zip/rfd/svgtypes/flate2 and OSAKit wrappers; existing Skia notices remain at the retained version. Only inactive generated incremental caches older than 72 hours were removed when build free space dropped to 11GiB; active caches and source were retained.

## Official sources

- [lopdf 0.47.0](https://github.com/J-F-Liu/lopdf/releases/tag/v0.47.0)
- [svgtypes 0.16.2](https://github.com/linebender/svgtypes/releases/tag/v0.16.2)
- [flate2 1.1.11](https://github.com/rust-lang/flate2-rs/blob/v1.1.11/CHANGELOG.md)
- [ZIP 9.5.0](https://github.com/zip-rs/zip2/releases/tag/v9.5.0)
- [rfd 0.18.0](https://github.com/PolyMeilex/rfd/releases/tag/0.18.0)
- [Vite 8.3.6](https://github.com/vitejs/vite/releases/tag/v8.3.6)
- [React plugin 6.2.1](https://github.com/vitejs/vite-plugin-react/releases/tag/plugin-react@6.2.1)
- [moxcms 0.9.2](https://github.com/awxkee/moxcms/releases/tag/0.9.2)
- [FreeType](https://freetype.org/), [libpng](https://www.libpng.org/pub/png/libpng.html)
