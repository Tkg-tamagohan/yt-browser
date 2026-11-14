//! チャットポップアップ窓の位置・サイズと表示設定の解釈
//! （FR-67・FR-68、仕様決定 CR・CS）。
//!
//! `chat.popup_geometry` は `chat_popup_open` が窓を立てるときに読み、
//! 移動・リサイズの窓イベントで [`PopupGeomTracker`] 経由で保存する。
//! 表示設定（`chat.popup_show_author` / `chat.popup_font_size` /
//! `chat.popup_opacity`）はポップアップ窓側のフロントが `settings_get`
//! で読んで反映するため、ここにあるのは解釈の正だけで、窓生成経路は
//! これらの値を参照しない。

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// `chat.popup_geometry` の値として扱う窓の位置とサイズ。
/// 単位は論理ピクセル（`Moved` / `Resized` イベントの物理値を
/// `scale_factor` で換算して保存し、`WebviewWindowBuilder` の
/// `.position` / `.inner_size` が論理値を取るため往復で素直に復元できる）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PopupGeometry {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// `PopupGeometry` を `WxH+x+y` 形式の保存文字列にする（x・y は符号付き）。
/// `pip.geometry`（設計書 §4.5）と同じ見た目の形式で、位置部が必須な点だけ
/// 異なる（ポップアップは位置も復元するため）。
pub(crate) fn format_popup_geometry(g: &PopupGeometry) -> String {
    format!("{}x{}{:+}{:+}", g.w, g.h, g.x, g.y)
}

/// `chat.popup_geometry` の保存値を解釈する（FR-67、仕様決定 CR）。
/// 受理形式は `WxH+x+y`（W・H は 2〜5 桁、x・y は符号付き 1〜5 桁の
/// 論理ピクセル）。未設定・空・形式外は None（既定の位置とサイズで開く）。
pub(crate) fn parse_popup_geometry(value: Option<String>) -> Option<PopupGeometry> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^(\d{2,5})x(\d{2,5})([+-]\d{1,5})([+-]\d{1,5})$").unwrap()
    });
    let value = value?;
    let caps = re.captures(value.trim())?;
    Some(PopupGeometry {
        w: caps[1].parse().ok()?,
        h: caps[2].parse().ok()?,
        x: caps[3].parse().ok()?,
        y: caps[4].parse().ok()?,
    })
}

/// 画面外復元のガードで要求する、モニターとの重なりの最小辺長（論理
/// ピクセル）。これ未満の重なりではタイトルバーを掴めず窓を取り戻せない
/// 可能性があるため画面外とみなす（暫定値）。
pub(crate) const POPUP_MIN_VISIBLE_PX: i64 = 48;

/// 保存した geometry が復元先として妥当か（画面外復元のガード、FR-67）。
/// `monitors` は `(x, y, w, h)` の論理ピクセル矩形列。保存位置とサイズの
/// 矩形がいずれかのモニターと縦横とも [`POPUP_MIN_VISIBLE_PX`] 以上
/// 重なれば真。モニター列挙が空（取得失敗など）の場合は判定不能として
/// 真を返す（保存値を信じる。Wayland 等で位置復元が効かない環境は
/// OS 側に委ねる）。
pub(crate) fn popup_geometry_visible(g: &PopupGeometry, monitors: &[(i32, i32, u32, u32)]) -> bool {
    if monitors.is_empty() {
        return true;
    }
    let (gx1, gy1) = (i64::from(g.x), i64::from(g.y));
    let (gx2, gy2) = (gx1 + i64::from(g.w), gy1 + i64::from(g.h));
    monitors.iter().any(|&(mx, my, mw, mh)| {
        let (mx2, my2) = (i64::from(mx) + i64::from(mw), i64::from(my) + i64::from(mh));
        let overlap_w = gx2.min(mx2) - gx1.max(i64::from(mx));
        let overlap_h = gy2.min(my2) - gy1.max(i64::from(my));
        overlap_w >= POPUP_MIN_VISIBLE_PX && overlap_h >= POPUP_MIN_VISIBLE_PX
    })
}

/// 移動・リサイズイベントごとの DB 書き込み間隔の下限。ドラッグ中は
/// 連続してイベントが届くため、末尾は [`PopupGeomTracker::take_pending`]
/// の force 呼び出し（窓の破棄時）で確定させる。
const GEOM_WRITE_THROTTLE: Duration = Duration::from_millis(400);

/// ポップアップ窓の移動・リサイズで更新される最新の geometry と、
/// DB 書き込みの間引き状態（FR-67、仕様決定 CR）。
/// イベントが片方の値（位置またはサイズ）しか持たないため、両方を
/// 記憶してマージした `WxH+x+y` を保存する。片方が欠けたままでは
/// 未完成として書き込まない（破棄時も同様に欠けていれば保存しない）。
pub(crate) struct PopupGeomTracker {
    inner: Mutex<PopupGeomInner>,
}

