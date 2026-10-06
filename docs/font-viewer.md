# Font Viewer / フォントビューア / 字体查看器

[Done] 文字パネル右上のメニューで「文字設定」「フォントビューア」を切り替える。選択オブジェクトがなくても閲覧でき、既存の文字パネルのドッキング／フローティングを利用できる。フォント名検索、和文／欧文／お気に入り、文書で使用中／最近使用の絞り込み、ファミリーと各スタイルの展開に対応。

サンプル文字とサイズ、一覧のサンプル／フォント名モードとサイズを変更できる。クリックでプレビューし、「選択文字に適用」またはダブルクリックで、既存の文字範囲書式経路へ適用する。最近使用は適用成功後のみ記録する。お気に入りと名前付きグループはUI環境設定として保存し、追加・削除・グループからの除外に対応。文書データ・フォントデータはRust側にあり、ウインドウはコマンドを送る。UI環境設定は既存の共有オリジンのlocalStorageへ保存し、storageイベントでも更新する。

[Done] Fontdbの共通システム／LiveTypeフォントを使用。PostScript名で具体的なウェイト／イタリックの書体を指定し、描画・ネイティブ編集・SVGの既定フォント解決で同じ書体を選ぶ。CSSの複数ファミリーの優先順も維持する。和文判定は「あ」「漢」、欧文は「A」「z」を収録し、和文に該当しない書体へ絞り込む。文字の言語分類や全字形の保証ではない。未収録のサンプル文字は既存の代替フォント方針で表示することをビューアに明示する。

プレビューはRust workerでSVGを生成し、usvgで実フォントを輪郭化した小さなプレビュー画像をバイナリIPCで返す。フォントファイルや作品のフレームデータをWebViewへ送らない。サンプル128文字、8～96px、幅120～1024px、高さ32～512px、出力512KiB以下。WebViewではBlob画像として表示するため、CSPの画像ソースだけにblob:を許可する。文字列はXMLエスケープし、生成済み画像をimgとして表示する。表示行の仮想化、160msの更新待ち、同時ジョブ2件、24エントリー（最大12MiB）のプレビューキャッシュを使用し、Blob URLはコンポーネント解放時に破棄する。

[Done] 検証：フォントカタログの名前一意性、輪郭プレビュー、入力・出力制限、具体的なスタイルの選択をRustテストで確認。Browser plugin not availableのため、既存Chromeを独立したヘッドレス環境のPlaywrightで検証。実際のRust APIを読み取り専用probeから呼び、Tauri IPCを検証用ページで代替した。メニュー・検索・お気に入り・グループ・成功後の最近使用・選択書体の適用・戻り、幅420pxと280pxでの表示、コンソールエラーなしを確認。これはWKWebViewの実ウインドウを操作したテストではない。スクリーンショットは `/private/tmp/lp-font-viewer-desktop.png` と `/private/tmp/lp-font-viewer-narrow.png`。

[Next] WKWebView実ウインドウでの一連の操作、複雑な文字・カラー／可変フォントの追加検証。
[Later] 合成フォントの作成・編集、Adobeクラウドのフォント管理は別機能として対応する。参照画像の合成／クラウドのボタンを未実装のまま表示しない。

English: Open Font Viewer from the Character panel menu. Search and filter families, inspect actual outlined previews and styles, manage favorites and collections, and apply an exact face to selected text. Unsupported characters use fallback fonts. The font data remains in Rust; only bounded preview images reach the WebView.

简体中文：通过字符面板菜单打开字体查看器。支持搜索、字体分类、真实轮廓预览、样式展开、收藏与分组，并可将具体字体应用到所选文字。缺失字形使用替代字体。字体数据保留在 Rust，仅将有大小限制的预览图传给 WebView。
