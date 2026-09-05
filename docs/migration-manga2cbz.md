# manga2cbz から mop-plugin-manga への移行ガイド (Migration Guide)

本文書は、従来の Python/CLI 版 **manga2cbz** から、mop ファーストパーティプラグイン **`mop-plugin-manga`** への移行手順および設定対照表です。

---

## 1. 移行の概要とメリット

`mop-plugin-manga` は、旧 manga2cbz の機能（ZIP/RAR/7z/TAR 等のアーカイブから WebP CBZ への一括変換・ディレクトリ監視）を Rust ネイティブで再実装したプラグインです。

### 主な改善点
1. **パフォーマンス向上**: libarchive および libvips の C バインディングを直接利用し、メモリ消費量を抑えつつ高速に画像リサイズ・WebP エンコードを実施。
2. **安全性・サンドボックス**: mop 本体とは別システムユーザー (`mop-plugin-manga`) で動作し、親ディレクトリ脱出や危険なシンボリックリンクのハードリミットを内包 (SPEC §16)。
3. **Web UI による一元管理**: 変換キュー、進行中の進捗バー、失敗したアーカイブの再試行、ディレクトリ監視ステータスを mop の Web 画面から監視・操作可能。
4. **安全なトランザクション**: 変換中の一時ディレクトリは `/tmp/mop-manga-*` に分離され、中断・失敗時にも元アーカイブを破損させず自動クリーンアップ。

---

## 2. 設定パラメータ対照表 (Mapping Table)

旧 manga2cbz (CLI オプション / 設定ファイル) と `mop-plugin-manga` の設定項目の対照表です。

| 旧 manga2cbz パラメータ | mop-plugin-manga 設定キー | 型 | デフォルト値 | 説明 |
| :--- | :--- | :--- | :--- | :--- |
| `--watch-dir`, `watch_dir` | `watch_dir` | string (path) | `""` | 監視対象ディレクトリ (新規アーカイブ検出時に自動変換) |
| `--output-dir`, `output_dir` | `output_dir` | string (path) | `""` | 変換後 CBZ の出力先ディレクトリ (`watch_dir` と同一は不可) |
| `-q`, `--quality`, `quality` | `quality` | integer (1-100) | `85` | WebP 圧縮品質 (可逆圧縮希望の場合は 100 または専用フラグ) |
| `--max-dimension` | `max_dimension` | integer (px) | `1920` | 画像の最大幅・高さ (超過時にアスペクト比を維持して縮小) |
| `--concurrency`, `-j` | `workers` | integer | `2` | 同時並行で処理するアーカイブ数 |
| `--delete-original` | `delete_original` | boolean | `false` | 変換成功後に元アーカイブを削除するかどうか |
| `--poll-interval` | `poll_interval_sec` | integer (sec) | `10` | ディレクトリポーリング・inotify 再検知の間隔 |

> [!CAUTION]
> **ハードリミット制約 (SPEC §16)**:
> - `watch_dir` と `output_dir` に同一のディレクトリ、または包含関係にあるディレクトリを指定することはできません (無限ループ防止)。
> - 出力先ディスクの空き容量が最低空き容量 (デフォルト 2GB) を下回る場合、新規変換ジョブは自動的に一時停止します。

---

## 3. 移行手順 (Step-by-Step)

### ステップ 1: 依存パッケージとプラグインのインストール

```bash
# 依存ライブラリのインストール
sudo apt-get install -y libarchive13 libvips42

# プラグインパッケージのインストール
sudo dpkg -i mop-plugin-manga_0.1.0_amd64.deb
```

### ステップ 2: プラグイン設定の投入 (Web UI)

1. mop Web コンソール (`http://<server-ip>:8787`) に管理者でログインします。
2. 左メニューの **Plugins** → **Manga Conversion** を開きます。
3. **Settings** タブで、旧 manga2cbz で使用していたパスとパラメータを入力します:
   - `Watch Directory`: (例: `/data/incoming/manga`)
   - `Output Directory`: (例: `/data/library/manga`)
   - `WebP Quality`: `85`
   - `Max Dimension`: `1920`
   - `Workers`: `2`
4. **Save Draft** をクリックし、変更差分 (Diff) を確認します。
5. **Apply Settings** をクリックして設定を反映します。

### ステップ 3: テスト変換の実行

単一のアーカイブで変換動作を確認します。

1. Web UI の **Jobs** または **Manga** 画面から **Convert Archive** を選択します。
2. テスト用アーカイブのパスを指定してジョブを開始します。
3. リアルタイムログとプログレスバーを確認し、出力先ディレクトリに期待通りの `.cbz` が生成されているか検証します。

### ステップ 4: 旧 cron / 定期実行タスクの停止

新旧の処理が重複して元アーカイブの競合や破損が起きないよう、旧 manga2cbz の cron または systemd timer を停止・無効化します。

```bash
# cron を利用していた場合
crontab -e
# manga2cbz に関連する行をコメントアウト

# systemd timer を利用していた場合
sudo systemctl stop manga2cbz.timer
sudo systemctl disable manga2cbz.timer
```

### ステップ 5: ディレクトリ監視の有効化

1. mop Web コンソールの **Manga Conversion** プラグイン画面を開きます。
2. **Directory Watcher** を **Enabled (有効)** に切り替えます。
3. テストファイルを `watch_dir` に投入し、数秒以内に自動的にジョブがキューイングされて変換が開始されることを確認します。

---

## 4. ロールバック手順 (切り戻し)

何らかの問題が発生して旧環境へ切り戻す必要がある場合:

1. mop Web コンソールで **Directory Watcher** を **Disabled (無効)** にします。
2. 旧 manga2cbz の cron または systemd timer を再開します:
   ```bash
   sudo systemctl enable --now manga2cbz.timer
   ```
3. mop のプラグインサービスを停止します:
   ```bash
   sudo systemctl stop mop-plugin-manga.service 2>/dev/null || true
   ```
