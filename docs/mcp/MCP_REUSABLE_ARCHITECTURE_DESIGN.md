# 汎用MCP機能 設計仕様書

**版:** 1.0 / **基準日:** 2026-11-13  
**対象:** Tauri 2 + Rustを主用途とし、Rustネイティブ・Webサービス・GUIアプリへ再利用できるMCPサーバー基盤  
**プロトコル:** MCP 2026-07-28（優先）、2025-11-25系との相互運用（SDKがサポートする範囲）

## 0. 目的と達成像

ユーザーが外部AI（ChatGPT、Codex、Claude、MCP対応LLMクライアント等）に自然言語で指示したとき、AIが自作アプリの機能を発見し、現在状態を取得し、適切な操作を実行して、結果を確認できるようにする。

MCP通信・接続・認証・操作許可・イベント・エラー・監査・テストは**共有ライブラリ**として実装し、アプリ固有の操作は**アダプター**として追加する。AIにGUIの座標クリックを原則として強要しない。AIと人間の操作は同じ内部コマンドエンジンを使用する。

**非目標:** このMCP基盤自体にLLM推論機能を組み込むこと、全OSの画面操作を無制限に許可すること、任意shellコマンドをAIへ公開すること、何も実装していないアプリ機能を自動生成できると主張すること。

## 1. アーキテクチャ

```text
# /ABSOLUTE_PROJECT_ROOT/docs/architecture-overview.txt (設計図の仮想配置)
外部MCPクライアント / AIホスト
           |
  [stdio または Streamable HTTP]
           |
+----------v-----------------------------+
| mcp_transport（rmcp）                   |
| 仕様版対応 / discovery / tools/list      |
+----------+-----------------------------+
           |
+----------v-----------------------------+
| mcp_security                             |
| 認証 / scope / allowlist / 承認 / 制限    |
+----------+-----------------------------+
           |
+----------v-----------------------------+
| mcp_core                                |
| ToolRegistry / Schema / State / Audit    |
| 実行制御 / revision / idempotency        |
+----------+-----------------------------+
           |
+----------v-----------------------------+
| AppAdapter（アプリごとに実装）             |
| operation定義 / ドメインID / 状態取得      |
+----------+-----------------------------+
           |
+----------v-----------------------------+
| 既存のApplication Core / Command Bus     |
| Document/SceneGraph/Simulator/Undo       |
+----------------------------------------+
           |
      UI再描画・イベント通知
```

### 1.1 モジュール責務

| モジュール | 役割 | アプリ依存 |
|---|---|---|
| `mcp_core` | ToolRegistry、Adapter契約、実行結果、状態リビジョン、メタデータ、ページング | なし |
| `mcp_transport` | RMCPラップ、stdio、Streamable HTTP、仕様版差異の吸収 | なし |
| `mcp_security` | 認証、権限、承認、Origin/Host、レート制限、データ秘匿 | なし |
| `mcp_audit` | 呼び出し履歴・変更の追跡、シークレット除外 | なし |
| `mcp_tauri` | Tauriライフサイクル、AppState、イベント、権限UIとの接続 | Tauriのみ |
| `sample_adapter` | メモリ上のサンプル文書・オブジェクトの読み書き | テスト用 |
| `your_app_adapter` | 実際の編集コマンドとOperationを接続 | 各アプリ固有 |

## 2. 現行プロトコル準拠

- Rust公式SDK **RMCP** の安定リリースを使用。依存バージョンをCargo.lockに固定し、APIを最新公式サンプルで照合する。
- 2026-07-28版では**リクエスト単位のメタデータを使うステートレス**な仕様が基本。`server/discover`、`tools/list`、`tools/call`等をSDKで実装する。旧2025系は`initialize`等の方式が異なるため、**同じ初期化手順をハードコードしない**。
- 2026版の必要な `_meta`、ヘッダー等はSDKで正しくエンコード・検証する。独自JSON-RPCの生実装は避ける。
- ツールの列挙順序を安定化し、一覧メタデータにバージョンを付ける。変更検知・キャッシュ・通知はSDKと接続先が対応するとき利用する。
- `Resources` / `Prompts` は任意。**重要な状態取得は必ずToolでも提供**する。接続先AIによる対応差を吸収する。
- 長時間処理、購読、MCP Apps UI、拡張Taskは第2段階以降。拡張機能はクライアント能力を確認してから利用する。

