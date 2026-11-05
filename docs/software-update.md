# ソフトウェアアップデート

LumaPaintメニューの「アップデートを確認…」からGitHub Releasesの更新を確認する。日本語・英語・简体中文に対応。確認、ダウンロード、署名検証はRust側で行い、WebViewには進捗だけを送る。

新しい版がある場合は、ダウンロード前とインストール／再起動前に確認する。ダウンロード後、Tauriの更新プラグインで署名を検証する。全ウインドウの作品・非表示タブ・共有文書に未保存の変更がある場合、インストールしない。作品を保存してから再実行する。ネットワークエラーやキャンセル後は再試行できる。同時に複数の更新処理は開始しない。

現在の配布対象は既存の0.0.1と同じmacOS Apple Silicon。0.0.1には更新機能がないため、0.0.2を一度手動インストールする必要がある。以降はアプリ内で更新できる。更新パッケージの署名はApple Developer ID署名／公証とは別の仕組み。

## 配布手順

1. Rust workspace、package.json、package-lock.json、tauri.conf.jsonのバージョンを合わせる。
2. 公開鍵をtauri.conf.jsonへ設定する。秘密鍵はGit管理しない。GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`に登録する。
3. `scripts/check-release-version.py`を実行し、変更をコミットしてGitHubへpushする。
4. `v0.0.2`のようなタグをpushする。ReleaseワークフローがmacOS・Windows・Ubuntuの検査後、Macアプリ、DMG、署名付き更新アーカイブ、latest.jsonをドラフトリリースへ作る。
5. ダウンロードしたlatest.jsonを`scripts/check-update-manifest.py`で確認する。配布アプリで更新確認、署名検証、再起動、未保存データの保護を確認してドラフトを公開する。

`latest.json`のURLは`https://github.com/matsushibadenki/LumaPaint/releases/latest/download/latest.json`。署名を検証する公開鍵を初回配布後に変更すると、既存アプリへ通常の更新を届けられなくなるため、秘密鍵の保管を継続する。

## 0.0.2の状態

- 🟢 [Done] 更新処理・三言語メニュー、バージョン0.0.2、署名付き配布ワークフローを実装。
- 🟠 [Next] 署名付きでビルドし、実アプリとGitHub Actionsを検証してリリース公開。
- 🟢 [Done] 更新署名の公開鍵を組み込み、配布用のGitHub Actions Secretを設定。
- ⭕️ [Pending] Windows／Linux／Intel Macへのバイナリ配布。現状はApple Silicon向けの既存配布を継続する。
