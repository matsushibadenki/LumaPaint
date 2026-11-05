# AdobeStock_1832685201.ai の複雑ベクター試験

## 入力と試験範囲

ユーザー指定の `/Users/littlebuddha/Desktop/AdobeStock_1832685201.ai` を使用。拡張子はAIだが実体はDOS EPS／PostScriptで、PDF互換AIではない。Tauriの直接読み込みは `Malformed("pdf.header")` で失敗した。

ローカルのIllustrator 2026で開き、SVG 1.1・文字アウトライン・画像埋め込み設定で `/tmp/AdobeStock-1832685201-QA.svg` へ別名保存した。元のAIのSHA-256は試験前後で一致：`727582ba2a6b1cbc47c752c82ec244b38a10d76291718578ea1dec5121f2f886`。元ファイルを上書きしていない。

SVGコピーの構造：

| 項目 | 数 |
| --- | ---: |
| ファイルサイズ | 1,834,970 bytes |
| path | 2,669 |
| rect / polygon | 3 / 9 |
| グループ | 870 |
| 円形／線形グラデーション | 55 / 14 |
| グラデーション停止点 | 165 |
| クリッピングパス | 2 |
| image / text | 0 / 0 |

ラスター画像へ置き換えず、元のSVGソースを保持するレイヤーで検証した。ただし現在のSVG読み込みでは、個別に選択できるネイティブのベクターオブジェクトには変換されない（native objects = 0）。今回の対象は複雑なSVGベクターの表示・パンであり、2,669個の編集可能なオブジェクトの一括編集試験ではない。

元のストックデータ、SVGコピー、画像プレビューはリポジトリに含めない。再実行可能な試験は外部ファイルを引数として読む。

## 同一バイナリでの比較

Apple M4、800×554描画先、Rust testプロファイル（既存設定のrenderer/core opt-level 2）、Skia有効。SVG解析＋表示用RGBA生成のCPU時間を計測。各倍率で再解析あり／再利用ありを交互に実行し、最初の1組を除いた6組の中央値。各試行で表示位置を変更し、小数座標のパンも含む。

| 表示倍率 | 再解析あり | 解析結果を再利用 | 短縮率 |
| --- | ---: | ---: | ---: |
| 全体表示（約70.8%） | 227.273 ms | 74.107 ms | 67.4% |
| 200% | 298.398 ms | 144.155 ms | 51.7% |
| 400% | 399.935 ms | 246.141 ms | 38.5% |

初回解析153.276 ms、キャッシュ計上量29,836,240 bytes。SVG読み込みは370.814 ms、文書サイズ1040×715。読み込み時間にはフォント等の初期化が含まれ、UI操作開始からの時間ではない。

全体表示・200%・400%それぞれ7条件、計21条件で再解析版と保持版の全画素が完全一致。原作はCMYKで、SVGはIllustratorによるRGB変換を経るため、この一致試験は印刷色や元AIとの画素一致を保証するものではない。

## Tauri実機

更新済みTauriアプリ、Metal、Apple M4、800×554 px、backing scale 1。全体表示、200%、400%で花柄・曲線・グラデーション・重なりを目視確認した。200%と400%では、80×50画面ポイントのパンと逆方向への復帰を3往復ずつ行った。

| 倍率 | 計測された描画フレーム | ホスト再描画中央値 | 最大値 |
| --- | ---: | ---: | ---: |
| 200% | 6 | 148.415 ms | 151.54 ms |
| 400% | 6 | 252.315 ms | 257.01 ms |

全12フレームで解析キャッシュヒット各1回、SVG再解析0回。毎フレーム1,772,800 bytesの表示範囲を再ラスタライズしている。SVGラスタライズに約145〜147ms／248〜253msかかり、ホスト再描画時間の大部分を占める。解析再利用は効いているが、複雑なSVGを高倍率でパンする操作はまだ重い。

ここでのホスト時間は`render_canvas`の計測値。短い自動ドラッグでは入力イベントが統合されるため、連続入力時のFPSや物理画面への表示遅延には換算しない。Illustratorの表示速度との定量比較も今回の範囲外。

確認用文書は `/tmp/AdobeStock-Complex-QA.lumapaint` に保存。保存されたSVG全文がIllustrator出力と一致することも検証した。

## 再実行と証跡

```sh
LUMAPAINT_COMPLEX_SVG=/tmp/AdobeStock-1832685201-QA.svg \
  CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo test \
  -p lumapaint-renderer --features skia --lib \
  complex_artwork_redraw_timing -- --ignored --nocapture
```

`LUMAPAINT_COMPLEX_OUTPUT`に既存のディレクトリを指定すると、3倍率の参照PNGも出力する。通常のCIはライセンス付きの外部ファイルを必要としないよう、この試験をignoredにする。

今回追加したのは手動試験と記録のみ。試験成功、renderer全ターゲットの警告禁止Clippy、整形・差分検査成功。

- [比較計測ログ](performance-complex-vector-2026-11-04/benchmark.log)
- [実機フレーム別結果と入力構造](performance-complex-vector-2026-11-04/results.json)
- [実機描画ログ](performance-complex-vector-2026-11-04/native-render.log)
- [Clippy](performance-complex-vector-2026-11-04/clippy.log)

## 次の優先課題

- 🟢 [Done] 指定AIから変換した複雑SVGの比較試験・全画素一致・Tauriの拡大と往復パンを確認。
- 🟠 [Next] 取り込んだ複雑SVGのGPU描画対応、可視領域に必要な処理への限定。解析よりも高倍率のラスタライズを優先する。
- 🔴 [Later] EPS形式AIの直接読み込み、個別編集できるベクターへの取り込み、EPS不対応時のエラー表示改善。
- ⭕️ [Pending] Windows／Linux実機、他社アプリとの同条件の物理表示遅延比較。
