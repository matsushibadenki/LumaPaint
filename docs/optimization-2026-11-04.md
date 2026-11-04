# 定期最適化：参照ラスタ生成のUI要約を省略（2026-11-04）

Problem:
`thumbnails::render_sized_ink`は画像合成に寸法・canvas色・描画対象レイヤーだけを必要とするが、`Document::snapshot()`でUI用の全object/text/page/asset要約を毎回作っていた。

Root cause:
画像生成とUIモデル取得が結合し、native vectorの全件走査、文字・paint/style情報のclone、contour解析、asset一覧用BTreeMap構築等を画像生成のたびに実行していた。最後に必要なのはsnapshotのwidth/height/canvas_colorとlayer ID membershipだけだった。

Evidence:
変更前のmetrics有効な32/600/4,096/10,000図形のfixtureで、参照1回につき同数の`document_snapshot_default_stroke_styles_elided`を記録した。これらのfixtureは全objectがdefault styleなのでUI object要約の作成件数に対応する。1秒のCPU sampleでも`Document::snapshot`配下の`LayerObjectSnapshot`構築、`contour_closed/contours`を確認した。full snapshotではasset要約用の全object mapも構築する。数値時間測定はmetrics無効で別実行した。

Changed files:
- `crates/lumapaint-core/src/document.rs` — 読み取り専用`artwork_svg_layers()`とmembership回帰。
- `crates/lumapaint-renderer/src/thumbnails.rs` — ラスタ合成のfull UI snapshotを直接getter／iteratorへ置換し、再要約しない不変条件・多言語・編集guideの画素と状態の回帰を追加。
- `crates/lumapaint-renderer/examples/bench_native_reference.rs` — 既存translation fixtureから作った再実行用native-vector計測。
- 本報告、測定証跡、roadmap追記。

Change:
寸法とcanvas色は既存の軽量getterを使用。新しいiteratorはhidden layerを保持し、active saved-path editing layerを除外する。従来のsnapshotのレイヤーmembershipと同じ条件で、UIの全object要約を作らず参照できる。通常のUI snapshotと既存の`svg_layers()`は変更しない。panelのmask一覧が行う別のsnapshotは今回の対象外。

Why it should improve performance:
画像の結果に使わないUIデータを生成しない。membership取得部分の全object走査／asset map／全layer ID探索を、対象layerの読み取りへ縮小する。ラスタライズそのもののO(N)処理は残り、画像生成全体がO(layer数)になったとは主張しない。新しい保持state、cache、worker、依存、unsafe、GPU転送は追加していない。

## Before / After

開始HEADは`d693ada8cfdf8c0e339344f9487f96204f879e09`。開始時にobject conversionのcore/renderer/UI編集と`clipped_qa.rs`が存在した。それらをreset・stash・commitしていない。実行中にもconversion／QA／roadmapの編集が進み、該当差分は維持した。このためcommit単体の凍結比較ではない。今回のreference fixtureはconversion経路を呼ばないが、厳密な本番比較への一般化は避ける。source hash、バイナリSHA-256、環境と初期statusはmetadata等を参照。

環境: rustc 1.99.1 (58cf8f9c3 2026-10-13) (Homebrew)、ARM64、Release（thin LTO、codegen-units=1）、renderer default features。GPUを使うアプリ全体ではなくCPU参照生成のmicrobenchmarkである。

native fixtureは32/600/4,096/10,000図形、選択1個。既存`bench_translation`と同じprototype・配置で、一部の図形は文書外にあり、可視描画面積が全件数と比例するデータではない。10,000件はstress fixtureであり、アプリの読込／保存容量保証ではない。参照解像度は256×171 RGBA8。初期化・JSON整合確認・stdout・画素出力はタイマー外。

Before/Afterは別保存した実行ファイルを交互に5 trial、各20 warm samples。奇数trialはAfterを先行。以下は各variant/条件100点のpooled分布、p95/p99はnearest rank、medianは中央2点の平均。初回をwarmに混ぜない。各trial中央値もJSONに保存した。自分のReleaseビルドと対象回帰テストは終了してから計測した。他のシステム負荷は隔離しておらず、受入れSLOの値ではない。

