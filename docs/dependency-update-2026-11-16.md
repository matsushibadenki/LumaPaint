# Dependency update — 2026-11-16

| Rust dependency | Before | After |
| --- | --- | --- |
| wgpu | 30.0.1 | 30.0.2 |
| i_overlay | 9.1.0 | 9.2.0 |
| crc32fast | 1.5.4 | 1.5.5 |
| ZIP | 9.5.0 | 9.6.0 |

Cargo resolved 51 new package/version pairs without prerelease suffixes. The compatible-update dry run reports zero further updates. Tauri stays on 2.x. No production application or vendor source was modified. Rust/wgpu ownership, JS IPC boundaries, and existing UI languages are unchanged. The regenerated license inventory has 655 entries, with no missing license metadata. The existing untracked docs/mcp work was left intact.

[Registry audit](dependency-validation-2026-11-16/registry.json), [release metadata](dependency-validation-2026-11-16/releases.json), [complete Cargo delta](dependency-validation-2026-11-16/cargo-delta.json).

- [wgpu 30.0.2](https://github.com/gfx-rs/wgpu/releases/tag/v30.0.2) includes surface-acquisition and device-loss fixes. [ZIP 9.6](https://github.com/zip-rs/zip2/releases/tag/v9.6.0) fixes append handling and raw-copy metadata. Existing call sites compile unchanged. [i_overlay](https://crates.io/crates/i_overlay/9.2.0) and [crc32fast](https://crates.io/crates/crc32fast/1.5.5) remain within their current major series.
- 🔴 [Later] Skia 0.157.0 remains upstream-marked prerelease. moxcms 0.9.2 is unchanged from the rejected 2026-11-09 candidate; the Display P3 reference exceeded the existing tolerance. Revisit when stable status or a passing color regression is available.
- 🔴 [Later] [usvg/resvg 0.50](https://github.com/linebender/resvg/blob/v0.50.0/CHANGELOG.md) adds optional/lazy bounds, recursive-reference handling and font resolver changes. A three-way merge against the published 0.49 and 0.50 crates exposed five conflicted usvg files spanning CSS parsing, conversion, font layout and tree bounds. Retained both patched 0.49 crates and their shared svg2pdf adapter. Revisit with a dedicated API migration and exact SVG/Metal/vertical-font regressions. The other vendored sources and bundled FreeType 2.15.0 and libpng 1.6.59 remain unchanged; private zlib/native Skia pins are upstream-managed.
- ⭕️ [Pending] The Vite 8.3.8 candidate in the working tree cannot be accepted under the stable-only requirement: its Rolldown 1.2.18–1.2.x range resolves an optional WASI binding with exact emnapi 2.0.0-beta.2 dependencies. 8.3.7 has the same range. Prepared a separate stable-only patch [for review](dependency-validation-2026-11-16/npm-stable-only.patch) retaining Vite 8.3.6 and updating React plugin 6.2.1→6.3.0 and compatible PostCSS. The patch passes git apply --check; npm ci, frozen pnpm validation, frontend build, measurement/color-mode and document-layout regressions passed on the separate stable candidate. Automatic approval review rejected applying replacement files due to possible loss of uncommitted work, so the reviewed replacement was not applied. Do not treat the candidate npm locks as stable-only.

- 🟢 [Done] Rust fmt and workspace/all-target Clippy passed. The candidate frontend passed npm ci, frozen pnpm lock verification, frontend build, architecture and measurements/color-mode tests.
- 🟢 [Done] Workspace tests: 1,371 passed, zero failed, 99 ignored. Workspace doctests and the Blocks runtime check also passed.
- 🟢 [Done] Shared Metal: 5 passed. GPU: 43 passed and two known failures (crossing 7/255 against 4/255; DryBrush 255 vs 203). Both are identical to the previously reproduced baseline diagnostics. The two crossing images are byte-identical to the November 9 fixtures and the six-pass output was visually inspected.
- 🟢 [Done] The local macOS debug bundle built with a temporary updater-artifacts override using the current Vite candidate. The stable-only frontend was tested separately and has not been bundled into a macOS app.
- ⭕️ [Pending] Stable-only macOS bundle, signed updater packaging, full GUI and Windows/Linux remain unverified. The previously deferred app latency comparison is outside this run.
