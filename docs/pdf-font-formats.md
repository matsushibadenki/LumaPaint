# PDFフォント形式の対応

PDF読み込みの字形処理はRust内で完結する。通常のTrueType／OpenTypeはttf-parser、PostScript系の追加経路は同梱FreeType（freetype-rs 0.38.0）を使用する。保持するのは容量制限付きの輪郭命令で、FreeTypeのネイティブポインターをDocumentやWebViewへ渡さない。

| PDF内の形式 | 状態 | 読み取り経路・検証 |
| --- | --- | --- |
| TrueType、OpenType TrueType | [Done] | FontFile2／OpenType。既存の輪郭・文字幅・クリップ回帰テスト。Encoding未指定／Symbolicの内蔵Macintosh (1,0)・Windows Symbol (3,0)を使用し、記号用コードをUnicodeとして推測しない |
| OpenType CFF | [Done] | FontFile3 OpenType。単純フォント／CIDの元の輪郭を使用 |
| CIDFontType2 | [Done] | CIDToGIDMap、Identity-H/V、埋め込みEncoding CMap、W/DW、W2/DW2 |
| CIDFontType0C（裸のCID CFF） | [Done] | 標準行列は既存sfntアダプター、その他はFreeType。裸CFFのFreeTypeロード番号がCIDである点を区別。合成CIDフォントのCID 42／7を検証 |
| Type 1（PFA／PFB、PDFのバイナリFontFile） | [Done] | FreeType。独自矩形字形の内蔵Encoding、Differences、PDF幅900とフォント幅600の分離を検証 |
| Type1C（裸の非CID CFF／Type 2 charstring） | [Done] | FreeTypeで文字コードを解決し、標準行列の輪郭は元CFFを直接解析。独自CFFの字形・PDFのDifferences・送り位置を検証 |
| Type 3 | [Done] | FontMatrix、CharProcs、d0/d1、Resources／ページからの継承、描画ストリームを既存PDF interpreterへ接続。Trは仕様通り3のみ非表示、その他は字形描画でクリップを追加しない |
| MMType1、CID Type 1 | [Next] | FreeTypeへの解析経路を接続。実際の当該形式の独立フィクスチャとAdobe実出力による検証は未完了 |
| TTC／OTC（静的TrueType／CFF） | [Done] | 埋め込みコレクションのPostScript名をPDFのBaseFont／FontNameと照合し、一意の書体を選択。後方書体のTrueTypeとCID CFFが単独書体と同じ輪郭SVGになることを検証。複数候補／不明な名前は省略を報告 |
| 可変・カラー・ビットマップsfnt | [Next] | 通常sfnt経路では未対応として報告。Type 3内の対応済み画像描画は別経路 |
| WOFF／WOFF2、BDF／PCF／FNT／PFR等 | [Later] | PDFの標準埋め込みフォント形式ではない。アプリの編集用フォント導入／変換として別途対応が必要 |

「全フォント形式を完全対応」とは判定していない。未埋め込みフォントの完全再現、定義済み日本語CMap全種類、任意の親CMap、Unicode抽出／編集可能文字、可変・カラー字形の完全再現も未完了。読み込んだ文字の主な保持形態は輪郭であり、編集可能な文字への復元を意味しない。

追加経路も既存のフォント4MiB・フォント数128・コレクション内書体64・ページ内字形65,536・コンテンツ予算に従う。標準行列の裸の非CID CFF輪郭は元データをttf-parserでも直接解析し、小数座標100.25／100.5の保持をテストする。FreeTypeのみで読む追加形式ではヒンティングを無効にして26.6座標で取得するが、デコード中の丸めを含む小数座標の完全保持は未検証。1字形4,096命令、保持容量を同じ予算から差し引く。Type 3は字形ストリーム1MiB・再帰16段まで。保持する描画リソースも予算へ算入し、自己参照と予算超過を拒否するテストを追加。壊れた文字幅、未対応字形、容量超過は成功扱いにせず、既存の変換レポート／失敗経路を使う。

