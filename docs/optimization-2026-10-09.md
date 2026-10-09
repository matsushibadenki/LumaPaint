# 定期最適化の初回基準測定（2026-10-09）

`ソフトウェア最適化指示書.md`に従い、毎週水曜10:00（Asia/Tokyo）の計測・改善を登録した。既存の依存更新とは別の処理。既存のrendererとroadmapの未コミット変更を保持した。

## Baseline

実行: `cargo run --release --offline -p lumapaint-core --example bench_translation -- --seconds 1 --runs 3`。macOS開発環境、Release、合成矩形1,000/4,096件、選択1件。正常終了。単位はµs。移動は各回1秒、snapshotは20サンプル、保存・読込は10サンプル。保存はJSON生成、読込はJSON解析とDocument復元であり、ディスクI/Oを含まない。

| Objects | Operation | median | p95 | p99 | maximum |
|---:|---|---:|---:|---:|---:|
| 1,000 | move run 1 | 2.416 | 3.500 | 5.791 | 10,894.208 |
| 1,000 | move run 2 | 3.375 | 3.542 | 5.417 | 14,343.667 |
| 1,000 | move run 3 | 3.375 | 3.542 | 6.416 | 14,512.500 |
| 4,096 | move run 1 | 3.541 | 3.792 | 8.083 | 14,689.041 |
| 4,096 | move run 2 | 3.542 | 3.792 | 7.667 | 20,380.208 |
| 4,096 | move run 3 | 3.542 | 3.791 | 6.708 | 14,511.334 |
| 1,000 | UI snapshot | 487.417 | 786.000 | 786.000 | 786.000 |
| 4,096 | UI snapshot | 4,464.333 | 11,359.167 | 11,359.167 | 11,359.167 |
| 1,000 | state save | 2,819.458 | 10,472.459 | 10,472.459 | 10,472.459 |
| 4,096 | state save | 14,445.750 | 29,574.250 | 29,574.250 | 29,574.250 |
| 1,000 | state load | 11,318.833 | 16,216.750 | 16,216.750 | 16,216.750 |
| 4,096 | state load | 87,271.625 | 124,307.291 | 124,307.291 | 124,307.291 |

移動の診断値は全回でedited_objects=1、scanned_objects=0、full_layer_snapshots=0、svg_generations=0、history_transform_bytes=80、svg_patch_bytes=45。Undo/Redo後と保存再読込後のSVG一致assertionが成功した。

## 判断と次回作業

Problem: snapshotとDocument復元に件数増加時のコストが見られる。

Root cause: 未確定。wall timeだけではallocation・serialization・復元処理の寄与やOSスケジューリングの影響を分離できない。

Evidence: 上記基準値。移動のp99は低いが最大値に長い尾がある。20/10サンプルのp95/p99は最大値と一致するため、尾の安定した推定ではない。

Changed files: この計測記録のみ。アプリ実装変更なし。

Change / Why / Before / After: 初回baseline。改善率は未算出。

CPU impact: wall timeのみ測定。CPU timeとlock waitは未測定。

GPU / Memory / I/O / Energy impact: 未測定。診断の履歴・パッチbytesは全メモリ確保量ではない。

Correctness verification: benchmark内のSVG一致、architecture check、`node scripts/test-measurements.mjs`、`node scripts/test-color-modes.mjs`が成功。色検証はLab D50往復・参照色・gray輝度・gamut clippingを対象とし、アプリ全体の描画精度を保証しない。

Regression risk: 実装未変更。計測は短時間で、背景負荷の統制なし。アプリFPS・入力遅延・実ファイル互換性への一般化は禁止。

Keep / Revert: 基準値を保持。原因を特定する前の最適化は行わない。

- [Done] 定期実行設定、初回Release baselineと上記正しさチェック。
- [Next] snapshotとstate loadをprofileし、反復数・実データを増やして原因を特定、最小修正と前後比較。
- [Later] large/stress、allocation、cache、IPC、idle負荷を段階的に追加。
- [Pending] 今回未実施の実機UI入力遅延・GPU frame time・電力・Adobe実出力比較。他OS/GPUの検証。
