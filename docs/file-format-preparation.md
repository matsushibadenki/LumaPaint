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
| PSD | 部分アダプターを共通読み込みへ接続 | 現在はPSD v1、3チャンネル不透明RGB8、統合画像、非圧縮／PackBitsのみ。レイヤー等の情報を捨てる場合は許可とConversionReportが必要 |
| KRA | 準備 | まず検証したmergedimageのプレビュー読み込みを検討。Krita独自レイヤーや機能の完全保持とは区別する |
| EXR | 準備 | half／float・HDR、チャンネル、data/display window、multipart。現在のRGBA8タイルへ無断で8bit化しない |
| RAW | 読み込みのみ計画 | カメラRAWの現像アダプター、機種・CFA・WB・色空間。任意の生バッファは幅・高さ・配列・エンディアンの明示が別途必要 |
| HEIF / HEIC | 準備 | 実行環境のコーデック機能検査、主画像・補助画像・ICC・HDR・向き |
| AVIF | 準備 | コーデック機能検査、透過・色深度・HDR・フレーム |
| AI | 条件付き読み込みを計画 | PDF互換部分を検証してPDF経路へ渡す。旧PostScript系やIllustrator固有情報は別扱い。ネイティブAI書き出しは計画対象にしない |

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
- [Next] PSDのホスト接続とレイヤー保持、SVG／PDFの既存準備からメニュー接続。
- [Later] EXRと高精度モデル、RAW現像、HEIF／AVIF、KRAの段階的読み込み、AIのPDF互換部分。

## 一次資料

- [OpenRaster file layout](https://www.openraster.org/baseline/file-layout-spec.html)
- [Krita ORA](https://docs.krita.org/en/general_concepts/file_formats/file_ora.html)
- [Krita HEIF / AVIF](https://docs.krita.org/en/general_concepts/file_formats/file_heif.html)
- [Adobe Illustrator保存とPDF互換](https://helpx.adobe.com/illustrator/using/saving-artwork.html)
- [KDEの形式とカメラRAW拡張子](https://github.com/KDE/kimageformats/blob/master/README.md)