## 3. 接続・起動モード

### 3.1 Local Streamable HTTP（デスクトップ推奨）

- デフォルト `127.0.0.1`（必要なら`::1`）のみで待受。`0.0.0.0`禁止を既定にする。
- アプリ起動中に`/mcp`のエンドポイントを公開し、同じアプリプロセスのDocument Modelへ接続する。
- 動的ポートまたは設定ポート。起動時にポート衝突を検出し、安全な失敗/再選択ができる。
- `Origin`は許可済みOrigin以外を403（Originが付かない正常なCLIクライアントはOrigin欠落のみを理由に拒否しない）。`Host`もallowlist検証。DNS rebinding対策を実装。
- localhostでも認証トークン／明示的ペアリングで編集権限を制限する。トークンはOS資格情報ストアへ保存し、設定画面に平文で表示・ログ出力しない。
- TauriのWebViewにローカルHTTPサーバーの認可境界を委ねない。HTTP側で独立検証する。

### 3.2 stdio（ローカルクライアント用）

- AIクライアントが専用MCPサーバー子プロセスを起動する方式。
- **既に起動しているGUIアプリを操作する場合:** 子プロセス内で文書を別に開くのではなく、専用のIPC Bridgeを介してGUIアプリの唯一のCommand Busへ接続する。macOS/LinuxならUnix Domain Socket、WindowsならNamed Pipe等を使用し、OSユーザー確認・認証・適切なアクセス権を適用。
- `stdout`はMCPメッセージ専用。ログ・デバッグ情報は`stderr`へ。
- IPC Bridgeが未接続の場合は構造化エラーを返し、勝手に二重インスタンスを生成しない。

### 3.3 ChatGPT等のリモート接続

- ChatGPT接続には、対応する**Secure MCP Tunnel**または、認証付きの公開**HTTPS Streamable HTTP**を用いる。クライアントのプラン・ワークスペース・ツール権限の制約を仕様書に明記する。
- リモート公開は初期設定で無効。公開する場合は正式な認証・スコープ・監査・レート制限・TLS・テナント／インスタンス境界を追加。
- クラウド側を公開する場合でも、デスクトップアプリへの通信は明示的に許可したトンネル／ゲートウェイを経由させ、外部からPCへ任意到達可能なポートを開かない。
- OAuthの発行/検証、issuer/audience/scope、token更新、利用者の同意は対象クライアントと展開方式に合わせて公式仕様に従う。単純な共通シークレットは正式な公開マルチユーザー環境の代替にしない。

## 4. AppAdapter契約（最重要）

MCPサーバー内にアプリ固有の操作ロジックを持ち込まない。Adapterは**既存の内部操作API**に変換するだけとする。

- `app_info()` : 名称、バージョン、機能群、ドキュメント説明を返す。
- `context(principal)` : 開いている文書ID、選択ID、アクティブビュー、revision、利用可能範囲を返す。最小権限で情報を削減。
- `operations()` : Toolとして公開可能な個々の操作定義（名前、説明、JSON Schema、権限、危険度、読み取り専用か、冪等性、Undo可能性、バージョン）を返す。
- `invoke(context, operation, arguments)` : 正規のCommand Busを呼び、構造化結果を返す。
- `snapshot(document_id, options)` : 取得数制限・ページング付きで現在状態を返す。
- `subscribe()` : アプリ内の変更イベントを提供する。第1段階では内部イベントのみでもよい。
- `preview()` / `commit()` : 対応アプリでは変更前検証と確定を分離。非対応の場合はサポートしない旨を明示。

