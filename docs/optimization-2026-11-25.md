# 定期計測・現状維持（2026-11-25）

## Problem

前回はchannel_viewの複製削減候補がsmallとpeak RSSを悪化させ、撤回した。現在のRelease版でパネル・scene参照生成の性能と画素を再確認し、局所変更を採用できる根拠を調べた。

## Root cause

CPU sampleではSVG解析／描画とchannel_viewの描画用複製に処理が残る。11/18の候補のメモリ回帰原因は未分解。今回のprofileだけでは、安全で費用対効果のある追加の局所変更を特定できていない。

## Evidence

Releaseを現在のCargo.lockから再ビルドして、既存bench_panel_thumbnailsとbench_scene_referenceを実行。パネル6条件とscene6条件の各median／p95／p99／maximumを取得した。独立した診断でscene warm 150/150 hit、warm再解析0、BeforeのPNG／RGBAとの全hash一致を確認。[集計](performance-2026-11-25/summary.json)、[画素比較](performance-2026-11-25/pixel-comparisons.json)、[profile](performance-2026-11-25/profile.txt)。

## Changed files

本報告・証跡・roadmapのみ。production、benchmark、テストのソースは変更していない。

## Change

現状維持。前回不採用の複製削減を再導入せず、cache容量・精度許容差を変更していない。

## Why it should improve performance

今回は採用変更がないため改善率は算出しない。追加の複雑化を避け、次の判断用baselineを記録した。

## 環境・データ・限界

macOS 27.2.1 (26B131)、ARM64、rustc 1.99.1、Release・default features・thin LTO・codegen-units=1・offline locked。HEAD、実行ファイルとsource hashはmetadata。開始時の依存更新・EDFXキャンバス分割・マスク確定処理・CI／i18n等の未コミット作業を保持し、その状態を基準にした。開始git statusはinitial-status.txt。ソースをreset／stash／commitしていない。

最適化指示書全文、提供AGENTS指示、git差分、roadmap、11/18最適化報告、11/20マスクRelease QAと11/23依存報告、既存benchを確認。パネルはRust所有Documentをblocking workerへ渡してCPUで縮小生成し、小さなpreview PNGを返す既存経路。Document／State／Renderer所有、Tauri 2・wgpu、大容量データのRust/GPU保持と既存の多言語・docking構成を維持した。

これはhost CPU microbenchmark。実機の入力・WebView・IPC待ち・GPU present・物理表示を含まない。11/20の実機マスクQAは3600×3200・4レイヤー・128px brushの別条件であり、今回の値と結合しない。EDFX配置・変形や現在のマスク確定変更のGUI受入れも今回の成功範囲に含めない。

パネルは960×640文書を64×43 PNGに変換。32/600/4096/10000のnative矩形、実イラストdocs/images/illust.svg、英語・日本語・简体中文のtext。背景pixel maskと図形vector maskは左半分・disabled・density 0.5。黒い矩形は一部が文書外で、可視描画面積は件数と比例しない。ユーザー制作文書は未提供。

sceneは960×640を256×171 RGBAへ変換。32/600/4096/8192のimported SVG矩形、同イラスト、多言語文字＋mask＋blurの6条件。trial毎に識別用コメントを変えたsourceを使用する。

パネルは5process×各条件初回1＋warm30、sceneは1processで各条件5trial×初回1＋warm30。各条件150warm点。中央値は中央2点平均、p95/p99はnearest rank。初回はwarm集計へ含めない。fixture作成・出力比較・状態JSON比較・stdoutはタイマー外。metricsは別実行でscene各5trial×初回1＋warm5。profileも独立processのpanel 150warmで2秒・1ms間隔。自分のbuild／testsと計測は重ねていない。システム全体の負荷・clock・熱は隔離していない。

## Before / After

今回は変更前後の対比較を作っていない。以下は現在版のbaseline。単位ms。

| 処理 | 条件 | median | p95 | p99 | maximum |
|---|---|---:|---:|---:|---:|
| panel | 32 | 0.951 | 1.518 | 2.115 | 2.419 |
| panel | 600 | 1.423 | 1.797 | 2.881 | 2.996 |
| panel | 4096 | 3.004 | 5.711 | 10.998 | 12.048 |
| panel | 10000 | 42.153 | 46.285 | 58.672 | 59.568 |
| panel | artwork | 2.677 | 4.099 | 5.349 | 6.026 |
| panel | text | 1.006 | 1.226 | 1.565 | 1.612 |
| scene | small | 0.302 | 0.316 | 0.322 | 0.333 |
| scene | medium | 1.049 | 1.091 | 1.487 | 1.959 |
| scene | large | 5.834 | 8.184 | 12.966 | 25.313 |
| scene | stress | 11.525 | 12.242 | 12.393 | 12.435 |
| scene | artwork | 1.923 | 2.132 | 2.313 | 3.707 |
| scene | text_blur | 1.565 | 1.615 | 1.714 | 1.765 |

11/18のBeforeより今回のパネルtailは大きいが、依存・ソース・実行日の異なる非同時比較であり、コード由来の性能低下とは断定しない。大きな値を除外していない。同一バイナリの確認値も別記し、過去日の値を今回の改善率へ換算しない。

## CPU impact

パネルprocess全体のretired instructions中央値35,615,185,492、user＋system中央値1.83秒。fixture・JSON検証・PNG比較等を含み、アプリframeのCPU時間と区別する。SVG解析・描画とchannel_viewの複製はprofileに残る。allocation回数とcopy総bytesは未測定。

