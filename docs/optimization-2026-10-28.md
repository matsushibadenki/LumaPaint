# 定期計測：キャッシュ並行利用の再確認（2026-10-28）

Problem:
前回は単独の参照再生成で改善を確認したが、workspaceと参照の共用時の競合・eviction・maximumと、既存の並行キャッシュを既定化する妥当性が未確認だった。今回はbaselineと既存の並行診断を実行した。

Root cause:
本番入力遅延の新しい原因は特定していない。現在のキャッシュは競合時に待たず再解析へfallbackするため、高頻度の同時lookupだけを行う合成負荷ではmissが増える可能性がある。一方、実際のラスタライズや入力を含まない診断から、UIの支配的なコストとは判断できない。

Evidence:
Releaseの既存`bench_scene_reference`と`benchmark_shared_parsed_cache_workers`を実行。後者は1/8 source、1/4/8 worker、約1.2KiB/64.2KiBの多言語text、200 operations/worker、round 0を除く7 roundでSerialとParallelを交互比較する。これは局所的なcache/parse診断で、workspaceの実描画やGUI入力を同時操作する試験ではない。

Changed files:
本記録、`docs/performance-2026-10-28/`の証跡、roadmapの追記のみ。アプリ・ライブラリ・benchmark実装を変更していない。

Change:
実装変更なし。既存のMutex既定値、opt-inの`LUMAPAINT_PARALLEL_TEXT_CACHE=1`、32件／32MiB推定枠を維持した。

Why it should improve performance:
今回は改善を裏付ける実測条件が揃わないため、高速化を主張しない。根拠の弱い既定値変更やcache拡張を残さない方針を適用した。

## Baselineと測定条件

開始／終了HEADは`e741d6f8ab2bafe7ce7e6c1fb85dd5dec071cc1d`。開始時からコア、PSD/PSB・タイル形式、Tauri、UI、roadmap等に17 tracked filesの未コミット変更とPSD/PSB fixture追加が存在した。これらを変更・reset・stash・commitしていない。計測対象のcrate内Rustソース、examples、Cargo.toml/lockのハッシュを開始／終了で比較し、追加・変更なしを確認した。したがって計測対象はHEAD単体ではなく、記録したdirty worktreeである。

環境は`metadata.json`、全対象ソースのhashは`initial-source.json`と`source-stability.json`、バイナリ・素材・benchmarkのSHA-256はmetadataに保存。Release、renderer default features、thin LTO／codegen-units=1。参照のCPU resvg経路を対象とする。

初回preflightはテスト用ビルドと重なったため、時間データを除外した。その後は本automationのビルド／テストと時間測定を順番に実行した。ただし同じマシンで別のnative CPU負荷約100%とbrowser renderer約70%を一時点で確認した。負荷を停止・suspend・優先度変更していない。このsnapshotだけで以下の外れ値の原因を断定しない。

以下は6条件×5 trial×100 warm samplesの**観測値**。他のシステム負荷の統制ができておらず、実装変更の採否や10/21からの劣化率の根拠には採用しない。p95/p99はnearest rank、medianは中央2点の平均。coldは別列としてCSVに保存し、warm表へ混ぜない。

| 条件 | warm数 | median ms | p95 ms | p99 ms | maximum ms |
|---|---:|---:|---:|---:|---:|
| 32図形 | 500 | 1.125 | 4.377 | 8.502 | 13.241 |
| 600図形 | 500 | 4.829 | 12.819 | 60.640 | 152.082 |
| 4,096図形 | 500 | 23.479 | 55.307 | 97.532 | 159.344 |
| 8,192図形・保持対象外 | 500 | 111.733 | 242.160 | 621.736 | 989.193 |
| Illustrator由来SVG | 500 | 7.642 | 15.335 | 22.273 | 60.787 |
| 3言語＋mask/blur | 500 | 1.928 | 2.149 | 2.451 | 2.856 |

同じ保存済みReleaseバイナリで、artwork/stressを順序反転しながら5 trial×30 warm samplesで再測定した。これら2条件では前の大きな値は再現しなかった。異なる測定区間の差であり、高速化率や回帰解消率とは解釈しない。

| 同一バイナリの再測定 | warm数 | median ms | p95 ms | p99 ms | maximum ms |
|---|---:|---:|---:|---:|---:|
| Illustrator由来SVG | 150 | 1.948 | 2.122 | 2.268 | 2.378 |
| 8,192図形・保持対象外 | 150 | 20.895 | 21.735 | 24.172 | 26.069 |

Before:
現行のSerial（Mutex）既定値を測定した。上記の時間分布には大きな揺れがあり、本番のp95/p99や最大遅延を保証できない。

After:
新しい実装は作成していない。既に実装済みのParallel（RwLock）との局所診断では、7 roundのbatch wall time中央値に、1.2KiB・1 source・4 workersでSerial 3.046ms／Parallel 0.302ms、1.2KiB・8 sources・4 workersでSerial 5.639ms／Parallel 13.054msという逆方向の結果も見られた。全12条件のraw値・hit/miss/競合・dispatch/worker時間は`cache-workers.log/json`に保存した。各cell 7点のtailは安定したp95/p99推定には少なく、診断metricsのオーバーヘッドと背景負荷も含む。これらだけで既定値を切り替えない。