#[derive(Debug)]
struct PopupGeomInner {
    pos: Option<(i32, i32)>,
    size: Option<(u32, u32)>,
    dirty: bool,
    last_write: Instant,
}

impl PopupGeomTracker {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(PopupGeomInner {
                pos: None,
                size: None,
                dirty: false,
                last_write: Instant::now(),
            }),
        }
    }

    /// 移動イベントの位置（論理ピクセル）を記録する。
    pub(crate) fn note_moved(&self, x: i32, y: i32) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.pos = Some((x, y));
            inner.dirty = true;
        }
    }

    /// リサイズイベントのサイズ（論理ピクセル）を記録する。
    pub(crate) fn note_resized(&self, w: u32, h: u32) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.size = Some((w, h));
            inner.dirty = true;
        }
    }

    /// 未記録側の位置を埋める（イベントで位置が一度も来なかった場合に
    /// 窓の現在地から補うためのもの。更新自体は dirty にしない）。
    pub(crate) fn needs_pos(&self) -> bool {
        self.inner.lock().is_ok_and(|i| i.pos.is_none())
    }

    pub(crate) fn fill_pos(&self, x: i32, y: i32) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.pos.is_none() {
                inner.pos = Some((x, y));
            }
        }
    }

    /// 未記録側のサイズを埋める（[`Self::fill_pos`] と同じ用途）。
    pub(crate) fn needs_size(&self) -> bool {
        self.inner.lock().is_ok_and(|i| i.size.is_none())
    }

    pub(crate) fn fill_size(&self, w: u32, h: u32) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.size.is_none() {
                inner.size = Some((w, h));
            }
        }
    }

    /// 保存すべき変更があれば `WxH+x+y` 文字列を返し、未保存フラグを
    /// 下ろす。`force` なら間引き期間を待たない。変更が無い・まだ
    /// 間引き期間内・位置かサイズが未完成なら None を返す
    /// （未完成の場合は dirty を維持し、後続のイベントで完成を待つ）。
    pub(crate) fn take_pending(&self, force: bool) -> Option<String> {
        let mut inner = self.inner.lock().ok()?;
        if !inner.dirty {
            return None;
        }
        if !force && inner.last_write.elapsed() < GEOM_WRITE_THROTTLE {
            return None;
        }
        let (x, y) = inner.pos?;
        let (w, h) = inner.size?;
        inner.dirty = false;
        inner.last_write = Instant::now();
        Some(format_popup_geometry(&PopupGeometry { x, y, w, h }))
    }
}

/// `chat.popup_show_author` の保存値を解釈する（FR-68、仕様決定 CS）。
/// `on` / `true` / `1` / `yes`（大小文字・前後空白を許容）のときだけ
/// 投稿者名を表示し、未設定・その他の値は従来どおり非表示。
/// 受理集合は `player.close_on_eof`・`feed.show_shorts` と同一
/// （明示的な on 系値のみ真）。
/// 表示側だけの設定でバックエンドでは参照しないが、解釈の正をここに置き
/// フロント側の `chat-popup-logic.ts` が同一解釈を持つ（テストで検証）。
#[allow(dead_code)]
pub(crate) fn popup_show_author_enabled(value: Option<String>) -> bool {
    match value {
        Some(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "on" | "true" | "1" | "yes"
        ),
        None => false,
    }
}

/// `chat.popup_font_size` の既定値（px、暫定）。既存の行高
/// （`.chat-item` の 0.88rem ≒ 14px）に合わせる。
#[allow(dead_code)]
pub(crate) const DEFAULT_POPUP_FONT_SIZE: f64 = 14.0;
/// `chat.popup_font_size` の受理範囲（px、暫定）。
#[allow(dead_code)]
pub(crate) const POPUP_FONT_SIZE_RANGE: std::ops::RangeInclusive<f64> = 10.0..=32.0;

/// `chat.popup_font_size` の保存値を解釈する（FR-68、仕様決定 CS）。
/// 受理範囲内の数値だけを採用し、未設定・空・非数・範囲外・非有限は
/// 既定値へ戻す。解釈は `player.initial_speed` と同じ形式。
/// バックエンドでは参照しない（解釈の正のみ置く。理由は上と同じ）。
#[allow(dead_code)]
pub(crate) fn popup_font_size(value: Option<String>) -> f64 {
    value
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && POPUP_FONT_SIZE_RANGE.contains(v))
        .unwrap_or(DEFAULT_POPUP_FONT_SIZE)
}

