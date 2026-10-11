//! mpv のオーディオ出力デバイス列挙（FR-32、仕様決定 BA）。
//! `mpv --audio-device=help` の出力行を `{name, description}` へパースする。

use super::MpvError;
use crate::model::AudioDevice;

/// `mpv --audio-device=help` の出力を `{name, description}` 列へパースする。
/// 形式は ``  'name' (description)  `` の行のみを拾う（バナー行や空行は無視）。
/// description は行末の `(...)` 全体を取る（desc 自身が括弧を含みうるため
/// 最初の `(` でなく最後の対を剥がす）。`'auto'` のような説明なし行も
/// デバイスとして受理する。先頭空白付き以外の行は拾わない。
pub(crate) fn parse_audio_device_list(output: &str) -> Vec<AudioDevice> {
    let mut out = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        // 'name' のペアで始まる行だけがデバイス行
        let Some(rest) = line.strip_prefix('\'') else {
            continue;
        };
        let Some(end) = rest.find('\'') else {
            continue;
        };
        let name = &rest[..end];
        if name.is_empty() {
            continue;
        }
        let desc = rest[end + 1..].trim();
        // description は `(...)` の形のときだけ取る（`(` 始まり `)` 終わり）。
        // desc 内の括弧に対応するため、末尾が ')' の行では最初の '(' から
        // 末尾までをまるごと説明として扱う
        let description = if desc.starts_with('(') && desc.ends_with(')') && desc.len() >= 2 {
            desc[1..desc.len() - 1].to_string()
        } else {
            String::new()
        };
        out.push(AudioDevice {
            name: name.to_string(),
            description,
        });
    }
    out
}

/// `mpv --audio-device=help` を実行してデバイス列を返す。
/// mpv が見つからない場合は MpvError::Spawn、非零終了や出力が空なら空列。
pub(crate) async fn list_audio_devices() -> Result<Vec<AudioDevice>, MpvError> {
    let mut cmd = tokio::process::Command::new("mpv");
    cmd.arg("--audio-device=help");
    #[cfg(windows)]
    cmd.creation_flags(crate::CREATE_NO_WINDOW);
    let output = cmd.output().await.map_err(MpvError::Spawn)?;
    if !output.status.success() {
        return Ok(Vec::new());
    }
    // mpv は UTF-8 で出力する。壊れたバイト列は lossy で読み、
    // パースできない行は parse 側が捨てる
    Ok(parse_audio_device_list(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(test)]
mod tests {
    use super::parse_audio_device_list;

    /// `mpv --audio-device=help` の実出力形（mpv 0.41、Windows/WASAPI）。
    /// `'name' (desc)` 形式で、desc 自身が括弧を含みうる。
    #[test]
    fn audio_device_list_parses_real_output() {
        let out = "mpv v0.41.0-244-gaf9c81fa1 Copyright\n\
                   === audio-device=help ===\n\
                   List of detected audio devices:\n\
                     'auto' (Autoselect device)\n\
                     'wasapi/{518d0545-7f74-4937-8b72-f451bd455d1a}' (MOTIV Mix Virtual Input (Shure Virtual Audio))\n\
                     'wasapi/{5d72021e-7c8c-4fd1-8574-f7c09ba4b4ce}' (ヘッドホン (2- Shure MV7+))\n\
                     'openal' (Default (openal))\n";
        let devs = parse_audio_device_list(out);
        assert_eq!(devs.len(), 4);
        assert_eq!(devs[0].name, "auto");
        assert_eq!(devs[0].description, "Autoselect device");
        assert_eq!(
            devs[1].name,
            "wasapi/{518d0545-7f74-4937-8b72-f451bd455d1a}"
        );
        assert_eq!(
            devs[1].description,
            "MOTIV Mix Virtual Input (Shure Virtual Audio)"
        );
        assert_eq!(devs[2].description, "ヘッドホン (2- Shure MV7+)");
        assert_eq!(devs[3].name, "openal");
    }

    /// 説明なし・壊れた行の取りこぼし（パース不能行は拾わない）。
    #[test]
    fn audio_device_list_skips_malformed() {
        let out = "noise line\n\
                     'bare'\n\
                     'x' (desc\n\
                     '' (empty name)\n\
                     'dev' trailing\n";
        let devs = parse_audio_device_list(out);
        assert_eq!(devs.len(), 3);
        assert_eq!(devs[0].name, "bare");
        assert_eq!(devs[0].description, "");
        // `(desc` は閉じ括弧が無いため description なし（name 自体は拾う）
        assert_eq!(devs[1].name, "x");
        assert_eq!(devs[1].description, "");
        // `trailing` は `(...)` 形でないため description なし
        assert_eq!(devs[2].name, "dev");
        assert_eq!(devs[2].description, "");
    }
}
