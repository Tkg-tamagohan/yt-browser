# yt-browser

mpv による軽量な再生と、ローカル完結の購読、履歴、NG 管理を一体化した YouTube 専用ブラウザ（専ブラ）。

現状は要件定義と設計のフェーズであり、実装は未着手である。

## 技術スタック

| 層 | 採用 |
|---|---|
| コア | Rust + Tokio |
| シェル / UI | Tauri v2 + Svelte 5 + TypeScript |
| 再生 | mpv プロセス（JSON IPC 制御） |
| ストリーム解決と検索 | yt-dlp 子プロセス |
| チャットと関連動画 | InnerTube クライアント（自前、安定層のみ） |
| DB | SQLite（rusqlite、WAL） |

## 文書

| 文書 | 内容 |
|---|---|
| [docs/requirements-definition.md](docs/requirements-definition.md) | 要件定義書。仕様の正 |
| [docs/decision-records.md](docs/decision-records.md) | 協議で確定した判断の記録（仕様決定 A 以降） |
| [docs/design.md](docs/design.md) | 概要設計と詳細設計（アーキテクチャ、IPC、DB、チャット、NG、エラー方針） |
| [docs/implementation-plan.md](docs/implementation-plan.md) | フェーズ別の実装計画と引き継ぎ手順 |

## ライセンス

MIT
