# 追加ファイル形式の対応準備

操作別の状態はRustの `lumapaint-formats::io::FILE_FORMATS` が唯一の管理元。
アダプターの実装、既存ホスト処理、実装予定は別のフィールドで返す。
予定をダイアログの対応済みフィルターとして使わない。

| 形式 | 現在 | 次の実装方針 |
| --- | --- | --- |
| PNG | macOSで開く／画像読み込みあり | 独立コーデック、透過・ICC・dpi保持、書き出し |
| JPG / JPEG | macOSで開く／画像読み込みあり | 同じ形式IDに統一、品質指定、透過を背景へ合成する選択 |
| WebP | macOSで選択レイヤーへ読み込みあり | 独立コーデック、可逆／非可逆、透過、アニメーション |
| GIF | 準備 | フレーム選択・遅延・ループ、パレット・透明色・ディザ |
| BMP | 準備 | 圧縮方式・色深度を限定して開始 |
| TIFF / TIF | 準備 | ページ選択、ICC、16bit／浮動小数点、圧縮方式 |
| ICO | 準備 | 内部サイズ／画像の選択、複数サイズ生成、透過 |
| ORA | 準備 | ZIP内のmimetype・stack.xml・PNGレイヤー、階層・順序・座標・不透明度・合成モード |
| PSD | macOSの「開く」からRustワーカーで新規タイルタブへ読み込み | 現在はPSD v1、通常合成RGB8ラスターレイヤー（最大16枚）または統合画像、非圧縮／PackBits／ZIP（予測なし／あり、8bit）。負のレイヤー数で指定される統合透明度に対応し、保存用の追加チャンネルは欠落を報告。レイヤー等の情報を捨てる場合は許可とConversionReportが必要 |
| KRA | 準備 | まず検証したmergedimageのプレビュー読み込みを検討。Krita独自レイヤーや機能の完全保持とは区別する |
| EXR | 準備 | half／float・HDR、チャンネル、data/display window、multipart。現在のRGBA8タイルへ無断で8bit化しない |
| RAW | 読み込みのみ計画 | カメラRAWの現像アダプター、機種・CFA・WB・色空間。任意の生バッファは幅・高さ・配列・エンディアンの明示が別途必要 |
| HEIF / HEIC | 準備 | 実行環境のコーデック機能検査、主画像・補助画像・ICC・HDR・向き |
| AVIF | 準備 | コーデック機能検査、透過・色深度・HDR・フレーム |
| AI | macOSでPDF互換部分を部分読み込み | 未対応PDF内容はレポート。旧PostScript系やIllustrator固有データは未対応。ネイティブAIの書き出しは別のparser／writerが必要 |

## 入出力契約

- `format_from_extension` は大文字・小文字と別名を正規化する。jpg/jpeg、tif/tiff、heif/heicを同じ形式へ結び付ける。
- RAW関連のdng/cr2/cr3/nef/nrw/arw/raf/rw2/orf/pef/srwは将来のカメラRAW経路を予約する。登録はデコード対応を意味しない。
- `ReadContent` はLP DocumentとTiledRasterStateを区別する。PSDをSVGへ偽装せず、Rust側にネイティブタイルを返す。ネイティブのJSON形式とタイル形式も読み分ける。
- `ReadOptions` はページ、フレーム、解像度、非可逆変換許可を保持する。選択番号が無意味な形式はエラーにし、未実装コーデックは常に未対応エラーを返す。
- 外部ファイルを解析してから文書タブ／レイヤーを更新する。失敗時は元文書を維持する。エラーコードとConversionReportはUI側で英語・日本語・简体中文に表示する。
- 画像サイズ、展開後バイト数、ページ・フレーム数、ZIP展開量、深さ、処理時間の上限をコーデック実装時に適用する。ZIPのパス逸脱・参照欠損・過大展開は読み込み前に拒否する。
- EXR／HDR／16bit以上の実装はモデルの対応を先に整備する。8bitフォールバックには明示選択と変換報告を要求する。
- 出力は画像品質・透明度・ICC・メタデータ・ページ／フレーム範囲を指定し、変換完了後に原子的に保存する。元文書のネイティブ保存状態を変えない。
- CPUコーデックが必要な形式はRustワーカーで処理する。リサイズ・色変換・合成など対応できる処理はGPUサービスに分離し、失敗時はCPUへフォールバックする。WebViewへ大容量フレームを転送しない。
- コーデックの依存関係、配布ライセンス、OS別のビルド・実行可否を確認してから対応を有効にする。

## 優先順位

