# mop 仕様書

- バージョン: 0.2 (ドラフト確定版)
- 日付: 2026-08-20
- 対象読者: 実装者および AI コーディングエージェント
- この文書は mop v1 の唯一の仕様の正 (source of truth) とする。会話メモやコードコメントより優先する。
- 変更履歴: 0.2 — E2E テスト方針 (§20) を拡張、mop-watch のテスト用抽象化 (§9.5) を追加、CI 要件を明記

---

## 1. 概要

**mop** (master-of-process) は、Ubuntu / Debian 上で動作する Rust 製の常駐デーモン兼 Web アプリケーションである。

1. 指定した systemd ユニット、Docker コンテナ、Docker Compose サービスの状態とログを確認できる。
2. allowlist された対象に限り、start / stop / restart を実行できる。
3. モダンな Web フロントエンド (Vue 3 SPA + PWA) を内蔵し、ローカルに公開する。
4. プラグイン機構により、バックエンド機能と Web UI の両方を拡張できる。
5. 既存ツール manga2cbz の機能は、first-party プラグイン `mop-plugin-manga` として分割・移植する。

### 1.1 非目標 (v1 でやらないこと)

- `docker exec`、任意シェル、任意 `systemctl` の実行
- Docker image の pull / build / push
- 複数ホスト管理 (単一ホスト専用)
- Prometheus 等のメトリクス収集基盤
- ログの長期保存 (mop はログを DB に保存しない)
- 通知機能 (Discord / Slack / メール)。将来プラグインで追加可能にする
- OIDC / OAuth / メール検証。将来の拡張とする
- Cloudflare Access 等、特定リバースプロキシ・IdP への依存
- プラグインのオンラインレジストリ / 自動ダウンロード

---

## 2. 用語

| 用語 | 意味 |
|---|---|
| ホスト | mop 本体プロセス |
| リソース | 監視・操作対象。`systemd_unit` / `docker_container` / `compose_service` / `compose_project` |
| 管理対象 | allowlist または `mop.managed=true` ラベルで mop が管理を許可されたリソース |
| プラグイン | ディレクトリ配置で導入される拡張。backend (別プロセス) と ui (Custom Element) を持てる |
| ジョブ | 非同期の操作・変換単位。`jobs` / `job_events` テーブルと SSE で追跡する |
| Save / Apply | 設定の保存 (ドラフト) と適用 (検証 + 反映) を分離する仕組み |

---

## 3. アーキテクチャ

```text
ブラウザ (Vue 3 SPA / PWA)
   │ HTTPS or HTTP (同一オリジン)
   ▼
mop ホスト (Rust / tokio / axum)
   ├── mop-http      : REST / SSE / 静的 SPA 配信
   ├── mop-auth      : セッション認証・RBAC
   ├── mop-db        : SQLite (sqlx)
   ├── mop-watch     : systemd (D-Bus / zbus), Docker (bollard), journal
   ├── mop-jobs      : ジョブキュー・監査
   └── mop-plugin    : プラグイン supervisor
            │ Unix socket JSON-RPC 2.0
            ▼
        mop-plugin-manga (別プロセス、専用ユーザー)
            └── libarchive / libvips / ffmpeg
```

- ホストは 1 バイナリ。既定 bind は `127.0.0.1:8787`。
- TLS 終端は行わない。nginx / Caddy 等のリバースプロキシが担当する (任意)。
- ホストは root で動かさない。専用システムユーザー `mop`。
- プラグインはホストとは別プロセス・別ユーザー。クラッシュはホストに伝播しない。

---

## 4. リポジトリ構成

