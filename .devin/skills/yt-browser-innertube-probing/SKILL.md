---
name: yt-browser-innertube-probing
description: yt-browser で InnerTube（YouTube の内部 API）の応答に依存する機能（チャット・関連動画など）を実装または修正するときの手順。仕様書の想定を実応答プローブで先に潰す方法、チャットリプレイの確定済み事実（get_live_chat_replay・endTimestamp による終了判定）、継続トークン種別を扱う。
---

# InnerTube の実応答プローブ

InnerTube に依存する機能では、仕様書や一般知識の想定どおりに実装せず、まず実際の応答を観測して確かめる。
Phase 33 では「watch 応答の継続トークン種別でライブとリプレイを自動判定する」という前提が実応答で崩れ、設計を後から直した。

## プローブの手順

1. watch ページの HTML を取得し、`ytInitialData` を抜いて継続トークンやメタを拾う。

```bash
curl -s "https://www.youtube.com/watch?v=<VIDEO_ID>" -A "Mozilla/5.0" \
  | grep -o 'ytInitialData = .*;</script>' > /tmp/yi.txt
```

`reloadContinuationData`・`liveChatReplayContinuationData`・`playerSeekContinuationData` などの継続キーと、`isLiveNow`・`endTimestamp` の有無を jq で確認する。

2. 継続トークンを InnerTube エンドポイントへ POST して応答を確かめる。
   最小ペイロードは `context`（clientName/clientVersion）と `continuation`。
   実装前にこの一手でエンドポイントの正誤が分かる（例：`reload` 系トークンは `live_chat/get_live_chat` へ投げると 400 になり、`get_live_chat_replay` で受け付ける）。

3. yt-dlp でチャンネルのタブ一覧や動画メタを取る補助確認も使える（`yt-dlp --flat-playlist`、`-j` で JSON 出力）。

## 確定済みの事実（実応答で確認）

チャットリプレイについて実機で確かめた経路は次のとおり。

- 終了済み配信の watch ページが返す初段トークンは `reloadContinuationData` で、`liveChatReplayContinuationData` は出ない。
- `reload` トークンは **`live_chat/get_live_chat_replay`** へ POST する（`get_live_chat` では 400）。
- 応答は `replayChatItemAction` で各アクションに `videoOffsetTimeMsec` が付く。
  次の継続は `liveChatReplayContinuationData`（`playerSeekContinuationData` が併存するがバッファ方式では使わない）。
- 終了判定は `isLiveNow:false` だけでは不十分。
  開始予定の配信（予約ロビー）も `isLiveNow:false` だが `endTimestamp` を持たない。
  終了扱いは `endTimestamp` の存在を必須にする（実例として開始予定配信で `endTimestamp` なしを確認）。
- 補足として、実装済みの最終的な振る舞いは `docs/design.md` §6.2 が正。
  本スキルは「未確定事項をどう検証するか」と「応答の読み方」に限定し、実装の正本は design.md に委ねる。

## プローブ時の注意

- InnerTube はアカウント非連携の方針のため Cookie なしの応答を見る（AGENTS.md の秘密情報規約）。
- 応答の形は時期で変わりうる。
  本スキルの「確定済みの事実」も、現在の実装や設計文書と食い違う場合は再プローブして実応答を正とし、design.md 側を更新する。