- [Done] 全指定形式のID・拡張子別名・分類・操作別の現在／予定状態を共通化。PSD共通読み込み、タイル保持、未対応形式の拒否をテスト。
- [Next] PNG／JPEG／WebPの独立コーデックと書き出し。次にBMP／TIFF／GIF／ICOとORAのレイヤー読み込み。
- [Done] PSDのmacOSホスト接続。三言語の損失確認を行い、未保存のタイルタブへ追加。既存文書を保持し、保存再読込・復旧を合成ファイルで検証。
- [Done] 通常合成のPSD RGB8ラスターレイヤーを独立して保持。名前・順序・表示・不透明度・透明度・基本保護設定をネイティブ保存で維持。
- [Done] PSDの20バイト形式の通常ラスターマスクをRaw／PackBitsから保持。無効化・反転を維持し、マスク画素・合成・ネイティブ保存をテスト。用紙内の部分領域と白／黒の領域外背景も保持し、RGBと独立した寸法と位置を検証。レイヤー相対座標（負のオフセットを含む）を解決。20バイト形式のユーザーマスク密度は領域外背景・反転を含めて画素に反映し、元の数値を保持しないTier B変換として確認する。用紙外・ぼかし・ベクターマスクパラメータは統合画像へ切り替える。
- [Done] PSD RGB8のZIP（予測なし）を統合画像・レイヤー・マスクで読み込み。全画像バッファを追加せず行単位で展開し、上限・終端・チェックサム・過不足と余剰入力を検証。
- [Done] PSD RGB8の予測付きZIPにも対応。行ごとに8bit差分を復元し、複数レイヤー・統合透明度・密度付きマスクでRawと同一のネイティブ状態を検証。
- [Next] PSDの追加マスク形式／グループなど追加のレイヤー機能とPhotoshop実ファイルの画面検証、PDF読み込みの追加互換。SVG／PDFの開く・書き出し・ページ選択はmacOSで接続済み。
- [Later] EXRと高精度モデル、RAW現像、HEIF／AVIF、KRAの段階的読み込み。AIのPDF互換部分は部分対応済み。

## 一次資料