## GPU impact

GPU upload・GPU frame timeは未測定。shaderとGPU資源所有に変更なし。

## Memory impact

パネルprocess maximum RSS中央値341.781MiB。常駐アプリ・VRAMの実使用量には換算しない。

sceneの診断はwarm 150 hit、warm miss／bypass／parse 0。初回を含む全180呼出しは30 miss／30 parse／30 admission、bypass 0、eviction 17。trialごとの識別コメント変更による新規sourceも含む。admitted bytes総計151,598,380は累積であり、peakではない。insertion時の最大retained chargeは66,180,992 bytes（既存64MiB上限内）。allocatorやRSSの直接計測値と区別する。編集時のinvalidationはこのワークロードで測定していない。

## I/O impact

preview／stateのIPC量とdisk／network時間は未測定。出力保存は診断実行のタイマー外。

## Energy impact

消費電力とidle wakeupは未測定。命令数を電力に換算しない。

## Correctness verification

6パネルの全PNG・ID・順序が11/11のhashと一致し、30scene RGBAが10/28のhashと一致。各process内の初回／warmも完全一致し、生成前後DocumentState JSONも不変。解像度・許容差を変更していない。

同一panelバイナリを検証終了後にさらに5process×30warmで再実行した。各条件150点、別系列として保存。

| 条件 | 確認median ms | p95 | p99 | maximum |
|---|---:|---:|---:|---:|
| 32 | 1.908 | 2.628 | 2.882 | 3.010 |
| 600 | 3.089 | 4.207 | 4.685 | 7.437 |
| 4096 | 7.431 | 10.287 | 11.495 | 11.736 |
| 10000 | 94.795 | 128.642 | 214.093 | 265.375 |
| artwork | 5.680 | 23.679 | 59.948 | 209.978 |
| text | 2.189 | 3.121 | 7.774 | 11.516 |

同じ実行ファイルでも確認系列の全medianが大きく変動した。確認後の観測では10 logical CPUに対してload average 15.69、node約272%とpython3各約99%が3processあった。観測は系列終了後の1時点であり、各sampleの遅延原因の直接証明ではない。環境負荷の影響が考えられるため、コード起因の回帰や改善を確定しない。他のprocessは停止・変更していない。追加の繰り返しで数値を選別せず、両系列を保存して終了した。

既存回帰はthumbnails 11件、core channel 1件、効果history／saved state 1件、SVG/PDF vector_io_roundtrip 15件、計28件成功。対象の透明度・disabled/density・マスク更新・選択channel・多言語・Undo/Redo・保存再読込を確認。core／renderer libの警告禁止Clippy、architecture、frontend TypeScript、16辞書の英語・日本語・简体中文i18n検査も成功。全workspace／Skia／明示GPU試験は今回実行していない。11/23報告の既知brush GPU失敗2件は今回の成功件数に含めない。

## Regression risk

追加のproduction変更はない。現在の未コミットEDFX分割・マスク確定最適化等について、実機の全受入れが成功したとは扱わない。今回のワークロードに含まれない操作・外部文書の正しさを一般化しない。システム負荷を揃えたtail評価、GPU・入力遅延・電力はPending。

## Keep / Revert

現状維持。新しい局所変更を採用する根拠が不足しているため、推測によるパッチを追加しなかった。11/18に撤回した候補を再導入していない。今回の改善率・回帰率は未確定。計測・画像・既存回帰の記録だけを残す。

## コマンドと再現

リポジトリrootでReleaseを再ビルドし、実行ファイルを別保存した。path placeholderは作業用のdirectoryへ置き換える。初回のpanel-1〜5と同一バイナリconfirmation-1〜5は別集計。

~~~sh
cargo build --release --offline --locked -p lumapaint-renderer --example bench_panel_thumbnails
cargo build --release --offline --locked -p lumapaint-renderer --example bench_scene_reference
# panelを各5process実行
 env -u LUMAPAINT_RENDER_METRICS /usr/bin/time -l /path/to/panel 30
# panelはBENCH_OUTPUT、sceneはLUMAPAINT_BENCH_PIXELSへ初回の出力を保存
BENCH_OUTPUT=/path/to/outputs LUMAPAINT_RENDER_METRICS=1 /path/to/panel 2
 env -u LUMAPAINT_RENDER_METRICS /path/to/scene --samples 30 --runs 5
LUMAPAINT_BENCH_PIXELS=/path/to/pixels LUMAPAINT_RENDER_METRICS=1 /path/to/scene --samples 5 --runs 5
# panel 150を別processで実行し、その所有PIDへsample
sample PID 2 1 -file /path/to/profile.txt
LUMAPAINT_RENDER_METRICS=1 cargo test --offline --locked -p lumapaint-renderer --lib thumbnails::tests -- --test-threads=1
cargo test --offline --locked -p lumapaint-core --lib document_channels::tests
cargo test --offline --locked -p lumapaint-core --lib ordinary_layer_effects_snapshot_history_and_saved_state
cargo test --offline --locked -p lumapaint-renderer --test vector_io_roundtrip
cargo clippy --offline --locked -p lumapaint-core -p lumapaint-renderer --lib -- -D warnings
python3 scripts/check-core-boundary.py
./node_modules/.bin/tsc --noEmit
node scripts/check-i18n.mjs
git diff --check
~~~
