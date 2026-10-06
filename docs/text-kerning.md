# 文字詰め / Kerning / 字偶距

[Done] 文字パネルの文字詰め欄に「メトリクス」「オプティカル」「和文等幅」を追加。旧データの省略値はメトリクス。既存の文字範囲書式経路で適用し、挿入文字の書式・混在表示・Undo/Redo・ネイティブ保存へ接続する。

- メトリクス / Metrics / 度量：フォントのカーニング情報を利用する。macOSはAppKit、ポータブルレイアウトはRustybuzzのGPOS／kern処理を利用する。
- オプティカル / Optical / 光学：グラフェム単位で字形輪郭を取り、ベジェ輪郭を細分化した32帯の外縁間隔から調整する。目標アキは0.08 em、過剰な接近を避けるため調整を±0.25 emに制限する。空白・改行を詰めず、文字サイズ・比率・ベースライン・回転が異なる境界では適用を控える。これはLumaPaint独自の近似計算で、Adobeの非公開処理と同一ではない。
- 和文等幅 / Japanese monospaced / 日文等宽：全角和文・全角約物を1 em送りにする。欧文・半角カナはフォントのメトリクスを維持する。全角空白は元のフォントの空白幅を維持する。

字間（1/1000 em）は文字詰め方式と独立して加算する。macOSではNSTrackingで字間を指定し、カーニングの自動処理を失わないようにする。オプティカル／和文等幅の対象では既存のペアカーニングを差し引いたNSKern補正を適用する。編集後のグリフ・文字位置をSVGへ渡し、画面と書き出しが同じ測定配置を使う。処理はRust内で実行し、WebViewへフォント輪郭を送らない。

[Done] テスト：旧データの既定値、選択範囲だけの変更、測定結果の無効化、保存復元・Undo/Redo、輪郭間隔と補正上限、空白・改行、サロゲートペアのUTF-16位置、字間の加算、横書き・縦書きの配置。macOSのNSKernについてArial「AV」のネイティブ位置を別プロセスで調べ、0指定が自動カーニングを停止し、非0値は既定のカーニングへ加算されることを確認。

[Next] Adobe実出力との比較、複雑な接続文字・可変／カラーフォント・縦中横・ルビを含む組版の追加検証。自動字詰めは手動のペア数値指定を含まない。

English: The Character panel offers Metrics, Optical and Japanese monospaced spacing for selected characters or the insertion style. Tracking remains additive. Optical spacing is LumaPaint's bounded contour-distance approximation; Adobe-identical output is not claimed.

简体中文：字符面板提供度量、光学和日文等宽字偶距，可应用于所选字符或插入样式。字距独立叠加。光学模式采用 LumaPaint 的轮廓距离近似算法，尚未验证与 Adobe 输出完全一致。

参考：[Adobe カーニングと字送り](https://helpx.adobe.com/in/illustrator/desktop/design-with-text/edit-format-text/adjust-kerning-and-tracking.html)、[Apple NSKern](https://developer.apple.com/documentation/appkit/nskernattributename)、[Apple NSTracking](https://developer.apple.com/documentation/appkit/nstrackingattributename)。
