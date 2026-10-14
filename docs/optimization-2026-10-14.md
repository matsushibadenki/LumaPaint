# 定期最適化：平行移動行列の固定成分の変換を省略（2026-10-14）

Problem:
文書復元では、ベクターソースのcanonical判定のためにSVG全体を生成する。前回の基準測定に続き、この処理とUI snapshotを再調査した。

Root cause:
`from_document_state`内の`vector_svg`が全オブジェクトの行列を文字列化する。一般的な平行移動行列でも、固定成分1・0・0・1の浮動小数点フォーマットを毎回実行していた。

Evidence:
初回の200サンプル分解では4,096件の復元medianが3,644.375µs、単独SVG生成が3,258.000µs、オブジェクト検証が115.000µs、JSON解析が6,191.834µs。独立した測定なので、これらを厳密な加算可能な内訳とは扱わない。1秒のCPUスタック採取でも`vector_svg`配下の`alloc::fmt::format_inner`と浮動小数点フォーマットが確認できた。snapshotは約1,187µsであり、今回は変更していない。

Changed files:
`crates/lumapaint-core/src/document.rs`、本報告、`docs/roadmap.md`、`docs/performance-2026-10-14/`の測定ソース・CSV・ログ。

Change:
`slot_matrix`で線形成分が正確に1・正の0・正の0・1である場合だけ固定文字列を使用し、平行移動2成分の変換を行う。負の0、回転、拡大縮小、せん断は従来のフォーマットを使用する。小数精度・パディング・SVG長さは変えない。キャッシュ、保持状態、スレッド、依存を追加しない。

Why it should improve performance:
対象行列ごとに不要な4成分の数値変換を省く。同じSVG bytesをより少ない仕事で生成する。

## Before / After

macOS 27.0.1（26A434）、ARM64、rustc 1.99.0（b940084d7、Homebrew）。開始時HEADは`6611b0e8dccafb2692c2266931765ff69b36fe18`、作業ツリーはclean。Beforeは同HEAD、Afterは本変更を適用した作業ツリー。ReleaseのLTO thin／codegen-units=1に対応した最適化条件。

既存の`bench_translation`を`--seconds 1 --runs 3`で実行し、次にそのfixtureとreport関数から分解測定用の`profile.rs`を作成した。データは同じ合成矩形100/1,000/4,096/10,000件、選択1件。10,000件は今回のstress条件であり、実文書やアプリの容量保証ではない。JSON解析と復元を分け、復元用の入力cloneは計測区間の外、結果のdropも計測区間の外に置いた。

初回Afterの既存benchmarkはテスト・コンパイルと重なったため効果判定から除外した。証跡は`after-build-overlap.*`に残す。ビルド終了後、変更前後の分解測定実行ファイルを交互に実行し、偶数回はAfterを先行させた。全サイズ3回×各200サンプル、1,000/4,096件では追加で5回×各200サンプルの確認を行った。追加確認はビルド・テストと重ねていない。

以下は追加5回の**各回のpercentileの中央値**であり、全サンプルを結合したpercentileではない。rawな各回の統計は`confirmation.csv`を参照。

| 件数 | 処理・指標 | Before µs | After µs | 差分 µs | 変化 |
|---:|---|---:|---:|---:|---:|
| 1000 | svg median | 807.333 | 771.833 | -35.500 | -4.40% |
| 1000 | svg p95 | 829.708 | 797.125 | -32.583 | -3.93% |
| 1000 | svg p99 | 899.583 | 817.208 | -82.375 | -9.16% |
| 1000 | restore median | 1031.375 | 944.583 | -86.792 | -8.42% |
| 1000 | restore p95 | 1055.041 | 964.666 | -90.375 | -8.57% |
| 1000 | restore p99 | 1086.459 | 985.708 | -100.751 | -9.27% |
| 4096 | svg median | 3287.334 | 2992.458 | -294.876 | -8.97% |
| 4096 | svg p95 | 3386.000 | 3109.792 | -276.208 | -8.16% |
| 4096 | svg p99 | 3721.250 | 3352.000 | -369.250 | -9.92% |
| 4096 | restore median | 3966.042 | 3599.084 | -366.958 | -9.25% |
| 4096 | restore p95 | 4188.584 | 3721.458 | -467.126 | -11.15% |
| 4096 | restore p99 | 4524.042 | 4080.708 | -443.334 | -9.80% |

全サイズの3回測定ではSVG生成のmedianが約7〜10%、復元が約7〜9%短縮。1,000件復元の最初のp95/p99は悪化したが追加5回では改善した。4,096件SVGの追加確認の最悪maximumはBefore 6,903.083µs、After 15,225.583µs。1,000件復元でもBefore 1,155.750µs、After 1,408.042µsの最悪値を記録した。maximumは改善と主張しない。OSや他アプリの負荷を隔離しておらず、長い尾の原因は未特定。CSVにはmaximumと8.33/16.67ms超過率も保持する。前回10/09の数値との差を、この変更の効果とは扱わない。