/// `chat.popup_opacity` の既定値（暫定）。既存は完全不透明なので 1.0。
#[allow(dead_code)]
pub(crate) const DEFAULT_POPUP_OPACITY: f64 = 1.0;
/// `chat.popup_opacity` の受理範囲（暫定）。下限は窓を見失わない程度の
/// 0.2 とする（0 に近い値で画面から消える事故を防ぐ）。
#[allow(dead_code)]
pub(crate) const POPUP_OPACITY_RANGE: std::ops::RangeInclusive<f64> = 0.2..=1.0;

/// `chat.popup_opacity` の保存値を解釈する（FR-68、仕様決定 CS）。
/// 範囲内の数値だけを採用し、未設定・空・非数・範囲外・非有限は既定値へ
/// 戻す。不透明度は窓枠ではなくポップアップ内容の CSS opacity に
/// 適用する（暫定解釈。OS の窓レベル半透明化は対象外）。
/// バックエンドでは参照しない（解釈の正のみ置く。理由は上と同じ）。
#[allow(dead_code)]
pub(crate) fn popup_opacity(value: Option<String>) -> f64 {
    value
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && POPUP_OPACITY_RANGE.contains(v))
        .unwrap_or(DEFAULT_POPUP_OPACITY)
}

#[cfg(test)]
mod tests {
    use super::{
        format_popup_geometry, parse_popup_geometry, popup_font_size, popup_geometry_visible,
        popup_opacity, popup_show_author_enabled, PopupGeomTracker, PopupGeometry,
        DEFAULT_POPUP_FONT_SIZE, DEFAULT_POPUP_OPACITY,
    };

    /// DB-CP-01: `chat.popup_geometry` の受理形式（FR-67、仕様決定 CR）。
    /// `WxH+x+y`（符号付き座標必須）だけを受理し、未設定・空・形式外は None。
    #[test]
    fn parse_popup_geometry_values() {
        assert_eq!(
            parse_popup_geometry(Some("360x560+100+50".to_string())),
            Some(PopupGeometry {
                w: 360,
                h: 560,
                x: 100,
                y: 50
            })
        );
        // 負座標（プライマリより左上のモニター）も受理する
        assert_eq!(
            parse_popup_geometry(Some("360x560-120+8".to_string())),
            Some(PopupGeometry {
                w: 360,
                h: 560,
                x: -120,
                y: 8
            })
        );
        // 前後空白は落として解釈する
        assert_eq!(
            parse_popup_geometry(Some("  360x560+0+0  ".to_string())),
            Some(PopupGeometry {
                w: 360,
                h: 560,
                x: 0,
                y: 0
            })
        );
        for ng in [
            "",
            "   ",
            "360x560",      // 位置部は必須（復元に必要）
            "360x560+100",  // 座標が片方だけ
            "360x560+100+", // 座標が途中で切れた
            "x560+100+50",  // 幅が欠落
            "360x+100+50",  // 高さが欠落
            "9x9+0+0",      // 1 桁は受理しない（pip.geometry と同じ下限制約）
            "abc x 123",
            "360x560+10+10; rm",
            "360*560+10+10",
            "-360x560+10+10",
        ] {
            assert_eq!(
                parse_popup_geometry(Some(ng.to_string())),
                None,
                "{ng} は拒否されるべき"
            );
        }
        assert_eq!(parse_popup_geometry(None), None);
    }

    /// DB-CP-02: 保存文字列の往復と画面外ガード（FR-67、仕様決定 CR）。
    /// モニターと十分に重なる位置だけを復元対象とし、
    /// 外れた位置や僅かな重なりは位置復元を見送る。
    #[test]
    fn popup_geometry_roundtrip_and_visibility() {
        let g = PopupGeometry {
            w: 360,
            h: 560,
            x: -120,
            y: 8,
        };
        let s = format_popup_geometry(&g);
        assert_eq!(s, "360x560-120+8");
        assert_eq!(parse_popup_geometry(Some(s)), Some(g));

        // フル HD 1 面 + 右側にもう 1 面（論理ピクセル）
        let monitors = [(0_i32, 0_i32, 1920_u32, 1080_u32), (1920, 0, 1920, 1080)];
        let inside = PopupGeometry {
            w: 360,
            h: 560,
            x: 100,
            y: 50,
        };
        assert!(popup_geometry_visible(&inside, &monitors));
        // 右モニターに掛かる位置も復元対象
        let right = PopupGeometry {
            w: 360,
            h: 560,
            x: 1900,
            y: 10,
        };
        assert!(popup_geometry_visible(&right, &monitors));
        // 完全に画面外は復元しない
        for g in [
            PopupGeometry {
                w: 360,
                h: 560,
                x: -2000,
                y: 0,
            },
            PopupGeometry {
                w: 360,
                h: 560,
                x: 5000,
                y: 0,
            },
            PopupGeometry {
                w: 360,
                h: 560,
                x: 0,
                y: -800,
            },
            PopupGeometry {
                w: 360,
                h: 560,
                x: 0,
                y: 5000,
            },
        ] {
            assert!(!popup_geometry_visible(&g, &monitors), "{g:?} は画面外扱い");
        }
        // 端に僅かだけ残る位置（重なりが POPUP_MIN_VISIBLE_PX 未満）も画面外扱い
        let sliver = PopupGeometry {
            w: 360,
            h: 560,
            x: -330, // 右端が 30px だけモニター内
            y: 0,
        };
        assert!(!popup_geometry_visible(&sliver, &monitors));
        // モニター列挙が空のときは判定不能として保存値を信じる
        assert!(popup_geometry_visible(&inside, &[]));
    }

