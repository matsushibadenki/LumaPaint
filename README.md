# LumaPaint

漫画・イラスト・AI生成・アニメーションを共通の非破壊モデルで扱う制作アプリです。
現在はAdobe製品のワークスペース構成を参考にした編集画面と、macOS向けの基本ブラシを実装しています。
960×640のキャンバスに描画し、色・径の変更、ストローク単位のUndo/Redo、単一レイヤーの表示切替ができます。
`.lumapaint`ファイルへの保存・別名保存・再読込に対応しています。復旧用コピーの自動保存と、次回起動時の作業復旧に対応しています。AI生成は未実装です。

上部の11分類のメニューから操作できます。「ファイル」に開く・保存・別名で保存、「編集」にUndo/Redo、「表示」にズーム、「ウインドウ」にパネル操作を配置しています。未実装の項目は「準備中」と表示し、無効化しています。
ファイルメニュー左側のアプリアイコンから設定を開き、一般タブで言語、アピアランスタブで外観を変更できます。
イメージメニューの「カラーモード」からRGB／CMYKを選択できます。選択したモードは作品ファイルと復旧用コピーに保持され、今後のモードは共通の定義へ追加できます。
同じイメージメニューの「階調」から8／16／32 bitsを選択でき、選択値は作品ファイルと復旧用コピーに保持されます。
編集メニューの「カラー設定」では、RGB用のsRGB・Display P3・Adobe RGB、CMYK用のJapan Color 2001 Coatedからカラープロファイルを選択できます。モードを変更した場合は互換性のある既定プロファイルへ自動的に切り替わります。
右側のプロパティエリアは、ブラシ・ドキュメント・レイヤーをタブで切り替えられます。タブはドラッグ＆ドロップまたはAlt＋左右矢印キーで並べ替えでき、選択中のタブと並び順は端末内に保存されます。
ファイルメニューの「SVGをレイヤーとして読み込み」からSVGファイルを追加できます。SVGは縦横比を保ってキャンバス中央へ配置され、レイヤーパネルで個別に表示を切り替えられます。元のSVGデータは作品ファイルと復旧用コピーへ保持されます。

macOSでは基本的なSVG図形をGoogle Skiaで描画し、複雑なSVGはresvgで描画します。ベクター編集に向けたパス演算基盤も追加しています。制御点編集UIは後続の実装です。[採用範囲とビルド条件](docs/vector-engine.md)を参照してください。

## 開発環境（macOS優先）

- Node.js 22以上、npm
- Rust stable（Cargo、rustfmt、Clippyを含む）
- Xcode Command Line Tools（`xcode-select --install`）
- macOS 12以上。まず開発中のMacで動作確認し、対応範囲を段階的に検証します。

```sh
npm ci
npm run tauri dev
```

UIのみのプレビューは `npm run dev` です。ブラウザではRustコアに接続されず、その旨が画面に表示されます。

```sh
npm run build
npm run check:rust
npm run test:rust
npm run tauri build -- --bundles app
```

macOSアプリの出力先は `target/release/bundle/macos/LumaPaint.app` です。
開発用アプリは `npm run tauri build -- --debug --bundles app` で生成でき、
出力先は `target/debug/bundle/macos/LumaPaint.app` です。
配布用署名・公証、正式なアイコン、正式なアプリ識別子の確定は今後行います。

## 構成

```text
src/                      React + TypeScript（UI・翻訳・IPC窓口）
src-tauri/                Tauri 2（デスクトップ起動・OSとの接続）
crates/lumapaint-core/    OS・UI非依存のRustコア
crates/lumapaint-renderer/  wgpu描画・WGSLシェーダー（OS非依存）
docs/                    設計・開発方針・ロードマップ
```

macOSではRust + wgpu + MetalでWKWebViewの子NSViewに描画します。
拡大・縮小・全体表示、ウィンドウのサイズ変更、外観変更に応じて必要なフレームだけ描画します。
倍率は全体表示を100%とする相対値です。GPU情報は準備完了表示のツールチップで確認できます。
ブラウザと他OSでは未対応の案内を表示し、ズーム操作を無効化します。
UIの言語（英語・日本語・简体中文）と外観（システム・ライト・ダーク）は端末内に保存します。
APIキーはまだ取り扱いません。描画データはメモリ内に保持し、保存操作でファイルへ書き出します。

## 他プラットフォーム

Windows・Linuxでも共有できる構成とし、CIに3 OSのコンパイルチェックを配置しています。
macOS以外の実機動作とパッケージ配布は未検証です。OS固有の機能はTauri側に閉じ込めます。
WindowsはMSVCビルドツール・WebView2、LinuxはWebKitGTK等の依存パッケージが必要です。
詳細は [Tauriの前提条件](https://v2.tauri.app/start/prerequisites/) を参照してください。

[開発方針](docs/development.md) / [ロードマップ](docs/roadmap.md) / [設計仕様書](docs/LumaPaint設計仕様書.md)

## 作品の保存

上部の「保存」「別名で保存」「開く」を使います。ショートカットは⌘S、⇧⌘S、⌘Oです。
ファイルはバージョン付きJSON形式で、線の座標・色・径とレイヤーの表示状態を保持します。
Undoの取り消し済み分岐やUI設定は作品ファイルに含めません。詳しくは [保存形式](docs/project-format.md) を参照してください。

## 作業の復旧

ストローク確定やUndo/Redoの後に、作品ファイルとは別の復旧用コピーをバックグラウンドで保存します。
異常終了後は起動時の「前回の作業を復旧」から戻せます。復旧した作品は手動で保存してください。
未確定のストロークや書き込み途中の変更は復旧できない場合があります。[復旧機能の仕様](docs/recovery.md) を参照してください。
