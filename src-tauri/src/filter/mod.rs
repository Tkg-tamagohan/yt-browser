//! NG フィルタの評価エンジン（設計書 §6.3、§8 の `filters` テーブル）。
//! 対象（target）ごとに literal は Aho-Corasick の一括マッチ、
//! regex は RegexSet に束ねて評価する。フィルタ変更のたびに `rebuild` で
//! 全件作り直す（件数は少数前提。増分更新はしない）。

use std::collections::HashMap;

use aho_corasick::AhoCorasick;
use regex::RegexSet;

use crate::model::Filter;

/// target 単位の NG 判定器。`chat::ChatPoller` が Arc で共有し、
/// フィルタ登録・削除のたびに丸ごと差し替える。
#[derive(Debug, Default)]
pub struct Matcher {
    /// target -> literal パターン集合（部分一致・大小文字区別あり）。
    literal: HashMap<String, AhoCorasick>,
    /// target -> regex パターン集合。
    regex: HashMap<String, RegexSet>,
    /// コンパイルに失敗して除外したパターン（診断用）。
    /// `filter_add` は登録時に regex を検証するため、ここに入るのは
    /// ライブラリ差異などで既存行が再コンパイルに失敗した場合のみ。
    invalid: Vec<String>,
}

impl Matcher {
    pub fn empty() -> Self {
        Self::default()
    }

    /// `filters` 全行から評価器を作り直す。`enabled` の行だけが対象。
    pub fn rebuild(filters: &[Filter]) -> Self {
        let mut literal_pats: HashMap<String, Vec<String>> = HashMap::new();
        let mut regex_pats: HashMap<String, Vec<String>> = HashMap::new();
        for f in filters.iter().filter(|f| f.enabled) {
            match f.kind.as_str() {
                "literal" => literal_pats
                    .entry(f.target.clone())
                    .or_default()
                    .push(f.pattern.clone()),
                "regex" => regex_pats
                    .entry(f.target.clone())
                    .or_default()
                    .push(f.pattern.clone()),
                _ => {}
            }
        }
        let mut invalid = Vec::new();
        let literal = literal_pats
            .into_iter()
            .filter_map(|(target, pats)| match AhoCorasick::new(&pats) {
                Ok(ac) => Some((target, ac)),
                Err(_) => {
                    invalid.extend(pats);
                    None
                }
            })
            .collect();
        // RegexSet は集合全体で一括コンパイルするため、1 件の不正パターンで
        // target 内の全パターンが落ちてしまう。先に個別コンパイルで検証し、
        // 良いものだけを束ねる。
        let regex = regex_pats
            .into_iter()
            .filter_map(|(target, pats)| {
                let good: Vec<String> = pats
                    .into_iter()
                    .filter(|p| {
                        if regex::Regex::new(p).is_ok() {
                            true
                        } else {
                            invalid.push(p.clone());
                            false
                        }
                    })
                    .collect();
                RegexSet::new(&good).ok().map(|set| (target, set))
            })
            .collect();
        Self {
            literal,
            regex,
            invalid,
        }
    }

    /// `target` に対する `text` が NG か。空テキストは常に NG ではない。
    /// literal は部分一致、regex は `is_match`（部分一致）で判定する。
    pub fn is_blocked(&self, target: &str, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        if let Some(ac) = self.literal.get(target) {
            if ac.is_match(text) {
                return true;
            }
        }
        if let Some(set) = self.regex.get(target) {
            if set.is_match(text) {
                return true;
            }
        }
        false
    }

    /// 作り直し時にコンパイル失敗で除外したパターン一覧。
    pub fn invalid_patterns(&self) -> &[String] {
        &self.invalid
    }

    /// 動画系 target の NG 判定（FR-9）。一覧の表示経路（フィード・検索・関連動画）で
    /// `is_ng_video` に渡せる値を評価する。`video_desc` は現行の取得経路
    /// （RSS・ytsearch flat・InnerTube next）のどれにも説明文フィールドが無く
    /// 評価対象外（決定記録 Phase 6 に記録）。
    pub fn is_video_ng(
        &self,
        title: &str,
        channel_title: Option<&str>,
        channel_id: Option<&str>,
    ) -> bool {
        if self.is_blocked("video_title", title) {
            return true;
        }
        for (target, text) in [("channel_title", channel_title), ("channel_id", channel_id)] {
            if let Some(t) = text {
                if self.is_blocked(target, t) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(target: &str, kind: &str, pattern: &str) -> Filter {
        Filter {
            id: 0,
            target: target.into(),
            kind: kind.into(),
            pattern: pattern.into(),
            enabled: true,
            created_at: String::new(),
        }
    }

    /// FT-01: literal パターンは登録 target にだけ部分一致でヒットする。
    #[test]
    fn literal_matches_only_its_target() {
        let m = Matcher::rebuild(&[f("chat_text", "literal", "売れ筋")]);
        assert!(m.is_blocked("chat_text", "この売れ筋商品が"));
        assert!(!m.is_blocked("chat_text", "普通の発言"));
        // 別 target には掛からない
        assert!(!m.is_blocked("chat_author", "売れ筋"));
    }

    /// FT-02: regex パターンは部分一致でヒットする。
    #[test]
    fn regex_matches() {
        let m = Matcher::rebuild(&[f("chat_text", "regex", r"^\d{3}-\d{4}$")]);
        assert!(m.is_blocked("chat_text", "123-4567"));
        assert!(!m.is_blocked("chat_text", "1234567"));
    }

    /// FT-03: 無効化行は評価対象から外れる。
    #[test]
    fn disabled_filter_is_ignored() {
        let mut row = f("chat_text", "literal", "NG");
        row.enabled = false;
        let m = Matcher::rebuild(&[row]);
        assert!(!m.is_blocked("chat_text", "NG ワード"));
    }

    /// FT-04: コンパイル失敗の既存行は rebuild 時に除外され、
    /// 他のパターンの評価を妨げない。
    #[test]
    fn invalid_regex_is_dropped_not_fatal() {
        let m = Matcher::rebuild(&[
            f("chat_text", "regex", "(unclosed"),
            f("chat_text", "regex", "ok[0-9]+"),
        ]);
        assert!(m.is_blocked("chat_text", "ok123"));
        assert_eq!(m.invalid_patterns(), &["(unclosed".to_string()]);
    }

    /// FT-05: 空テキスト・空パターン集合はヒットしない。
    #[test]
    fn empty_inputs_never_match() {
        let m = Matcher::rebuild(&[f("chat_text", "literal", "NG")]);
        assert!(!m.is_blocked("chat_text", ""));
        assert!(!Matcher::empty().is_blocked("chat_text", "NG"));
    }

    /// FT-06: 動画系 target は is_video_ng で評価される（FR-9）。
    /// 取れないフィールド（None）は判定対象から外れる。
    #[test]
    fn video_targets_match() {
        let m = Matcher::rebuild(&[
            f("video_title", "literal", "切り抜き"),
            f("channel_title", "regex", r"公式$"),
            f("channel_id", "literal", "UCbad"),
        ]);
        assert!(m.is_video_ng("【切り抜き】雑談", None, None));
        assert!(m.is_video_ng("通常タイトル", Some("公式チャンネル公式"), None));
        assert!(m.is_video_ng("通常タイトル", None, Some("UCbad123")));
        assert!(!m.is_video_ng("通常タイトル", Some("個人勢"), Some("UCgood")));
        assert!(!m.is_video_ng("通常タイトル", None, None));
    }
}
