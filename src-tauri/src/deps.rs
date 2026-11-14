//! 依存コマンドの検出（FR-74、仕様決定 CY）と初回セットアップ判定
//! （FR-75、仕様決定 CZ）。
//! mpv・yt-dlp・deno の検出状況・バージョン・解決パスを `deps_status`
//! コマンドが一覧で返し、設定画面の診断節と初回起動の依存検出画面が
//! 同じ一覧を表示する。

use std::time::Duration;

use tokio::process::Command;

use crate::db::Db;
use crate::model::DepStatus;
use crate::yt::YtDlpResolver;

/// 設定キー: 初回起動の依存検出画面を表示済みにするフラグ（FR-75、仕様決定 CZ）。
/// 値は "on" / "off"。未設定・その他の値は未表示（初回起動で表示する）。
pub const SETTING_SETUP_DEPS_SHOWN: &str = "setup.deps_shown";

/// `setup.deps_shown` の解釈（FR-75、仕様決定 CZ）。
/// on 系（on/true/1/yes、大小文字・前後空白を許容）のみ「表示済み」とし、
/// 未設定・その他の値は未表示として扱う（受理集合は feed.show_shorts 系と同一）。
pub fn setup_deps_shown_enabled(value: Option<String>) -> bool {
    match value {
        Some(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "on" | "true" | "1" | "yes"
        ),
        None => false,
    }
}

/// `<cmd> --version` の検出待ち上限。応答しない実行ファイルに
/// 引きずられないよう yt::which_exists と同じ 10 秒で打ち切る。
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// PATH 解決するコマンドを `name --version` で検出する。
/// spawn 成功かつ終了コード 0 を検出とみなし、解決パスはコマンド名を返す
/// （mpv・deno は PATH 解決のみの経路で、絶対パスの解決は持たない）。
pub async fn probe_command(name: &str) -> DepStatus {
    match run_version(name).await {
        Some(out) if out.status.success() => DepStatus {
            name: name.to_string(),
            detected: true,
            path: Some(name.to_string()),
            version: version_first_line(&out.stdout),
        },
        _ => DepStatus {
            name: name.to_string(),
            detected: false,
            path: None,
            version: None,
        },
    }
}

/// `<cmd> --version` の stdout から表示用のバージョン文字列を取る。
/// 先頭の非空行を使い（mpv はバージョン行の後にビルド情報が複数行続き、
/// deno も v8 / typescript の行が続くため）、行末の `(...)` 補足
/// （deno の `(stable, release, ...)` のような付記）は削って表示用に
/// 短くする。非空行が無ければ None。
pub fn version_first_line(stdout: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(|l| match l.rfind(" (") {
            Some(i) if l.ends_with(')') => l[..i].trim_end().to_string(),
            _ => l.to_string(),
        })
}

/// `name --version` をタイムアウト付きで実行する。
/// spawn 失敗・タイムアウトは None（`kill_on_drop(true)` で
/// タイムアウト時に子プロセスも確実に終了する）。
async fn run_version(name: &str) -> Option<std::process::Output> {
    let mut cmd = Command::new(name);
    cmd.arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(crate::CREATE_NO_WINDOW);
    match tokio::time::timeout(PROBE_TIMEOUT, cmd.output()).await {
        Ok(res) => res.ok(),
        Err(_) => None,
    }
}

/// mpv・yt-dlp・deno の検出状況をこの順で返す（FR-74、仕様決定 CY）。
/// mpv・deno は PATH 上の `<cmd> --version` で検出し、yt-dlp は
/// `YtDlpResolver` の解決順（設定 → 同梱 → PATH）の結果を使う。
/// 起動直後の一覧表示を速くするため 3 件の検出は並行して行う。
pub async fn collect_deps_status(db: &Db, resolver: &YtDlpResolver) -> Vec<DepStatus> {
    let (mpv, (ypath, yversion), deno) = tokio::join!(
        probe_command("mpv"),
        resolver.status(db),
        probe_command("deno")
    );
    vec![
        mpv,
        DepStatus {
            name: "yt-dlp".to_string(),
            detected: ypath.is_some(),
            path: ypath,
            version: yversion,
        },
        deno,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DB-DP-01: `--version` 出力の先頭非空行の抽出（FR-74、仕様決定 CY）。
    /// mpv のようにバージョン行の後にビルド情報が続く出力では先頭行だけを
    /// 表示用に採用し、deno のような行末の `(...)` 補足は削る。
    /// 空出力・空白のみの出力は None とする。
    #[test]
    fn version_first_line_takes_first_non_empty_line() {
        // mpv --version の実形: 1 行目がバージョン、以降にビルド情報が続く
        let mpv = b"mpv v0.40.0-dirty\nbuilt on Sun Apr 20 2026\nFFmpeg version: 7.1\n";
        assert_eq!(
            version_first_line(mpv).as_deref(),
            Some("mpv v0.40.0-dirty")
        );
        // deno --version の実形: 1 行目がランタイムのバージョン
        let deno = b"deno 2.3.1 (stable, release, x86_64-pc-windows-msvc)\nv8 13.7.152.6-rusty\ntypescript 5.8.3\n";
        assert_eq!(version_first_line(deno).as_deref(), Some("deno 2.3.1"));
        // 先頭の空行は飛ばして最初の非空行を取る
        assert_eq!(
            version_first_line(b"\n\n  2025.06.30  \nrest").as_deref(),
            Some("2025.06.30")
        );
        // 空出力・空白のみはバージョン不明として None
        assert_eq!(version_first_line(b""), None);
        assert_eq!(version_first_line(b"  \n \n"), None);
    }

    /// DB-DP-02: `setup.deps_shown` の解釈（FR-75、仕様決定 CZ）。
    /// on 系の値だけを「表示済み」として受理し、未設定・その他の値は
    /// 未表示（初回起動で依存検出画面を出す条件）とする。
    #[test]
    fn setup_deps_shown_enabled_values() {
        for on in ["on", "true", "1", "yes", "ON", " on "] {
            assert!(
                setup_deps_shown_enabled(Some(on.to_string())),
                "{on} は表示済み"
            );
        }
        for off in ["off", "false", "0", "no", "", "invalid"] {
            assert!(
                !setup_deps_shown_enabled(Some(off.to_string())),
                "{off} は未表示"
            );
        }
        assert!(!setup_deps_shown_enabled(None));
    }

    /// DB-DP-03: 存在しないコマンド名の検出は未検出として返す
    /// （FR-74、仕様決定 CY）。spawn 失敗をエラーにせず、
    /// detected=false・path/version=None の行になることを確認する。
    #[tokio::test]
    async fn probe_command_missing_returns_not_detected() {
        let st = probe_command("yt-browser-nonexistent-cmd-9f3b2c").await;
        assert!(!st.detected);
        assert_eq!(st.path, None);
        assert_eq!(st.version, None);
        assert_eq!(st.name, "yt-browser-nonexistent-cmd-9f3b2c");
    }
}
