# Pen path closing — 2026-10-26

## Behavior

Pressing the first anchor enters a closing gesture; release commits one closed path. A drag creates symmetric outgoing/incoming handles at the existing first anchor. Clicking alone preserves its previous handles. The closing target retains the existing screen-space tolerance and does not create another anchor. The closed curve and handles are included in the draft preview. After crossing the two-screen-point drag threshold, moving back inside it continues to update the handle rather than retaining its old length. Escape cancels the draft.

## Regression coverage

The pen subset passed 10 tests. After the native crash fix below, the complete macOS native library suite passed **226 tests, with 2 opt-in benchmarks ignored**. The closing regression checks the exact outgoing/incoming control coordinates, no duplicate anchor, preview closure, one-step undo/redo, click-only handle preservation, returning near the starting point, and cancellation. It also calls `direct_preview` while holding `CANVAS.borrow_mut()` to reproduce the native renderer's ownership contract.

Logs: `/tmp/lumapaint-pen-close-results.log` (initial subset), `/tmp/lumapaint-pen-close-final-tests.log` (complete final suite), `/tmp/lumapaint-pen-close-final-build.log` (final debug build). The first ordinary Cargo test launch was interrupted after waiting at startup and is not counted as a passing run. Builds and final tests used `CARGO_INCREMENTAL=0` after repeated long compiler waits; this is a verification command setting, not a repository configuration change.

## Native acceptance and correction

The isolated macOS debug bundle `app.lumapaint.pen-close-qa` embeds the production WebView assets and renders using Metal on Apple M4. A new 800×600 document was displayed at 84.33%. A vector layer received screen anchors (300,330), (600,330), (600,580). Dragging the first point to (370,390) committed a visibly curved closed path.

Selecting the result with Direct Selection then aborted. The captured diagnostic log `/tmp/lumapaint-pen-close-native.log` identifies `RefCell already mutably borrowed` at `box_tolerance`, called from `direct_preview` while `redraw` already held the canvas mutably. `direct_preview` now computes the same 14-screen-point corner-guide distance from its supplied zoom (`14 / zoom`), without accessing CANVAS. The complete final suite includes that render-time mutable-borrow condition and passes.

The corrected app was rebuilt, signed and launched with diagnostic logging (`/tmp/lumapaint-pen-close-final-native.log`). After an initial `noWindowsAvailable` interruption, the user brought the QA app forward and the final native acceptance completed:

- Fresh 800×600 document, one vector layer, 84.33% display, real Metal canvas.
- Screen anchors (300,330), (500,280), (600,500), closing drag from (300,330) to (350,240): the closing seam visibly became a continuous curve.
- `A` switched to Direct Selection; clicking the first anchor displayed both handles at approximately (350,240) and (250,420), with no crash.
- Command-Z removed the path in one operation; Command-Shift-Z and selecting the first point restored the same curve and both handles.
- The app saved `/tmp/lumapaint-pen-close-qa.lumapaint`. Its single path has 10 control points (3 cubic segments), identical first/last anchors, and a trailing `Z`. The saved seam is anchor `[172.332,149.4071]`, outgoing `[231.62054,42.687744]`, incoming `[113.04346,256.12646]`; their sum equals twice the anchor within 0.0001 document pixels. The saved file therefore retains the actual curve geometry rather than only a preview.
- The final diagnostic log contained no panic, and the saved QA app quit normally.

The final native all-target Clippy check passed with `-D warnings` (`/tmp/lumapaint-pen-close-clippy.log`); formatting and diff checks passed. No UI strings or IPC payloads changed.

The first long CUA attachment wait is not an application latency measurement. The failed initial app and final corrected app are distinguished above; no success is claimed for the failing run.

Physical tablet input and Windows/Linux native behavior remain Pending. No physical-input latency or Release performance result is claimed.
