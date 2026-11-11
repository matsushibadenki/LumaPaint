# 定期最適化・パネルサムネイルのUI要約省略（2026-11-11）

## Problem

thumbnails::renderがマスクのレイヤーID列挙のためにDocument::snapshot()を作成し、全ネイティブ図形のUI要約・contour解析・asset一覧を再生成していた。11/4に省略したscene reference内部とは別の、パネル生成外側の処理。

## Root cause

マスクの対象membership取得にUI snapshot APIを使用していた。必要な順序は背景layer-1と、一時パス編集ガイドを除いた全描画レイヤー。非表示レイヤーのマスクも対象となる。

## Evidence

Release baselineを先に実行し、2秒・1ms間隔のCPU sampleでthumbnails::render → Document::snapshot → LayerObjectSnapshot → contour_closedを確認。別のmetrics有効実行で32/600/4096/10000図形の各パネル生成につき同数のUI object要約を確認。修正後は全て0。[証跡](performance-2026-11-11/metadata.json)、[profile](performance-2026-11-11/profile.txt)、[集計](performance-2026-11-11/summary.json)に保存。

## Changed files

- crates/lumapaint-renderer/src/thumbnails.rsに列挙変更と、既存マスク表示テストの再要約禁止assertionを追加。
- crates/lumapaint-renderer/examples/bench_panel_thumbnails.rsに再実行用パネル計測。既存bench_native_referenceのfixtureを使用した構成。
- 本報告、測定証跡、roadmap追記。

## Change

既存artwork_svg_layers()とstd::iter::once("layer-1")を連結し、借用したIDを順番に参照する。返すマスクPNGに必要なIDだけを所有文字列にする。各マスクのenable/density正規化とcoverage・PNG生成は従来処理を使用する。

## Why it should improve performance

対象レイヤーのIDを得るための全object要約と派生処理を除去。既存getterでmembershipを維持し、新規cache、保持state、worker、依存を追加する必要がない。

## 環境・条件・手順

開始HEADはa557233543d4298314ee8cc8d9bbe76f292f50ac、開始git statusはclean。既存の変更はなく、実行中も今回の対象外の差分は確認されなかった。AGENTS指示、最適化指示書全文、roadmap、11/4性能報告、11/6–9マスク改善と最新依存更新の報告、既存benchmarkを確認した。依存更新を含む現在ソースからBeforeを再ビルドした。

macOS 27.2.1 (26B131)、ARM64、rustc 1.99.1。Release・default features・thin LTO・codegen-units=1・同一Cargo.lock。RustのDocumentを入力にするhost CPU microbenchmarkで、実機UI操作とは区別する。アプリではpanel_thumbnailsがRust所有文書を取得してblocking workerでこの関数を呼ぶ。UIへ返す小さなパネルPNG、Rust/GPUの本体データ所有、Tauri 2・wgpu・ドッキング等を維持する。

small/medium/large/stressは32/600/4096/10000ネイティブ矩形。既存translation fixture由来で、一部は文書外にある。文書を960×640、サムネイルを64×43へ揃えた。背景のpixel maskと図形レイヤーのvector maskは左半分、disabled、density 0.5。加えてリポジトリの実イラストdocs/images/illust.svgと英語・日本語・简体中文のテキストを同じ2マスク条件で測定した。ユーザー制作の実文書は未提供。stressの件数は製品容量保証を表さない。

1プロセスで6条件を順番に実行。各条件初回1回とwarm 30回を計測。Before/Afterを交互順5組、奇数組はAfter先行、各variant/条件150 warm点。初回をwarm集計へ混ぜない。中央値は中央2点の平均、p95/p99はnearest rank。fixture構築、JSON整合確認、PNG byte比較、stdoutは個々のタイマー外。CPU/RSS/命令数はこれらも含むプロセス全体。

Release buildとprofile用プロセスの終了を確認してから交互測定を開始。自分のテスト・Clippyは交互測定終了後に開始した。システム全体の他負荷・熱・clockは隔離していない。tailの差を一般的な最大遅延保証へ外挿しない。metrics有効実行とCPU sampleは別枠で、時間分布から除外。

## Before / After

単位ms。各セルはBefore → After。

| 条件 | median | p95 | p99 | maximum | median差 |
|---|---:|---:|---:|---:|---:|
| 32 | 0.982 → 0.980 | 1.069 → 1.067 | 1.181 → 1.125 | 1.274 → 1.218 | -0.16% |
| 600 | 1.632 → 1.459 | 1.843 → 1.646 | 2.098 → 1.754 | 2.256 → 1.864 | -10.59% |
| 4096 | 4.671 → 3.292 | 5.207 → 3.729 | 5.406 → 3.910 | 5.629 → 3.988 | -29.53% |
| 10000 | 45.857 → 42.408 | 48.726 → 44.195 | 51.656 → 54.330 | 66.715 → 58.488 | -7.52% |
| artwork | 2.629 → 2.649 | 2.792 → 2.842 | 2.874 → 3.032 | 2.930 → 3.077 | +0.77% |
| text | 0.995 → 0.999 | 1.065 → 1.097 | 1.101 → 1.136 | 1.128 → 1.146 | +0.40% |

600/4096/10000図形の各5組のtrial中央値もAfterが小さかった。small・イラスト・多言語の差は約1%以下で、改善と認定しない。10000図形のp99は5.18%悪化した。medianやCPU仕事量とtailを分け、最大値低減や実UIの応答改善を保証しない。