```text
mop/
├── Cargo.toml                 # workspace
├── crates/
│   ├── mop-core/              # ドメインモデル、設定、エラー型
│   ├── mop-db/                # sqlx + SQLite、migration、repository
│   ├── mop-auth/              # axum-login 統合、RBAC、登録フロー
│   ├── mop-watch/             # systemd / docker / compose コレクタ (trait + 実装 + fake)
│   ├── mop-jobs/              # ジョブキュー、監査ログ
│   ├── mop-plugin/            # manifest パース、supervisor、JSON-RPC クライアント
│   ├── mop-plugin-sdk/        # プラグイン作者向け SDK (first-party も使用)
│   ├── mop-http/              # axum サーバー、REST / SSE、SPA 配信
│   └── mop-cli/               # バイナリ mop: serve / doctor / backup / restore / plugin
├── plugins/
│   └── manga/                 # mop-plugin-manga (§17)
├── web/                       # Vue 3 + Vite + TypeScript SPA
├── e2e/                       # Playwright E2E (§20.2)
├── deploy/
│   ├── deb/                   # パッケージング
│   ├── mop.service
│   ├── 50-mop.rules           # polkit ルール雛形
│   └── nginx.conf.example
├── .github/workflows/ci.yml
├── docs/
├── SPEC.md                    # 本ファイル
└── AGENTS.md
```

---

## 5. 実行モデルとファイル配置

| パス | 用途 |
|---|---|
| `/usr/bin/mop` | ホストバイナリ |
| `/etc/mop/config.toml` | 設定ファイル |
| `/var/lib/mop/mop.db` | SQLite DB (WAL) |
| `/var/lib/mop/plugins/<id>/<version>/` | プラグイン配置 |
| `/var/lib/mop/backups/` | バックアップ出力 |
| `/run/mop/mop.sock` | 管理ソケット (将来用) |
| `/run/mop/host.sock` | プラグインからのイベント受信ソケット |
| `/run/mop/plugins/<id>.sock` | プラグイン RPC ソケット (0660, mop 所有) |

- systemd `StateDirectory=mop`, `RuntimeDirectory=mop` でディレクトリを作成する。
- ユニットの hardening: `NoNewPrivileges=true`, `ProtectSystem=strict`, `ProtectHome=true`, `PrivateTmp=true`。
- `SupplementaryGroups=systemd-journal` (journal 読取用)。Docker 使用時は rootless socket 優先、不可なら `docker` グループ。

---

## 6. 設定ファイル

`/etc/mop/config.toml`:

```toml
[server]
bind = "127.0.0.1:8787"
# public_url = "https://mop.example.com"   # リバースプロキシ利用時

[database]
path = "/var/lib/mop/mop.db"

[auth]
registration = "first_user"   # first_user | open | closed
min_password_len = 10
session_hours = 12

[resources.systemd]
units = ["caddy.service", "nginx.service"]
allow_actions = ["start", "stop", "restart"]

[resources.docker]
containers = ["komga"]
label_selector = "mop.managed=true"
allow_actions = ["start", "stop", "restart"]

[limits.logs]
history_lines_per_resource = 500
ring_buffer_lines_per_resource = 5000
max_line_bytes = 65536
max_streams_per_user = 5
max_streams_per_instance = 50

[limits.actions]
rate_limit_per_user_per_minute = 10

[plugins]
dir = "/var/lib/mop/plugins"
crash_limit = 5           # この回数
crash_window_secs = 300   # この時間内にクラッシュしたら自動 disable

[backup]
dir = "/var/lib/mop/backups"
```

設定の優先順位: 環境変数 `MOP_*` > config.toml > デフォルト値。

---

## 7. データベース

