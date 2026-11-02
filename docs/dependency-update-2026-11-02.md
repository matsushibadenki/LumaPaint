# Dependency update — 2026-11-02

## Scope and versions

Queried 74 external package names/registry entries, including workspace and vendor dependencies, against official crates.io max_stable_version and npm latest. No new upstream-marked prerelease was adopted. Tauri stays 2.x. English/Japanese/Simplified Chinese, Rust Document/State/Renderer ownership and wgpu rendering remain unchanged; large frames remain outside JS IPC.

| Package | Before | After |
| --- | --- | --- |
| zip | 4.6.1 | 9.2.1 |
| crc32fast | 1.5.3 | 1.5.4 |

Cargo resolved 24 added/new-version package records and removed 23 old records. npm direct dependencies are already latest stable; compatible transitive updates cover rolldown 1.2.15→1.2.16, @oxc-project/types 0.155.0→0.156.0 and postcss 8.5.31→8.5.32, including platform bindings (18 package records). npm and pnpm resolutions were synchronized by importing the updated package-lock. Frozen pnpm lock verification passes; npm outdated is empty and npm reports zero known vulnerabilities.

Saved exact registry data and lock changes: [evidence](dependency-validation-2026-11-02/registry.json), [Cargo changes](dependency-validation-2026-11-02/cargo-changes.json), [npm changes](dependency-validation-2026-11-02/npm-changes.json).

## Compatibility and upstream constraints

- Skia 0.156.0 was considered because crates.io reports it as max_stable_version, but the official GitHub release explicitly sets prerelease=true. The GitHub latest-stable endpoint returns 404 and the recent release list (including the existing baseline) uses that flag. Conservatively retained the current 0.153.3 instead of adopting a new upstream-marked prerelease. Only that candidate's manifest/lock changes, GPU flush API migration and native notice edits were removed; the other candidate groups were evaluated independently. Reconsider when upstream confirms a stable release, or the user explicitly accepts the project's GitHub prerelease convention. [Official release metadata](dependency-validation-2026-11-02/skia-deferred-release.json). Candidate-state checks are excluded from final validation counts.
- moxcms 0.9.1 is a stable candidate, but the existing Photoshop half-transparent Display P3 regression produced premultiplied RGB max error 10/255 against the unchanged limit of 9/255 (16,384 pixels, alpha max 1/255). Reverting only moxcms to 0.8.1 and rerunning the same test gives max 9/255 and passes. Its update was removed; no tolerance was relaxed or color conversion patched to hide the difference. Reconsider a newer stable release that passes this same reference test. [A/B result](dependency-validation-2026-11-02/moxcms-compatibility.json).
- ZIP 9 changes metadata/extra-field APIs and expands parallel extraction/writing APIs. LumaPaint retains its in-memory, limited-feature ZIP operations; no new extraction or parallel API is enabled. Existing IDML and After Effects interchange tests cover import/export behavior and rejected traversal paths.
- ZIP 9 makes entry-name decoding fallible. Production IDML now maps name errors to its existing invalid-archive error and owns the decoded name before reading entry data; examples/tests use the corresponding fallible API. A malformed UTF-8 local/central name regression was added. Traversal/size/encryption/duplicate checks remain in place.
- CRC remains in its stable 1.x line.
- usvg/svg2pdf/simplecss/block/FreeType vendor patches remain untouched. Upstream version ranges for harfrust, skrifa, strict-num, miniz_oxide and pdf-writer are retained; they are not forcibly moved across incompatible ranges merely to deduplicate the lockfile.
- Final Cargo dry-run reports zero compatible updates and retains the upstream-constrained generic-array 0.14.7, toml 0.8.2, toml_datetime 0.6.3 and toml_edit 0.20.2. [Dependency chains](dependency-validation-2026-11-02/retained-constraints.json).
- image 0.25.10 also requires moxcms 0.8.x internally; after excluding the color-regression candidate, both image and the direct conversion path share 0.8.1.
- Direct HTTP checks confirm standalone FreeType 2.15.0 and libpng 1.6.59 remain current. Cached search-engine copies still showed older releases, so they were not used to downgrade vendor sources. FreeType's modified private zlib and Skia's internal native pins remain upstream-managed.
- The Cargo license inventory now covers 634 packages with no missing license metadata. Skia native source pins and notices remain at the baseline after excluding that candidate. The retained moxcms 0.8.1 BSD/Apache and adopted ZIP 9.2.1 MIT notice originals are included.

## Validation

