# yt-browser 導入と使い方

mpv による軽量な再生と、ローカル完結の購読と履歴と NG 管理を一体化した YouTube 専用ブラウザ（専ブラ）です。
この文書はエンドユーザー向けの導入手順と操作方法をまとめたものです。

## 動作要件

- OS: Linux（WebKitGTK 4.1 環境）または Windows 10 以降
- 必須の外部コマンド（いずれも PATH から解決される必要がある）
  - `mpv`: 再生エンジン
  - `yt-dlp`: ストリーム解決と検索
- 推奨
  - `deno`: YouTube の解読を yt-dlp が外部 JS ランタイムに委ねる環境で必要になる

### 依存コマンドの導入例

Ubuntu / Debian の場合。

```sh
sudo apt install mpv yt-dlp
# もしくは新しい yt-dlp が必要なら
# sudo curl -L https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp -o /usr/local/bin/yt-dlp && sudo chmod +x /usr/local/bin/yt-dlp
```

Windows の場合は winget を使えます。

```powershell
winget install mpv mpv
winget install yt-dlp.yt-dlp
winget install DenoLand.Deno
```

インストール後は、新しいターミナルや再起動で PATH が通ることを確認してください。

## インストール

GitHub Releases から配布物を取得します。

- Linux: `*.AppImage`（実行権限を付けて起動）、`*.deb`、`*.rpm`
- Windows: `*_x64_en-US.msi`（インストーラ）または `*-setup.exe`（NSIS）

ソースから動かす場合は次のとおりです。

```sh
pnpm install
pnpm tauri dev        # 開発起動
pnpm tauri build      # AppImage / deb / rpm を生成（src-tauri/target/release/bundle 以下）
```

## 画面と機能

ナビゲーションは左のタブで切り替えます。

### 再生（トップ）

- URL または動画 ID を入力して「再生」で mpv 窓を開きます
- 「PiP で再生」は最前面で枠なしの小窓で起動します
- 再生中のインスタンスはプレイヤーカードとして並び、複数同時再生に対応します
- 視聴履歴に途中再生の位置が残り、「続きから再生」で再開できます

### フィード

- チャンネル ID（UC...）、URL、@handle を入力して購読します
- 購読チャンネルの新着が一覧に流れ、未読管理とカテゴリ分けができます
- 「今すぐ更新」で手動ポーリングします

### 検索

- キーワード検索と関連動画の表示に対応します
- 行の「購読」「ブロック」でチャンネル操作ができます

### ライブラリ

- 履歴タブ: 視聴履歴（途中位置つき）を一覧表示や再生や削除できます
- お気に入りタブ: 動画のブックマークを管理します
- プレイリストタブ: 任意の一覧を作成して動画を登録できます

### 設定

- 画質（ytdl-format）のプリセットまたは自由記述
- ホイール音量変化量、PiP の位置とサイズ
- SponsorBlock のカテゴリ別動作（スキップ / 通知のみ / 無効）
- NG フィルタを登録できます
  対象は動画タイトルや動画説明文やチャンネル名やチャンネル ID やチャット本文やチャット投稿者で、種別は部分一致と正規表現から選べます
- ブロック中チャンネルの管理
- チャット履歴の全文検索

## 再生操作

- mpv 窓上のホイール: 再生中は音量、一時停止中はコマ送り（wheel.lua）
- プレイヤーカード上の操作: 一時停止 / 再開、シーク、音量、速度、画質、1 コマ送り / 戻り、PiP 化 / 解除、終了
- mpv 標準のショートカットも有効です（q で終了、m でミュートなど）
- ライブ配信では「チャットを表示」でチャットパネルを開きます（NG フィルタがそのまま効きます）

## データの保存場所

設定、履歴、購読、チャットログはすべてローカルの SQLite に保存されます（アカウント連携はありません）。

- Linux: `~/.local/share/io.github.tkg-tamagohan.yt-browser/yt-browser.db`
- Windows: `%APPDATA%\io.github.tkg-tamagohan.yt-browser\yt-browser.db`
- ログ: 同ディレクトリ配下の `logs/yt-browser.log`

## トラブルシューティング

- 「yt-dlp が見つかりません」: yt-dlp をインストールして PATH を通すか、設定 `ytdlp.path` で直接パスを指定してください
- 再生が `Sign in to confirm you're not a bot` で止まる場合は YouTube の bot 判定です
  時間を置くか、yt-dlp 側の手順（`--cookies-from-browser` 等）で対処してください
- yt-dlp 更新ボタンが失敗する場合、システム管理のパス（/usr/bin 等）では権限不足で `yt-dlp -U` が失敗します
  手動で更新してください
- 描画位置とクリック位置がずれる場合（まれに GPU なし環境で発生）、ウィンドウの再読み込みで回復します
  一時停止中のカードも復元されます

## 既知の制限

細かい既知事項は [実装計画](implementation-plan.md) の「残課題」節を参照してください（一過性の再解決失敗、mpv 側の geometry 制約、絵文字表示はフォント依存など）。
