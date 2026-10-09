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
/// 設定キー: 再生の既定表示を PiP 小窓にするか（仕様決定 AJ）。
/// 値は "on" / "off"。未設定・その他の値は "on"（PiP が既定）として扱う。
pub(crate) const SETTING_PIP_DEFAULT: &str = "pip.default";
/// 設定キー: PiP 小窓のサイズを動画のアスペクト比に追従させるか（仕様決定 AL）。
/// 値は "on" / "off"。未設定・その他の値は "on"（追従する）として扱う。
pub(crate) const SETTING_PIP_FIT_ASPECT: &str = "pip.fit_aspect";
/// 設定キー: HDR→SDR 変換のトーンマッピング方式（mpv `--tone-mapping`、仕様決定 Y）。
pub(crate) const SETTING_HDR_TONE_MAPPING: &str = "hdr.tone_mapping";
/// 設定キー: HDR ピーク輝度のフレーム計測（mpv `--hdr-compute-peak`、仕様決定 Y）。
pub(crate) const SETTING_HDR_COMPUTE_PEAK: &str = "hdr.compute_peak";
/// 設定キー: mpv への追加引数（空白区切りで spawn 引数の末尾へ、仕様決定 Y）。
pub(crate) const SETTING_MPV_EXTRA_ARGS: &str = "mpv.extra_args";
/// 設定が無い・不正なときの既定値。画面右下寄せの 480x270。
pub(crate) const DEFAULT_PIP_GEOMETRY: &str = "480x270-40-40";

/// `pip.geometry` / 既定値として受け付ける mpv geometry 形式
/// （`WxH` と任意の `+-x+-y` のみ。mpv に渡す値なので曖昧な入力を残さない）。
pub(crate) fn is_valid_pip_geometry(s: &str) -> bool {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"^\d{2,5}x\d{2,5}([+-]\d{1,5}[+-]\d{1,5})?$").unwrap())
        .is_match(s)
}

/// `pip.default` の保存値を解釈する（仕様決定 AJ）。
/// `off` / `false` / `0` / `no`（大小文字・前後空白を許容）だけが
/// 通常窓既定（false）で、それ以外と未設定は PiP 既定（true）を返す。
/// フロント側の同一解釈は `src/lib/pip.ts` の `pipDefaultEnabled`。
pub(crate) fn pip_default_enabled(value: Option<String>) -> bool {
    match value {
        Some(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "false" | "0" | "no"
        ),
        None => true,
    }
}

/// `pip.fit_aspect` の保存値を解釈する（仕様決定 AL）。
/// `pip.default` と同じ受理集合: `off` / `false` / `0` / `no`
/// （大小文字・前後空白を許容）だけが追従なし（false）で、
/// それ以外と未設定は追従あり（true）を返す。
/// フロント側の同一解釈は `src/lib/pip.ts` の `pipFitAspectEnabled`。
pub(crate) fn pip_fit_aspect_enabled(value: Option<String>) -> bool {
    pip_default_enabled(value)
}

/// `WxH±x±y` 形式の geometry 文字列を `(w, h, 位置サフィックス)` に分解する。
/// `is_valid_pip_geometry` の受理形式のみ前提（呼び出し側で検証済み）。
/// 位置部が無い入力は `""` を返す。不正な入力は None。
fn split_pip_geometry(s: &str) -> Option<(u32, u32, &str)> {
    // WxH 部は数字と `x` だけなので、最初の `+`/`-` が位置部の始まり
    let pos_at = s.find(['+', '-']).unwrap_or(s.len());
    let (wh, pos) = s.split_at(pos_at);
    let (w, h) = wh.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?, pos))
}

