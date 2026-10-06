# Color None verification — 2026-10-06

## Implemented behavior

None is distinct from black and from an RGBA transparent color. Vector fill/stroke use absent paint; text uses a serialized noColor flag and retains glyph layout. Brush None makes no document/history edit; erasing remains enabled. Gradient stop None sets alpha to zero; choosing RGB restores visible alpha. The synthetic built-in None swatch is always available; custom None swatches persist in the shared library. Tone Studio None affects unlocked palette entries; locked colors remain, and null entries export as None swatches.

## Browser QA

Browser plugin was unavailable. Used bundled Playwright with installed Chrome, local Vite server, and a temporary controlled fixture rendering the real application components. This does not verify the native macOS document window end to end.

| Page/surface | Verification | Result |
|---|---|---|
| Common picker | None indicator, red slash, HEX restoration, Japanese/English/Simplified Chinese | Pass |
| Color panel | None selected state and retained RGB | Pass |
| Gradient panel | Stop alpha 0, RGB restores alpha 255 | Pass |
| Swatches | Built-in None sample, application callback | Pass |
| Text panel | None submits selection style patch | Pass |
| Tool dialog | Brush None survives Apply | Pass |
| Narrow viewport | 390×760, picker inside viewport with margins | Pass |
| Desktop viewport | 1100×820, usable controls and no overlay | Pass |
| Tone Studio | None, locked color preservation, null export, RGB restoration, three locales | Pass |

No blank page, Vite error overlay, browser console errors, or page errors in the exercised flows. Screenshots inspected during verification: `/private/tmp/lp-none-swatches.png`, `/private/tmp/lp-none-desktop.png`, `/private/tmp/lp-none-narrow.png`, `/private/tmp/lp-tone-none.png`. Temporary fixtures removed after checks.

## Rust verification and limitations

Regression tests cover unpainted geometry selection, atomic Undo/Redo, native round trip, legacy Brush defaults, None brush and independent erasing, selected UTF-16 text ranges and unchanged geometry, invisible text pixels versus neighboring visible characters, shared None swatch persistence, and canonical unpainted SVG geometry.

Core/formats boundary checks, workspace tests, Clippy with warnings denied, formatting, TypeScript, and macOS debug app build are checked. GPU-only tests remain ignored under the workspace's normal test configuration. No Adobe application comparison or live native editor automation was performed. SVG preserves basic unpainted shapes; grouped/clipped invisible editability and Adobe format round-trip fidelity are not claimed by this change.
