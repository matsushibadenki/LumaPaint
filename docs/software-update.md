# ソフトウェアアップデート

LumaPaintメニューの「アップデートを確認…」からGitHub Releasesの更新を確認する。日本語・英語・简体中文に対応。確認、ダウンロード、署名検証はRust側で行い、WebViewには進捗だけを送る。

新しい版がある場合は、ダウンロード前とインストール／再起動前に確認する。ダウンロード後、Tauriの更新プラグインで署名を検証する。全ウインドウの作品・非表示タブ・共有文書に未保存の変更がある場合、インストールしない。作品を保存してから再実行する。ネットワークエラーやキャンセル後は再試行できる。同時に複数の更新処理は開始しない。

現在の配布対象は既存の0.0.1と同じmacOS Apple Silicon。0.0.1には更新機能がないため、0.0.2を一度手動インストールする必要がある。以降はアプリ内で更新できる。更新パッケージの署名はApple Developer ID署名／公証とは別の仕組み。

## 配布手順

1. Rust workspace、package.json、package-lock.json、tauri.conf.jsonのバージョンを合わせる。
2. 公開鍵をtauri.conf.jsonへ設定する。秘密鍵はGit管理しない。GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`に登録する。
3. `scripts/check-release-version.py`を実行し、変更をコミットしてGitHubへpushする。
4. リリース対象コミットのタグをpushするか、対象ブランチでReleaseワークフローを手動実行する。ReleaseワークフローがmacOS・Windows・Ubuntuの検査後、Macアプリ、DMG、署名付き更新アーカイブ、latest.jsonをドラフトリリースへ作る。
5. ダウンロードしたlatest.jsonを`scripts/check-update-manifest.py`で確認する。配布アプリで更新確認、署名検証、再起動、未保存データの保護を確認してドラフトを公開する。

`latest.json`のURLは`https://github.com/matsushibadenki/LumaPaint/releases/latest/download/latest.json`。署名を検証する公開鍵を初回配布後に変更すると、既存アプリへ通常の更新を届けられなくなるため、秘密鍵の保管を継続する。

## 0.0.2の状態

- 🟢 [Done] 更新処理・三言語メニュー、バージョン0.0.2、署名付き配布ワークフローを実装。
- 🟢 [Done] リリース対象コミット `0f62d94` のmacOS・Windows・Ubuntu CIがすべて成功。[CI結果](https://github.com/matsushibadenki/LumaPaint/actions/runs/41967110799)。
- 🟢 [Done] Macの検証用アプリからローカル配信した署名済み0.0.2をダウンロードし、署名検証・インストール・再起動を確認。更新後のバージョンは0.0.2で、実行ファイルのSHA-256は署名した配布元と一致。改変データでは署名検証エラーを表示し、正規データで再試行できた。
- 🟢 [Done] 未保存の非表示タブと別ウインドウの作品がある場合はインストールを止め、保存後に許可することをRustテストで確認。
- 配布先は[GitHub Releasesのv0.0.2](https://github.com/matsushibadenki/LumaPaint/releases/tag/v0.0.2)。GitHub側の署名付きビルド・配布情報の検査結果は[Release実行履歴](https://github.com/matsushibadenki/LumaPaint/actions/runs/41967177190)を参照。
- 🟢 [Done] 更新署名の公開鍵を組み込み、配布用のGitHub Actions Secretを設定。
- ⭕️ [Pending] Windows／Linux／Intel Macへのバイナリ配布。現状はApple Silicon向けの既存配布を継続する。
