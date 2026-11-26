# Dependency update — 2026-11-23

| Dependency | Before | After |
| --- | --- | --- |
| Tauri (Rust) | 2.12.5 | 2.12.7 |
| tauri-build | 2.7.4 | 2.7.6 |
| @tauri-apps/api | 2.12.4 | 2.12.5 |
| @tauri-apps/cli | 2.12.4 | 2.12.6 |
| pollster (renderer and host) | 1.0.1 | 1.0.3 |
| ZIP | 9.6.0 | 9.8.1 |
| Vite | 8.3.8 | 8.3.6 (exact pin) |
| Rolldown and native bindings | 1.2.19 | 1.2.16 |
| @rolldown/pluginutils | 1.0.1 | 1.1.1 |
| csstype | 3.2.3 | 3.2.4 |
| source-map-js | 1.2.2 | 1.2.4 |

The working tree was clean at the start (HEAD f9bd541). Existing application changes, vendored patches, UI languages and Rust Document/State/Renderer ownership were preserved. No application API migration was needed; Tauri remains on 2.x and large frame data remains on the Rust/GPU side. CI commands were reviewed in `.github/workflows/ci.yml`.

Cargo resolved 40 new package/version/source pairs and removed 35, all stable and not yanked according to the official version endpoints. A subsequent `cargo update --dry-run` resolves zero additional compatible changes. Cargo's eight constrained newer dependencies were not forced. npm and pnpm contain the same 81 package/version pairs, without prerelease suffixes. The Cargo license inventory now covers 662 packages; the new npm inventory covers 81. Neither lacks license metadata. The bundled ZIP notice exactly matches the 9.8.1 source LICENSE.

[Registry audit](dependency-validation-2026-11-23/registry.json), [official releases](dependency-validation-2026-11-23/releases.json), [native and intermediate releases](dependency-validation-2026-11-23/native-and-releases.json), [Cargo version metadata](dependency-validation-2026-11-23/cargo-metadata.json), [Cargo delta](dependency-validation-2026-11-23/cargo-delta.json), [npm delta](dependency-validation-2026-11-23/npm-delta.json).

- [Tauri 2.12.7](https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.12.7) updates its runtime, Wry runtime, macros and build utilities. [CLI 2.12.6](https://github.com/tauri-apps/tauri/releases/tag/tauri-cli-v2.12.6) fixes development-server HTML handling. [JS API 2.12.5](https://github.com/tauri-apps/tauri/blob/dev/packages/api/CHANGELOG.md) fixes mocked event listeners and wraps scale-change sizes in PhysicalSize. Existing call sites compile unchanged.
- [ZIP 9.7](https://github.com/zip-rs/zip2/releases/tag/v9.7.0) exposes safe fixed-header parsing; [9.8](https://github.com/zip-rs/zip2/releases/tag/v9.8.0) adds a zstd option; [9.8.1](https://github.com/zip-rs/zip2/releases/tag/v9.8.1) fixes header length checks. LumaPaint retains its existing deflate-only feature selection. [pollster 1.0.3](https://crates.io/crates/pollster/1.0.3) stays in the current major series.
- 🟢 [Done] Resolved the previous npm stable-only pending item. Vite 8.3.7/8.3.8 requires Rolldown ~1.2.18; its WASI package pins emnapi 2.0.0-beta.2. Rolldown 1.3.3 lies outside that range. Vite is therefore pinned to 8.3.6 with the compatible locked Rolldown 1.2.16. Removed both beta packages and their WASI dependency chain. No upstream constraint was overridden. Revisit when Vite's supported graph contains only stable versions.
- The previous replacement was rejected because it might discard uncommitted changes. This run established a clean baseline, checked the narrow patch with `git apply --check`, and received automatic approval to apply that patch. It did not replace whole files from HEAD or discard local work. The prior approval blocker is resolved.
- 🔴 [Later] Patched usvg/resvg 0.49 and svg2pdf remain unchanged. Latest SVG 0.50 still requires the dedicated CSS/text/bounds migration documented on November 16. Preserve custom rendering fixes and re-evaluate with SVG/font/Metal regressions. Skia 0.157.0 is still marked prerelease upstream. moxcms 0.9.2 remains the same candidate that exceeded the existing Display P3 tolerance; re-evaluate after an upstream change passes that reference. FreeType 2.15.0 and libpng 1.6.59 remain the official current versions; private zlib and Skia native pins remain upstream-managed. Other newer indirect major versions (e.g. nanoid) remain constrained by their parents.

- 🟢 [Done] npm ci, frozen pnpm lock verification, frontend build, architecture boundaries, measurements, color modes and document-layout regressions passed. The initial architecture attempt ran before Cargo lock resolution and failed; rerunning after resolution passed. Manifest editing also briefly produced an invalid serde_json version and mismatched pollster pins; both were corrected before compilation, without changing serde_json.
- 🟢 [Done] Rust fmt and workspace/all-target Clippy passed. `cargo test --workspace --locked`: 1,379 passed, zero failed, 100 ignored; all workspace doctests passed in the same run. Blocks runtime: 1 passed.
- 🟢 [Done] Shared Metal: 5 passed. Explicit GPU tests on Apple M4/Metal: 43 passed, 2 failed. Crossing pen-lift difference and DryBrush rotation (pixel 739,240: 255 vs 203) match the previously reproduced baseline failures. Both crossing PNGs are byte-identical to November 16, and the six-pass image was visually inspected. Assertions and tolerances were preserved; the two failures remain unresolved existing issues.
- See [Rust log](dependency-validation-2026-11-23/rust.log), [GPU log](dependency-validation-2026-11-23/gpu.log), [Metal log](dependency-validation-2026-11-23/metal.log), [frontend log](dependency-validation-2026-11-23/front-final.log) and [model/boundary log](dependency-validation-2026-11-23/model.log).


- 🟢 [Done] `npm run tauri build -- --debug --bundles app --config '{"bundle":{"createUpdaterArtifacts":false}}'` passed and produced a 233.37 MiB macOS app. This bundle uses the stable-only npm graph, resolving the previous stable-only bundle pending item. The override was command-local; updater configuration/public keys were not changed.
- 🟢 [Done] Launched the newly built app by its exact bundle path. Japanese menus, compact toolbars, layer panel and A4 white canvas displayed at 1200×800; status reported Metal / Apple M4. Created a fresh test document, deferred the old recovery-copy prompt without restoring/deleting copies, drew one diagonal brush stroke, observed Undo remove it and Redo restore it (stroke count 1→0→1). No missing canvas/panel rendering was observed in this smoke test. The unsaved test document remains open; no existing artwork file was edited.
- ⭕️ [Pending] Signed updater packaging, release-mode bundle, Windows/Linux and comprehensive GUI coverage (including English/Simplified Chinese visual checks, docking/floating, multiple screens and SVG/font interactions) remain unverified. Existing multilingual code was unchanged. The user-deferred real-app latency comparison remains deferred; no latency result is claimed.
