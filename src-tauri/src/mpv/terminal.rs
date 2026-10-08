//! 終端イベント（end-file / ソケット切断）と意図的リロードの区別を管理する。
use crate::util::lock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// 終端イベント（end-file / ソケット切断）と意図的なリロードの区別を管理する。
/// `loadfile ... replace`（画質変更）は旧ファイルの `end-file` を発生させるが、
/// これを終了と取り違えないために `pending_replaces` で回数を数える。
#[derive(Default)]
pub(crate) struct TerminalTracker {
    /// 記録済みの終了理由。Some ならこのインスタンスは終端済み。
    ended_reason: Mutex<Option<String>>,
    /// 発行済みで未処理の `loadfile ... replace` の回数。
    pending_replaces: AtomicU32,
}

impl TerminalTracker {
    /// `loadfile ... replace` を発行する直前に呼ぶ。発生する end-file は無視される。
    pub(crate) fn begin_replace(&self) {
        self.pending_replaces.fetch_add(1, Ordering::SeqCst);
    }

    /// replace コマンド自体が失敗したときに予約を取り消す。
    pub(crate) fn cancel_replace(&self) {
        self.pending_replaces.fetch_sub(1, Ordering::SeqCst);
    }

    /// 新しいファイルのロード完了。end-file の取りこぼしでカウンタが残った場合の掃除。
    pub(crate) fn on_file_loaded(&self) {
        self.pending_replaces.store(0, Ordering::SeqCst);
    }

    /// end-file を終端イベントとして処理するなら true を返し理由を記録する。
    /// replace 由来の end-file は消化して false を返す。二重記録はしない。
    pub(crate) fn on_end_file(&self, reason: &str) -> bool {
        if self.pending_replaces.load(Ordering::SeqCst) > 0 {
            self.pending_replaces.fetch_sub(1, Ordering::SeqCst);
            return false;
        }
        self.record(reason)
    }

    /// `eof-reached=true`（keep-open 下で end-file が来ない経路の終端）。
    /// replace 予約を消化しない: キュー済みの旧ファイル EOF 通知が予約を消して
    /// 続く replace 由来の end-file を終端にしてしまう競合を防ぐ。
    /// 予約中に無視した EOF は、新ファイルが同じ末尾位置から再開されて
    /// すぐ再度 eof-reached になるため、最終的には終端として受理される。
    pub(crate) fn on_eof(&self) -> bool {
        if self.pending_replaces.load(Ordering::SeqCst) > 0 {
            return false;
        }
        self.record("eof")
    }

    /// 終端理由を一度だけ記録する。既に記録済みなら false。
    fn record(&self, reason: &str) -> bool {
        let mut ended = lock(&self.ended_reason);
        if ended.is_some() {
            return false;
        }
        *ended = Some(reason.to_string());
        true
    }

    /// ソケット切断。まだ終端が記録されていなければ "process_exit" で記録して true。
    pub(crate) fn on_disconnect(&self) -> bool {
        self.record("process_exit")
    }

    /// 履歴保存用の completed 判定。終端理由が eof のときのみ true。
    pub(crate) fn completed(&self) -> bool {
        lock(&self.ended_reason).as_deref() == Some("eof")
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalTracker;

    /// 終端判定: eof は completed、それ以外の終了は不完全のまま。
    #[test]
    fn terminal_completed_only_for_eof() {
        let t = TerminalTracker::default();
        assert!(!t.completed());
        assert!(t.on_end_file("eof"));
        assert!(t.completed());

        let t = TerminalTracker::default();
        assert!(t.on_end_file("error"));
        assert!(!t.completed());
    }

    /// 終端判定: 画質変更（loadfile replace）由来の end-file は終了扱いしない。
    #[test]
    fn replace_end_file_is_ignored() {
        let t = TerminalTracker::default();
        t.begin_replace();
        assert!(!t.on_end_file("stop"));
        // 予約は1回分だけ。次の真の end-file は終端として処理される
        assert!(t.on_end_file("eof"));
        assert!(t.completed());
    }

    /// 終端判定: replace 発行失敗時の取消でカウンタが残らない。
    #[test]
    fn cancel_replace_keeps_terminal_detection() {
        let t = TerminalTracker::default();
        t.begin_replace();
        t.cancel_replace();
        assert!(t.on_end_file("eof"));
    }

    /// 終端判定: 二重の終端イベントと切断通知は一度だけ受理する。
    #[test]
    fn terminal_is_recorded_once() {
        let t = TerminalTracker::default();
        assert!(t.on_end_file("eof"));
        assert!(!t.on_end_file("stop"));
        assert!(!t.on_disconnect());

        let t = TerminalTracker::default();
        assert!(t.on_disconnect());
        assert!(!t.on_end_file("eof"));
        assert!(!t.completed());
    }

    /// 終端判定: file-loaded で取りこぼした replace 予約を掃除する。
    #[test]
    fn file_loaded_clears_pending_replaces() {
        let t = TerminalTracker::default();
        t.begin_replace();
        t.on_file_loaded();
        // 予約が残っていないので次の end-file は終端になる
        assert!(t.on_end_file("eof"));
    }

    /// 終端判定: replace 予約中に届いた旧ファイルの eof-reached は
    /// 予約を消費しない（続く replace 由来の end-file が終端になる競合の防止）。
    /// begin_replace → 旧 eof-reached → 旧 end-file(stop) → file-loaded の順を検証する。
    #[test]
    fn stale_eof_during_replace_does_not_consume_reservation() {
        let t = TerminalTracker::default();
        t.begin_replace();
        // 旧ファイルの EOF 通知がキュー残りで到着しても予約を消化しない
        assert!(!t.on_eof());
        // replace 由来の end-file は引き続き予約を消費して無視される
        assert!(!t.on_end_file("stop"));
        t.on_file_loaded();
        // 新ファイルが同じ末尾位置から再度 EOF になれば終端として受理される
        assert!(t.on_eof());
        assert!(t.completed());
    }
}