| 図形数 | median B→A ms | p95 B→A ms | p99 B→A ms | maximum B→A ms |
|---:|---:|---:|---:|---:|
| 32 | 0.728 → 0.830 | 1.616 → 1.198 | 2.244 → 2.791 | 3.115 → 3.024 |
| 600 | 2.369 → 2.258 | 3.950 → 3.329 | 4.620 → 4.648 | 5.098 → 11.688 |
| 4096 | 37.170 → 34.259 | 60.436 → 40.451 | 75.565 → 43.657 | 203.079 → 44.382 |
| 10000 | 98.253 → 82.732 | 128.533 → 111.380 | 141.546 → 127.453 | 406.350 → 169.035 |

32図形の最初のmedianは悪化し、600図形の結果にも揺れがあるため、core/renderer Clippyの終了を確認した後に追加5 trial×200 warm samples（各variant/条件1,000点）を取得した。最初にClippyと重なった追加試行は`small-confirmation-build-overlap.csv`へ分離し、効果判定に使わない。追加の有効試行は32/600の完全なcohortを取得した時点で所有する計測processだけを終了し、後続stressを省いた。正しさの検証は別の完全実行で行った。

| 図形数 | median B→A ms | p95 B→A ms | p99 B→A ms | maximum B→A ms |
|---:|---:|---:|---:|---:|
| 32 | 1.207 → 1.030 | 7.829 → 6.665 | 32.440 → 21.916 | 71.486 → 78.341 |
| 600 | 5.325 → 5.363 | 32.050 → 44.808 | 85.471 → 123.175 | 193.249 → 262.878 |

小条件の有利不利が測定区間で逆転し、600図形の追加試行ではmedian差が約0.7%、tailも悪化した。今回の環境では時間のばらつきが大きく、これらを確立した高速化率またはコード由来の遅延回帰とは断定しない。最大値の上限、実GUI input latency、FPSの改善を主張しない。次の採用根拠は処理件数・CPU仕事量・画素同値性である。

### 処理量と資源の比較

metrics有効の別実行で、native fixtureのUI要約件数は各参照につき32/600/4,096/10,000→全て0。SVG解析／cacheの条件は変更していない。UI snapshot内部のdefault-style metricは0でもkeyが存在するため、追加回帰はkeyの不在も確認する。新しいcacheのhit率やmemoryを最適化効果として報告しない。

別の`time -l`比較は全4 native条件、初回1＋warm 5、Before/After各3 processを交互実行した。以下はprocess全体の中央値で、fixture作成・JSON確認・出力等も含み、単一frameのCPU時間ではない。

| 項目 | Before | After | 差 |
|---|---:|---:|---:|
| retired instructions | 9,433,426,139 | 8,638,728,611 | −8.42% |
| user＋system CPU time | 1.31s | 1.07s | −18.32% |
| maximum RSS | 156.39MiB | 151.22MiB | −5.17MiB（約−3.31%） |

全3回のAfter命令数は全3回のBeforeより少なく、単なるwall-clockの短縮だけで採用判断していない。RSSは短い計測processのpeakであり、アプリ常駐量やVRAMの削減率ではない。allocation総数・copy総bytesは未計測で、UI要約の削減件数をmalloc回数と同一視しない。

CPU impact:
不要なUI要約とその派生処理を省略し、上記の仕事量減少を確認。アプリ全体のmain CPU・lock wait・idle wakeupは未測定。

GPU impact:
描画backend、shader、GPU resource、uploadを変更していない。今回のGPU frame time・GPU実機回帰は未実行。

Memory impact:
新規の保持領域なし。UI要約の一時構築を省略。上記RSSと常駐cacheの予算は別物で、既存cache上限は変更しない。

I/O impact:
形式・IPC・通信変更なし。大容量frameはRust側に保持する。実I/O速度やfrontend IPC量は未測定。

Energy impact:
未測定。命令数削減から電力・jouleの削減率を推測しない。