Adapterの入出力にGUIのDOM、マウスイベント、WebView限定オブジェクトを含めない。

### 4.1 契約のRustスケッチ

以下は実装の構造を伝えるための**参考インターフェース**であり、CodexはプロジェクトとSDKの実際の型に合わせてコンパイル可能なものにする。

```rust
// /ABSOLUTE_PROJECT_ROOT/crates/mcp_core/src/adapter.rs
use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait AppAdapter: Send + Sync {
    async fn app_info(&self) -> Result<AppInfo, AdapterError>;
    async fn context(&self, principal: &Principal) -> Result<AppContext, AdapterError>;
    fn operations(&self) -> Vec<OperationDescriptor>;
    async fn invoke(
        &self,
        context: OperationContext,
        operation: &str,
        arguments: Value,
    ) -> Result<OperationResult, AdapterError>;
}
// AppInfo, AdapterError, Principal, AppContext, OperationContext,
// OperationDescriptor, OperationResult は mcp_core 側で定義する。
```

**Command Busは唯一の変更経路**とする。UIとMCPが別々にScene Graphを変更する設計は禁止。

## 5. Tool公開方式

### 5.1 共通（すべてのアプリで可能な範囲で提供）

| ツール | 機能 | 権限 |
|---|---|---|
| `core.get_app_info` | アプリ名、版、説明、操作モデル | `app.read` |
| `core.get_capabilities` | 対応機能、ドキュメント、制限、拡張 | `app.read` |
| `core.get_context` | 現在開いている文書・選択・revision | `document.read` |
| `core.list_documents` | 許可された文書一覧 | `document.read` |
| `core.get_document_state` | 範囲指定で文書の構造化スナップショット | `document.read` |
| `core.get_operation_help` | 操作と引数の詳細説明、使用例 | `app.read` |
| `core.get_change_history` | 限定範囲の監査・Undo履歴 | `document.read` |
| `core.preview_changes` | 提案変更の検証・影響範囲・プレビュー | `document.write` |
| `core.commit_changes` | 有効なプレビューの確定（承認を再確認） | `document.write` |
| `core.undo` | 可能な場合のみ操作取消 | `document.write` |

**ツールの公開範囲は認可によって変化する。** 機能が未実装なら虚偽のツールを公開しない。共通Tool名は例であり、実際には識別子衝突をテストする。

### 5.2 ドメイン専用操作

- 例：`graphics.create_shape`、`graphics.move_shape`、`circuit.create_node`、`circuit.connect_ports`、`audio.set_parameter`、`document.insert_text`。
- ドメインツールは各Adapterから**型付きJSON Schema**とともに登録する。クライアントが個別機能を発見しやすくする。
- 巨大な機能群はカテゴリ、read/writeを分け、必要に応じてページング・段階的開示を行う。ただし動的なtools/listが使えないクライアントでも基本Toolで最低限操作できる設計とする。
- **禁止例:** `execute_shell(command)`, `eval_js(source)`, `write_any_file(path, data)`を汎用機能として無制限公開すること。

### 5.3 OperationDescriptor の必要項目

`name`, `title`, `description`, `input_schema`, `output_schema`, `required_scopes`, `risk_level`, `read_only`, `destructive`, `idempotent`, `supports_undo`, `supports_preview`, `version`, `examples`。

Tool annotationsはUIやAIのための**ヒント**であり、セキュリティ判定は実行側のPolicy Engineで強制する。

## 6. 状態・同時編集・ID

- 各Documentに一意の`document_id`と単調増加する`revision`を持たせる。
- 編集APIでは`expected_revision`を受け、現在状態と違う場合は `REVISION_CONFLICT` を返す。自動上書きしない。
- ノード・図形・トラックなどは内部で永続的かつ推測困難なIDを持つ。画面上の座標や表示名を識別子の代用にしない。
- `get_document_state`は全文取得を強制せず、`cursor`, `limit`, `filter`, `since_revision`等を利用できる。
- 明示的なスコープを維持し、**現在アクティブなウィンドウ＝操作権限**という暗黙の前提を置かない。
- 2026-07-28版はプロトコルにセッション状態を持たない。長い作業の追跡にはアプリが発行する明示的な`operation_id`/`preview_token`等を使用する。
- メモリ上の変更と永続保存を区別する。`save`は独立した操作とし、勝手にディスクへ保存しない。

