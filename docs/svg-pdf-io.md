# 独立SVG / PDF I/O

`lumapaint-formats`がparser／writerと変換レポートを所有し、Rustホストが文書登録・履歴・ダイアログ・必要なラスタ化を管理する。coreとI/OはSkia・wgpu・Tauriに依存しない。WebViewへフレームを転送しない。

| 形式 | macOSで開く | レイヤー読み込み | macOS書き出し |
| --- | --- | --- | --- |
| SVG | 用紙寸法を保持した未保存文書 | 既存の読み込み経路 | 編集可能なSVG、ブラシはPNG |
| PDF | 全ページ／指定ページ・dpi指定、部分対応 | 選択ページを新しいレイヤーへ | ベクターPDF、文字はアウトライン、特殊効果はページ画像 |
| AI | PDF互換部分の全ページ／指定ページ | PDF互換ページを新しいレイヤーへ | AI固有形式は未対応 |

SVG出力ではCSS／useを解決した図形・文字・グラデーション・クリップ／マスクを明示要素で保存する。各レイヤーの定義IDを隔離し、文書クリップ、フォルダーの実効表示・均一マスク、不透明度を適用する。ブラシ・消しゴムの合成PNGはRustで生成する。文字は同じフォントが必要で、LP固有のライブ編集情報／履歴を完全に往復する保存形式ではない。

PDF writerはsvg2pdf（text／filterレンダー機能を無効化）を使う。文字をusvgでアウトライン化し、通常のパス・線・標準グラデーション・クリップ・マスク・画像をベクターPDFへ変換する。フィルターや繰り返しグラデーションはホストの全文書PNGへ戻す。ページ画像化は1600万画素までで、変換前にレポートで確認する。

PDF readerはlopdfと上限付きgraphics interpreterを使う。ページ番号、36〜1200dpi（Documentの解像度上限）、継承MediaBox／CropBox、回転・UserUnit・物理ページ寸法の保持、基本パス・塗り・線・アフィン変換・クリップ・Form、8bit RGB／Gray画像とJPEG、画像のソフトマスク・色キー透過・Decode配列に対応する。画像の変形・クリップを保持し、同じ画像は再利用する。分離した透明グループはグループ単位で不透明度と合成モードを適用する。非分離グループは外側の不透明度1・通常合成・ソフトマスクなしの場合に対応する。線形（axial）と焦点が円内にある円形（radial）グラデーション、線形補間のType 2／Type 3関数、複数ストップと不連続な色境界、変形した塗り／線と直接shadingに対応する。graphics-stateのAlpha／LuminosityソフトマスクはFormの変形・BBoxクリップとともにSVGマスクへ変換する。既知のsRGB／Gray ICC以外の色は近似変換を報告する。埋め込みTrueType／OpenTypeの単純フォント（Standard／WinAnsi／MacRoman Encoding、Differences）とIdentity-H／Identity-Vおよび埋め込みEncoding CMapのCIDFontType2／CID OpenType CFFを輪郭パスとして保持する。文字幅、TJの字間調整、文字・単語間隔、横倍率・ベースライン・行送り・文字行列、塗り／線と文字クリップを適用する。埋め込みのない単純フォントはシステムフォントで近似し、置き換えを報告する。不可視文字は省略を報告する。Type 1／Type1C／Type 3の追加対応とフォント形式ごとの検証範囲は[フォント対応表](pdf-font-formats.md)を参照。編集可能な文字情報、定義済み日本語CMap／任意の親CMap・可変／カラー／画像フォント、16bit／CMYKなどの画像、ステンシル／Matte／異なる寸法の画像マスク、タイルパターン、非線形／サンプル／PostScript補間関数、非拡張shading／shading BBox／一般の二円radial、ノックアウト／一部の非分離透明グループ、非標準のマスクtransfer／背景色などは未対応を報告し、明示的に許可した場合だけ部分読み込みする。読み込み側は書き出し側より対応範囲が狭い。暗号化PDFとAI固有／旧PostScriptデータは拒否する。

入力はSVG 4MiB、PDF 32MiB、展開ストリーム合計16MiB、参照深さ32、Form深さ16、20万命令、画像1枚419万画素／一辺8192px／画像展開合計64MiB、グラデーション関数の再帰深さ8／256ストップ、フォント128個／1プログラム4MiB（共通16MiB展開予算を使用）／65536字形などの上限を持つ。無効入力・上限超過は元文書を変更しない。出力のPNG添付も容量・寸法・デコード・非アニメーションを検証する。

三言語の変換レポートを確認後、一時ファイルから原子的に保存する。読み取り専用ExportSnapshotを使い、元の保存状態・履歴・編集中のストロークを変更しない。対応表はRustのFILE_FORMATSを唯一の管理元とする。