- [OpenRaster file layout](https://www.openraster.org/baseline/file-layout-spec.html)
- [Krita ORA](https://docs.krita.org/en/general_concepts/file_formats/file_ora.html)
- [Krita HEIF / AVIF](https://docs.krita.org/en/general_concepts/file_formats/file_heif.html)
- [Adobe Illustrator保存とPDF互換](https://helpx.adobe.com/illustrator/using/saving-artwork.html)
- [KDEの形式とカメラRAW拡張子](https://github.com/KDE/kimageformats/blob/master/README.md)

## PSDの統合透明度

- [Done] RGB8統合画像の透明度をタイルへ保持。レイヤー数が負の場合だけ最初の追加チャンネルを透明度として適用する。その他のアルファ／スポットチャンネルは保持できないことを三言語で確認し、不透明な画像を誤って透明にしない。
- [Done] 統合プレビューの白背景を除去して直線アルファへ変換。完全透明画素はRGBも0にし、タイル合成で二重にアルファを掛けない。半透明色は8bitプレビューの量子化精度に制限される。
- [Done] 入力512MiB・寸法8192pxに加えて、全チャンネルの展開総量を512MiBに制限。レイヤー記録・チャンネル長・統合データ長・RLE行を検証してからタイルを生成。
- [Done] Raw／PackBitsの通常合成RGB8ラスターレイヤー、ASCII／luni Unicode名、用紙内の位置、順序、可視性、不透明度、チャンネル-1の透明度、透明度保護／全保護を保持。レイヤー画素は直線アルファを使用し、統合プレビュー用の白背景補正を適用しない。
- [Next] 追加形式のレイヤーマスク・グループ・用紙外の画素・追加の合成機能とメタデータを独立して保持。16bit以上、CMYK、ICC変換は未対応。Photoshop 2026で自作PSBの読み込み／再保存と名前・表示・ロックを確認済み。実制作PSDの文字・効果・ICCを含む操作は未検証。

根拠：[Adobe PSD仕様](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)のLayer count／Image data、白背景の除去の挙動は[psd-toolsの統合画像処理](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/api/pil_io.py)も参照。実装はRustで行い、Pythonへの実行時依存を追加しない。

レイヤー保持は最大16枚、展開合計とタイル格納量はそれぞれ512MiBまで。マスク、効果、クリッピング、非通常合成、未知の追加タグ、用紙外の矩形、レイヤー数上限超過があれば、一部だけ保持せず全体を統合画像に切り替えてTier Cで確認する。破損した対応チャンネルや過剰展開は読み込みを拒否する。非ASCIIの旧Pascal名は文字コードを推測せず、luni名がない場合は統合画像へ切り替える。

PSD密度の画素変換は[psd-toolsのマスク合成](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/composite/composite.py)の白への補間を参照し、Rust側で8bitへ丸める。ネイティブの既存密度設定とは意味が異なるため、数値を直接コピーしない。

8bitのZIP予測復元は[psd-toolsの予測圧縮処理](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/compression/__init__.py)の行単位の差分／累積和を参照。Rustの行バッファ内で復元し、Pythonへの実行時依存は追加しない。

## Adobe互換・日本語の検証範囲（2026-10-05）

「読み込める」「見た目を再現する」「編集可能な情報を保持する」「Adobeへ再保存できる」を分けて判定する。
形式名や日本語の文字列が残るだけで完全互換としない。変換時に失う情報はConversionReportで確認する。

| 形式 | 独立I/Oの現在の範囲 | 日本語の根拠・残る差 |
| --- | --- | --- |
| SVG | 読み込み・書き出し。対応するベクターとSVGソースを保持 | フォントが必要な文字を報告。日本語の縦組み・禁則・ルビ・字形置換とAdobe往復は未保証 |
| PDF | 対応するパス・文字輪郭の読み込みとベクター書き出し | Identity-H/VのType2に加えてCID-keyed OpenType CFFのType0を読み込み。非連続CIDと縦横位置を画素比較。編集可能な日本語文字、外部CMap、raw CIDFontType0C、組版全体は未対応 |
| AI | PDF互換部分の読み込み | PDF側のCID改善を共有。Illustrator独自の編集情報とネイティブAI書き出しは未対応 |
| PSD | RGB8通常ラスターレイヤー、対応マスク、または損失確認付き統合画像の読み込み | Unicodeレイヤー名を保持。文字レイヤーを編集可能な日本語テキストとして保持する機能とPSD書き出しは未対応 |
| PSB | v2の64bit長・32bitRLE行長を扱うRGB8部分読み込み。macOS「開く」へ接続 | PSDと同じUnicode名・画素を合成ファイルで検証。Photoshop再保存PSBも、合成プレビューとの画素比較を通過すればレイヤーを保持。省略する編集・色設定を報告し、差があれば統合画像へ切り替える。巨大画像、文字レイヤーの編集、PSB書き出しは未対応 |
| INDD | 直接読み込み・書き出しは未対応 | InDesignの組版・リンク・フォント・ページ情報を保持するモデルとアダプターが必要。IDMLを交換経路として別途設計し、INDDと同一視しない |

PSB対応も入力／展開512MiB、8192px、通常ラスターレイヤー最大16枚の既存上限を維持する。
AdobeのPSB最大寸法をそのまま読み込めるとの意味ではない。PSD/PSBの拡張子に対応しないヘッダーバージョンは拒否する。

- [Done] PDF／PDF互換AIの埋め込みCID OpenType CFFに対応。CIDをGIDと誤認しない。Identity-H/Vの独立した画素比較を追加。
- [Done] PSB v2の部分読み込み・拡張子／操作登録・macOSホスト接続。4圧縮方式、Unicode名、ネイティブ保存、切断／過大64bit長を検証。
- [Done] Photoshop 2026で自作PSBを開いてレイヤー名を確認し、別名保存した実ファイルのレイヤー保持、統合プレビューとの比較、損失報告を回帰テスト化。
- [Next] Adobe実ファイルでCIDFontType0C／日本語CMap・縦組み記号を検証し、SVG／PDFのフォントと字形の不足を明示する。Photoshopの追加メタデータをネイティブ形式と書き出しで保持する。
- [Next] PSD／PSBの文字・グループ・合成・ICCと書き出し、AI固有データの保持、InDesignのIDML交換モデルと入出力を段階実装。
- [Later] INDD直接アダプターとAdobeの高度な編集情報を保持する往復。各形式・各機能の実製品比較が揃うまでは完全互換と判定しない。

InDesignの交換形式については[Adobeの旧バージョン互換案内](https://helpx.adobe.com/sg/indesign/desktop/troubleshoot/file-and-output-issues/cant-open-in-previous-versions.html)を参照する。


## Photoshopの追加情報とプレビュー検証

- [Done] PSD／PSBの参照点 `fxrp` とメタデータコンテナー `shmd` のサイズ・項目数・フラグを検査。既知の文書追加ブロック（CAI／GenI／OCIO／cinf／FMsk）を省略する場合は、レイヤー合成と統合プレビューを比較してから保持する。空のPattも許可する。未知のキー、効果・文字・未対応の合成機能は従来どおり全体を統合する。
- [Done] ロックの位置／画素部分指定は、現在のLPモデルの全体ロックへ保守的に変換。元の細かな設定と追加編集・色設定の欠落はTier Dで確認する。保護されたレイヤーを自動解除しない。
- [Done] Photoshop 2026で再保存したPSBの日本語／简体中文名、表示・画素・ロックを3レイヤーで保持し、ネイティブ保存でも検証。RGB／アルファの比較はpremultiplied RGBA8でチャンネルごとの誤差1/255以内。差が大きければ `psd.previewMismatch` と `psd.layersFlattened` を通知して統合画像を使う。
- [Done] 比較処理はRustワーカー内で完結する。ライブ文書と同じ独立CPU合成カーネルを使い、文書画素全体を複製せずタイル単位に比較。追加検証用の統合プレビューはタイル面積4,194,304画素（16MiB）まで。それより大きい追加設定付き文書は比較を省いてレイヤーを保持せず、統合する。
- [Next] 部分ロック・参照点・色設定・追加メタデータをモデルとして保持し、Photoshopへの再書き出しで復元する。画素比較は保存時のRGB8プレビューを基準とし、ICCによる表示色、将来の編集時の挙動や文字編集の互換を保証しない。