Correctness verification:
- 変更前後の**10条件**（native 4、既存scene 6）の全RGBA buffersが完全一致。scene側はsmall/medium/large/stress、Illustrator由来SVG、英語・日本語・简体中文＋mask/blur。raw比較のSHA-256・byte数を保存。
- 完全実行の各試行でwarm画素と初回画素が一致し、DocumentStateのJSONも前後一致。
- 新規core membership test 1件成功。hidden layerを保持し、active saved-path editing layerを除外し、UI snapshotと同じmembershipであることを確認。
- renderer thumbnails 11件成功（metrics有効、single-thread）。新規試験はUI summaryを作らないこと、多言語の状態不変、画面全体を覆う編集中guideが混入しないことを画素で確認。既存の紙・mask・hidden・stroke等の試験も通過。
- SVG/PDF往復15件成功。
- core lib、renderer lib＋新exampleのClippy（warnings拒否）、変更Rust filesのfmt、core/formats/renderer境界、対象diffの空白チェックが成功。
- 他作業のtarget lock待ちを解消するため、自分の待機cargoだけを止め、独立した一時targetで回帰・Clippyを実行した。他作業のprocessは停止していない。
- frontend／全workspace／skia有効の全suite／GPU／実GUI受入れは今回未実行。他の進行中conversion編集を合格認定しない。10条件の同値性をAdobe全機能互換の保証とは扱わない。

Regression risk:
active editing guideの除外とhidden layerを含むmembershipを誤るとpreview/exportが変わるため、getterの独立した期待値と実画素で回帰を確認した。時間の分布には逆方向の変化もあり、操作感や全ケースのtail改善は未確定。実GUI、異なるOS/GPU、幅広い制作データが残る。

Keep / Revert:
Keep。参照に不要なUI object処理を0件へ減らし、CPU命令数が全3比較で減り、画素・状態・membership・往復の回帰が成功した。大きなwall-clockのばらつきから速度保証を作らず、CPU仕事量の削減を根拠とする。cacheやstateを増やす代わりに、軽量なmodel所有のiteratorで同じmembershipを得る小さな変更として採用。実GUIで変更由来の回帰が再現すれば、この変更だけを戻す。

## 再実行

```sh
cargo build --release --offline -p lumapaint-renderer --example bench_native_reference --example bench_scene_reference
./target/release/examples/bench_native_reference --samples 20 --runs 5
LUMAPAINT_RENDER_METRICS=1 ./target/release/examples/bench_native_reference --samples 3 --runs 1
cargo test --offline -p lumapaint-core artwork_layers_match_ui_membership_without_path_guides
LUMAPAINT_RENDER_METRICS=1 cargo test --offline -p lumapaint-renderer thumbnails::tests -- --test-threads=1
cargo test --offline -p lumapaint-renderer --test vector_io_roundtrip
cargo clippy --offline -p lumapaint-renderer --lib --example bench_native_reference -- -D warnings
cargo clippy --offline -p lumapaint-core --lib -- -D warnings
```

Beforeは計測exampleのみ追加し実装変更前に同条件でbuildして別保存。Afterとの交互実行・比較条件はmetadataとCSVに記録した。`LUMAPAINT_BENCH_PIXELS`に既存ディレクトリを指定すると各native条件のRGBAを保存できる。実装patchは`source.patch`、exampleのhashはmetadataを参照。競合を避ける場合は固有の`CARGO_TARGET_DIR`を使い、別作業のlockやprocessを変更しない。

- 🟢 [Done] 参照生成の不要UI要約の省略、model所有のmembership iterator、Release前後比較、CPU命令数・状態・画素・guide/hidden・往復の検証。
- 🟠 [Next] 負荷を統制した実GUIで小条件を含むinput latency・p95/p99を確認し、参照／workspace混合時の効果を測る。
- 🔴 [Later] panel mask一覧に残るsnapshot、他のallocation/copy、idle/IPCをprofileして次の費用対効果を選ぶ。
- ⭕️ [Pending] Windows/Linux実機、専用機器による物理表示遅延・電力など開発環境外の検証。
