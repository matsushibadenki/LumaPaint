# 定期最適化：シーン参照のSVG再解析を削減（2026-10-21）

Problem:
シーン／セル参照のピクセルキャッシュがrevision・frame・sizeの変更でmissした際、静的な背景などの同じSVGも再解析していた。通常の文字キャッシュは、文字のないSVGとマスク・blurを含む文字を保持していなかった。

Root cause:
`cel_previews::prepare_onion` → `thumbnails::scene_pixels` → `rasterize_svg_reference`が、通常の文字向け`parsed_text::parse`へ接続されていた。先週追加されたworkspace用の解析ツリーキャッシュを参照ワーカーは利用していなかった。

Evidence:
変更前の別実行の診断では、600図形の温まった再生成median約1,729µsに対し解析約686µs、実作品SVGでは約3,243µsに対し解析約1,308µs。10回ずつの反復でどちらも解析10回・hit 0回。これらのmedianは個別の統計であり厳密な加算可能な内訳ではない。`sample`の1秒スタックでも`Tree::from_str`、XML変換・図形変換の反復を確認した。

Changed files:
- `crates/lumapaint-renderer/src/vector.rs` — 参照生成の接続と回帰テスト。
- `crates/lumapaint-renderer/src/vector/parsed_text.rs` — 既存キャッシュへのadmission・保持推定量・evictionの診断を追加。
- `crates/lumapaint-renderer/examples/bench_scene_reference.rs` — 再実行可能なCPU参照生成benchmark。
- 本報告、`docs/performance-2026-10-21/`の数値・ログ・patch、`docs/roadmap.md`。

Change:
参照生成で既存の`parse_workspace`を利用し、workspaceと同じ不変の派生ツリーを共有する。新しいキャッシュ、スレッド、依存は追加しない。32 entries／32MiBの推定保持上限、512KiB以下の入力、全文一致、LRU、競合時に待たず解析へfallbackする方針を維持する。画像・gradient・pattern・GaussianBlur以外のfilterなど、既存の保持対象外資源は引き続き保持しない。通常キャンバスのGPU経路、RustのDocument/State/Renderer所有、大容量画像をJSへ送らない境界は変更しない。

Why it should improve performance:
同じSVGのXML解析・図形変換・文字組版を省き、必要な解像度のラスタライズと合成を実行する。参照サイズやpan/zoomは解析ツリーのキーに含めない。ソース変更は全文一致で区別し、過去ソースへの復帰も同じ正確な内容だけを再利用する。

## Before / After

開始時HEAD `da3e5d5b4f621032473aa73823f62ae341bfbdbe`、作業ツリーclean。最初に計測用exampleのみ追加してBeforeを作成し、その後に本実装を加えてAfterを作成した。macOS 27.0.1（26A434）、ARM64、rustc 1.99.0、Release（opt-level=3 / thin LTO / codegen-units=1）。このbenchmarkはrendererのdefault featuresでビルドした。参照のCPU resvg経路はskia有効時も同じであり、skia付きの回帰検証は別途行った。バイナリ・素材・計測ソースのSHA-256と環境・コマンドは`metadata.json`に保存。

全fixtureは960×640文書、参照256×171 RGBA8。合成図形small=32、medium=600、large=4,096、stress=8,192。実データはリポジトリの`docs/images/illust.svg`（405 path・11 polygon）を埋め込んだ構成で、ユーザーの制作プロジェクトとは区別する。さらに英語・日本語・简体中文＋マスク・Gaussian blurを含むfixtureを用いた。初回のfixtureでは埋込み位置にXML宣言を残す誤りがあったため修正し、成功した試行を取り直した。失敗した試行は受入れ統計に含めない。

同じ計測ソースのBefore/After実行ファイルを交互に、各条件5回・各回100 warm samples、合計各variant/条件500 samples。奇数trialはAfter先行・条件順も逆転。コンパイル、テスト、profiler、resource計測を受入れ時間測定と重ねていない。初回coldは別記録とし、表は**warmの全500サンプルのpooled分布**。p95/p99はnearest rank（ceil(N×p)-1）、medianは中央2点の平均。各trialの統計も`summary.json`に保存。最初のcoldは各条件5点しかなく、特に初回フォント探索を含む値から安定したtailや改善率を主張しない。