## CPU impact

計測プログラム全体のretired instructions中央値38,828,988,632 → 35,714,116,808（−8.02%）。全5回のAfterが全5回のBeforeより少ない。user＋system中央値1.95 → 1.80秒（−7.69%）。UI要約件数はnative各N件 → 0件。allocation回数、copy総バイト、lock wait、アプリ入力遅延・frame timeは未測定。

## GPU impact

GPU upload量・GPU frame timeは未測定。このCPUパネル経路の出力全体は一致し、描画shaderやGPU資源管理の変更はない。

## Memory impact

process maximum RSS中央値350.203 → 340.766MiB（−9.438MiB、−2.69%）。アプリ常駐量やVRAMの削減率へ外挿しない。新しいcacheは追加していない。parse cacheのhit/miss/invalidationは今回計測していない。

## I/O impact

タイマー内でファイル／network／IPCの呼出しは追加していない。PNGの生成とRust側の返値はbyte一致。アプリIPC時間は未測定。証跡への書き出しは診断実行のタイマー外。

## Energy impact

電力・idle wakeupは未測定。CPU命令数削減を消費電力の削減率へ換算しない。

## Correctness verification

6条件全てでBefore/Afterのlayers・masks・channels全PNGおよびID／順序を含む返値がbyte一致。各process内の初回／warmも完全一致。各条件の生成前後DocumentState JSONも不変。出力SHA-256はmetadataに記録。解像度や許容誤差を変更していない。

- metrics有効・単一test threadのrenderer thumbnailsテスト11件成功。今回追加の「UI snapshotを作らない」assertion、マスク透明度／disabled／density／編集更新、ピクセル・レイヤー・channel、端タイルを確認。
- 既存core membershipテスト1件成功。hidden layerを含め、active saved-path guideを除外したID列がsnapshotと同じ順序・内容になることを確認。
- 既存core効果のhistory／saved stateテスト1件成功。Undo/Redo・保存状態再構築・旧形式の互換を確認。
- renderer vector_io_roundtrip 15件成功。SVG/PDF・多言語文字等の保存再読込／画素回帰を確認。
- renderer libと新benchmarkのClippy警告禁止、対象Rust fmt、architecture check、frontend TypeScript、git diff空白検査が成功。

全workspace／Skia／GPU全試験は今回実行していない。11/9報告の既知GPU brush比較2件の失敗は未解消で、今回の成功件数に含めない。

## Regression risk

membership取得がsnapshotの変更と乖離する可能性は、既存のmembership一致テストで検出する。背景、hiddenレイヤー、guide除外と順序を維持する。実機パネル操作・複数ドッキングウインドウの更新・Windows/Linuxは今回の変更後に未確認。GUI・物理入力遅延・消費電力はPending。10000図形のp99の悪化を記録し、今後同条件の実機tailで確認する。

## Keep / Revert

Keep。600/4096/10000条件の全5組の中央値、約8%のprocess命令数削減、不要要約0件と全返値byte一致により採用。small/実イラスト/多言語で時間改善を認定しない。新規cacheや所有状態を導入せず、既存APIへ局所置換する範囲で完結。今回の数値をアプリ入力遅延や最大遅延の改善率として扱わない。

## 再実行コマンド

作業ディレクトリはリポジトリroot。Beforeは本変更のproduction差分を適用する前、同じbenchmarkソース・Cargo.lockで生成した。Before/AfterのSHA-256はmetadataを参照。

~~~sh
cargo build --release --offline --locked -p lumapaint-renderer --example bench_panel_thumbnails
# 変更前後それぞれの実行ファイルを別名で保存し、以下を交互に5回実行
env -u LUMAPAINT_RENDER_METRICS /usr/bin/time -l /path/to/variant 30
# 独立した診断実行。出力先directoryは予め作成する
BENCH_OUTPUT=/path/to/png-set LUMAPAINT_RENDER_METRICS=1 /path/to/variant 2
# プロファイル用は150 warm samples。sampleは所有する計測PIDを指定
sample PID 2 1 -file /path/to/profile.txt

LUMAPAINT_RENDER_METRICS=1 cargo test --offline --locked -p lumapaint-renderer --lib thumbnails::tests -- --test-threads=1
cargo test --offline --locked -p lumapaint-core --lib artwork_layers_match_ui_membership_without_path_guides
cargo test --offline --locked -p lumapaint-core --lib ordinary_layer_effects_snapshot_history_and_saved_state
cargo test --offline --locked -p lumapaint-renderer --test vector_io_roundtrip
cargo clippy --offline --locked -p lumapaint-renderer --lib --example bench_panel_thumbnails -- -D warnings
rustfmt --edition 2021 --check crates/lumapaint-renderer/src/thumbnails.rs crates/lumapaint-renderer/examples/bench_panel_thumbnails.rs
python3 scripts/check-core-boundary.py
./node_modules/.bin/tsc --noEmit
git diff --check
~~~

生CSV・process資源・profile・検証ログはperformance-2026-11-11へ保存。バイナリと比較用出力は作業用tempに保持し、リポジトリにはハッシュを記録した。benchmark先頭のコメントのみ計測後に更新し、実行ロジックは同じ。measurement-source.patchは計測時のソース、source.patchは最終差分を記録する。
