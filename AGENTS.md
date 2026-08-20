# AGENTS.md — mop

このリポジトリは **mop** (master-of-process) を開発する。Ubuntu/Debian 上で動く Rust 製デーモンで、systemd ユニット・Docker コンテナ・Compose サービスの状態/ログ/再起動を、内蔵の Vue 3 PWA から行う。プラグイン機構を持ち、manga2cbz は first-party プラグインとして移植する。

## 仕様の正

- **`SPEC.md` が唯一の仕様の正 (source of truth)**。不明点・矛盾があればコードを書かずに質問するか、SPEC.md の該当セクションを引用して確認を求めること。
- 仕様にない機能 (docker exec、任意シェル、マルチホスト、ログ保存など) を勝手に追加しない。SPEC.md §1.1 の非目標を参照。

## スタック

- バックエンド: Rust (tokio, axum, sqlx + SQLite WAL, zbus, bollard, axum-login, tower-sessions, password-auth/Argon2id)
- フロントエンド: Vue 3 + Vite + TypeScript + Pinia (SPA、SSR なし)。ビルド成果物はバイナリに埋め込む
- プラグイン: 別プロセス + Unix socket JSON-RPC 2.0。UI は Vue Custom Element
- パッケージ: .deb (Ubuntu 24.04 / Debian 12+, x86_64 / aarch64)

## レイアウト

`crates/` (mop-core, mop-db, mop-auth, mop-watch, mop-jobs, mop-plugin, mop-plugin-sdk, mop-http, mop-cli), `plugins/manga/`, `web/`, `deploy/`。詳細は SPEC.md §4。

## コマンド

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cd web && pnpm install && pnpm typecheck && pnpm build
```

コミット前に上記をすべてパスさせること。

## 絶対に守ること (SPEC.md §19 の不変条件)

1. mop を root で動かす設計にしない
2. `systemctl` / `docker` CLI のサブプロセス実行禁止。D-Bus (zbus) / Engine API (bollard) のみ
3. allowlist / `mop.managed=true` 以外のリソースを扱わない
4. ログ本文を DB やファイルに保存しない (journald / Docker logging driver が正)
5. プラグインは別プロセス・別ユーザー。Unix socket は 0660
6. 認可は必ずサーバー側。状態変更 API は Origin チェック + レート制限
7. バックアップに平文の秘密情報を含めない
8. manga プラグインのハードリミット (パス検証、容量上限、watch/output 重複禁止、失敗時クリーンアップ) を緩和しない
9. セキュリティ上の判断に迷ったら、緩和する方向ではなく厳しくする方向を選び、ユーザーに報告する

## 作業の進め方

- 実装は SPEC.md §21 のマイルストーン (M1→M6) 順に行う。指示がない限り複数マイルストーンを一度に実装しない
- コードを書く前に Implementation Plan を提示し、承認後に着手する
- 各マイルストーンは「独立して動作確認できる状態」で完了させ、テストを含める
- 完了時は変更内容・検証方法・残課題を Walkthrough としてまとめる

## コーディング規約

- コード・識別子・コミットメッセージは英語。ドキュメントと UI 文言は日本語可
- エラーは `thiserror` で型化し、API 境界では `{ error: { code, message } }` に変換する
- 時刻は UTC の RFC 3339 文字列で保存する
- ID は ULID を使う
- unsafe は原則禁止。必要な場合は理由をコメントし、ユーザーに明示する
