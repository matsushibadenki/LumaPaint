# 定期最適化・channel_viewの複製削減候補を不採用（2026-11-18）

## Problem

パネルのchannel_viewが全SVGレイヤーを描画用文書へ複製した後、背景／マスク表示では全て破棄し、通常レイヤー表示では選択以外を破棄していた。

## Root cause

全体の描画用コピーを先に作り、表示するレイヤーを後から絞る順序。大きな文書で不要なString・VectorObjectのcloneと解放が発生する。

## Evidence

現在ソースのRelease baselineを取得後、2秒・1ms間隔のsampleでchannel_view → clone_for_renderingを確認。候補ではsampleの当該複製経路がほぼ消えたが、同条件5組のprocess peak RSSが全組で増加し、smallの時間分布も悪化した。[計測集計](performance-2026-11-18/summary.json)、[修正前profile](performance-2026-11-18/profile.txt)、[候補profile](performance-2026-11-18/after-profile.txt)。profile全体の時間にはfixture準備等も含まれ、sample数をframe時間へ換算していない。

## Changed files

候補はcrates/lumapaint-core/src/document.rsとdocument_channels.rs。最終的に今回のproduction変更は全て戻した。本報告・証跡・roadmapへの記録を残した。候補のみの差分をcandidate.patchに保存。

## Change

既存clone_for_renderingの共通処理へ複製済みSVGレイヤーを引数として渡すprivate helperを設け、channel_viewだけ対象レイヤーを先に選ぶ候補を試した。背景とマスクは空、その他は選択レイヤーのみ。通常のclone_for_renderingは従来通り全レイヤーを渡す構成。

## Why it should improve performance

コピーしてすぐ破棄するレイヤーを生成しない。新しいcache・状態・依存は不要。大きな条件では時間と命令数が改善したが、プロセスの実メモリとsmallの回帰を含めて採否を判断した。

## 環境・データ・手順

macOS 27.2.1 (26B131)、ARM64、rustc 1.99.1、Release、default features、thin LTO、codegen-units=1、offline locked。開始HEADと全ソース／バイナリhashはmetadataに記録。開始時からdocument.rs、layer_effects.rs、paint_cache.rs、root_paint.rs、channel_tools_macos.rs、roadmap.mdに未コミット変更があった。背景マスクの複製削減等の既存作業を保持し、その状態をBeforeとしてビルドした。候補rollback後の既存productionソースhash一致を確認した。commit／reset／stashは実施していない。

最適化指示書全文、提供されたAGENTS指示、git差分、roadmap、11/11性能報告、11/16依存報告、既存benchを確認。Rust Document／State／Renderer、wgpuとWebViewの境界を確認し、panel_thumbnailsのblocking worker内にあるCPUパネル生成を対象とした。Tauri 2・GPU所有・既存ドッキング／多言語UIへ変更を加えていない。

既存bench_panel_thumbnailsを使用。960×640文書、64×43のPNG。32/600/4096/10000のnative矩形、リポジトリの実イラストdocs/images/illust.svg、英語・日本語・简体中文textの6条件。背景pixel maskと図形vector maskは左半分・disabled・density 0.5。矩形は同じ黒色で一部が文書外にあり、全件数に比例する描画面積や製品容量の保証にはならない。ユーザー制作の文書は未提供。

最初に現状5process×各条件30warmを測定し、別processでprofile。その後候補をビルドし、Before（current）／候補（after）を交互5組×各条件30warmで比較。奇数組は候補先行。各variant・条件150点、初回を除外し、medianは中央2点の平均、p95/p99はnearest rank。fixture作成・JSON比較・PNG比較・stdoutは各呼出しのタイマー外。CPU/RSS/命令数はそれらも含むprocess全体。metricsとprofileは独立実行。自分のビルド・テストと交互計測は重ねていない。他のシステム負荷・clock・熱は隔離していない。

実機UIの入力遅延と、このhost CPU microbenchmarkを区別する。候補は不採用のため実アプリへの受入れを追加していない。

## Before / After

以下のAfterは不採用候補。最終ソースはBeforeへ復元。単位ms。

| 条件 | median B→候補 | p95 B→候補 | p99 B→候補 | maximum B→候補 | median差 |
|---|---:|---:|---:|---:|---:|
| 32 | 0.906 → 0.942 | 0.930 → 0.980 | 0.938 → 1.049 | 0.949 → 1.157 | +4.00% |
| 600 | 1.329 → 1.283 | 1.353 → 1.327 | 1.362 → 1.367 | 1.367 → 1.387 | -3.43% |
| 4096 | 2.746 → 2.170 | 2.862 → 2.227 | 2.973 → 2.246 | 3.029 → 2.253 | -20.98% |
| 10000 | 39.579 → 36.692 | 40.376 → 37.663 | 40.518 → 37.905 | 40.729 → 38.105 | -7.29% |
| artwork | 2.452 → 2.461 | 2.591 → 2.587 | 2.664 → 2.630 | 2.703 → 2.656 | +0.35% |
| text | 0.959 → 0.961 | 0.998 → 0.989 | 1.100 → 0.998 | 1.408 → 1.011 | +0.16% |