| 条件 | median B→A ms | p95 B→A ms | p99 B→A ms | maximum B→A ms | median変化 |
|---|---:|---:|---:|---:|---:|
| 32図形 | 0.335 → 0.295 | 0.361 → 0.601 | 0.403 → 0.825 | 0.501 → 0.906 | -11.86% |
| 600図形 | 1.778 → 1.040 | 2.325 → 1.269 | 4.757 → 2.047 | 74.437 → 2.425 | -41.49% |
| 4,096図形 | 10.592 → 5.641 | 12.062 → 6.535 | 13.765 → 8.598 | 15.487 → 21.949 | -46.74% |
| 8,192図形（保持対象外） | 21.115 → 21.004 | 22.624 → 23.009 | 25.642 → 35.488 | 28.381 → 44.420 | -0.53% |
| Illustrator由来SVG | 3.377 → 1.990 | 3.986 → 2.591 | 4.633 → 3.816 | 5.730 → 4.607 | -41.06% |
| 3言語＋マスク＋blur | 3.091 → 1.559 | 3.495 → 1.671 | 3.590 → 1.804 | 3.747 → 1.998 | -49.57% |

smallでは最初のp95/p99が悪化し、stressでもtailに揺れがあったため、ビルド等を重ねず追加5回×200 warm samples（各variant/条件1,000点）を交互測定した。

| 条件 | median B→A ms | p95 B→A ms | p99 B→A ms | maximum B→A ms |
|---|---:|---:|---:|---:|
| 32図形 | 0.332 → 0.292 | 0.358 → 0.315 | 0.405 → 0.343 | 0.491 → 0.486 |
| 8,192図形（保持対象外） | 20.946 → 20.936 | 21.788 → 21.782 | 22.575 → 22.464 | 31.432 → 37.133 |

保持対象外のstressには速度向上を主張しない。smallの追加確認では中央値・p95/p99が改善し、stressの中央値・p95はほぼ同等。最大値はlarge/stress等で悪化した試行もあり、最大遅延の改善・上限は保証しない。他アプリやOSの負荷は隔離していない。10/14に観測したSVG生成の外れ値についても、今回の参照生成測定で原因を断定していない。

## Cache effectiveness

時間測定とは別にmetricsを有効化し、各条件5 trial×初回1＋warm 5を計測した。

| 項目 | Before | After |
|---|---:|---:|
| 5つの保持対象条件のwarm解析数（125呼出し） | 125 | 0 |
| 同warm cache hit | 0 | 125（100%） |
| 同条件のcold解析 | 25 | 25 |
| stressの全解析（保持対象外、30呼出し） | 30 | 30 |
| 全条件の解析合計 | 180 | 55 |

Afterでは25 admissions、累計admitted estimate 62,674,660 bytes、13 evictions。入力変更時に旧キーを破壊する方式ではなく、別キーとしてmiss／解析／admissionする。明示的なinvalidationは追加していない。`svg_parses`を実際の再構築量として記録し、反復だけでinvalidateするキャッシュを作っていない。

1回の参照呼出しにつきSVG一つなので、admission時に採取した保持推定量の最大33,519,464 bytes（約31.97MiB）は既存32MiB上限内だった。これはRSSではない。複数admissionイベントのcounterは加算値になるため、一般のフレームでその合計を常駐量と解釈しない。退避されたツリーを他のArc利用者が保持している間や共有font DB・allocator等はこの上限の外にある。既存の容量・件数・同長ソース相違・競合・retirement検証も通過した。実アプリでのworkspaceと参照ワーカーの共用時のhit率は今回の単独反復から推定しない。

CPU impact:
上表のホストwall timeと、`sample`によるCPU stackを計測した。別に`time -l`で、全6条件を各5 trial×初回1＋warm 5実行するプロセスをBefore/After各3回交互測定。プロセス全体のuser+system CPU時間の中央値は1.33→1.06秒（約20.3%減）、retired instructions中央値は26,152,013,188→21,200,895,097（約18.9%減）。fixture準備・正しさチェック・出力・フォント初期化等も含む値であり、単一操作やアプリのCPU/frameではない。最初のrestricted実行は統計アクセス権不足で失敗したため、同じBefore/Afterを通常権限で取り直した。lock waitやidle wakeupは今回定量化していない。

GPU impact:
参照のresvg処理、ラスタ出力、GPUへのアップロード形式・回数は変更なし。実GPUの参照合成テストを実行した。GPU frame time、GPU完了、入力→物理表示は未測定。

Memory impact:
同じresource試行のmaximum RSS中央値は52,428,800→63,897,600 bytes、50.00→60.94MiB（+10.94MiB、+21.88%）。ツリーを保持する代償がある。workspaceと参照で既存の32MiB推定枠を共有し、上限を追加・拡大していない。推定量、RSS、OS footprintは区別し、生値を保存した。allocation総回数・copy総量は未測定。

I/O impact:
ファイル形式、IPC、大容量データの境界を変更していない。画素比較とファイル出力は時間計測の外。実ファイルI/Oのレイテンシ・ネットワーク転送量は未測定。

