# Document Modelと外部エンジンの境界

`lumapaint-core`はLumaPaint独自のDocument、VectorObject、線・ベジェ形状、処理グラフ、タイル、検証、履歴、独自DocumentStateを所有する。Skia、usvg、resvg、tiny-skia、wgpu、フォントデータベース、Tauri、lumapaint-formatsには直接・間接とも依存しない。`scripts/check-core-boundary.py`で通常依存の全経路を検査し、CIで再導入を防ぐ。

```text
                         ┌→ Skia / resvg Renderer → wgpu → 画面
LP Document / State ──────┼→ SVG I/O（限定出力）
                         ├→ PDF I/O（未実装）
                         ├→ AI I/O（未実装）
                         └→ Native I/O（v1 JSON / タイルコンテナ）
```

上図はデータの流れ。crateの通常依存は逆向きで、`lumapaint-renderer → lumapaint-core`、`lumapaint-formats → lumapaint-core`、`lumapaint-svg → lumapaint-core`となる。Rustホストが各サービスを接続する。coreからI/O・rendererへ、rendererからI/Oへ、I/Oからrenderer・Skia・wgpu・UIへ向かう依存はない。CIで各境界の直接・間接依存を検査する。描画は現行のSkia＋複雑SVG向けresvgを維持する。

## 独立I/Oの契約

`Document::document_state`はファイルの署名や版番号を持たない独自状態を返し、`Document::from_document_state`はモデルの整合性を検証して復元する。`DocumentState`は現在のv1モデルに対応し、タイル文書の`TiledRasterState`とはまだ別型。既存JSONとの互換性のため、状態の省略値やSVGソース保持は維持する。

`lumapaint-formats::native`がv1の署名・版・8MiB上限・JSON符号化を所有する。`lumapaint-formats::tile_container`がタイル形式のマニフェスト・CRC・バイナリ境界を所有する。通常保存、再読込、自動復旧はこのI/O層を使用する。`NativeDocumentCodec`をimportすると従来の`document.encode()`／`Document::decode()`呼び出し構文が使えるが、実装はcoreに存在しない。coreの単体試験には旧fixture用の試験専用codecだけを残す。

`ExportSnapshot::capture(&Document)`はクローン上で未確定ブラシを完了し、文書のrevision・履歴・選択・保存済み状態を変更せず独自状態を取得する。`DocumentExporter`はこの読み取り専用状態、`ExportOptions`を受け、bytes・media type・`ConversionReport`を返す。I/Oは描画サーフェス、Skiaオブジェクト、GPUテクスチャ、WebViewを受け取らない。書き出し成功はネイティブ保存済み状態を更新しない。ファイル選択・原子的置換・文書採用はホストの責務。

`FormatId`と`CAPABILITIES`がNative／SVG／PDF／Illustrator／PSDの実装状況を公開する。PDFとAIは共通ディスパッチで明示的にUnsupportedを返し、拡張子だけ変えたファイルを作らない。実際のPDF／AI parser・writerを実装するときも同じ境界へ接続する。PDF互換AIとAI固有データの能力は分けて検証する。

SVGの初期I/Oは元ソース保持の読込と、各表示レイヤーを分離したSVG imageへ埋め込む限定出力。CSSや同名IDが別レイヤーへ干渉しない。寸法、可視性、順序、不透明度、簡易マスク係数、白／透明背景を扱う。元オブジェクトの直接編集性は維持しないためTier Cレポートと`allow_lossy`が必要。ブラシと文書クリップは明示的に拒否する。GUIのSVG書き出しメニューにはまだ接続していない。文字・ICC・SVGオブジェクトとしての往復互換を保証するものではない。

## SVG編集の契約

coreの`SvgGeometryBackend`が、元SVG・レイヤーID・文書寸法から独自の`VectorObject`を取得し、`SvgEdit`の配列を元SVGへ反映する境界を定義する。境界を越える値は文字列、寸法、ID、LumaPaintの独自型だけ。usvgのTree・Path・Group、tiny-skiaのPath、XMLソース位置、フォント、解析キャッシュは`lumapaint-svg`内部に閉じ込める。

Documentは元XMLを保持し、SVGアダプターの描画ツリーを正本にしない。アダプターは入力を変更せず、変更後の完全なSVGを返す。Documentが検証、変更の一括確定、Undo/Redo、保存を担当する。インスタンスの独立化やCSSの解決はアダプターの責務。

`Document::set_svg_geometry_backend`で各Documentへ実行時サービスを接続する。グローバル登録は使わず、別Documentでは異なる実装を使える。DocumentのクローンはサービスへのArcを共有するため、プレビューとUndoでも同じ実装を使う。サービスは文書・ウインドウ・選択の状態を所有しない。

保存JSONにはサービスを含めない。`Document::default`とネイティブI/Oでのdecodeはアダプターなしで動作し、SVGの保持・保存を行える。読み込みSVG内部のダイレクト編集を使うホストは、読み込み後に`lumapaint_svg::attach(&mut document)`を実行する。macOSホストは初期文書と文書のアクティブ化時に接続する。接続がない場合、読み込みSVGの編集対象は空になり、内部編集のプレビューはエラーを返す。独自ベクターの編集・保存にはアダプターは不要。

## 独立性の範囲

VectorPathは現在もSVG互換のd文字列とFillRuleを持ち、coreはXML検証にroxmltree、パス構文・幾何処理にsvgtypesを使う。この外部形式・構文への依存は残るが、描画エンジンの型や動作への依存ではない。Skiaの交換でDocumentや保存形式を変更する必要はない。

既存の`VectorPathEngine`もcoreで定義され、rendererの`SkiaPathEngine`がパス演算を実装する。SVG編集とファイルI/Oも同じ依存方向へ揃えた。SVGと描画側のusvgバージョンは互換のため揃えているが、その選択をDocument Modelへ持ち込まない。

## 検証

- coreだけで、異なる実装の接続、文書ごとの独立性、クローン、保存形式の不変、アダプター未接続の再読込、失敗時の文書・履歴不変を検証する。
- SVGアダプター側で、形状・CSS・use、元XMLの保持、識別子、保存・再読込、複数節点編集、Undo/Redoを検証する。
- I/O側で、旧v1の省略値、署名・版・未知／重複フィールド、不正状態、上限、タイルの往復・破損を検証する。
- renderer側で、SVG編集前後と限定SVG出力の描画ピクセルを比較する。縦横比が異なる出力も検証する。

```sh
python3 scripts/check-core-boundary.py --offline
cargo test -p lumapaint-core --offline
cargo test -p lumapaint-svg --offline
cargo test --workspace --offline
```
