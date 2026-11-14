# yt-browser

mpv による軽量な再生と、ローカル完結の購読、履歴、NG 管理を一体化した YouTube 専用ブラウザ（専ブラ）。
対応 OS は Linux と Windows で、macOS は対象外。

[実装計画](docs/implementation-plan.md)の全フェーズ（Phase 0〜46）が実装済み（実機検証系の受け入れは一部未検証として残る）。
残る制限と観察メモは実装計画の「残課題」節にまとめている。

## 主な機能

- 動画再生：複数インスタンスの同時再生、既定の PiP 小窓、コマ送り、ループ、概要欄、リンクコピー
- キューと連続再生：一時キュー（再起動後も復元）とプレイリストごとのキューを同時に扱える
- 購読フィード：チャンネル RSS のポーリング、カテゴリ分け、未読管理、チャンネル別表示と過去動画のバックフィル
- 検索と関連動画：yt-dlp 検索と InnerTube の関連動画
- フィルタ：チャンネルブロックと、対象と方式を選べる NG フィルタ
- ライブチャット：リアルタイム表示、終了済み配信のリプレイ同期、別ウィンドウへのポップアップ
- ライブラリ：視聴履歴（途中位置のレジューム）、お気に入り、プレイリスト（YouTube からの取り込みと並べ替え）
- 調整：画質、PiP の位置とサイズ、起動ディスプレイ、HDR と mpv の引数、HW デコード、ウインドウごとの音声出力先
- 外部連携：ブラウザ拡張（Chrome や Edge などの Chromium 系）からの起動とキュー追加、`yt-browser://` スキーム、Native Messaging
- 自動更新：AppImage と NSIS 版が GitHub Releases 経由で更新できる

機能ごとの詳細と対応する要件とフェーズは [docs/features.md](docs/features.md) を参照。

## 開発環境

- Rust（stable）、Node.js 24、pnpm
- Tauri の Linux 依存：`libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`
- 実行依存：mpv、yt-dlp（いずれも PATH から解決。配布物はアプリ本体のみで同梱しない）

```sh
pnpm install
pnpm tauri dev    # 開発起動
pnpm check        # svelte-check
(cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings)
```

## 技術スタック

| 層 | 採用 |
|---|---|
| コア | Rust + Tokio |
| シェル / UI | Tauri v2 + Svelte 5 + TypeScript |
| 再生 | mpv プロセス（JSON IPC 制御） |
| ストリーム解決と検索 | yt-dlp 子プロセス |
| チャットと関連動画 | InnerTube クライアント（自前、安定層のみ） |
| DB | SQLite（rusqlite、WAL） |

## yt-dlp まわりの運用メモ

- yt-dlp の解決順は「設定 `ytdlp.path` → 同梱リソース → PATH の `yt-dlp`」。現在の運用はシステムインストール前提（配布物はアプリ本体のみで同梱しない。同梱は今後の選択肢として残す）
- YouTube 解読のため yt-dlp が外部 JS ランタイムを要求する環境がある。`deno` を PATH に入れておく
- PO Token を要求される環境では、設定 `ytdlp.po_token`（設定画面の「依存コマンド」節）で値を yt-dlp 呼び出しへ伝達できる（FR-86、仕様決定 DK）。yt-dlp 側の手順（`--cookies` や外部プロバイダ）の利用も引き続き可能（bot 判定の回避手順は [docs/user-guide.md](docs/user-guide.md) の「bot 判定の回避（cookies.txt）」を参照）
- アプリ内の「yt-dlp 更新」ボタンは `yt-dlp -U` を呼ぶ。システム管理のパスでは権限不足で失敗し得る

## 文書

| 文書 | 内容 |
|---|---|
| [docs/user-guide.md](docs/user-guide.md) | 導入方法と使い方（エンドユーザー向け） |
| [docs/features.md](docs/features.md) | 実装済み機能の一覧（要件 ID とフェーズとの対応） |
| [docs/requirements-definition.md](docs/requirements-definition.md) | 要件定義書。仕様の正 |
| [docs/decision-records.md](docs/decision-records.md) | 協議で確定した判断の記録（仕様決定 A 以降） |
| [docs/design.md](docs/design.md) | 概要設計と詳細設計（アーキテクチャ、IPC、DB、チャット、NG、エラー方針） |
| [docs/implementation-plan.md](docs/implementation-plan.md) | フェーズ別の実装計画と引き継ぎ手順 |
| [docs/updater-runbook.md](docs/updater-runbook.md) | 自動更新の運用手順 |

## ライセンス

MIT