## 7. 操作トランザクション

推奨フロー：`read state → propose/preview → policy + approval → commit → verify`。

**preview:** 対象文書、`expected_revision`、operation一覧、予想差分、リスク、必要権限、可逆性を評価し、期限付き`preview_token`を返す。プレビュー自体は副作用を起こさない。

**commit:** 呼び出し側のID、対象、トークン署名、プレビュー時のrevision、実行権限、必要ならユーザー承認を検証し、Command Busに渡す。再利用されたcommitキーは二重適用しない。

**結果:** `operation_id`, `status`, `document_id`, `revision_before`, `revision_after`, `changed_object_ids`, `warnings`, `undo_token`（取得可能なとき）, `duration_ms`, `is_error`。

**重要:** すべてのアプリで原子的なトランザクションやUndoが可能とは限らない。その場合は明示的に`atomic=false`/`undo_supported=false`とし、部分成功・復旧手順を返す。処理中のキャンセルが既に適用済みの変更を必ず取り消すとは約束しない。

## 8. 認証・権限・承認

### 8.1 スコープ例

`app.read`, `document.read`, `document.write`, `document.delete`, `filesystem.export`, `simulation.run`, `network.access`。

各Toolに必要スコープ・対象文書・ユーザID・操作範囲・危険度を付ける。認証は通信レイヤー、**認可は毎回サーバー側**で評価する。

### 8.2 承認ルール

| リスク | 例 | 推奨動作 |
|---|---|---|
| 低 | 状態取得、表示中オブジェクト取得 | 許可済みスコープなら実行 |
| 中 | ノード配置、パラメータ変更 | 設定に基づく承認／限定自動実行、Undo付き |
| 高 | 大量変更、上書き保存、エクスポート | 差分提示と明示的承認 |
| 重大 | 完全削除、外部送信、秘密情報アクセス | 個別承認と強い権限制限。標準では無効 |

AIがTool引数に`confirmed: true`を書いただけで承認済みと扱ってはいけない。**信頼できるアプリUIまたは認証されたクライアントの承認経路**で確認する。

### 8.3 攻撃面への対策

- プロンプトインジェクション、Tool Poisoning、Tool Shadowing、データ持ち出し、Confused Deputyを脅威モデルに含める。
- 文書本文、外部リソース、ツール返却文字列は**非信頼データ**。そこに書かれた命令で権限・承認・接続先を変更しない。
- パス正規化、対象ディレクトリ境界、外部URL allowlist、SSRF対策、過大JSON・画像・ループ・DoS対策を行う。
- 本番での秘密鍵・トークンのログ記録禁止。マスク、監査ログ保持期間設定、削除機能。
- ローカルHTTPに対してOrigin/Host検証、loopbackバインド、認証を適用。CORSを無条件に`*`としない。
- Client側がTool annotationsを信用するとは限らない。サーバー側のPolicy Engineが最終判定する。

## 9. エラーモデル・観測性

単なる`false`ではなく、LLMが復旧に使える構造化エラーを返す。

| code | 典型原因 | recovery_hint |
|---|---|---|
| `INVALID_ARGUMENT` | 型・値・座標範囲エラー | 該当フィールドと許容値を示す |
| `DOCUMENT_NOT_FOUND` | 文書IDが存在しない/非公開 | 文書一覧を再取得 |
| `OBJECT_NOT_FOUND` | 対象IDがない | stateを再取得 |
| `REVISION_CONFLICT` | 他操作により状態変更 | 再読取して再計画 |
| `PERMISSION_DENIED` | スコープ不足 | ユーザーへ権限申請 |
| `APPROVAL_REQUIRED` | 確認が必要 | 信頼できるUIへ承認を要求 |
| `NOT_SUPPORTED` | 機能未実装 | capabilitiesで代替を確認 |
| `APP_NOT_CONNECTED` | stdio Bridge先が未起動 | 対象アプリ起動を案内 |
| `TIMEOUT_OR_UNKNOWN` | 応答遅延・結果不確実 | operation_idで状態照会、安易に再送しない |

