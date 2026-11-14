---
name: yt-browser-release
description: yt-browser のリリース作成とバージョン更新の手順。bump_version.py による 4 箇所一括更新（Windows では出力文字化け防止のため PYTHONUTF8=1 を推奨）、バンプ PR、マージコミットへのタグ push、公開後の資産と latest.json の署名確認を扱う。リリースを作成またはバージョンを更新するときに使用する。手順の正本は docs/updater-runbook.md。
---

# リリースの作成

手順の正本は `docs/updater-runbook.md` で、秘密鍵の登録や updater の動作前提もそちらに書かれている。
本スキルは流れの概略と、実作業で詰まった点だけを載せる。

## 流れ

1. リリースブランチを切る（`devin/<エポック秒>-release-X.Y.Z`）。
2. バージョンを一括更新する。

```bash
PYTHONUTF8=1 python scripts/bump_version.py 0.6.0
PYTHONUTF8=1 python scripts/bump_version.py --check   # 4 箇所の整合確認
```

`bump_version.py` のファイル読み書き自体はすべて `encoding="utf-8"` を指定済みのため読み込み失敗は起きないが、Windows の cp932 環境では標準出力やログの日本語表示の文字化けを防ぐため `PYTHONUTF8=1` を付けて実行する。
対象は `tauri.conf.json`、`package.json`、`Cargo.toml`、`Cargo.lock` の 4 箇所。

3. バンプを PR にして CI パス後にマージする（このリポジトリは変更をすべて PR 経由にする規約）。
4. main の**マージコミット**にタグを打って push する（軽量タグが既存運用）。

```bash
git checkout main && git pull
git tag v0.6.0 && git push origin v0.6.0
```

`v*` タグ push で release ワークフローが起動し、両 OS のビルド→`latest.json` 生成→ドラフト作成→アセット添付→公開まで自動で進む。
タグを打つ位置や打ち直しの注意は runbook の「手動でタグを作成するときの注意点」にある。

## 公開後の確認

```bash
gh release view v0.6.0 --json assets --jq '.assets[].name'
```

期待する資産は NSIS セットアップ exe・AppImage・deb・rpm・拡張 zip・`latest.json`。
`.sig` の個別アセットと `.msi` は仕様決定 AK で付かないのが正常で、欠落ではない。
署名は `latest.json` の `platforms` 各エントリの `signature`（440 文字）に埋め込まれる。
実物をダウンロードして `version` と両プラットフォームの `signature` があるか確認する。

```bash
gh release download v0.6.0 -p latest.json -R <owner>/<repo> --clobber
```
