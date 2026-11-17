# Codex向け実装指示書：汎用MCP基盤を自作アプリへ実装

## Codexへの最上位指示（この文書をそのまま渡す）

あなたはシニアRust/Tauri 2エンジニアとして、対象リポジトリに**再利用可能な汎用MCPサーバー基盤**を実装してください。最終目的は、ChatGPT、Codex、Claude等の外部MCP対応AIクライアントから、アプリの状態を取得し、許可された操作を直接実行し、結果とエラーを取得できるようにすることです。

**参照すべき設計:** 同じフォルダの `MCP_REUSABLE_ARCHITECTURE_DESIGN.md`。ここにない具体的な実装は最新のMCP仕様と公式RMCP/Tauri 2ドキュメントを確認して決めてください。

最重要条件：MCP基盤はアプリ固有のコードから分離し、**別のTauri 2/RustアプリでもAdapterを差し替えるだけで利用できる**構成にしてください。

## 1. 最初に行うこと

1. リポジトリルートの絶対パス、README、Cargo workspace、`src-tauri`、既存UI、Command Bus、データモデル、保存、Undo/Redo、アプリ状態管理、セキュリティ構成を調査。
2. Gitの現在状態と既存テストの成否を確認。既存の未コミット変更を勝手に消さず、上書きせず、rebase/reset/cleanをしない。
3. 既存機構に沿った実装配置と小さな段階的変更計画を決める。Rust非採用リポジトリなら理由を明記して対応方針を提案し、可能なら汎用crateを独立に実装。
4. SDKは公式の`modelcontextprotocol/rust-sdk`（RMCP）を第一候補とし、実際にインストール可能な安定版とfeature名を照合。2026-07-28系の`server/discover`と2025系の`initialize`が異なることに注意。自前で旧式RPCに固定しない。
5. READMEに仕様サマリー、実装状況、使い方、制限、接続方式を記載。

## 2. 必須アーキテクチャ

アプリ固有部分と独立した`mcp_core`、`mcp_transport`、`mcp_security`、`mcp_audit`を用意し、Tauri 2なら`mcp_tauri`を介して統合してください。Cargo workspace化は既存構成を壊さない範囲で行い、必要なら1crate内部モジュールから開始可能ですが、責務分離と将来の外部crate化を維持してください。

**禁止事項:** GUIにマウスクリック命令を送り込むだけの実装、WebView内だけでMCPを完結する実装、アプリコアの変更ロジックをMCP用に複製する実装、APIキーのハードコード、無制限のshell実行、認証なしで全インターフェースに待受、未実装機能を成功として返すモック。

## 3. AppAdapterの実装

アプリからMCP基盤へ渡すAdapterを用意し、最低限以下を実装する。

- アプリ情報・Capabilities・現在状態・開いている文書の一覧と構造化状態取得。
- Adapterが公開するドメインごとの**型付きOperation**一覧（name, description, input/output JSON schema, required scopes, risk level）。
- `invoke`による既存Command Busの実行と、UIの同期更新。
- 操作結果は適用対象ID、適用前後revision、変更ID、失敗理由、Undo可否を返す。
- 実アプリの操作コマンドがまだ接続できない場合は、**InMemory SampleAdapter**で確実に稼働する最小サンプルを先に実装し、本番のツールを未対応として明示する。

**サンプルで必要な操作:** `sample.list_objects`、`sample.create_object`、`sample.update_object`、`sample.delete_object`（削除は承認あり）、`core.get_document_state`。同じサンプルをstdioとHTTPの両方で操作できるようにする。実アプリではこれらのサンプルツールをそのまま公開せず、Adapterの登録内容へ置き換える。

## 4. MCPツール・スキーマ

- `core.get_app_info`, `core.get_capabilities`, `core.get_context`, `core.list_documents`, `core.get_document_state`, `core.get_operation_help`をアプリが実装可能な範囲で提供。
- 書き込みは汎用の任意コマンドを受け付けるのではなく、Operationごとに個別Toolとして公開し、`tools/list`で正確に説明。
- すべての入力をJSON Schema・値域・対象存在・操作権限で検証する。`additionalProperties: false`を基本にし、必要に応じた例外のみ許可。
- 出力も構造化し、LLM可読メッセージと機械可読の結果を返す。
- Tool annotationsの`readOnlyHint`/`destructiveHint`等は正確に付けるが、認可判定には使用しない。
- 説明に、利用場面、引数の単位・座標系、実行副作用、返却値、失敗時にすべきことを書く。
- 一覧・状態取得ではページングとサイズ上限を設定。巨大ドキュメントを丸ごと返さない。

## 5. 仕様版とTransport