Energy impact:
電力・jouleは未測定。CPUの仕事量が減ったことは確認したが、消費電力の改善率は算出しない。

Correctness verification:
- 全6条件×5 trialの**30組**で、変更前後のRGBA全bytesが一致。各試行内のwarm画素もcoldと一致し、DocumentStateのJSONは前後一致。比較manifestのSHA-256・寸法・byte数を保存。
- rendererの通常287 tests＋SVG/PDF往復12 tests成功。通常suiteの62 ignoredを検証済みとは扱わない。
- 新規`reference_tree_reuse_preserves_pixels_sizes_and_source_changes`をmetrics有効・single-threadで別実行し成功。workspaceとの共有で再解析しないこと、サイズ変更、マスク・blur・回転半透明の多言語文字、ソース変更と復帰のfresh画素一致、寸法拒否を検証。競合時は正しさを維持するfallbackを許容。
- Tauriのscene cache／worker 2 tests成功（document/revision/frame/sizeの分離、LRU、stale key拒否）。オプトインworker benchmarkは今回未実行。
- `gpu_onion_overlay_matches_surface_formats_and_keeps_half_alpha --ignored`を通常GPUアクセスで明示実行し成功。RGBA8 sRGB／BGRA8 sRGB／RGBA16Floatで参照合成を検証。残りのignored GPU testsまで通過と扱わない。
- renderer skia付きall-target Clippy（warnings拒否）、Rust fmt、core/formats/rendererの境界、寸法・ルーラー、Lab D50色変換の回帰、diff check成功。
- UI変更なし。frontend buildと実GUIの60秒×5 trial受入れは今回の検証に含めない。既存の未完了受入れを完了へ変更していない。Adobeとの全面的な描画一致や全効果・全OSの品質保証も主張しない。

Regression risk:
参照とworkspaceが同じLRU枠を使うため、混合操作ではeviction／競合が増える可能性がある。非待機fallbackと既存上限を維持する。今回のwarm hit 100%は単独fixtureでの値。coldのadmissionコストとRSS増加、large/stressのmaximumの揺れを残課題として保持する。

Keep / Revert:
Keep。対象の600/4,096図形・実作品SVG・多言語blurで中央値が約41〜50%短縮し、再解析削減とCPUの仕事量減少を確認し、画素・状態・保存互換の回帰が通過した。smallの追加確認でも効果が再現した。RSS増加を明記し、既存の保持枠を維持した小さな接続変更として採用する。保持対象外stressの変化は誤差として扱い、最大遅延の改善を主張しない。次回、混合操作で変更由来の回帰が再現した場合は今回の接続だけを戻す。

## 再実行

```sh
cargo build --release --offline -p lumapaint-renderer --example bench_scene_reference
./target/release/examples/bench_scene_reference --samples 100 --runs 5
LUMAPAINT_RENDER_METRICS=1 ./target/release/examples/bench_scene_reference --samples 5 --runs 5
cargo test --offline -p lumapaint-renderer --features skia
LUMAPAINT_RENDER_METRICS=1 cargo test --offline -p lumapaint-renderer --features skia reference_tree_reuse_preserves_pixels_sizes_and_source_changes -- --test-threads=1
cargo test --offline -p lumapaint-renderer --features skia gpu_onion_overlay_matches_surface_formats_and_keeps_half_alpha -- --ignored
cargo test --offline -p lumapaint cel_previews::tests::scene -- --test-threads=1
cargo clippy --offline -p lumapaint-renderer --features skia --all-targets -- -D warnings
```

Beforeは計測example追加後・実装変更前を同じ条件でビルドし、実行ファイルを別保存する。`metadata.json`と`source.patch`で対象を識別する。`--case small|medium|large|stress|artwork|text_blur`で条件を選び、Afterと別プロセスで交互実行する。`LUMAPAINT_BENCH_PIXELS=<既存の出力ディレクトリ>`でcoldのRGBAを保存できる。性能測定ではmetricsを無効にし、コンパイル・検証・profilerと重ねない。

- 🟢 [Done] CPU参照のbaseline/profile、既存解析キャッシュへの接続、保持量・eviction診断、前後比較とRGBA・状態・往復・GPU・ワーカーの回帰検証。
- 🟠 [Next] 混合操作でworkspaceと参照の共用キャッシュhit率・競合・maximumを計測。実アプリでの入力遅延・cold側の費用とGUI受入れを進める。
- 🔴 [Later] idle wakeup、IPC、より大きい実データと未対応資源の保持コストの計測。
- ⭕️ [Pending] 未提供のユーザー制作プロジェクト、Windows/Linux実機、専用機器での物理表示遅延・電力、Adobe参照出力での検証。