## CPU impact

process retired instructions中央値35,279,844,550 → 33,989,406,988（−3.66%）。user＋system中央値1.66 → 1.57秒（−5.42%）。4096図形のmedianは約21%短縮した一方、smallは約4%増加。allocation回数・copy総bytes・lock waitは未測定。cloneを通るレイヤー数の減少をallocatorの実測値と同一視しない。

## GPU impact

GPU upload・GPU frame timeは未測定。GPU処理の変更はなく、CPUパネル生成の測定に限定。

## Memory impact

process maximum RSS中央値340.094 → 349.063MiB（+8.969MiB、+2.64%）。Before5回は339.78–340.80MiB、候補5回は349.02–349.36MiBで全て増加。複製削減にもかかわらずRSSが増える原因の分解は未完了。allocatorの再利用・配置等も関与し得るため、論理的なclone削減を実メモリ削減と主張しない。

## I/O impact

各呼出しのPNG返値は一致。アプリIPC・disk・network時間は未測定。

## Energy impact

電力・idle wakeup未測定。命令数差を電力へ換算しない。

## Correctness verification

6条件の全layers／masks／channelsのPNG・ID・順序がBefore／候補でbyte一致し、11/11の保存SHA-256とも全一致。各process内の初回／warm出力と、生成前後DocumentState JSONも同一。解像度や数値許容差を変えていない。

- 候補状態で既存core channel試験1件、renderer thumbnails 11件、効果history／saved state 1件、vector_io_roundtrip 15件が成功。選択レイヤーchannel・背景・マスク・disabled/density・SVG/PDF・多言語・Undo/Redo・保存状態の該当経路を検証した。
- 候補のcore／renderer lib Clippy警告禁止、対象fmt、architecture、frontend TypeScriptが成功。Clippy最初の呼出しは重複--libで引数エラーとなり、修正後のコマンドで完了した。両方のログを残した。
- 全workspace／Skia／GPU試験は未実行。11/16依存報告の既知GPU brush比較失敗2件を今回の成功として数えていない。
- rollback時に変更前productionソースの全保存hash、document_channelsのHEAD内容、既存roadmapの内容一致を検証した。

## Regression risk

候補のsmall latencyとRSS回帰の原因は未分解。候補を戻したため、その回帰を最終productionへ残していない。開始時に存在した別作業のマスク最適化を含む現状全体について、実機受入れやWindows/Linux・GPU／物理入力遅延の成功を主張しない。

## Keep / Revert

Revert。大規模条件のmedianと命令数削減を確認したが、全5回で約9MiB増えるpeak RSSとsmall中央値約4%悪化が残った。今回追加した2ファイルのhunkだけを戻し、開始時の未コミット作業は全て保持した。新たな推測修正を追加せず今回の試行を終了する。

## コマンドと再現

同じCargo.lock・同じ既存benchmarkで、候補適用前の実行ファイルをcurrent、候補後をafterとして別保存した。候補差分はcandidate.patch。最初のbaseline-1〜5.csvは現状確認、比較表は交互順current-1〜5.csv／after-1〜5.csvから集計。profile・metrics実行を時間比較へ混ぜていない。

~~~sh
cargo build --release --offline --locked -p lumapaint-renderer --example bench_panel_thumbnails
# 変更前後の実行ファイルを別保存して、順序を交替して各5回
 env -u LUMAPAINT_RENDER_METRICS /usr/bin/time -l /path/to/variant 30
# 診断専用。BENCH_OUTPUTのdirectoryは予め作成
BENCH_OUTPUT=/path/to/outputs LUMAPAINT_RENDER_METRICS=1 /path/to/variant 2
# variant 150を別processで実行し、その所有PIDをsample
sample PID 2 1 -file /path/to/profile.txt
cargo test --offline --locked -p lumapaint-core --lib document_channels::tests
LUMAPAINT_RENDER_METRICS=1 cargo test --offline --locked -p lumapaint-renderer --lib thumbnails::tests -- --test-threads=1
cargo test --offline --locked -p lumapaint-core --lib ordinary_layer_effects_snapshot_history_and_saved_state
cargo test --offline --locked -p lumapaint-renderer --test vector_io_roundtrip
cargo clippy --offline --locked -p lumapaint-core -p lumapaint-renderer --lib -- -D warnings
rustfmt --edition 2021 --check crates/lumapaint-core/src/document.rs crates/lumapaint-core/src/document_channels.rs
python3 scripts/check-core-boundary.py
./node_modules/.bin/tsc --noEmit
git diff --check
~~~

入力遅延・frame time・allocation/copy・cache hit/miss/invalidation・IPC・GPU upload・idle wakeup・実消費電力は今回の定量結果に含めない。CPU sampleにはSVG再解析の負荷も残る。大きな入力のcache適格性・保持上限や実機での呼出し頻度を調べる必要があり、この1回でcache容量を変更していない。次の判断は実機profileとメモリ分解で行う。
