# mop デプロイメント・運用ガイド (Deployment and Operations)

本文書は、**mop** (master-of-process) デーモンおよび各種プラグインのインストール、初期設定、バックアップ・リストア、アップグレード、およびアンインストール手順をまとめた運用マニュアルです。

---

## 1. 前提要件とセキュリティ設計

### システム要件
- **OS**: Ubuntu 24.04 LTS / Debian 12 (Bookworm) 以降 (x86_64, aarch64)
- **Init / Service**: systemd (v245 以降推奨)
- **権限管理**: PolicyKit (polkitd / policykit-1)
- **コンテナ (任意)**: Docker Engine (bollard API 経由)

### セキュリティ不変条件 (SPEC §19)
- mop デーモンは **root で動作しません**。専用システムユーザー `mop` (グループ `mop`) で実行されます。
- `systemctl` や `docker` CLI のサブプロセス実行は行わず、D-Bus (`zbus`) および Docker Engine API (`bollard`) 経由でのみ制御します。
- 各プラグインは別プロセス・別システムユーザー (`mop-plugin-manga`, `mop-plugin-video` 等) として起動され、共有グループ `mop-ipc` (パーミッション `2770`) を介して Unix socket 通信を行います。

---

## 2. インストール手順

### A. Debian / Ubuntu パッケージ (.deb) によるインストール (推奨)

配布されている `.deb` パッケージを使用してインストールします。

```bash
# 依存パッケージのインストール (Ubuntu/Debian)
sudo apt-get update
sudo apt-get install -y systemd policykit-1 libsqlite3-0

# mop 本体および推奨プラグインのインストール
sudo dpkg -i mop_0.1.0_amd64.deb \
            mop-plugin-manga_0.1.0_amd64.deb \
            mop-plugin-video_0.1.0_amd64.deb

# 依存関係が不足している場合の修復
sudo apt-get install -f
```

インストール時に以下が自動実行されます:
1. システムグループ `mop`, `mop-ipc` の作成
2. システムユーザー `mop`, `mop-plugin-manga`, `mop-plugin-video` の作成
3. `mop` ユーザーへの `mop-ipc` および `systemd-journal` グループの付与
4. ディレクトリ `/var/lib/mop` (0750), `/etc/mop` の作成
5. 初回設定ファイル `/etc/mop/config.toml` の生成
6. polkit 認可ルール `/etc/polkit-1/rules.d/50-mop.rules` の生成
7. systemd サービス `mop.service` の登録と自動起動有効化

### B. スタンドアロン tarball による手動インストール

パッケージマネージャを使用しない環境では、スタンドアロン tarball を利用します。

```bash
tar -xzf mop-0.1.0-linux-x86_64.tar.gz
cd mop-0.1.0-linux-x86_64

# インストーラを実行 (root 権限)
sudo ./install.sh
```

---

## 3. 初期セットアップと管理者アカウント登録

### 3.1 設定ファイルの確認 (`/etc/mop/config.toml`)

デーモン起動前に `/etc/mop/config.toml` を確認・編集します。

```toml
[server]
bind = "127.0.0.1:8787"
# public_url = "https://mop.example.com" # リバースプロキシ配下の場合

[database]
path = "/var/lib/mop/mop.db"

[auth]
registration = "first_user" # 最初の1ユーザーのみ管理者として登録可能
min_password_len = 10
session_hours = 12

[backup]
dir = "/var/lib/mop/backups"

[resources.systemd]
units = ["caddy.service", "nginx.service"]
allow_actions = ["start", "stop", "restart"]

[resources.docker]
containers = ["komga"]
label_selector = "mop.managed=true"
allow_actions = ["start", "stop", "restart"]
```

### 3.2 サービスの起動

```bash
sudo systemctl start mop.service
sudo systemctl status mop.service
```

### 3.3 管理者アカウントの作成

ブラウザで `http://<サーバーIP>:8787` にアクセスします。
初回アクセス時はセットアップ画面が表示されます。管理者ユーザー名および 10 文字以上の強力なパスワードを入力して登録を完了します。
登録完了後、自動的に一般ユーザー登録は締め切られます (`registration = "first_user"` の場合)。

---