/// PiP 小窓のサイズを動画のアスペクト比へ追従させる（仕様決定 AL、FR-19）。
/// `base`（`pip.geometry` の `WxH±x±y`）の WxH を上限枠として、
/// 表示アスペクト比を保ったまま内接するサイズに丸める。位置サフィックスは
/// そのまま維持する（隅アンカーが変わらない）。`aspect` が未検出・不正
/// （0 以下・非有限）なら `base` をそのまま返す。
pub(crate) fn fit_pip_geometry(base: &str, aspect: Option<f64>) -> String {
    let Some((w, h, pos)) = split_pip_geometry(base) else {
        return base.to_string();
    };
    let Some(ar) = aspect.filter(|a| a.is_finite() && *a > 0.0) else {
        return base.to_string();
    };
    let box_ar = f64::from(w) / f64::from(h);
    let (nw, nh) = if ar >= box_ar {
        (w, (f64::from(w) / ar).round().max(1.0) as u32)
    } else {
        ((f64::from(h) * ar).round().max(1.0) as u32, h)
    };
    format!("{nw}x{nh}{pos}")
}

/// `hdr.tone_mapping` として受理する値（mpv 0.41 の列挙値のうち、
/// 本アプリが GUI で出す実用的な集合）。`auto` と空文字は未指定として
/// mpv 既定（環境に応じた自動）に任せるためここには含めない。
pub(crate) fn is_valid_tone_mapping(s: &str) -> bool {
    matches!(
        s,
        "clip"
            | "mobius"
            | "reinhard"
            | "hable"
            | "gamma"
            | "linear"
            | "spline"
            | "bt.2390"
            | "bt.2446a"
    )
}

