# ネイティブキャンバスの技術検証

## 今回の範囲

macOSでTauriのWKWebViewに子NSViewを追加し、wgpu 27のMetalバックエンドで描画する。
WebView側は配置と操作パネルを担当し、画像をCanvas 2Dへ転送する構成にはしない。
現在は固定960×640のセッション内ドキュメントにハード円ブラシで描画する。
コアがストローク履歴を保持し、GPUが線分をインスタンス描画する。v1 JSONへの保存・再読込を実装。Sparse Tile、将来のグラフコンテナ、RGBA16F合成は後続。

## モジュール境界

- `src/CanvasPreview.tsx`：ResizeObserver、外観・ズーム、表示状態、エラー・再試行。
- `src/bridge.ts`：`sync_canvas`呼び出しを直列化。StrictModeやアンマウントでも解除順序を守る。
- `src-tauri/src/canvas.rs`：入力検査とOS別ホストへのディスパッチ。他OSは`unsupported`を返す。
- `src-tauri/src/canvas_macos.rs`：NSView所有、座標変換、メインスレッドでのサーフェス作成・描画。
- `crates/lumapaint-renderer`：wgpuのGPU初期化・描画・サーフェス再構成。AppKitには依存しない。

TauriはネイティブWebView APIを使用するため2.11系列に固定する。
依存関係更新時はmacOS実機で座標、起動と終了を再検証する。

## 座標と描画

DOMの`getBoundingClientRect()`はCSSビューポート内の座標を返す。
WKWebViewの子NSViewはタイトルバーも含むnative boundsを使うため、`safeAreaInsets`の分だけ補正する。
flipped／unflipped双方を扱い、表示領域をSafe Area内にクリップする。
画面倍率はJSから信用せず、NSWindowの`backingScaleFactor`から取得して物理ピクセルへ変換する。

GPUプレビューはsRGBサーフェスに線形色を出力する。設計仕様のRGBA16F中間合成・ICC・CMYKは未実装。
倍率は「その時点の表示領域に全体を収めた大きさ」を1とする0.25〜4倍。
作品ピクセルと画面ピクセルを一致させる100%表示は、ドキュメントモデルと一緒に導入する。

## ライフサイクル

1. UIの変更をrequestAnimationFrameでまとめ、同時に複数の描画IPCを送らない。
2. main windowのwith_webview内でNSViewを作成・更新する。
3. メインスレッド専用の領域がビューとレンダラーを所有する。
4. リサイズ時のみサーフェスを再構成し、通常時はuniformを更新して1フレーム描画する。
5. 非表示・解除・ウィンドウ破棄時にビューを取り外す。surfaceを破棄した後でNSViewのretainを解放する。
6. Lost／Outdatedはサーフェス再構成後に一度再試行。他の失敗はUIに返し、再試行で再生成する。

有限値・ズーム範囲をRust側で検査し、物理8192pxを超えるプレビューはGPU割当て前に拒否する。
デバイスエラーは保持して次の描画要求で返す。アイドル時にエラー検出用の連続描画は行わない。

## 残る検証・制約

- 初期GPU構築とプレビュー描画は現在メインスレッドで行う。現状は小規模な基本描画用であり、大規模作品に向けて描画スレッドへ分離する。
- マウス入力を実装済み。タブレット固有入力、パン、ペン筆圧は未実装。
- 高DPI変換は単体テストで検証。異なる倍率の実ディスプレイ間移動、スリープ・GPUデバイス喪失の実機検証は別途必要。
- WebViewのページ拡大率変更を前提としない。キャンバスの拡大縮小には専用のズーム操作を使う。
- WebView上のHTMLポップオーバーをネイティブビューの上に重ねる設計は未検証。操作UIは描画領域の外に置く。
- Windows・Linuxのホストは未実装。共通レンダラーを使い、OS固有のビュー所有を各ホストへ追加する。
- wgpu 27の間接依存`block 0.1.6`にRustのfuture-incompatibility警告がある。現行のビルドは成功するが、ツールチェーン更新時に再評価する。

参照：[Tauri with_webview](https://docs.rs/tauri/latest/tauri/webview/struct.WebviewWindow.html#method.with_webview)・[wgpu](https://docs.rs/wgpu/27.0.1/wgpu/)

## 基本描画の契約

`Document`はブラシ色・径と点列を所有する。最大65,536点、径1〜128pxに制限し、有限座標を検査する。
Undo/Redoはストローク単位。新規ストローク確定でRedo分岐を破棄する。非表示レイヤーへの描画は受け付けない。
`edit_document`で履歴・表示を更新し、`document-changed`イベントでUIへスナップショットを送る。
ネイティブビューの一時破棄ではドキュメントを保持する。ファイルへ保存した内容は再読込できる。
各描画時に全線分を再構築するため、大量ストロークの性能最適化は今後必要。
