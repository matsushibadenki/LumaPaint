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


## 2026-10-22 — square wheel and common picker

🟢 [Done] The common ColorPanel defaults to a square saturation/value field inside the hue ring. Compact square/triangle buttons switch geometry without changing RGB. The preference uses an external UI-preference store with localStorage persistence, same-window notifications and storage events for sibling windows. If storage writes are blocked, the current session still switches. Existing document ownership and color commands remain in Rust. The square uses CSS gradients; the triangle retains its existing bounded canvas. HSV edit coordinates preserve saturation at RGB black so increasing brightness restores the selected color instead of gray.

🟢 [Done] MotionPanel, ScreentonePanel and OnionControls now use ColorPickerPopover instead of HTML color inputs. Motion and screentone retain their explicit Apply workflow. Onion colors stage edits and commit the latest draft on picker close, preserving the opposite color and unrelated settings. Disabled controls and localized labels remain. Source search of src/ and src-tauri/ finds no remaining type=color inputs, NSColorPanel/NSColorWell/NSColorPicker or showPicker calls.

Browser plugin not available (no browser skill in the session); existing Playwright/Chromium used without installing dependencies. The flow under test is a controlled fixture using production components and workspace CSS → choose HSV coordinates or enter HEX → switch shape/open each replaced picker → verify the resulting RGB, preference and callbacks. Local URL: http://127.0.0.1:1422/; viewports: 900×800 and 360×640. The existing 1420 server was left untouched. Test fixtures, server and scripts reside under /tmp rather than committed source; the temporary 1422 server was stopped after verification.

| Check | Result |
|---|---|
| Page identity and meaningful content | Pass |
| Framework overlay, page errors and console warnings/errors | None |
| Square midpoint, white/pure-color positions and black | Pass |
| Saturation retained at black and restored by brightness | Pass |
| Triangle pure color/white coordinates | Pass |
| Shape switch preserves RGB | Pass |
| Reload, another page and live storage-event sync | Pass |
| Storage-write failure retains session shape | Pass |
| English, Japanese, Simplified Chinese labels | Pass |
| Narrow popover bounds and screenshot | Pass |
| Onion previous-color HEX commit preserves next color | Pass |
| Disabled onion trigger | Pass |
| Screentone and motion HEX reflected in picker draft | Pass |

Square, triangle and narrow screenshots were visually inspected. TypeScript/Vite build, architecture boundaries and diff checks pass. The pre-existing large-bundle advisory remains. This UI-only change does not require a Rust rebuild. Native document rendering, native WebView/window storage behavior, and Rust-side Apply/save/Undo were not exercised by the fixture; physical native-window integration remains ⭕️ [Pending].
