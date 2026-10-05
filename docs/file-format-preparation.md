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
| PSD | macOSの「開く」からRustワーカーで新規タイルタブへ読み込み | 現在はPSD v1、通常合成RGB8ラスターレイヤー（最大16枚）または統合画像、非圧縮／PackBits／ZIP（予測なし）。負のレイヤー数で指定される統合透明度に対応し、保存用の追加チャンネルは欠落を報告。レイヤー等の情報を捨てる場合は許可とConversionReportが必要 |
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
- [Next] 追加形式のレイヤーマスク・グループ・用紙外の画素・追加の合成機能とメタデータを独立して保持。ZIP予測圧縮、16bit以上、CMYK、ICC変換は未対応。Photoshop実ファイルの画面操作は未検証。

根拠：[Adobe PSD仕様](https://www.adobe.com/devnet-apps/photoshop/fileformatashtml/)のLayer count／Image data、白背景の除去の挙動は[psd-toolsの統合画像処理](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/api/pil_io.py)も参照。実装はRustで行い、Pythonへの実行時依存を追加しない。

レイヤー保持は最大16枚、展開合計とタイル格納量はそれぞれ512MiBまで。マスク、効果、クリッピング、非通常合成、未知の追加タグ、用紙外の矩形、レイヤー数上限超過があれば、一部だけ保持せず全体を統合画像に切り替えてTier Cで確認する。破損した対応チャンネルや過剰展開は読み込みを拒否する。非ASCIIの旧Pascal名は文字コードを推測せず、luni名がない場合は統合画像へ切り替える。

PSD密度の画素変換は[psd-toolsのマスク合成](https://github.com/psd-tools/psd-tools/blob/main/src/psd_tools/composite/composite.py)の白への補間を参照し、Rust側で8bitへ丸める。ネイティブの既存密度設定とは意味が異なるため、数値を直接コピーしない。