- 🟢 [Done] Frontend build, architecture boundaries, measurement/color-mode, workspace notification, timeline and document-layout regressions.
- 🟢 [Done] npm/pnpm lock verification and npm vulnerability audit.
- 🟢 [Done] Final Rust fmt and workspace/all-target Clippy with warnings denied.
- 🟢 [Done] Workspace unit/integration suites: 1,290 passed, zero failed, 85 ignored. This includes the ZIP invalid-name regression, IDML/After Effects interchange, PSD color/alpha and SVG/PDF/font checks.
- 🟢 [Done] Workspace doc tests passed after a package artifact rebuild. The first workspace command completed all runtime suites but hit rustdoc E0463 for renderer/Tauri lookup; rebuilding package doc artifacts and rerunning `cargo test --workspace --doc --locked` succeeded without disabling tests or changing final source/lock. This repair is not counted as another runtime-suite pass.
- 🟢 [Done] Explicit macOS Blocks runtime: 1 passed. Shared Metal/cache suite: 5 passed.
- 🟠 [Next] Explicit GPU suite: 42 passed, 2 failed out of 44. Both failures reproduce individually with the updated graph and an isolated `git archive HEAD` baseline using the original Cargo.lock: brush crossing has source-over formula difference 2/255 but connected/separate difference 7/255 against limit 4/255; grain rotation has DryBrush material=false at (739,240), 255 versus 203. These are pre-existing failures, not passed checks. The dependency update does not change renderer/shader source or relax either assertion. Follow up on brush crossing/rotation independently of dependency maintenance. [Baseline comparison](dependency-validation-2026-11-02/gpu-baseline-comparison.json).
- 🟢 [Done] Inspected generated [one-stroke](dependency-validation-2026-11-02/crossing-one-stroke.png) and [six-pass](dependency-validation-2026-11-02/crossing-six-passes.png) output: strokes/soft edges/intersections are present; the fixed crossing appearance is retained. Both complete PNG files are byte-identical to the original-dependency baseline. This confirms no change for these fixtures, not approval of the two failed numerical guards or full GUI acceptance. [Hashes](dependency-validation-2026-11-02/gpu-baseline-image-comparison.json).
- 🟢 [Done] `npm run tauri build -- --debug --bundles app` succeeded, including the frontend rebuild, producing the 221.76MiB macOS debug app. No running editor was restarted and no user document was modified.

[Saved final results](dependency-validation-2026-11-02/results.json).
- ⭕️ [Pending] Later parallel frontend edits, full interactive GUI acceptance, signed/notarized Release and Windows/Linux native execution. Existing user-deferred input-latency comparisons remain deferred.

## Environment and preserved work

Preserved the current user diff in roadmap, timeline view/model/tests/CSS. Before edits a binary patch snapshot was saved in /tmp/lumapaint-before-2026-11-02.patch. No source files from that work were reverted. The timeline work continued to change during maintenance; those edits were left intact. The Tauri frontend rebuild and repeated model/notification/measurement/color/layout regressions passed for the observed snapshot. Additional parallel timeline edits occurred afterward; those later frontend edits are outside this run’s verification and were not overwritten. [Preserved snapshot](dependency-validation-2026-11-02/preserved-frontend-snapshot.json).

Initial Rust compilation was blocked in readdir on the accumulated target/debug/deps directory (directory metadata size ~37MB), confirmed with a one-second compiler sample. Only this run's Cargo/compiler processes were stopped. Reproducible old incremental caches and the generated dependency directory were quarantined/recreated and old generated artifacts were removed; source, lock definitions, bundles and user files were preserved. Compilation then resumed normally. This environment interruption is not attributed to a dependency API failure.

An artifact-only rustdoc lookup failure during candidate/feature switching was resolved by package rebuilding and the full workspace doc-test rerun; no test was disabled.

## Upstream sources

- [rust-skia 0.156.0 release](https://github.com/rust-skia/rust-skia/releases/tag/0.156.0)
- [moxcms 0.9.1](https://github.com/awxkee/moxcms/releases/tag/0.9.1)
- [ZIP 9.2.1 changelog](https://github.com/zip-rs/zip2/blob/v9.2.1/CHANGELOG.md)
- [crc32fast 1.5.4 registry](https://crates.io/crates/crc32fast/1.5.4)
- [FreeType news](https://freetype.org/)
- [libpng stable](https://www.libpng.org/pub/png/libpng.html)

- [Rolldown 1.2.16](https://github.com/rolldown/rolldown/releases/tag/v1.2.16)
- [PostCSS 8.5.32](https://github.com/postcss/postcss/releases/tag/8.5.32)
- [aws-lc-rs 1.19.0](https://github.com/aws/aws-lc-rs/releases/tag/v1.19.0)