- [Done] PDFの埋め込みTrueType／OpenTypeと水平Identity-H CID文字を輪郭パスで読み込み。曲線字形・PDFの文字幅・字間・横倍率・ベースライン・文字行列・線・クリップを独立した合成フォントで画素比較。フォント再利用、字形数／容量／幅展開上限、未対応エンコードと欠けた配置を検証。
- [Done] PDF／PDF互換AIのページ選択ウインドウと36〜1200dpi指定。Rustでページ寸法を事前確認し、本文はRustに保持。開く／新レイヤー読み込みで物理寸法を保持し、キャンセル・閉じると保留データを破棄し、元文書が切り替わった場合は追加を拒否する。
- [Done] SVGの開く／書き出し、独立PDF writer、上限付きPDFパスreader、PDF互換AIの開く、三言語レポートと原子的保存。
- [Done] SVGの共有定義・CSS／use・グラデーション・クリップ／マスクを描画画素で照合し、PDFの基本パス往復と不正入力をテスト。
- [Done] PDFの8bit RGB／Gray画像・JPEG・画像透過と透明グループの部分対応。画像の上下反転・クリップ・グループ不透明度を画素比較し、Decode・色キー・ソフトマスクの優先順位・再利用・容量制限・不正入力をテスト。
- [Done] PDFの線形／円形グラデーション、複数色／不連続ストップ、色ごとの透明度、Alpha／Luminosityソフトマスクを読み込み。変形とグループ不透明度を画素比較し、未対応関数・再帰／ストップ上限・マスクの途中失敗を検証。
- [Done] Identity-V縦書きのDW2／W2（配列・範囲・間接参照）、字形原点と縦送り・TJ・横倍率を保持。合成フォントで画素比較し、不正な組・CID範囲・過剰な展開を拒否。
- [Done] 埋め込みEncoding CMapの1〜4バイトcodespaceとCID単独／範囲指定、notdef単独／範囲指定、WMode、Identity-H/V親を読み込み。コード境界は各バイトのcodespace範囲で判定し、CID→GID・文字幅・縦送りへ接続。複合フォントのTwは元の1バイトコード32だけに適用し、ExtGState Font指定は既存の文字状態を保持。合成日本語CID横／縦PDFの輪郭一致、文字間隔とExtGStateの回帰テストを追加。CMapは1MiB・展開65,536件・codespace64件を上限とし、保持マップも既存コンテンツ予算へ算入。仕様は[Adobe PDF §9.7.5／§9.3.3](https://developer.adobe.com/document-services/docs/assets/35e4369068f86065372c18787171a17e/PDF_ISO_32000-1.pdf)を参照。
- [Done] FontFile3のCIDFontType0Cを、元のCFFデータを含む内部sfntアダプターで解析。標準0.001 FontMatrix・CID/GID・サブルーチン・PDF文字幅を保持。横／縦の合成CIDフォントで元OpenTypeと輪郭SVGが一致。ユーザー提供PDFのヒラギノ3書体で文字省略がなくなり、本文・INDEXの配置をmacOS PDFKit表示と確認（異なるバックエンドのアンチエイリアスやICC色の完全一致は主張しない）。ユーザーPDFと埋め込みフォントはリポジトリへ複製していない。
- [Done] PDFの埋め込みType 1（PFA／PFB／バイナリFontFile）とType1Cに同梱FreeTypeの独立輪郭経路を追加。内蔵Encoding・Differences・PDF幅と字間を保持。Type 3のFontMatrix・CharProcs・d0/d1・描画リソースを読み取り、Trの扱いをISO仕様に合わせる。独自字形で配置・字形・容量制限を検証。[形式別の検証範囲と未完了事項](pdf-font-formats.md)を明記。
- [Done] PDF埋め込みTTC／OTC（静的TrueType／CID CFF）の書体をPostScript名で選択。後方書体と単独フォントの輪郭SVG一致を検証し、名前の重複・不一致・矛盾・書体数超過を拒否。TrueTypeのEncoding省略／Symbolic時は内蔵Macintosh／Windows記号cmapで字形を解決し、4種類のコードブロックと曖昧な対応をテスト。詳細は[フォント対応表](pdf-font-formats.md)。
- [Next] PDF文字の追加フォント／定義済み日本語CMap・任意の親CMap・編集可能な文字情報、追加のshading／パターン、追加の画像形式と透明効果、ICCの忠実な読み込み。
- [Next] AI固有データのparser／writer。PDFを拡張子だけAIに変えて対応済みとはしない。
- [Done] 静的CSS変数と同種単位calc、viewBox基準のパーセントtranslateを描画／編集／出力の共通パーサーへ接続。
- [Next] SVG動的CSS・3D変形、%＋pxなど異種単位calc、fill-box基準の変形。

関連：[追加形式](file-format-preparation.md)、[ベクターエンジン](vector-engine.md)。

Identity-Vの配置は[Adobe PDF仕様 §9.4.4／§9.7.4.3](https://developer.adobe.com/document-services/docs/assets/35e4369068f86065372c18787171a17e/PDF_ISO_32000-1.pdf)に従う。字形はCIDToGIDMapで指定された輪郭を使用し、フォントの縦書き代替字形を独自に推測しない。

- [Done] 全ページPDF読み込み／書き出し。最大512ページを単一文書に保持し、個別寸法・指定DPI・ページ順をネイティブ保存後も維持する。全ページ読み込みは共有PDFを一度解析し、展開SVG合計128MiBを上限とする。指定ページモードとレイヤーへの単ページ読み込みも継続する。本文の既存互換制限およびネイティブ形式の64MiB上限は維持する。
