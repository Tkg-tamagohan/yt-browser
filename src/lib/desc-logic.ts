// 概要欄テキストの分割ロジック（FR-33、仕様決定 BB）。
// 本文中の http(s) URL と `mm:ss` / `h:mm:ss` 時刻表記を区切り、
// URL は既定ブラウザで開き、時刻表記はそのインスタンスへのシークとする。

export type DescSegment =
  | { kind: "text"; text: string }
  | { kind: "url"; text: string; url: string }
  | { kind: "seek"; text: string; seconds: number };

// 走査は URL を先に試す（クエリ文字列中の `12:34` 風トークンを
// 時刻と誤認しないため）。時刻表記は `h:mm:ss`（時間 : 2桁分 : 2桁秒）と
// `mm:ss`（1〜3桁分 : 2桁秒）を受理し、前後が数字・コロンで続かない
// 位置だけを対象にする（`12:34:56` は h:mm:ss、`1:2:3` や `16:9` は対象外）。
const TOKEN_RE =
  /https?:\/\/[^\s]+|(?<![\d:])(\d{1,3}):([0-5]\d)(?::([0-5]\d))?(?![\d:])/g;

// URL 末尾に連なる句読点・感嘆符・引用符は文脈側の記号とみなす
const TRAIL_PUNCT_RE = /[.,;:!?'"、。！？]+$/;

// 閉じ括弧は対応する開き括弧が URL 内に無いときだけ落とす
// （`https://…/Foo_(bar)` の括弧は残し、`(https://…)` の外側は剥がす）
const CLOSE_TO_OPEN: Record<string, string> = {
  ")": "(",
  "]": "[",
  "}": "{",
  "）": "（",
  "］": "［",
  "｝": "｛",
  "】": "【",
  "〉": "〈",
  "》": "《",
  "」": "「",
  "』": "『",
};

/// URL 候補文字列から、末尾の文脈記号を除いた本体と除いた残りを返す。
function splitUrlTail(raw: string): { url: string; rest: string } {
  let url = raw;
  for (;;) {
    const m = TRAIL_PUNCT_RE.exec(url);
    if (m) {
      url = url.slice(0, url.length - m[0].length);
      continue;
    }
    const last = url.charAt(url.length - 1);
    const open = CLOSE_TO_OPEN[last];
    if (open !== undefined) {
      const closes = url.split(last).length - 1;
      const opens = url.split(open).length - 1;
      if (closes > opens) {
        url = url.slice(0, -1);
        continue;
      }
    }
    break;
  }
  return { url, rest: raw.slice(url.length) };
}

/// 概要欄本文を text / url / seek の列に分割する。
/// 改行は text 区内にそのまま残り、表示側の pre-wrap で保持される。
export function parseDescription(text: string): DescSegment[] {
  const segs: DescSegment[] = [];
  let last = 0;
  for (const m of text.matchAll(TOKEN_RE)) {
    const start = m.index;
    if (start > last) {
      segs.push({ kind: "text", text: text.slice(last, start) });
    }
    if (m[0].startsWith("http")) {
      const { url, rest } = splitUrlTail(m[0]);
      if (url !== "") segs.push({ kind: "url", text: url, url });
      if (rest !== "") segs.push({ kind: "text", text: rest });
    } else {
      // 3 個目のグループがあれば h:mm:ss、無ければ mm:ss
      const hours = m[3] === undefined ? 0 : Number(m[1]);
      const minutes = Number(m[3] === undefined ? m[1] : m[2]);
      const seconds = Number(m[3] === undefined ? m[2] : m[3]);
      segs.push({
        kind: "seek",
        text: m[0],
        seconds: hours * 3600 + minutes * 60 + seconds,
      });
    }
    last = start + m[0].length;
  }
  if (last < text.length) {
    segs.push({ kind: "text", text: text.slice(last) });
  }
  return segs;
}
