# yt-browser

mpv による軽量な再生と、ローカル完結の購読、履歴、NG 管理を一体化した YouTube 専用ブラウザ（専ブラ）。

現在は[実装計画](docs/implementation-plan.md)に沿ったフェーズ別実装の途中である。

## 開発環境

- Rust（stable）、Node.js 24、pnpm
- Tauri の Linux 依存：`libwebkit2gtk-4.1-dev libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`
- 実行依存（Phase 1 以降）：mpv、yt-dlp

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

- yt-dlp の解決順は「設定 `ytdlp.path` → 同梱リソース → PATH の `yt-dlp`」。現在の運用はシステムインストール前提（Phase 8 の配布物もアプリ本体のみで同梱しない。同梱は今後の選択肢として残す）
- YouTube 解読のため yt-dlp が外部 JS ランタイムを要求する環境がある。`deno` を PATH に入れておく
- PO Token を要求される環境では、yt-dlp 側の手順（`--cookies-from-browser` や外部プロバイダ）に従う。アプリ側の伝達経路は後フェーズの課題
- アプリ内の「yt-dlp 更新」ボタンは `yt-dlp -U` を呼ぶ。システム管理のパスでは権限不足で失敗し得る

## 文書

| 文書 | 内容 |
|---|---|
| [docs/requirements-definition.md](docs/requirements-definition.md) | 要件定義書。仕様の正 |
| [docs/decision-records.md](docs/decision-records.md) | 協議で確定した判断の記録（仕様決定 A 以降） |
| [docs/design.md](docs/design.md) | 概要設計と詳細設計（アーキテクチャ、IPC、DB、チャット、NG、エラー方針） |
| [docs/implementation-plan.md](docs/implementation-plan.md) | フェーズ別の実装計画と引き継ぎ手順 |

## ライセンス

MIT