- RMCPで**Streamable HTTP**と**stdio**を両方提供。2026-07-28を優先し、SDKが許す範囲で2025-11-25系のクライアントと相互運用。
- ローカルHTTPは既定`127.0.0.1`のみ、`/mcp`、Host/Origin検証、接続認証、停止処理を持つ。
- stdioのstdoutはプロトコル専用。起動ログ・traceはstderr。GUIアプリが別プロセスで動作するときは、stdioブリッジが既存アプリへのIPC（Unix Socket/Windows Named Pipe等）を使い、重複した編集モデルを作らない。
- 外部のChatGPTへ接続する手順は**Secure MCP Tunnelまたは認証付きHTTPS**をドキュメント化。開発段階で外部ネットワークへの公開を自動で行わない。
- 対象クライアントで接続不能な仕様版があれば調査して互換設定を明示し、MCP仕様を独自の簡易HTTP APIにすり替えない。

## 6. 安全性と操作確認

1. MCP機能は既定OFF。ユーザーが有効化したときのみ起動。
2. ユーザーごと・接続先ごと・文書ごとに`read`/`write`/`delete`等の許可を持つ。
3. 高リスク操作には**アプリ内の独立した承認UI**を実装。AIによる`approved=true`指定だけで通過させない。
4. `expected_revision`による競合防止、冪等性キーによる二重適用防止。切断・タイムアウト時は不明な操作を自動再実行しない。
5. 可能なアプリではpreview/commitを導入し、変更差分と可逆性を示す。Undo/transaction非対応の場合は明示。
6. トークンはOSの安全な資格情報ストアを利用。ログ・設定ファイルへの平文記録禁止。
7. 無条件のCORS許可や任意URL・任意ファイルパス操作を作らない。Origin/Host検証、パストラバーサル、SSRF、過大リクエスト対策をテスト。
8. ツール引数・文書本文・MCP Resourceは信頼できない入力として扱い、そこに書かれた命令で権限を昇格させない。
9. TauriのWebView capability制限とMCPサーバーの認証・認可は**独立**して実装する。

## 7. UI要件（Tauri対象時）

MCP設定画面または既存設定画面内のセクションを追加：有効/無効、接続状況、ローカルポート/方式、許可スコープ、操作承認、監査ログ、トークン再発行、停止、接続テスト。MCP経由の変更はWebViewにすぐ反映。UI操作後の状態もMCPの読み取り結果に反映。ユーザーがアプリを閉じれば待受・Bridgeが停止する。

## 8. 実装順序

- **Step A:** 調査＋動作計画。既存の動作確認、アーキテクチャとファイル一覧を整理。
- **Step B:** `mcp_core`＋InMemory SampleAdapter＋単体テスト。
- **Step C:** RMCP接続（stdioとloopback HTTP）＋MCP Inspectorでの疎通。
- **Step D:** 認証、権限、承認、revision、idempotency、監査、エラーモデル。
- **Step E:** 対象アプリのAppAdapterとCommand Busに接続。GUI更新を確認。
- **Step F:** GUI設定、Docs、サンプル、CI、セキュリティ・結合テスト。

各Stepはテストに合格してから次へ進む。依存関係や既存実装に問題がある場合は代替を採用し、理由と未完了点を正確に記録する。ユーザーに別の作業を依頼せず、実行環境でできる範囲を自律的に完了する。

## 9. 受入基準（Definition of Done）

- ローカルMCP Inspectorからツール発見・状態取得・サンプル編集・エラー検証が成功する。
- Tauri対象なら**別のAIクライアントから既に開いているアプリを操作でき、画面に変更が反映される**。
- UI操作とMCP操作の結果が同じ状態モデルを通して観測できる。
- 権限未付与の変更、重大変更の無承認実行、古いrevision、重複リクエストを適切に拒否。
- stdio/Streamable HTTP両方の起動・停止が動作し、stdout汚染、切断クラッシュ、ポート衝突を防ぐ。
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` が成功、または失敗を詳細報告。
- 追加するRust/TS等の**コードファイルの先頭に、そのファイルの実際のプロジェクトルートを含む絶対パスをコメントで記載**する。JSONなどコメント非対応の構文では無効なコメントを加えず、対応するMarkdown設計書に絶対パス一覧を記録する。
- コードの部分修正時は、変更したディレクトリ・ファイル名・修正箇所を完了報告に記載する。
- 互換性、未実装機能、既知のセキュリティ制約、動作確認環境、実行コマンドを明記する。成功していないものを成功として報告しない。

## 10. 最終報告形式

1. 実装した共通モジュールとアプリ固有Adapterの構成。
2. 変更・追加したファイル（パスを含む）と主な変更箇所。
3. 起動手順・MCP Inspector接続手順・外部AIクライアント接続ガイド。
4. テスト結果（実行コマンド、合格/不合格、未実行）。
5. セキュリティ仕様、既知の制限、今後の拡張点。

**完了とはコードを書き終わったことではなく、外部クライアントがMCPで操作し、その結果がアプリ上で観察できたことを指す。**

## 11. 参考リンク

- https://modelcontextprotocol.io/specification/2026-07-28
- https://github.com/modelcontextprotocol/rust-sdk
- https://github.com/modelcontextprotocol/inspector
- https://developers.openai.com/plugins/deploy/connect-chatgpt
- https://tauri.app/security/capabilities/
