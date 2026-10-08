//! mpv プロセス管理と JSON IPC クライアント（設計書 §4.1）。
//! 1 再生 = 1 mpv プロセス + 1 IPC ソケット。マルチビューはインスタンスを増やすだけで済む
//! （仕様決定 B / FR-1）。再生制御はすべてこのモジュールを経由する。

mod ipc;
mod manager;
pub(crate) mod player;
mod spawn;
mod terminal;

pub(crate) use ipc::IpcClient;
use ipc::{IpcError, IpcEvent};
pub use manager::PlayerManager;
use thiserror::Error;

/// 未設定時の画質式（設計書 §4.3 の 1080p 上限プリセット）。
pub(crate) const DEFAULT_YTDL_FORMAT: &str = "bv*[height<=1080]+ba/b[height<=1080]";

/// ホイール分岐スクリプト（設計書 §4.2）。バイナリに埋め込み、
/// 起動時に app_data/mpv/wheel.lua へ書き出して `--script` で読ませる。
pub const WHEEL_LUA: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/mpv/wheel.lua"));
/// 設定キー: ホイール音量の変化量（script-opts `wheel-volume_delta` に渡す）。
pub(crate) const SETTING_WHEEL_VOLUME_DELTA: &str = "wheel.volume_delta";
/// PiP 小窓の `--geometry` 値（設定キー `pip.geometry`）。
pub(crate) const SETTING_PIP_GEOMETRY: &str = "pip.geometry";
/// 設定が無い・不正なときの既定値。画面右下寄せの 480x270。
pub(crate) const DEFAULT_PIP_GEOMETRY: &str = "480x270-40-40";

/// `pip.geometry` / 既定値として受け付ける mpv geometry 形式
/// （`WxH` と任意の `+-x+-y` のみ。mpv に渡す値なので曖昧な入力を残さない）。
pub(crate) fn is_valid_pip_geometry(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^\d{2,5}x\d{2,5}([+-]\d{1,5}[+-]\d{1,5})?$").unwrap())
        .is_match(s)
}

#[derive(Debug, Error)]
pub enum MpvError {
    #[error("mpv の起動に失敗: {0}。mpv がインストールされているか確認してください")]
    Spawn(#[source] std::io::Error),
    #[error("mpv の IPC ソケットが時間内に作成されなかった（mpv 起動失敗の可能性）")]
    SocketTimeout,
    #[error("mpv IPC: {0}")]
    Ipc(#[from] ipc::IpcError),
    #[error("インスタンス {0} は存在しない（既に終了した可能性がある）")]
    NoSuchInstance(u32),
}

#[cfg(test)]
mod tests {
    use super::is_valid_pip_geometry;

    /// 設計書 §4.5 の `pip.geometry` 受理形式（mpv に渡す値なので
    /// WxH 必須・符号付き座標は任意・曖昧な入力は残さない）。
    #[test]
    fn pip_geometry_validation() {
        for ok in [
            "480x270",
            "480x270-40-40",
            "1920x1080+0+0",
            "640x360+200-100",
        ] {
            assert!(is_valid_pip_geometry(ok), "{ok} は受理されるべき");
        }
        for ng in [
            "",
            "480",
            "x270",
            "480x",
            "480x270+",
            "480x270+10",
            "abc x 123",
            "480x270+10+10; rm -rf",
            "480*270",
            "-480x270",
        ] {
            assert!(!is_valid_pip_geometry(ng), "{ng} は拒否されるべき");
        }
    }
}