## 4. バックアップとリストア運用 (SPEC §15)

mop は SQLite の WAL モード稼働中であっても一貫性を保ったままオンラインバックアップを取得できます。

### 4.1 バックアップの作成

#### A. Web UI / HTTP API からの作成 (推奨)
管理者権限でログイン後、設定画面または API 経由で作成します。
- `POST /api/v1/backup`
  - バックグラウンドジョブ `backup.create` が起動
  - 監査ログ (`backup.create`) に記録
  - 作成されたアーカイブは `/var/lib/mop/backups/mop-backup-<timestamp>.tar.zst` に保存されます。

#### B. CLI からの手動作成
```bash
sudo -u mop mop backup create
```

### 4.2 バックアップ一覧の確認
```bash
sudo -u mop mop backup list
```

### 4.3 バックアップアーカイブの内容
バックアップアーカイブ (`.tar.zst`) には以下が含まれます:
- `manifest.json`: スキーマバージョン (v1)、mop バージョン、作成日時
- `database/mop.db`: `VACUUM INTO` による一貫したオンラインスナップショット
- `config/config.toml`: 機密情報 (`session_secret_key` 等) をマスクした設定
- `plugins/installed.json`: インストール済みプラグイン一覧
- `plugins/<id>/settings.json`: 各プラグインの適用済み設定
- `checksums.sha256`: アーカイブ内全ファイルの SHA-256 チェックサム

### 4.4 リストア手順 (オフライン復元)

復元はデータベースの整合性を保証するため、**mop デーモンを停止したオフライン状態**で行います。

```bash
# 1. サービスの停止
sudo systemctl stop mop.service

# 2. リストアの実行
sudo mop restore /var/lib/mop/backups/mop-backup-20260906T120000Z.tar.zst

# 3. サービスの再開
sudo systemctl start mop.service
```

リストア処理では以下が自動で行われます:
1. `mop.service` が停止しているかの確認 (D-Bus / host.sock 照合)
2. `checksums.sha256` によるアーカイブの改ざん・破損チェック
3. `manifest.json` のスキーマ互換性チェック
4. 既存データベースの退避バックアップ作成 (`mop.db.bak.<timestamp>`)
5. 残存する SQLite `-wal` / `-shm` 一時ファイルのクリーンアップ
6. データベースおよび設定ファイルの安全な書き戻し

---

## 5. アップグレードとロールバック

### アップグレード
```bash
# 新バージョンのパッケージを適用
sudo dpkg -i mop_0.2.0_amd64.deb mop-plugin-manga_0.2.0_amd64.deb

# データベースマイグレーションは起動時に自動適用されます
sudo systemctl restart mop.service
```

### ロールバック
旧バージョンのパッケージを再インストールし、アップグレード直前に作成したバックアップからリストアします。
```bash
sudo systemctl stop mop.service
sudo dpkg -i mop_0.1.0_amd64.deb
sudo mop restore /var/lib/mop/backups/pre-upgrade-backup.tar.zst
sudo systemctl start mop.service
```

---

## 6. アンインストール手順

### Debian / Ubuntu
```bash
# パッケージの削除 (設定ファイルとデータベースは保持)
sudo apt remove mop mop-plugin-manga mop-plugin-video

# 完全削除 (設定ファイル・ログを含めて削除する場合)
sudo apt purge mop mop-plugin-manga mop-plugin-video
# データベースも不要な場合は手動で削除
sudo rm -rf /var/lib/mop
```

### スタンドアロン tarball
```bash
cd mop-0.1.0-linux-x86_64
sudo ./uninstall.sh
```

---

## 7. トラブルシューティング

### サービスログの確認
mop は標準出力・標準エラー出力を journald に集約します。
```bash
# デーモンのリアルタイムログ
journalctl -u mop.service -f

# 直近のエラーログ
journalctl -u mop.service -p err -n 50
```

### プラグインが認識されない場合
1. プラグインディレクトリ `/var/lib/mop/plugins/<id>/<version>/` に `plugin.toml` とバイナリが存在するか確認します。
2. バイナリの実行権限 (0755) を確認します。
3. `/run/mop/plugins` のパーミッションが `2770 mop:mop-ipc` になっているか確認します。
