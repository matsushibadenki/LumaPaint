# After Effects exchange

通常文書の「書き出し」で **After Effects — PNG + JSX (ZIP)** を選択し、拡張子を `.zip` にします。ZIPを展開した後、After Effectsの「ファイル > スクリプト > スクリプトファイルを実行」で `Import-LumaPaint.jsx` を選びます。スクリプトと `frames` フォルダーは同じ場所に置いてください。

Choose **After Effects — PNG + JSX (ZIP)** in Export and use the `.zip` extension. Extract the archive, then run `Import-LumaPaint.jsx` using After Effects **File > Scripts > Run Script File**. Keep the script beside the `frames` folder.

在导出中选择 **After Effects — PNG + JSX (ZIP)**，使用 `.zip` 扩展名。解压后，在After Effects的「文件 > 脚本 > 运行脚本文件」中选择 `Import-LumaPaint.jsx`。脚本与 `frames` 文件夹必须放在同一目录。

## Contract

- Active page, frames `0..duration` (exclusive end), evaluated with the existing Rust animation evaluator, including text outlines for motion effects.
- Full document dimensions; no thumbnail downscaling. Uses the existing Rust document raster export path. Canvas background follows the document's background settings; choose a transparent canvas when transparency is required.
- PNG RGBA8 uses straight alpha. The script explicitly sets straight alpha, authored fps, square pixels, start time zero and duration `frames / fps`.
- A single frame is imported as a still; multiple frames are imported as a numerically ordered sequence. Each export is an isolated archive, avoiding unrelated frames from older exports.
- Flattened appearance only. Layers, editable keyframes, audio, ICC profiles, loops and inactive pages are not transferred. The script creates a new composition in the current AE project; it does not save or replace the project.
- Tile-only documents retain their existing PSD/PSB export path.
- Limits: 1–99 integer fps, 1–10,000 frames, dimensions at most 30,000 each, at most 16,777,216 pixels, at most 256 MiB total PNG payload. Errors abort before destination writing. The archive is assembled in Rust memory; peak memory includes the document copy and rendered frame.

## Validation

Format tests cover frame order, portable script paths, frame-rate/duration/alpha settings, single-frame import, invalid dimensions/fps/frame count and renderer failure propagation. The host regression test checks that active drawing and authored document state survive export. AE runtime acceptance and pixel comparisons are pending; script generation alone does not establish AE compatibility across versions.

Adobe's [scripting overview](https://helpx.adobe.com/after-effects/using/scripts.html) describes running ExtendScript scripts. The implementation uses the legacy ExtendScript API rather than the newer UXP API.

2026-10-30 verification: all 3 format tests and the host immutability regression test passed; `cargo check -p lumapaint --tests`, `cargo fmt --all -- --check`, architecture boundary checks and `git diff --check` passed. After Effects 2026 is installed, but Computer Use failed to attach with `timeoutReached`; actual script execution and visual comparison remain pending.
