# manga2cbz から mop-plugin-manga への移行ガイド (Migration Guide)

本文書は、従来の Rust 製 CLI ツール **manga2cbz** から、mop ファーストパーティプラグイン **`mop-plugin-manga`** への移行手順および設定対照表です。

---

## 1. 移行の概要とメリット

`mop-plugin-manga` は、旧 manga2cbz のコア機能（ZIP/RAR/7z/TAR 等のアーカイブから WebP CBZ への一括変換・ディレクトリ監視）を、mop デーモンのプラグインアーキテクチャに移植したコンポーネントです。

### 主な改善点
1. **プロセス分離とサンドボックス**: mop 本体とは別システムユーザー (`mop-plugin-manga`) で動作し、親ディレクトリ脱出や危険なシンボリックリンクのハードリミットを内包 (SPEC §16)。
2. **Web UI による一元管理**: 変換キュー、進行中の進捗バー、失敗したアーカイブの再試行、リアルタイムログを mop の Web 画面から監視・操作可能。
3. **安全なトランザクション**: 変換中の一時作業ディレクトリは `work_dir` 配下に分離され、中断・失敗時にも元アーカイブを破損させず自動クリーンアップ。
4. **イベント駆動・低負荷監視**: ポーリングではなく inotify (`notify`) を利用し、2 秒間のデバウンス (`notify + 2s debounce`) を経てファイル書き込み完了を検知して自動投入。
5. **WebP 仕様上限への自動対応**: WebP フォーマットの仕様上限である 16383px を超える画像は、アスペクト比を維持したまま 16383px 以内に自動縮小されます (設定パラメータは不要)。

---

## 2. 設定パラメータ対照表 (Mapping Table)

旧 manga2cbz と `mop-plugin-manga` (MangaConfig) の設定項目の対照表です。

| 旧 manga2cbz パラメータ | mop-plugin-manga 設定キー (`MangaConfig`) | 型 | デフォルト値 | 説明 |
| :--- | :--- | :--- | :--- | :--- |
| `--watch-dir` (複数可) | `watch_dirs` | array of strings (paths) | `["$HOME/manga"]` | 監視対象ディレクトリ一覧 (新規アーカイブ検出時に自動変換) |
| `--output-dir` | `output_dir` | string (path) | `"$HOME/manga-cbz"` | 変換後 CBZ の出力先ディレクトリ (`watch_dirs` と同一・包含は不可) |
| `--unknown-dir` | `unknown_dir` | string (path) | `"$HOME/manga-unknown"` | マンガと判定されなかったアーカイブの退避先ディレクトリ |
| `--work-dir` | `work_dir` | string (path) | `"$HOME/.cache/manga2cbz"` | アーカイブ展開・画像処理の一時作業ディレクトリ |
| `--concurrency`, `-j` | `workers` | integer | `2` | 同時並行で処理するアーカイブ変換ワーカー数 |
| `-q`, `--quality` | `webp_quality` | integer (1-100) | `92` | WebP 圧縮品質 |
| `--lossless` | `lossless` | boolean | `false` | WebP 可逆圧縮を使用するかどうか |
| `--images-only` | `images_only` | boolean | `false` | 画像のみを抽出し非画像ファイルを除外するかどうか |
| `--keep-non-images` | `keep_non_images` | boolean | `true` | テキスト等の非画像ファイルを CBZ 内に維持するかどうか |
| `--delete-original` | `delete_original` | boolean | `false` | 変換成功後に元アーカイブを削除するかどうか |
| `--threshold` | `manga_image_threshold` | integer | `5` | マンガアーカイブと判定する最低画像枚数 |
| `--overwrite` | `overwrite` | boolean | `false` | 既存の同名 CBZ 出力ファイルを上書きするかどうか |
| `--scan-on-start` | `scan_on_start` | boolean | `true` | プラグイン起動時に `watch_dirs` 内の既存未処理アーカイブを一括スキャンするか |
| `--reject-symlinks` | `reject_symlinks` | boolean | `true` | シンボリックリンクを含むアーカイブの展開を拒否するか (セキュリティ保護) |

> [!NOTE]
> **画像サイズと監視方式について**:
> - **最大解像度制限 (`max_dimension`)**: 手動設定は不要です。WebP フォーマットの仕様上限である 16383px を超える巨大画像は自動的にアスペクト比を維持して 16383px に縮小されます。
> - **ポーリング間隔 (`poll_interval_sec`)**: 旧 CLI のポーリングループとは異なり、Linux inotify イベントを検知し、ファイルの書き込み完了を確実にするための 2 秒デバウンス (`notify + 2s debounce`) を経て自動キューイングされます。

> [!CAUTION]
> **ハードリミット制約 (SPEC §16)**:
> - `watch_dirs` と `output_dir` に同一のディレクトリ、または互いを包含するディレクトリを指定することはできません (再帰無限ループ防止)。
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
3. **Settings** タブで、旧 manga2cbz で使用していたパスとパラメータを設定します:
   - `watch_dirs`: (例: `["/data/incoming/manga"]`)
   - `output_dir`: (例: `"/data/library/manga"`)
   - `unknown_dir`: (例: `"/data/library/unknown"`)
   - `webp_quality`: `92`
   - `workers`: `2`
   - `scan_on_start`: `true`
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

### ステップ 5: プラグインの有効化と既存ファイルスキャン

1. mop Web コンソールの **Manga Conversion** プラグイン画面を開きます。
2. プラグインを **Enable (有効化)** に切り替えます（または API `POST /api/v1/plugins/mop.manga/enable` を実行）。
3. `scan_on_start = true` が有効な場合、プラグイン起動と同時に `watch_dirs` 内の既存アーカイブが検出されて順次変換キューに投入されます。
4. 以降は inotify 監視 (`notify + 2s debounce`) により、`watch_dirs` に投入されたアーカイブが自動的に変換されます。

---

## 4. ロールバック手順 (切り戻し)

何らかの問題が発生して旧環境へ切り戻す必要がある場合:

1. mop Web コンソールで **Manga Conversion** プラグインを **Disable (無効化)** にします。
   （または CLI / HTTP API から無効化）:
   ```bash
   # 管理者セッション Cookie を使用して無効化
   curl -X POST -b cookies.txt http://127.0.0.1:8787/api/v1/plugins/mop.manga/disable
   ```
   > ※ mop のプラグインは systemd unit ではなく、mop supervisor によって別プロセスとして起動・監視されているため、`systemctl stop mop-plugin-manga` ではなく mop のプラグイン API から無効化します。

2. 旧 manga2cbz の cron または systemd timer を再開します:
   ```bash
   sudo systemctl enable --now manga2cbz.timer
   ```
