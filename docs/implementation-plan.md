# yt-browser 実装計画

[requirements-definition.md](requirements-definition.md) と [design.md](design.md) に基づくフェーズ別の実装計画。
各フェーズはおおむね 1 PR 単位を目安とし、終了時に受け入れ条件を満たすことをチェックする。

## 前提と環境

- 開発と検証の第一環境は Linux の Devin VM。
  mpv と yt-dlp は `apt` / `pip` / バイナリで入れられる
- Windows ビルドは Windows 環境が必要になるため、必要になった時点で Windows 子セッションまたはユーザー側で検証する
- GUI 実機検証は Devin VM の Chrome ではなく mpv / Tauri 本体の起動確認になるため、画面確認はコンピュータツール経由で行う

## フェーズ

### Phase 0：スキャフォールド

- [x] Tauri v2 + Svelte 5 + TypeScript の雛形を作成する
- [x] `cargo test`、`cargo clippy`、`npm run check`（svelte-check）を CI に載せる
- [x] DB 接続、マイグレーション枠組み、`settings` テーブルだけ先行して用意する
- [x] 受け入れ：空のウィンドウが起動し、テストが緑である

### Phase 1：mpv 再生の MVP

- [x] `mpv` モジュール：プロセス起動、Unix ソケット IPC、`observe_property`、コマンド送信
- [x] `commands`：`play_video` / `player_control` / `player_close`
- [x] 最小 UI：URL または動画 ID の入力 → 再生、進捗、音量、一時停止
- [x] `watch_history` への記録とレジューム（前回位置の復帰）
- [x] yt-dlp 導入方針（同梱か自動更新か）をここで確定する（要件 8 未決事項 → 決定記録「Phase 1 で確定した事項」）
- [x] 受け入れ：任意の動画 URL で再生、一時停止、シーク、終了が動き、再開時に前回位置へ復帰する（実機検証済み。自然終了時は mpv が自動終了し履歴は completed 保存される）

### Phase 2：コマ送りと画質選択

- [x] `frame-step` / `frame-back-step` のコマンド経路と UI ボタン
- [x] `wheel.lua` の同梱と「一時停止中＝コマ送り、再生中＝音量」の既定動作
- [x] 画質プリセットとフォーマット式の設定画面
- [x] 受け入れ：一時停止中のホイールで前後フレーム移動、再生中は音量変化、画質変更が即時反映される（実機検証済み。ホイール ±2/ノッチ・コマ送り ±1f・プリセット即時適用で実ストリーム切替を確認）

### Phase 3：SponsorBlock

- [ ] `sponsor` モジュール：API 取得、カテゴリ設定、位置監視によるスキップ
- [ ] `sponsor://skipped` の一時通知 UI
- [ ] 受け入れ：区間を持つ動画で自動スキップが発火し、設定でカテゴリを切り替えられる

### Phase 4：購読フィード

- [ ] `feed` モジュール：チャンネル RSS のポーリング（ETag / Last-Modified、間隔の適応化）
- [ ] チャンネル購読、カテゴリ分類、未読管理の DB と UI
- [ ] `feed://new_items` と `feed://status` のイベント配線
- [ ] 受け入れ：購読登録→新着が一覧に積まれる、一括既読と個別既読が機能する

### Phase 5：検索、関連動画、チャンネルブロック

- [ ] `yt-dlp ytsearch` による検索 UI と、`innertube` の `next` による関連動画
- [ ] `blocked_channels` と各一覧への適用
- [ ] 受け入れ：検索から再生、購読、ブロックに遷移でき、ブロックがフィード、検索、関連の三方に効く

### Phase 6：ライブチャット

- [ ] `innertube` クライアントと `chat` ポーラー、renderer の正規化
- [ ] NG フィルタエンジン（`filter` モジュール）とチャットへの適用
- [ ] `chat_logs` 保存と FTS 検索
- [ ] 受け入れ：配信中のチャットが流れ、NG が表示に効き、ログが後から検索できる

### Phase 7：ローカルデータと設定画面

- [ ] お気に入り、カスタムプレイリスト、履歴一覧の UI
- [ ] 設定画面の完成（フィルタ、ブロック解除、画質、ホイール割り当て）
- [ ] 受け入れ：ローカルデータの一覧、編集、削除が完結する

### Phase 8：PiP、マルチビュー、配布

- [ ] 複数 mpv インスタンスの並行制御と PiP 起動
- [ ] AppImage / MSI のビルドと Tauri updater の検討
- [ ] 受け入れ：2 窓同時再生と小窓 PiP が動き、配布物が生成される

## 引き継ぎ手順

- 進捗はこのファイルのチェックリスト（`[ ]` / `[x]`）で管理し、マージ時に更新する
- 作業再開時は「マージ済み Phase」「現在ブランチ」「未決事項」の三点を確認する
- ブランチは `devin/<エポック秒>-<短いスラッグ>`、変更は必ず PR 経由
- Windows 依存作業（MSI ビルド等）だけが残った段階で、Windows 環境への移管を検討する