/// `hdr.compute_peak` として受理する値。`auto` と空文字は未指定扱いで
/// mpv 既定（ターゲットが HDR のときのみ計測）に任せる。
pub(crate) fn is_valid_hdr_compute_peak(s: &str) -> bool {
    matches!(s, "yes" | "no")
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
    use super::{
        fit_pip_geometry, is_valid_hdr_compute_peak, is_valid_pip_geometry,
        is_valid_tone_mapping, pip_default_enabled, pip_fit_aspect_enabled,
        player::split_extra_args,
    };

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

    /// `pip.default` の解釈（仕様決定 AJ）。off 系のみ通常窓既定、
    /// それ以外と未設定は PiP 既定。
    #[test]
    fn pip_default_enabled_values() {
        for off in ["off", "OFF", " false ", "0", "no"] {
            assert!(
                !pip_default_enabled(Some(off.to_string())),
                "{off} は通常窓既定になるべき"
            );
        }
        for on in ["on", "true", "1", "yes", "", "  ", "invalid"] {
            assert!(
                pip_default_enabled(Some(on.to_string())),
                "{on} は PiP 既定になるべき"
            );
        }
        assert!(pip_default_enabled(None));
    }

    /// DB-PF-01: `pip.fit_aspect` の解釈（仕様決定 AL）。off 系のみ追従なし、
    /// それ以外と未設定は追従あり（既定 on）。
    #[test]
    fn pip_fit_aspect_enabled_values() {
        for off in ["off", "OFF", " false ", "0", "no"] {
            assert!(
                !pip_fit_aspect_enabled(Some(off.to_string())),
                "{off} は追従なしになるべき"
            );
        }
        for on in ["on", "true", "1", "yes", "", "  ", "invalid"] {
            assert!(
                pip_fit_aspect_enabled(Some(on.to_string())),
                "{on} は追従ありになるべき"
            );
        }
        assert!(pip_fit_aspect_enabled(None));
    }

    /// DB-PF-02: 枠内フィット（FR-19）。枠より横長の動画は高さが縮み、
    /// 位置サフィックスは維持される。
    #[test]
    fn fit_pip_geometry_wide() {
        // 2.39:1 相当（854x358）を 480x270 枠へ内接 → 480x201
        assert_eq!(
            fit_pip_geometry("480x270-40-40", Some(854.0 / 358.0)),
            "480x201-40-40"
        );
        // 位置部なしの geometry も受理される
        assert_eq!(
            fit_pip_geometry("480x270", Some(854.0 / 358.0)),
            "480x201"
        );
    }

    /// DB-PF-03: 枠内フィット（FR-19）。枠より縦長の動画は幅が縮む。
    #[test]
    fn fit_pip_geometry_tall() {
        // 9:16（0.5625）を 480x270 枠へ内接 → 152x270
        assert_eq!(
            fit_pip_geometry("480x270-40-40", Some(9.0 / 16.0)),
            "152x270-40-40"
        );
        // 1:1 は 270x270
        assert_eq!(
            fit_pip_geometry("480x270-40-40", Some(1.0)),
            "270x270-40-40"
        );
    }

    /// DB-PF-04: 枠と同じ比率（16:9）はそのまま。丸めで 1px 未満にならない。
    #[test]
    fn fit_pip_geometry_same_and_tiny() {
        assert_eq!(
            fit_pip_geometry("480x270-40-40", Some(16.0 / 9.0)),
            "480x270-40-40"
        );
        // 極端な横長（100:1）でも高さは最低 1px を維持する
        assert_eq!(
            fit_pip_geometry("480x270-40-40", Some(100.0)),
            "480x5-40-40"
        );
    }

    /// DB-PF-05: アスペクト未取得・不正値・不正 geometry はベース値をそのまま返す。
    #[test]
    fn fit_pip_geometry_fallback() {
        for base in ["480x270-40-40", "640x360"] {
            for bad in [None, Some(0.0), Some(-1.0), Some(f64::NAN)] {
                assert_eq!(
                    fit_pip_geometry(base, bad),
                    base,
                    "{base} / {bad:?} はベース値を返すべき"
                );
            }
        }
        // 受理形式でない文字列は無加工で返す（呼び出し側は検証済み前提だが
        // 壊れた値を増幅させない）
        assert_eq!(fit_pip_geometry("garbage", Some(1.0)), "garbage");
    }

    /// HDR 設定の受理集合（仕様決定 Y）。`auto`/空/未定義値は未指定扱いにするため拒否。
    #[test]
    fn hdr_settings_validation() {
        for ok in [
            "clip", "mobius", "reinhard", "hable", "gamma", "linear", "spline", "bt.2390",
            "bt.2446a",
        ] {
            assert!(is_valid_tone_mapping(ok), "{ok} は受理されるべき");
        }
        for ng in ["", "auto", "yes", "no", "ACES", "hable; rm -rf"] {
            assert!(!is_valid_tone_mapping(ng), "{ng} は拒否されるべき");
        }
        for ok in ["yes", "no"] {
            assert!(is_valid_hdr_compute_peak(ok), "{ok} は受理されるべき");
        }
        for ng in ["", "auto", "true", "1", "clip"] {
            assert!(!is_valid_hdr_compute_peak(ng), "{ng} は拒否されるべき");
        }
    }

    /// `mpv.extra_args` の分割（仕様決定 Y）。引用符内の空白は保持し、
    /// バックスラッシュはエスケープに解釈しない（Windows パスをそのまま書ける）。
    /// 引用開始は引数先頭または `=` 直後のみで、値の途中の引用符はリテラル。
    #[test]
    fn extra_args_split() {
        // 単純な空白区切り
        assert_eq!(
            split_extra_args("--target-colorspace-hint=yes --gpu-api=d3d11"),
            vec!["--target-colorspace-hint=yes", "--gpu-api=d3d11"]
        );
        // 空白を含む値は引用符で 1 引数にまとまる（ICC プロファイルパス想定）
        assert_eq!(
            split_extra_args(r#"--icc-profile="C:\Color Profiles\display.icc" --flag"#),
            vec![r#"--icc-profile=C:\Color Profiles\display.icc"#, "--flag"]
        );
        // 単引用符も同様。連続空白・前後空白は無視
        assert_eq!(
            split_extra_args("  --a='x y'   --b  "),
            vec!["--a=x y", "--b"]
        );
        // 未終端の引用符は残り全体を 1 引数として扱う
        assert_eq!(split_extra_args("--a=\"x y"), vec!["--a=x y"]);
        // 値の途中のアポストロフィはリテラル（引用開始は先頭または `=` 直後のみ）
        assert_eq!(
            split_extra_args("--icc-profile=/home/O'Brien/d.icc --gpu-api=opengl"),
            vec!["--icc-profile=/home/O'Brien/d.icc", "--gpu-api=opengl"]
        );
        // 引数先頭の引用符は区切りとして働く
        assert_eq!(split_extra_args("--a 'x y' --b"), vec!["--a", "x y", "--b"]);
        // 空・空白のみは引数なし
        assert!(split_extra_args("").is_empty());
        assert!(split_extra_args("   ").is_empty());
    }
}