    /// DB-CP-03: トラッカーの位置・サイズ統合と保存間引き（FR-67）。
    /// 両方が揃うまで保存せず、force（窓の破棄時）なら即座に吐き出す。
    #[test]
    fn popup_geom_tracker_pending() {
        let t = PopupGeomTracker::new();
        // 位置だけでは未完成で何も取り出せない（force でも同じ）
        t.note_moved(10, 20);
        assert_eq!(t.take_pending(true), None);
        // サイズが揃うと force で吐き出せる
        t.note_resized(360, 560);
        assert_eq!(t.take_pending(true), Some("360x560+10+20".to_string()));
        // 取り出し済みは dirty が下りて二度目は返さない
        assert_eq!(t.take_pending(true), None);
        // 変更直後（間引き期間内）は force 無しでは返さない
        t.note_moved(30, 40);
        assert_eq!(t.take_pending(false), None);
        // force なら直後でも返す
        assert_eq!(t.take_pending(true), Some("360x560+30+40".to_string()));
        // イベントで片側が欠けた経路を窓の現在地で補完する fill_* 系
        let t2 = PopupGeomTracker::new();
        t2.fill_pos(5, 6);
        t2.fill_size(300, 400);
        // fill だけでは dirty にならない（保存する変更ではない）
        assert_eq!(t2.take_pending(true), None);
        t2.note_resized(320, 420);
        assert_eq!(t2.take_pending(true), Some("320x420+5+6".to_string()));
    }

    /// DB-CP-04: `chat.popup_show_author` の解釈（FR-68、仕様決定 CS）。
    /// 明示的な on 系値のみ投稿者名表示で、未設定・その他は従来どおり非表示。
    #[test]
    fn popup_show_author_values() {
        for on in ["on", "ON", " true ", "1", "yes"] {
            assert!(
                popup_show_author_enabled(Some(on.to_string())),
                "{on} は投稿者名を表示すべき"
            );
        }
        for off in ["off", "false", "0", "no", "", "  ", "invalid"] {
            assert!(
                !popup_show_author_enabled(Some(off.to_string())),
                "{off} は非表示になるべき"
            );
        }
        assert!(!popup_show_author_enabled(None));
    }

    /// DB-CP-05: `chat.popup_font_size` の解釈（FR-68、仕様決定 CS）。
    /// 10〜32 の数値だけを受理し、未設定・空・非数・範囲外は既定 14 へ戻す。
    #[test]
    fn popup_font_size_values() {
        assert_eq!(popup_font_size(None), DEFAULT_POPUP_FONT_SIZE);
        assert_eq!(popup_font_size(Some("18".to_string())), 18.0);
        assert_eq!(popup_font_size(Some("  16.5 ".to_string())), 16.5);
        assert_eq!(popup_font_size(Some("10".to_string())), 10.0);
        assert_eq!(popup_font_size(Some("32".to_string())), 32.0);
        for bad in ["", "  ", "abc", "-1", "9", "33", "1e3", "NaN", "inf"] {
            assert_eq!(
                popup_font_size(Some(bad.to_string())),
                DEFAULT_POPUP_FONT_SIZE,
                "{bad} は既定値へ戻すべき"
            );
        }
    }

    /// DB-CP-06: `chat.popup_opacity` の解釈（FR-68、仕様決定 CS）。
    /// 0.2〜1.0 の数値だけを受理し、未設定・空・非数・範囲外は既定 1.0 へ戻す。
    #[test]
    fn popup_opacity_values() {
        assert_eq!(popup_opacity(None), DEFAULT_POPUP_OPACITY);
        assert_eq!(popup_opacity(Some("0.5".to_string())), 0.5);
        assert_eq!(popup_opacity(Some("  0.75 ".to_string())), 0.75);
        assert_eq!(popup_opacity(Some("0.2".to_string())), 0.2);
        assert_eq!(popup_opacity(Some("1".to_string())), 1.0);
        for bad in ["", "  ", "abc", "-0.5", "0.1", "1.1", "0", "NaN", "inf"] {
            assert_eq!(
                popup_opacity(Some(bad.to_string())),
                DEFAULT_POPUP_OPACITY,
                "{bad} は既定値へ戻すべき"
            );
        }
    }
}