SQLite を使用。起動時に必ず以下を設定する。

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA synchronous = NORMAL;
```

- ログ本文は保存しない (§11)。
- 書き込みは短いトランザクションに集約する (WAL は単一 writer)。
- マイグレーションは `schema_migrations` テーブルで管理し、冪等・前方のみ。ダウングレードはバックアップからの復元でのみ行う。

### 7.1 テーブル

```sql
CREATE TABLE users (
  id            TEXT PRIMARY KEY,           -- ULID
  username      TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,              -- Argon2id
  role          TEXT NOT NULL CHECK (role IN ('admin','operator','viewer')),
  disabled      INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

-- セッションは tower-sessions のテーブル (sessions) を使用する

CREATE TABLE audit_events (
  id            TEXT PRIMARY KEY,
  ts            TEXT NOT NULL,
  user_id       TEXT,
  username      TEXT,
  action        TEXT NOT NULL,              -- 例: resource.restart, plugin.enable, user.create
  resource_kind TEXT,
  resource_id   TEXT,
  detail_json   TEXT,
  result        TEXT NOT NULL               -- success | failure | denied
);

CREATE TABLE resources (                     -- 発見結果のキャッシュ
  id            TEXT PRIMARY KEY,           -- 例: systemd:caddy.service, docker:komga
  kind          TEXT NOT NULL,              -- systemd_unit | docker_container | compose_service | compose_project
  name          TEXT NOT NULL,
  display_name  TEXT,
  group_name    TEXT,
  source        TEXT NOT NULL,              -- allowlist | label
  labels_json   TEXT,
  first_seen    TEXT NOT NULL,
  last_seen     TEXT NOT NULL
);

CREATE TABLE plugins (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  version       TEXT NOT NULL,
  enabled       INTEGER NOT NULL DEFAULT 0,
  state         TEXT NOT NULL,              -- installed | enabled | running | degraded | disabled
  manifest_json TEXT NOT NULL,
  installed_at  TEXT NOT NULL,
  enabled_at    TEXT
);

CREATE TABLE plugin_permissions (
  plugin_id   TEXT NOT NULL REFERENCES plugins(id),
  capability  TEXT NOT NULL,                -- filesystem_read 等
  value_json  TEXT NOT NULL,
  granted_by  TEXT NOT NULL,
  granted_at  TEXT NOT NULL,
  PRIMARY KEY (plugin_id, capability, value_json)
);

CREATE TABLE plugin_settings (               -- 適用済み設定
  plugin_id  TEXT NOT NULL REFERENCES plugins(id),
  key        TEXT NOT NULL,
  value_json TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (plugin_id, key)
);

CREATE TABLE plugin_settings_draft (         -- Save 済み未 Apply
  plugin_id  TEXT NOT NULL REFERENCES plugins(id),
  key        TEXT NOT NULL,
  value_json TEXT NOT NULL,
  updated_by TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  PRIMARY KEY (plugin_id, key)
);

CREATE TABLE jobs (
  id          TEXT PRIMARY KEY,
  kind        TEXT NOT NULL,                -- resource.action | manga.convert 等
  plugin_id   TEXT,
  status      TEXT NOT NULL,                -- queued | running | succeeded | failed | canceled
  params_json TEXT NOT NULL,
  created_by  TEXT NOT NULL,
  created_at  TEXT NOT NULL,
  started_at  TEXT,
  finished_at TEXT,
  error       TEXT
);

CREATE TABLE job_events (
  job_id   TEXT NOT NULL REFERENCES jobs(id),
  seq      INTEGER NOT NULL,
  ts       TEXT NOT NULL,
  level    TEXT NOT NULL,
  message  TEXT NOT NULL,
  data_json TEXT,
  PRIMARY KEY (job_id, seq)
);

CREATE TABLE app_settings (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL
);
```

---

## 8. 認証・認可

### 8.1 スタック

- `axum-login` + `tower-sessions` (SQLite ストア)
- パスワードハッシュ: `password-auth` (Argon2id)
- Cookie: `HttpOnly; SameSite=Lax`。HTTPS 公開時は `Secure` を付与する
- CSRF: SameSite に加え、状態変更 API は `Origin` / `Referer` ヘッダを検証する
- トークンを localStorage に保存してはならない

### 8.2 登録フロー

- ユーザー 0 人の初期状態では `/setup` のみ有効。最初に登録されたユーザーが `admin` になる
- `registration = "first_user"`: 初回のみ登録可。以後 `/register` は 403
- `registration = "open"`: 誰でも登録可。既定ロール `viewer`
- `registration = "closed"`: 管理者がユーザー管理画面から作成する

### 8.3 ロール

| ロール | 権限 |
|---|---|
| `admin` | 全操作。ユーザー管理、プラグイン導入・有効化、設定 Apply、バックアップ/リストア |
| `operator` | リソース参照、ログ、start/stop/restart、ジョブ実行 |
| `viewer` | リソース参照、ログ閲覧のみ |

認可判定は必ずサーバー側で行う。未認証は `/api/v1/auth/*` と SPA アセット以外 401。

`GET /api/v1/auth/meta` のみ公開し、以下を返す:

```json
{ "needs_setup": false, "registration": "closed", "min_password_len": 10 }
```

---

## 9. リソースモデル

### 9.1 systemd ユニット

- 対象は `config.toml` の静的 allowlist のみ。glob (`*.service`) は v1 では許可しない
- 状態取得・操作は D-Bus (zbus) 経由。`systemctl` をサブプロセス実行してはならない
- 操作権限は polkit で最小化する。`deploy/50-mop.rules` を allowlist から生成し、`org.freedesktop.systemd1.manage-units` を許可された unit 名に限定する
- ログは systemd journal API (例: `journald-query` クレート or sd-journal) から取得

### 9.2 Docker コンテナ

- Docker Engine API (bollard) を使用。`docker` CLI をサブプロセス実行してはならない
- 発見方法は 2 系統:
  - `resources.docker.containers` の名前 allowlist
  - ラベル `mop.managed=true` による自動発見
- 任意ラベル:
  - `mop.display-name`: UI 表示名
  - `mop.group`: UI 上のグルーピング

### 9.3 Docker Compose

- Compose ラベル (`com.docker.compose.project`, `com.docker.compose.service`) から project / service を構築する
- **project / service の restart は `mop.managed=true` のコンテナのみを対象とする**。未管理コンテナ (DB 等) には一切触れない
- 停止は依存順の逆、起動は依存順 (depends_on を best-effort で考慮)

### 9.4 操作の共通モデル

- 操作は `start | stop | restart`
- すべての操作は非同期ジョブとし、API は `202 Accepted` + `job_id` を返す
- API 側で allowlist・ロール・レート制限 (`limits.actions`) を検証する
- UI は対象名を含む確認ダイアログを必須とする
- 実行前後の状態と結果を `audit_events` に記録する

### 9.5 テスト用の抽象化 (必須)

- mop-watch の systemd / Docker アクセスは trait で抽象化し、本番実装 (zbus / bollard) と fake 実装を差し替え可能にする
- E2E および CI では fake backend を注入し、実 systemd なしで画面・API・認可を検証できること
- Docker 依存の E2E はタグで分離し、Docker socket が無い環境では skip できること

---

## 10. ジョブと監査

- ジョブ状態: `queued → running → succeeded | failed | canceled`
- 進捗・ログは `job_events` に追記し、SSE で配信する
- 監査ログは追記専用。更新・削除 API を提供しない
- 監査対象: 認証 (login 成功/失敗)、リソース操作、設定 Apply、プラグイン enable/disable、ユーザー管理、バックアップ/リストア

---

## 11. ログ配信

**mop はログを永続化しない。** systemd は journald、Docker は logging driver を正とする。

- スナップショット: `GET /api/v1/resources/{id}/logs?tail=500&since=<ts>`
- ライブ: `GET /api/v1/resources/{id}/logs/stream` (SSE)
- 再接続時は `since=<最終受信時刻>` で元ソースから再取得する
- ホストはリソースごとに `ring_buffer_lines_per_resource` 行のリングバッファのみ保持
- 1 行は `max_line_bytes` で打ち切る
- 同時ストリーム数は `max_streams_per_user` / `max_streams_per_instance` で制限
- SSE のイベント形式: `data: {"ts":"...","stream":"stdout|stderr|journal","line":"..."}`

---

## 12. HTTP API

ベースパス `/api/v1`。JSON。エラーは `{ "error": { "code": "...", "message": "..." } }`。

| メソッド | パス | 認可 | 説明 |
|---|---|---|---|
| GET | `/auth/meta` | 公開 | セットアップ要否・登録モード |
| POST | `/auth/register` | モード依存 | ユーザー登録 (初回は admin) |
| POST | `/auth/login` | 公開 | ログイン (セッション Cookie) |
| POST | `/auth/logout` | 認証 | ログアウト |
| GET | `/auth/me` | 認証 | 自分の情報 |
| GET | `/users` | admin | ユーザー一覧 |
| POST | `/users` | admin | ユーザー作成 |
| PATCH | `/users/{id}` | admin | ロール変更・無効化・パスワードリセット |
| GET | `/resources` | viewer+ | 一覧。`?kind=` フィルタ |
| GET | `/resources/{id}` | viewer+ | 詳細・現在状態 |
| GET | `/resources/{id}/logs` | viewer+ | ログスナップショット |
| GET | `/resources/{id}/logs/stream` | viewer+ | ログ SSE |
| POST | `/resources/{id}/actions` | operator+ | `{ "action": "restart" }` → 202 + job_id |
| GET | `/jobs` | viewer+ | ジョブ一覧 |
| GET | `/jobs/{id}` | viewer+ | ジョブ詳細 |
| GET | `/jobs/stream` | viewer+ | ジョブ更新 SSE |
| GET | `/events/stream` | viewer+ | グローバルイベント SSE (リソース状態変化等) |
| GET | `/plugins` | viewer+ | プラグイン一覧 |
| POST | `/plugins/{id}/enable` | admin | 有効化 (capability 許可) |
| POST | `/plugins/{id}/disable` | admin | 無効化 |
| GET | `/plugins/{id}/settings` | admin | 適用済み + ドラフト設定 |
| PUT | `/plugins/{id}/settings` | admin | Save (ドラフト保存) |
| POST | `/plugins/{id}/settings/apply` | admin | Apply (検証 + 反映) |
| GET | `/plugins/{id}/settings/diff` | admin | ドラフトと適用済みの差分 |
| ANY | `/plugins/{id}/rpc/*` | プラグイン manifest 依存 | プラグイン RPC プロキシ |
| GET | `/config` | admin | ホスト設定表示 (秘密情報マスク) |
| PUT | `/config` | admin | ホスト設定 Save |
| POST | `/config/apply` | admin | ホスト設定 Apply (要再起動項目は再起動) |
| POST | `/backup` | admin | バックアップ作成 → job |
| GET | `/backups` | admin | バックアップ一覧 |
| POST | `/restore` | admin | リストア (maintenance mode のみ) |
| GET | `/health` | 公開 | 死活監視用 |

---

## 13. プラグインシステム

### 13.1 配置とライフサイクル

```text
/var/lib/mop/plugins/<plugin-id>/<version>/
├── plugin.toml
├── bin/<executable>
└── ui/
    ├── index.js          # ESM。Custom Element を定義する
    └── assets/*
```

状態遷移: `installed` (配置検出) → `enabled` (admin が capability 許可) → `running` (ホストがプロセス起動) → `degraded` (クラッシュ) → `disabled`。

- 有効化まではプロセスを起動しない
- `crash_limit` 回 / `crash_window_secs` 秒のクラッシュで自動 `disabled`
- アンインストール時、プラグインの `plugin_settings` / `plugin_permissions` / 永続データの削除を確認する

### 13.2 マニフェスト (plugin.toml)

```toml
id = "mop.manga"
name = "Manga Conversion"
version = "0.1.0"
api_version = "1"

[backend]
exec = "bin/mop-plugin-manga"

[ui]
entry = "ui/index.js"
element = "mop-plugin-manga"     # Custom Element タグ名
routes = ["/manga"]
nav = { title = "Manga", icon = "book" }

[capabilities]
filesystem_read  = ["/srv/manga/incoming"]
filesystem_write = ["/srv/manga/cbz", "/srv/manga/unknown"]
jobs = ["manga.convert", "manga.batch", "manga.inspect"]
resources_read = ["docker:komga"]
resources_action = []
network = false
```

capability は v1 ではこの 5 種のみ: `filesystem_read` / `filesystem_write` / `jobs` / `resources_read` / `resources_action` / `network`。

### 13.3 RPC (Unix socket JSON-RPC 2.0)

- プラグインは `/run/mop/plugins/<id>.sock` に JSON-RPC サーバーを立てる
- プラグインの stdout/stderr はホストが収集し、`plugin_id` 付きの構造化ログとして扱う (RPC と混ぜない)
- プラグインからホストへのイベントは `/run/mop/host.sock` へ接続して notification を送る

ホスト → プラグイン:

| メソッド | 説明 |
|---|---|
| `initialize` | 起動直後の初期化 (許可 capability、設定を渡す) |
| `describe` | 提供ジョブ種別・UI メタ情報の取得 |
| `config.schema` | 設定スキーマ (JSON Schema) の取得 |
| `config.validate` | ドラフト設定の検証 |
| `config.apply` | 設定の適用 |
| `job.submit` | ジョブ投入。`{ kind, params }` |
| `job.cancel` | ジョブ取消 |
| `doctor` | 依存関係・環境の診断結果 |
| `shutdown` |  graceful 停止 |

プラグイン → ホスト (notification):

| メソッド | 説明 |
|---|---|
| `job.progress` | `{ job_id, percent, message }` |
| `job.log` | `{ job_id, level, message }` |
| `job.finished` | `{ job_id, status, error? }` |

### 13.4 プラグイン UI (Vue Custom Element)

- ホストは `plugin.toml` の `ui.entry` を dynamic import し、指定された Custom Element を描画する
- プラグイン UI がホストの Vue Router / Pinia / 内部コンポーネント / 認証状態を直接 import することは禁止
- ホストから渡すのは以下のみ:

```ts
type MopPluginContext = {
  pluginId: string;
  apiBaseUrl: string;   // /api/v1/plugins/<id>/rpc
  currentUser: { id: string; username: string; role: "admin" | "operator" | "viewer" };
  theme: "light" | "dark" | "system";
};
```

- `context` はプロパティとして渡す。テーマ変更は `mop:theme` カスタムイベントで通知する
- Shadow DOM の使用を推奨する

---

## 14. Save / Apply

設定変更は必ず二段階。

1. **Save**: ドラフトを `plugin_settings_draft` (またはホスト設定のドラフト領域) に保存するだけ。動作は変わらない
2. **Apply**:
   - 差分と影響範囲を表示
   - 検証 (capability、パス重複、依存コマンド、書き込み権限)
   - 成功: 影響プラグインを graceful restart し、ドラフトを適用済みに移動。監査に記録
   - 失敗: 動作設定は一切変更せず、エラーを返す

---

## 15. バックアップ / リストア

`mop backup create` および管理 UI から実行 (ジョブとして実行)。

```text
mop-backup-<timestamp>.tar.zst
├── manifest.json          # mop バージョン、schema バージョン、作成日時
├── database/mop.db        # SQLite backup API で生成 (WAL の単純コピー禁止)
├── config/config.toml     # 秘密情報は除外または暗号化
├── plugins/
│   ├── installed.json     # id / version / enabled
│   └── <id>/
│       ├── plugin.toml
│       ├── settings.json
│       └── data/          # ホストに登録されたプラグイン永続データ
└── checksums.sha256
```

含めないもの: ログ本文、Docker image / volume / Compose ファイル、プラグインの実行ファイルと UI バンドル、セッション、平文の秘密情報。

リストアは maintenance mode のみ:

```text
mop.service stop
mop restore <backup.tar.zst>
  ├─ checksum 検証
  ├─ mop / schema バージョン検証
  ├─ 既存 DB の退避バックアップ
  ├─ DB / config / plugin data 復元
  └─ プラグイン version 不一致の報告 (バイナリは別途インストールが必要)
mop.service start
```

---

## 16. フロントエンド

### 16.1 技術

- Vue 3 + Vite + TypeScript の SPA。SSR は行わない
- ビルド成果物は mop バイナリに埋め込み (rust-embed 等)、同一オリジンで配信
- 状態管理は Pinia。API クライアントは `fetch` + `credentials: 'include'`
- 起動時に `GET /api/v1/auth/me`。401 なら `/login` または `/setup` へ

### 16.2 PWA

- `manifest.webmanifest`: name "mop", `display: standalone`, アイコン (192/512, maskable), theme_color
- Service Worker はアプリシェルのみキャッシュ。**`/api` は絶対にキャッシュしない**
- オフライン時はシェル + 「オフライン」表示のみ。ログ・操作はオンライン必須
- 更新はホストが新しい hashed assets を配信することで行う
- LAN IP での PWA インストールには HTTPS (リバースプロキシ) が必要。localhost は secure context のため不要

### 16.3 画面

| パス | 画面 | ロール |
|---|---|---|
| `/setup` | 初回セットアップ (admin 作成) | 未認証 |
| `/login` | ログイン | 未認証 |
| `/register` | 登録 (モード依存) | 未認証 |
| `/` | ダッシュボード (リソース概要・グループ表示) | viewer+ |
| `/resources/:id` | リソース詳細 (状態・ログ・操作) | viewer+ |
| `/jobs` | ジョブ一覧・詳細 | viewer+ |
| `/plugins` | プラグイン一覧・有効化 | admin |
| `/plugins/:id` | プラグイン設定 (Save/Apply/diff) | admin |
| `/settings` | ホスト設定 | admin |
| `/settings/users` | ユーザー管理 | admin |
| `/settings/backup` | バックアップ/リストア | admin |
| プラグイン定義ルート | プラグイン UI (Custom Element) | manifest 依存 |

---

## 17. mop-plugin-manga (manga2cbz 移植)

既存の manga2cbz スタンドアロンデーモンは退役させ、機能をプラグインへ移植する。

### 17.1 分割

| 移植元モジュール | 移植先 |
|---|---|
| `archive` `convert` `image` `cbz` `inspect` | mop-plugin-manga (ジョブ `manga.convert` / `manga.batch` / `manga.inspect`) |
| `video` (ffmpeg / libx265) | mop-plugin-manga (ジョブ `manga.video`) |
| `daemon` `classify` `dispatch` `worker` | mop-plugin-manga (watch 機能) |
| `config` (変換系) `paths` `error` `logging` 形式 | プラグイン内共通ライブラリ |

### 17.2 設定キー (既存から継承)

`watch_dirs`, `output_dir`, `video_dir`, `unknown_dir`, `work_dir`, `workers`, `webp_quality`, `lossless`, `keep_non_images`, `remove_macos_metadata`, `overwrite`, `images_only`, `dry_run`, `manga_image_threshold`, `delete_original`, `max_input_size_gib`, `max_extracted_size_gib`, `max_file_count`, `reject_symlinks`, `scan_on_start`

### 17.3 ハードリミット (UI や capability で緩和禁止)

- アーカイブ内パスの検証 (絶対パス・`..`・base 外への脱出を拒否)
- symlink 拒否オプション、展開総量・ファイル数上限
- watch_dirs と output_dir の相互包含禁止 (再処理ループ防止)
- 失敗時の一時ディレクトリ完全削除
- 1 ジョブ 1 スレッド (libarchive は !Sync、libvips の VipsImage は !Send)
- libvips の VipsApp をプラグインプロセス寿命と一致させる

### 17.4 依存とパッケージ

libarchive, libvips, ffmpeg (libx265) に依存するため、`mop-plugin-manga` は mop コアとは別の `.deb` とする。

---

## 18. パッケージング

- 対象: Ubuntu 24.04 LTS / Debian 12 以降、`x86_64` / `aarch64`
- 配布: `.deb` を正式。tar.gz は開発用
- パッケージ: `mop` (コア), `mop-plugin-manga` (任意)
- postinst: ユーザー `mop` 作成、ディレクトリ作成、polkit ルール生成、`mop.service` 有効化
- nginx 設定例を `deploy/nginx.conf.example` に同梱 (SSE 用に `proxy_buffering off` と `Upgrade` ヘッダが必須)

---

## 19. セキュリティ不変条件

実装者・エージェントは以下をいかなる理由でも緩和してはならない。

1. mop ホストを root で実行しない
2. `systemctl` / `docker` CLI をサブプロセス実行しない (D-Bus / Engine API のみ)
3. allowlist / `mop.managed=true` 以外のリソースを表示・操作しない
4. ログ本文を DB・ファイルに永続化しない
5. プラグインはホストと別プロセス・別ユーザーで実行する
6. プラグインの Unix socket は `0660` + 所有者制限
7. バックアップに平文の秘密情報を含めない
8. §17.3 のハードリミットを UI・設定・capability で無効化できない
9. 認可判定は必ずサーバー側で行う
10. 状態変更 API は Origin チェックとレート制限を必須とする

---

## 20. テスト要件

### 20.1 単体・統合テスト (Rust)

- 各 crate に単体テスト。パス安全性・認可・マニフェスト検証は必須
- `tests/` に API 統合テスト: 実バイナリ + fake backend (§9.5) で以下を検証
  - 初回セットアップ → admin 作成 → ログイン → リソース一覧
  - allowlist 外リソースへの操作が 403/404 になること
  - restart ジョブが監査に記録されること
  - プラグインのクラッシュがホストを落とさないこと
  - manga プラグイン: ZIP → WebP CBZ 生成、パストラバーサル拒否、失敗時に出力を残さない

### 20.2 E2E テスト (Playwright)

フロントエンドのテストは 2 層とする。

**層 1: 恒久テスト (`e2e/`、リポジトリに残す)**

- `@playwright/test` を使用。`e2e/playwright.config.ts` に設定
- 対象は **実バイナリ + 実 SPA**: web をビルドし、mop をテスト用設定 (一時 DB、fake backend、固定ポート `127.0.0.1:18999`) で起動してからテストする。`webServer` 設定で自動起動し、`baseURL` は同一オリジンを使う
- DB は実行ごとに新規作成し、シードは DB 直接操作ではなく API (setup / register) 経由で行う
- 最低限カバーするシナリオ:
  - 初回セットアップ画面で admin を作成できる
  - ログイン / ログアウト
  - ダッシュボードにリソース (fake) が表示される
  - リソース詳細でログが表示される
  - viewer ロールには操作ボタンが出ない (出ても API が拒否する)
  - PWA manifest が配信される
  - プラグイン画面 (M4 以降)、manga 画面 (M5 以降)
- 画面を追加・変更したら、対応する E2E を同じ PR で追加・更新する

**層 2: 開発中の目視確認 (Playwright MCP)**

- エージェントは UI 実装後、Playwright MCP で実際にアプリを開き、主要フローを操作してスクリーンショットで確認する
- Walkthrough に主要画面のスクリーンショットを添付する
- MCP による確認は層 1 のテストコードの代替にしない (必ず両方)

### 20.3 CI (GitHub Actions)

`.github/workflows/ci.yml` で PR ごとに以下を実行する。

- `cargo fmt --check`
- `cargo clippy --workspace -- -D warnings`
- `cargo test --workspace`
- web: `pnpm typecheck` / `pnpm build`
- e2e: Playwright (fake backend シナリオは必須、Docker 依存シナリオは runner に Docker がある場合のみ)

---

## 21. マイルストーン

| # | 内容 | 完了条件 |
|---|---|---|
| M1 | コア骨格 | config / DB migration / 認証・登録 / ユーザー管理 / SPA シェル / PWA manifest / `/health` / .deb 雛形 / **E2E ハーネス (setup・login シナリオ)** / CI 雛形 |
| M2 | リソース監視・操作 | systemd 状態+journal、Docker 状態+logs、SSE、allowlist、操作ジョブ+監査、polkit ルール、fake backend、**ダッシュボード・ログ画面の E2E** |
| M3 | Compose | project / service 一覧、管理対象のみ restart、E2E 追加 |
| M4 | プラグイン基盤 | manifest、supervisor、Unix socket RPC、UI Custom Element ローダー、hello プラグイン、E2E 追加 |
| M5 | manga 移植 | mop-plugin-manga (convert / batch / inspect / video / watch)、設定 Save/Apply、別 .deb、E2E 追加 |
| M6 | 仕上げ | バックアップ/リストア、hardening、E2E 完備、ドキュメント |

各マイルストーンは独立して動作確認可能な状態で完了させること。完了時にタグ `v0.1.0-m<N>` を打つ。
