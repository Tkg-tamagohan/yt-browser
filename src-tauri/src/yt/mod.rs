//! yt-dlp 子プロセスの呼び出し（仕様決定 A / 技術方針 P、設計書 §5）。
//! Phase 1 ではパス解決・バージョン確認・`yt-dlp -U` 更新のみを提供する。
//! 検索・メタデータ取得は `YoutubeBackend` トレイトとして Phase 5 で実装する。

use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;

use crate::db::Db;

/// yt-dlp 呼び出しのタイムアウト。`-U` や `-J` はネットワーク依存なので長めに取る。
const PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

/// 設定キー: yt-dlp のパスをユーザーが明示する場合の上書き設定。
pub const SETTING_YTDLP_PATH: &str = "ytdlp.path";
/// 設定キー: 画質式（`ytdl-format`）のユーザー上書き。
pub const SETTING_QUALITY_FORMAT: &str = "quality.format";

#[derive(Debug, Error)]
pub enum YtError {
    #[error("yt-dlp の起動に失敗: {0}")]
    Spawn(#[source] std::io::Error),
    #[error("yt-dlp が終了コード {code} で失敗: {stderr}")]
    Exit { code: i32, stderr: String },
    #[error("yt-dlp が時間内に応答しなかった")]
    Timeout,
    #[error("yt-dlp が見つからない。インストールするか設定 `ytdlp.path` でパスを指定してください")]
    NotFound,
}

/// yt-dlp のバイナリパス解決器。
///
/// 解決順（Phase 1 で確定。決定記録「Phase 1 で確定した事項」参照）:
/// 1. `settings.ytdlp_path`（`ytdlp.path` のユーザー指定）
/// 2. 同梱リソースの `yt-dlp`（配布フェーズで bundle に同梱する想定）
/// 3. PATH 上の `yt-dlp`（システムインストールのフォールバック）
#[derive(Clone)]
pub struct YtDlpResolver {
    /// 同梱リソース探索用のディレクトリ。Tauri の resource_dir を起動時に確定して保持する。
    resource_dir: Option<std::path::PathBuf>,
}

impl YtDlpResolver {
    pub fn new(resource_dir: Option<std::path::PathBuf>) -> Self {
        Self { resource_dir }
    }

    /// 利用する yt-dlp を解決する。見つからなければ None。
    /// 戻り値は実行可能なパスまたは `yt-dlp`（PATH 解決名）。
    pub async fn resolve(&self, db: &Db) -> Option<String> {
        // 1. 設定による明示パス
        if let Ok(Some(path)) = db.setting_get(SETTING_YTDLP_PATH) {
            if !path.is_empty() {
                return Some(path);
            }
        }
        // 2. 同梱リソース
        if let Some(dir) = &self.resource_dir {
            let name = if cfg!(windows) {
                "yt-dlp.exe"
            } else {
                "yt-dlp"
            };
            let bundled = dir.join(name);
            if bundled.exists() {
                return Some(bundled.to_string_lossy().into_owned());
            }
        }
        // 3. PATH
        if which_exists("yt-dlp").await {
            return Some("yt-dlp".to_string());
        }
        None
    }

    /// `ytdlp_status` 用に解決パスとバージョンを得る。
    pub async fn status(&self, db: &Db) -> (Option<String>, Option<String>) {
        let path = self.resolve(db).await;
        let version = match &path {
            Some(p) => version(p).await.ok(),
            None => None,
        };
        (path, version)
    }
}

/// `yt-dlp --version` を実行してバージョン文字列を返す。
pub async fn version(path: &str) -> Result<String, YtError> {
    let out = run_with_timeout(Command::new(path).arg("--version")).await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(500)
                .collect(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `yt-dlp -U`（セルフアップデート）を実行して出力末尾を返す。
/// システム管理のパスでは権限不足で失敗し得る。その場合も stderr を拾って返す。
pub async fn update(path: &str) -> Result<String, YtError> {
    let out = run_with_timeout(Command::new(path).arg("-U")).await?;
    if !out.status.success() {
        return Err(YtError::Exit {
            code: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(500)
                .collect(),
        });
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(stdout.trim().chars().take(500).collect())
}

/// PATH 解決可否。`--version` が起動できれば存在するとみなす。
async fn which_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn run_with_timeout(cmd: &mut Command) -> Result<std::process::Output, YtError> {
    match tokio::time::timeout(PROCESS_TIMEOUT, cmd.output()).await {
        Ok(res) => res.map_err(YtError::Spawn),
        Err(_) => Err(YtError::Timeout),
    }
}