参考：[Adobe ISO 32000-1 §9.3.6／§9.6／§9.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf)、[FreeType対応形式](https://freetype.org/freetype2/docs/index.html)、[FreeTypeライセンス](https://freetype.org/license.html)。FreeTypeはアプリに同梱し、OSへ追加インストールを要求しない。FreeTypeの著作権・ライセンス条件は同梱ソースのFTL.TXTに従う。

## 内蔵文字コードとコレクションの選択（2026-10-06）

- [Done] TrueTypeのEncoding省略時、またはSymbolicフラグ指定時は、内蔵Windows Symbol cmapを優先し、なければMacintosh Roman cmapを使う。Windowsの0x0000／F000／F100／F200ブロックは1バイトのPDFコードへ対応付ける。同じコードが複数の異なる字形に対応する場合は`pdf.font_mapping`を報告し、誤った字形へ置換しない。SymbolicのEncoding指定は仕様通り無視する。
- [Done] 同じ文字列のMacintosh／4種類のWindowsブロックで、既存の通常TrueTypeと字形・配置のSVGが一致する回帰テストを追加。PostScript系の内蔵Adobe Expert charmapも候補として認識するが、Expert独自フィクスチャの検証は未完了。
- [Done] TTC／OTC内の書体名はname ID 6で比較。ASCII Macintosh名とUnicode名を読み、正規の6大文字＋`+`のサブセット接頭辞のみ除去する。名前の欠落、重複、PDF内の矛盾する書体名、64書体超過、不正なヘッダーをテスト。元フォント／ユーザーPDFを追加でリポジトリへ複製していない。
- [Next] 定義済みCJK CMap、可変・カラー字形、MMType1／CID Type 1の実出力検証。コレクション読み込みはPDF readerの拡張であり、全PDFソフトでのコレクション埋め込み互換性やAdobe往復を保証しない。

参照：[OpenType Font Collections](https://learn.microsoft.com/en-us/typography/opentype/spec/otff#font-collections)、[Adobe ISO 32000-1 §9.6.6.4](https://opensource.adobe.com/dc-acrobat-sdk-docs/standards/pdfstandards/pdf/PDF32000_2008.pdf)。

## Adobe Fonts LiveType（2026-10-06）

macOSでは `~/Library/Application Support/Adobe/CoreSync/plugins/livetype` の `.r`／`.t`／`.w` にある不可視ファイルを、拡張子によらずSFNT署名で検出する。共通Rustローダーを文字描画・SVG編集・PDFの未埋め込みフォント検索で使用し、既存のシステムフォントとPostScript名が重なる場合は追加しない。AppKit編集にはプロセス内のCoreText登録を使用する。Adobeキャッシュの変更やOSへの永続インストールは行わない。登録・探索は起動中に一度行うため、外部でフォントを追加した場合はアプリを再起動する。

通常のOpenType／TrueType／コレクションが対象。`.e` の暗号化データ、権利情報XML、転送データベースは読み込まない。シンボリックリンクと64MiBを超えるファイルは除外する。PDFの代替用ローカルフォントは日本語大容量フォント向けに64MiBまで許可するが、既存コンテンツ予算にも従う。PDF埋め込みフォントの4MiB制限は変更しない。未埋め込みCIDフォント等、PDF解析側の未対応形式はこの探索追加だけでは対応にならない。

English: Readable hidden Adobe LiveType OpenType/TrueType collections are discovered once per process and shared by rendering, SVG editing and PDF substitution. Native registration is process-local. Encrypted cache files are excluded; restart after external font changes.

简体中文：读取 Adobe LiveType 缓存中可解析的隐藏 OpenType／TrueType 及字体集合，用于绘制、SVG 编辑和 PDF 替代字体。每个进程只扫描一次；原生字体注册仅在当前进程有效。不读取加密缓存；外部添加字体后请重启应用。