CPU impact:
上記はホスト処理のwall timeとCPUスタック採取。CPU time、lock wait、context switchを定量化していない。固定成分の数値変換を4回省略する。アプリの入力遅延・FPSの改善を測ったものではない。

GPU impact:
描画命令、GPU資源、upload量の変更なし。GPU frame timeは未測定。実GPU比較テストは正しさの検証であり、GPU性能測定ではない。

Memory impact:
新規の保持状態はない。SVG出力bytesは完全一致。allocation回数・ピークメモリは未測定であり、削減率を主張しない。

I/O impact:
保存形式と内容は変更なし。ディスク・network・IPCを含む性能は未測定。

Energy impact:
未測定。処理量を省いているが、消費電力削減率は推測しない。

Correctness verification:
- コア359件成功、2件の既存オプトインbenchmarkはignored。追加試験は特殊値と擬似乱数の有限f32、回転・せん断・拡大縮小・負の0について従来の行列文字列との完全一致を確認。既存の平行移動f32 round-tripと固定長・履歴・Undo/Redoも通過。
- 形式互換116件成功。既存benchmarkのUndo/Redoと保存再読込後のSVG一致assertionも成功。
- rendererの`transform_journal_refits_native_bounds_and_undo_restores_pixels`成功。これはSVG rasterのピクセル比較とnative bounds検証であり、実GPU試験とは区別する。
- rendererの`multilingual_text_translation_tracks_only_changed_transform_and_undo_redo`成功。
- 実GPUの`gpu_retained_text_frames_match_full_layer_at_multiple_zooms --ignored`を明示実行して成功。opacity・overlap・tiled条件を含む参照合成と比較。
- コアall-target Clippy（warnings拒否）、Rust fmt、core/formats/renderer境界チェック、寸法・ルーラーとLab D50色変換回帰テスト、diff check成功。
- UIコード変更なし。frontend buildとアプリ全体の実操作は今回未実施。既存テスト通過を完全なAdobe互換・全言語の全描画保証とは扱わない。

Regression risk:
固定成分の適用判定、負の0の表記保持を追加試験でカバーした。汎用行列は従来経路を維持するが、汎用行列中心の実文書性能は今回未測定。最大遅延の原因切り分け、実文書、別OS/GPU、idle負荷は残る。

Keep / Revert:
Keep。全サイズで中央値の改善が再現し、追加確認では対象のp95/p99も改善し、保存文字列と描画ピクセルの精度を維持した。変更は単一の小さな分岐に留めた。最大遅延の保証やアプリ全体の速度向上は主張せず、次回は制御条件で尾と汎用行列を確認し、変更由来の回帰が再現すればこの変更だけを戻す。

## 再実行

`profile.rs`は今回計測したstandaloneソースの保存版。対象revisionで既存Release exampleをビルドし、そのcore/serde_jsonのrlibにリンクして使う。同じコンパイラと依存条件でBefore/Afterを作り、ビルド完了後に交互実行する。

```sh
cargo run --release --offline -p lumapaint-core --example bench_translation -- --seconds 1 --runs 3
```

standaloneのリンクコマンドは次の形（rlibのhashは対象ビルドに合わせる）。システムリンカーにLLVM bitcodeを渡さないため`lto=thin`が必要。

```sh
rustc --edition=2021 -C opt-level=3 -C lto=thin -C codegen-units=1   docs/performance-2026-10-14/profile.rs   --extern lumapaint_core=target/release/deps/liblumapaint_core-HASH.rlib   --extern serde_json=target/release/deps/libserde_json-HASH.rlib   -L dependency=target/release/deps -o /private/tmp/lumapaint-profile
/private/tmp/lumapaint-profile 4096 restore
/private/tmp/lumapaint-profile 4096 svg
/private/tmp/lumapaint-profile 4096 decode
/private/tmp/lumapaint-profile 4096 snapshot
/private/tmp/lumapaint-profile 4096 validate
```

サンプル数は`LUMA_SAMPLES`環境変数（省略時200）。sampleによるstack取得は`sample <PID> 1 -file <path>`を使用。

- 🟢 [Done] Release基準測定、分解profile、固定成分の変換省略、前後比較、正しさ検証と証跡保存。
- 🟠 [Next] 汎用行列・実文書での効果とmaximumの原因を確認。アプリ実操作とGPU frame/host input latencyを計測し、JSON解析とsnapshot生成のallocationをprofileして次の対象を選ぶ。
- 🔴 [Later] さらに大きい実データ、IPC、cache hit率、idle wakeupの継続計測。
- ⭕️ [Pending] 開発環境にないWindows/Linux実機、Adobe参照出力、専用機器による物理入力から表示までの遅延・電力検証。microbenchmarkから推定しない。