## Cacheと正しさ

時間測定とは別にmetricsを有効にした5 trial×初回1＋warm 5では、保持対象5条件のwarm計125 callsはhit 125、miss 0、再解析0。保持対象外stressは25 warm callsすべてbypass／再解析。25 admissions、累計admitted estimate 62,674,660 bytes、13 evictions、insertion-eventの保持推定最大33,519,464 bytesを記録し、既存32MiB上限内だった。明示的なinvalidationは追加していない。これは常駐RSSの測定ではなく、共有font DBや退避後も利用中のArcは枠外。単独条件での100% hitを混合GUIへ一般化しない。

6条件×5 trialの30出力で、10/21に保存したRGBAのSHA-256と全て一致。各試行内のcold/warmも同一bytesで、DocumentStateのJSONは前後一致した。従って今回のfixture群では画素差を検出していない。ユーザー制作プロジェクト、追加中のPSD/PSB機能、全言語・全描画効果の品質を包括的に保証する検証ではない。

CPU impact:
アプリ実装変更なし。warmのホストwall time、batch wall/worker/dispatch、hit/miss/競合等を記録した。診断ログの`cpu_ns`はinclusiveなホスト区間であり、並行区間を加算してプロセスCPU時間とは扱わない。CPU time、allocation/copy総量、idle wakeupは今回未測定。

GPU impact:
GPU処理を変更していない。今回はCPU参照生成が対象で、実GPU試験・GPU frame time・GPU完了は未測定。

Memory impact:
cache枠・保持方針を変更していない。現在のcache保持推定は上記。RSS・VRAMは今回未測定で、前回のRSS増加がなくなったとは主張しない。

I/O impact:
形式・IPC・通信は変更なし。PNG等の保存・hash検証とJSON整合チェックは計測区間の外。I/O速度は未測定。

Energy impact:
未測定。電力削減率を推測しない。

Correctness verification:
- 現在のdirty sourceでRelease exampleをビルド成功。6条件・30出力の既存RGBA hash一致と試行内のbytes一致、文書状態不変。
- 既存の`benchmark_shared_parsed_cache_workers --ignored`をRelease／metrics有効で明示実行し1 test成功。全workerでhit+miss=処理数の整合を確認。
- `reference_tree_reuse_preserves_pixels_sizes_and_source_changes`をRelease／metrics有効・single threadで明示実行し1 test成功。fresh rasterとの画素一致、source変更・復帰、異なるサイズでの再利用、invalid dimensionsの拒否を確認。
- core/formats/rendererのarchitecture boundary check成功。
- GPU、全workspace tests、Clippy、frontend build、実GUI混合操作は今回実行していない。source実装は変更しておらず、他の進行中編集を検証済みとは扱わない。

Regression risk:
新しい製品コードはない。現在の大きな外れ値をコード回帰と断定せず、反対に全条件で回帰なしとも断定しない。時間比較の統制と実GUIのhotspot特定が残る。

Keep / Revert:
現行実装を維持。今回の実装patchはなく、Revert対象はない。参照の画素・再利用のチェックが正常で、並行cacheの有利不利も条件で変わり、統制されたUI測定がないため既定値変更を見送った。性能比較のために他の作業を中断しない。

## 再実行

ビルド完了後、重ならない順序で実行する。初期条件がdirty worktreeである点に注意し、ソースhashが変わった場合は同じbaselineと扱わない。

```sh
cargo build --release --offline -p lumapaint-renderer --example bench_scene_reference
./target/release/examples/bench_scene_reference --samples 100 --runs 5
LUMAPAINT_RENDER_METRICS=1 ./target/release/examples/bench_scene_reference --samples 5 --runs 5
LUMAPAINT_RENDER_METRICS=1 cargo test --release --offline -p lumapaint-renderer benchmark_shared_parsed_cache_workers -- --ignored --nocapture --test-threads=1
LUMAPAINT_RENDER_METRICS=1 cargo test --release --offline -p lumapaint-renderer reference_tree_reuse_preserves_pixels_sizes_and_source_changes -- --test-threads=1
python3 scripts/check-core-boundary.py --offline
```

- 🟢 [Done] 現行Releaseの計測・既存RGBA照合・cache再利用／並行診断・ソース不変の証跡保存。製品実装の変更なし。
- 🟠 [Next] 統制した環境でworkspaceと参照を混合する実GUIの入力遅延・cache hit率／競合・cold／maximumを測定し、局所診断の結果を照合する。
- 🔴 [Later] idle、IPC、allocation/copy、保持対象外資源を実測し、変更の費用対効果を選定。
- ⭕️ [Pending] Windows/Linux実機、専用機器による物理表示遅延・電力、未提供のユーザー制作プロジェクトでの受入れ。