結果には必ず人間可読の短い説明と機械可読の詳細を付ける。JSON-RPC上のエラーとTool実行失敗（`isError`）を区別する。ログはrequest_id, operation_id, principal, tool_name, duration, outcome, revisionのみを既定保存。機密の引数全文を既定保存しない。

## 10. GUI連携・同期

- ツールを使った変更は内部Command Busで実行し、ドキュメントモデル変更イベントをWebViewへ発行して表示を更新する。
- UIスレッドを長時間ブロックしない。スレッド安全な状態共有（Tokio/チャネルなど）を行う。
- Tauriの`capabilities`はWebViewからRustへのアクセス制限であり、MCP HTTP/stdioからの許可を代わりに実施するものではない。MCP独自の認証・権限処理が必須。
- スクリーンショットは任意の`view.get_screenshot`ツールとし、同意・マスキング・画素上限を持たせる。画像がなくても操作できる構造化状態APIを基本とする。
- ウィンドウを閉じた、文書が切り替わった、アプリ終了時のリソース解放や未完了操作の状態を定義する。

## 11. ユーザー向けMCP設定画面

必須：MCP有効/無効、待受モード・状態、接続クライアント一覧、読み取り/編集権限、承認ポップアップ、接続解除、操作ログ閲覧、接続テスト、トークン再発行。デフォルトは**OFF**、ローカル接続を推奨しリモート公開は明示的選択時のみ。

任意：特定のドキュメントのみ公開、危険操作の恒久無効化、スクリーンショット提供可否、監査ログ保存期間、AIによる一連の編集のプレビューダイアログ。

## 12. 具体的なディレクトリ案

Codexは既存リポジトリ構成を最初に確認して配置を決める。下記はCargo workspaceを採用できるときの標準案。

```text
# /ABSOLUTE_PROJECT_ROOT/ （以下は相対構造の例）
crates/
  mcp_core/src/{lib,adapter,registry,schema,context,result,revision}.rs
  mcp_transport/src/{lib,stdio,http}.rs
  mcp_security/src/{lib,auth,policy,approval,origin}.rs
  mcp_audit/src/{lib,events,redaction}.rs
  mcp_tauri/src/{lib,lifecycle,bridge,events}.rs
  sample_adapter/src/{lib,model,operations}.rs
src-tauri/
  src/mcp_adapter.rs
  capabilities/...
docs/
  mcp/README.md
  mcp/ARCHITECTURE.md
  mcp/SECURITY.md
  mcp/CLIENT_SETUP.md
  mcp/TOOL_CATALOG.md
  mcp/TEST_REPORT.md
tests/
  mcp_integration/...
```

外部リポジトリとして公開予定なら`mcp_core`群を独立Workspaceとし、Tauri統合はfeatureまたは別crateに分ける。既存リポジトリに重複する基盤があれば二重実装を避ける。

## 13. 最小実装（MVP）

**Phase 0 — 事前調査:** リポジトリ構造・コマンド経路・既存Undo・依存・OS対応を調べ、実際の編集経路を特定。

**Phase 1 — Core:** AppAdapter、ToolRegistry、Schema検証、基本状態取得、構造化エラー、SampleAdapterで独立起動。

**Phase 2 — Transport:** RMCPでstdioとloopback Streamable HTTP、仕様版互換、Inspector接続、stdio stdout厳格性。

**Phase 3 — Security:** Scope、承認、Host/Origin、認証、rate limit、Audit、secret redaction。

