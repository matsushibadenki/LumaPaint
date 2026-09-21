# ベクターエンジン：Skia

## 調査結果と採用範囲（2026-09-21）

Google Skiaを採用する。macOS Apple Siliconで`skia-safe 0.153.3`のビルド、CPUでのSVG描画、PathOpsの実行を確認した。
Skiaはパス描画・図形演算のエンジンであり、制御点編集UIやドキュメント履歴まで提供するエディターではない。
今回の実装はSVGレイヤーの描画への組み込みと、今後の編集機能が呼び出すパス演算の基盤まで。

## 描画経路

`SVG文字列 → usvgで解析・正規化 → Skia CPU → premultiplied RGBA8 → 既存wgpu／Metal合成`

- macOSホストは`lumapaint-renderer/skia`を有効にする。
- 単色の塗り・線を持つパスと基本図形をSkiaで描画する。変換、線幅、破線、塗りの規則、不透明度を正規化後のSVG経由で渡す。
- グラデーション、パターン、画像、テキスト、クリップ、マスク、フィルター、グループ不透明度、分離・混合、特殊な描画順・アンチエイリアス指定があるSVGは、全体を既存のresvgで描画する。要素ごとの混在合成はしない。
- 元SVGを作品ファイルに保持する。SkiaのDOMやバイナリ形式へ置き換えない。
- 縦横比を維持した中央配置と既存のGPUキャッシュを使う。CPUのN32画素を明示的にRGBA8888／premultipliedへ変換し、BGRAとの取り違えを防ぐ。
- 外部ファイル・ネットワーク参照をusvg側で無効にする。Skiaには正規化したSVGのみを渡す。
- Skiaを無効にしたrendererはresvgのみで動作する。これは代替描画経路であり、PathOpsの代替実装ではない。

Skia独自のGPUコンテキストを追加せず、現在のwgpu／Metalのビュー所有・合成に接続した。現段階ではCPU描画とテクスチャ転送が発生し、ズーム時は文書ピクセルのラスタ画像を拡大する。解像度に依存しない再描画とGPU共有は後続の性能設計で扱う。

## 編集エンジンの境界

コアの`VectorPath`はSVGの`d`文字列と`FillRule`を持つ。座標は文書座標、スタイルや変換は含めない。
`VectorPathEngine`は合体・左から右の差分・交差・排他的和、および塗り領域の点内包判定を定義する。
rendererの`SkiaPathEngine`がこの契約を実装し、入力を書き換えず結果を返す。閉じていない輪郭の演算も塗り領域として扱う。
入力は1 MiB／65,536点以下の有限座標に制限する。逆塗りは共通モデルに含めない。演算で空になった結果は有効。

`VectorObject`はオブジェクトID、名称、SVGパス、アフィン変換、塗り、線、表示状態を持つ。編集可能なベクターレイヤーとして作品ファイルに保存し、作成・オブジェクト追加／更新・選択IPCから操作できる。描画時だけ互換SVGへ変換するため、Skiaのオブジェクトは保存形式に露出しない。オブジェクト編集はブラシストロークと共通の順序付きUndo/Redo履歴に入り、ベクター選択状態も履歴移動に合わせて復元する。

macOSのネイティブキャンバスでは、ペン（P）、長方形／楕円（U／Shift+U）を文書座標の`VectorObject`として作成する。編集可能なベクターレイヤーがなければ自動作成し、描画色を塗りまたは線へ適用する。ベクター選択（V）は制御点メタデータから前面のオブジェクトをヒットテストし、Shiftで複数選択、ドラッグで一括移動できる。選択境界と制御点のキャンバス表示・直接編集、PathOpsを呼び出すUIは後続工程とする。

## ビルド・検証

依存は検証済みの`=0.153.3`に固定し、既定機能に`svg, webp`を追加する。この組合せには公式rust-skia配布のApple Silicon向けビルド済みバイナリがある。`svg`だけを追加した組合せは今回の配布で404となったため採用しない。
初回ビルドにはCargo依存とGitHub Releaseのダウンロードが必要。対象プラットフォーム・機能のバイナリがなければ上流のソースビルドへ移行し、追加ツールと時間が必要になる。

```sh
cargo test -p lumapaint-renderer --features skia
cargo test -p lumapaint-renderer --no-default-features
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run tauri build -- --debug --bundles app
```

自動テストはRGBAのチャンネル順、半透明画素、中央配置、穴、変換・破線、グループ不透明度、複雑SVGの互換経路、外部画像の拒否、演算結果の内包領域、曲線・空結果・不正入力を検証する。
Windows／LinuxのSkia有効構成とmacOS Intelの実行は未検証。UIは追加していないため翻訳の追加はない。

macOS Apple M4で検証用アプリから`tests/fixtures/sample-layer.svg`を読み込み、角丸背景・青い円・白い線の表示と作品保存を確認した。保存ファイルの元SVGも維持される。全体テスト61件、Skia無効構成14件、Clippy、フロントエンドビルド、開発用.appの生成が成功した。既存のGPU専用テスト6件は今回の通常テストでは対象外。

## 一次資料

- [Google Skia](https://github.com/google/skia/)
- [SkPathの構造と塗り規則](https://skia.org/docs/user/api/skpath_overview/)
- [PathOps](https://skia.org/docs/dev/present/pathops/)
- [CPU／GPU Canvasの作成](https://skia.org/docs/user/api/skcanvas_creation/)
- [Rust bindingsとビルド条件](https://github.com/rust-skia/rust-skia)
- [検証した配布バイナリ](https://github.com/rust-skia/skia-binaries/releases/tag/0.153.3)
- [tiny-skia](https://github.com/linebender/tiny-skia)：既存resvgが使用するRust実装。Skia全体の代替ではない。
