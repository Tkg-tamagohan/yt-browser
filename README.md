# yt-browser

mpv による軽量な再生と、ローカル完結の購読、履歴、NG 管理を一体化した YouTube 専用ブラウザ（専ブラ）。
対応 OS は Linux と Windows で、macOS は対象外。

[実装計画](docs/implementation-plan.md)の全フェーズ（Phase 0〜81）が実装済み（実機検証系の受け入れは一部未検証として残る）。
残る制限と観察メモは実装計画の「残課題」節にまとめている。

## 主な機能

- 動画再生：複数インスタンスの同時再生、既定の PiP 小窓、コマ送り、ループ、A-B リピート、スクリーンショット、字幕選択、概要欄、ブックマーク、SponsorBlock スキップとシークバー区間表示、リンクコピー
- キューと連続再生：一時キューとプレイリストごとのキューを DB に永続化し、再起動後は停止状態で復元して複数キューを同時に連続再生できる
- 購読フィード：チャンネル RSS のポーリング、カテゴリ分けと並べ替え、未読管理、自動既読、新着通知、チャンネル別表示と過去動画（`/shorts` 含む）のバックフィル
- 検索と関連動画：yt-dlp 検索、InnerTube の関連動画、予約済みライブ配信の待機と開始時の自動再生
- フィルタ：チャンネルブロックと、対象（`video_desc` 含む）と方式（部分一致、正規表現、ハイライト）を選べる NG フィルタ、NG ユーザーの専用管理
- ライブチャット：リアルタイム表示、終了済み配信のリプレイ同期、別ウィンドウへのポップアップ（位置とサイズ保存、表示カスタマイズ）
- ライブラリ：視聴履歴（レジューム、検索とページング）、お気に入り、あとで見る、プレイリスト（取り込みと並べ替え）、ライブラリ横断検索、視聴統計
- 調整：画質、PiP の位置とサイズ、一括整列、起動ディスプレイ、初期速度と音量、HDR と mpv 引数、HW デコード、音声出力先、テーマ、表示密度、日英 UI 切替
- 外部連携：ブラウザ拡張（Chrome や Edge などの Chromium 系）からの起動、再生位置引き継ぎ、キューやプレイリストへの追加、`yt-browser://` スキーム、Native Messaging
- 更新と保守：AppImage と NSIS 版の GitHub Releases 自動更新、設定や購読のエクスポートとインポート、DB メンテナンス、診断情報 zip エクスポート、通知履歴

機能ごとの詳細と対応する要件とフェーズは [docs/features.md](docs/features.md) を参照。

## 開発環境

- Rust（stable）、Node.js 24、pnpm
- Tauri の Linux 依存：`libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`
- 実行依存：mpv、yt-dlp（いずれも PATH から解決。配布物はアプリ本体のみで同梱しない）

```sh
pnpm install
pnpm tauri dev    # 開発起動
pnpm check        # svelte-check
pnpm test         # Vitest 単体テスト
(cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings)
```

## 技術スタック

| 層 | 採用 |
|---|---|
| コア | Rust + Tokio |
| シェル / UI | Tauri v2 + Svelte 5 + TypeScript（仕様決定 B、C） |
| 再生 | mpv プロセス（JSON IPC 制御） |
| ストリーム解決と検索 | yt-dlp 子プロセス |
| チャット、関連動画、概要欄、メタ補完 | InnerTube クライアント（自前、安定層のみ） |
| DB | SQLite（rusqlite、WAL） |

## yt-dlp まわりの運用メモ

- yt-dlp の解決順は「設定 `ytdlp.path` → 同梱リソース → PATH の `yt-dlp`」とする。
  現在の運用はシステムインストール前提のため配布物はアプリ本体のみとし、同梱は今後の選択肢として残す
- YouTube 解読のため yt-dlp が外部 JS ランタイムを要求する環境がある。
  `deno` を PATH に入れておく
- PO Token を要求される環境では、設定 `ytdlp.po_token`（設定画面の「依存コマンド」節）で値を yt-dlp 呼び出しへ伝達できる（FR-86、仕様決定 DK）。
  yt-dlp 側の手順（`--cookies` や外部プロバイダ）の利用も引き続き可能（bot 判定の回避手順は [docs/user-guide.md](docs/user-guide.md) の「bot 判定の回避（cookies.txt）」を参照）
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

MIT（仕様決定 J）
