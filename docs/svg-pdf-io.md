# 独立SVG / PDF I/O

`lumapaint-formats`がparser／writerと変換レポートを所有し、Rustホストが文書登録・履歴・ダイアログ・必要なラスタ化を管理する。coreとI/OはSkia・wgpu・Tauriに依存しない。WebViewへフレームを転送しない。

| 形式 | macOSで開く | レイヤー読み込み | macOS書き出し |
| --- | --- | --- | --- |
| SVG | 用紙寸法を保持した未保存文書 | 既存の読み込み経路 | 編集可能なSVG、ブラシはPNG |
| PDF | 部分対応、最初のページ | 未対応 | ベクターPDF、文字はアウトライン、特殊効果はページ画像 |
| AI | PDF互換部分だけ | 未対応 | AI固有形式は未対応 |

SVG出力ではCSS／useを解決した図形・文字・グラデーション・クリップ／マスクを明示要素で保存する。各レイヤーの定義IDを隔離し、文書クリップ、フォルダーの実効表示・均一マスク、不透明度を適用する。ブラシ・消しゴムの合成PNGはRustで生成する。文字は同じフォントが必要で、LP固有のライブ編集情報／履歴を完全に往復する保存形式ではない。

PDF writerはsvg2pdf（text／filterレンダー機能を無効化）を使う。文字をusvgでアウトライン化し、通常のパス・線・標準グラデーション・クリップ・マスク・画像をベクターPDFへ変換する。フィルターや繰り返しグラデーションはホストの全文書PNGへ戻す。ページ画像化は1600万画素までで、変換前にレポートで確認する。

PDF readerはlopdfと上限付きgraphics interpreterを使う。ページ番号、36〜1200dpi（Documentの解像度上限）、継承MediaBox／CropBox、回転・UserUnit・物理ページ寸法の保持、基本パス・塗り・線・アフィン変換・クリップ・Formに対応する。既知のsRGB／Gray ICC以外の色は近似変換を報告する。文字、画像XObject、パターン、shading、透明グループ／soft maskなどは未対応を報告し、明示的に許可した場合だけ部分読み込みする。読み込み側は書き出し側より対応範囲が狭い。暗号化PDFとAI固有／旧PostScriptデータは拒否する。

入力はSVG 4MiB、PDF 32MiB、展開ストリーム合計16MiB、参照深さ32、Form深さ16、20万命令などの上限を持つ。無効入力・上限超過は元文書を変更しない。出力のPNG添付も容量・寸法・デコード・非アニメーションを検証する。

三言語の変換レポートを確認後、一時ファイルから原子的に保存する。読み取り専用ExportSnapshotを使い、元の保存状態・履歴・編集中のストロークを変更しない。対応表はRustのFILE_FORMATSを唯一の管理元とする。

- [Done] SVGの開く／書き出し、独立PDF writer、上限付きPDFパスreader、PDF互換AIの開く、三言語レポートと原子的保存。
- [Done] SVGの共有定義・CSS／use・グラデーション・クリップ／マスクを描画画素で照合し、PDFの基本パス往復と不正入力をテスト。
- [Next] PDF画像・文字・shading・パターン・透明グループとICCの忠実な読み込み、UIページ選択、複数ページ／レイヤー追加。
- [Next] AI固有データのparser／writer。PDFを拡張子だけAIに変えて対応済みとはしない。
- [Done] 静的CSS変数と同種単位calc、viewBox基準のパーセントtranslateを描画／編集／出力の共通パーサーへ接続。
- [Next] SVG動的CSS・3D変形、%＋pxなど異種単位calc、fill-box基準の変形。

関連：[追加形式](file-format-preparation.md)、[ベクターエンジン](vector-engine.md)。