**Phase 4 — Tauri:** AppState、Command Bus、UI同期、設定画面、起動停止、選択ドキュメント制限。

**Phase 5 — 実アプリ連携:** 状態取得と読み取り操作→簡単な書き込み操作→Undo/Preview/Verifyの順に段階導入。

**Phase 6 — Advanced（別フェーズ）:** 外部公開認証、Tunnel、購読、長時間Task、MCP Apps UI、複数アプリの統合管理。

## 14. テスト計画・合格条件

最低限、以下をCIで自動化する。

1. 最新版MCPの`server/discover`・`tools/list`・`tools/call`がInspectorから成功。
2. 旧仕様クライアントがSDKのサポート範囲で接続できる。
3. 同じOperationをstdio/HTTPで呼んで同一の意味的結果を得る。
4. schema不正・余分な引数・境界値・不存在IDを安全に拒否。
5. 権限不足と承認なしの書き込みをサーバー側で拒否。
6. `Origin`不正、`Host`不正、任意の外部bind、認証不正を拒否。
7. 関係ない外部文書やテナントのIDへアクセス不可。
8. revision競合で上書きせず、取得・再試行が可能。
9. 同じidempotency keyを再送しても二重作成なし。
10. プレビュー後の状態変更で古いpreview tokenが拒否。
11. Undo対象が有効なら取り消して元の状態へ復帰。
12. 実操作がUIに反映され、UI操作がstate取得に反映。
13. 同時編集と大量オブジェクトでデータ競合・デッドロックがない。
14. stdioのstdoutにログなどの余計な文字が混じらない。
15. GUIアプリ未起動でstdio Bridgeが明確なエラーを返す。
16. 切断・再接続後も実際の文書状態が維持される。
17. エラー時にトークン・パスワード・秘密情報がログへ漏れない。
18. 大きい返却値はページング・切り詰め・サイズ制限で制御。
19. 停止時にlistener/IPC/未完了処理が正しく終了。
20. macOSで実機疎通、対応範囲のWindows/LinuxでビルドまたはCIを実施。

`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、MCP Inspectorスモークテストを合格条件とする。環境上実行できないテストは未実施として明記し、成功したと偽らない。

## 15. Codexの最終納品物

- 動く共通MCPライブラリ、SampleAdapter、Tauri統合アダプター（対象既存アプリがある場合）
- Toolカタログ、スキーマ、セキュリティポリシー、設定画面、接続手順
- stdioとHTTPそれぞれの起動例・テスト、Inspector実行方法
- 実行したコマンド、成否、未実装事項、既存アプリへの変更箇所
- アプリに追加するToolを実装する具体的な手順（テンプレート）

## 16. 原則

- 安全な既定値：MCPはOFF、外部公開なし、書き込みは明示的許可。
- 「任意操作をAIへ全面委任」ではなく、**明示的に公開した型付き機能をAIが選択・実行**する。
- エラーを曖昧にしない。アプリ状態の推測を避ける。
- UIのクリック模倣は構造化APIで実現できない例外的な機能のみに限定する。
- 新しいアプリではAdapterだけ追加すれば共通MCP通信・認証・監査が再利用できる。

## 17. 公式参照資料（2026-11-13確認）

- MCP 2026-07-28仕様: https://modelcontextprotocol.io/specification/2026-07-28
- MCP Tools: https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/tools.mdx
- MCP Discovery: https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/discover.mdx
- Rust SDK RMCP: https://github.com/modelcontextprotocol/rust-sdk
- MCP Inspector: https://github.com/modelcontextprotocol/inspector
- OpenAI MCP接続: https://developers.openai.com/plugins/deploy/connect-chatgpt
- OpenAI API MCP: https://developers.openai.com/api/docs/guides/tools-connectors-mcp
- Tauri capabilities: https://tauri.app/security/capabilities/
- OWASP MCP Security: https://cheatsheetseries.owasp.org/cheatsheets/MCP_Security_Cheat_Sheet.html
