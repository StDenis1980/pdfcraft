//! Tinta Office: the Tinta look, the colours and control shapes of Tinta Correo carried over to
//! Tinta PDF. The title bar has a wash of the app's colour; the tool bar sits on a soft grey;
//! panels, menus, dialogs and fields are white cards; buttons have no frame, only a tint of the
//! accent that deepens under the pointer; corners are rounder. The accent is the red of the Tinta
//! PDF icon.
//!
//! Only the chrome changes, never the layout or the pages. Off (Preferences › Appearance › Tinta
//! Office look, `view.tinta_look`), the interface is upstream's classic palette. The switch is
//! kept in the preferences as `tinta_look` and is on unless turned off; [`skin`] is the pure
//! mapping, so tests never touch the switch.

use std::sync::atomic::{AtomicBool, Ordering};

use egui::{Color32, CornerRadius, Stroke, Visuals};

use crate::theme::Tokens;

/// On until the preferences say otherwise.
static ON: AtomicBool = AtomicBool::new(true);

/// Whether the Tinta look is on. Never in this crate's unit tests: they share one process, and
/// upstream's compare colours with the classic palette whatever other tests switch on.
pub fn on() -> bool {
    !cfg!(test) && allowed() && ON.load(Ordering::Relaxed)
}

/// Set by the desktop app ([`allow`]): only it shows the Tinta look. Upstream's integration tests
/// and examples compare colours with the classic palette, so they keep it, unless `TINTA_LOOK` is
/// set in the environment (for screenshots of the Tinta look).
static ALLOWED: AtomicBool = AtomicBool::new(false);

/// Let this program show the Tinta look (the desktop app calls it once, at start).
pub fn allow() {
    ALLOWED.store(true, Ordering::Relaxed);
}

fn allowed() -> bool {
    static ENV: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    ALLOWED.load(Ordering::Relaxed) || *ENV.get_or_init(|| std::env::var_os("TINTA_LOOK").is_some())
}

/// Turn the Tinta look on or off; true when that changed it, so the caller re-applies the theme.
/// Never in unit tests, where re-applying the theme mid-test would change their frames.
pub fn set(on: bool) -> bool {
    !cfg!(test) && ON.swap(on, Ordering::Relaxed) != on
}

/// The palette in use: [`skin`] of `classic` while the Tinta look is on, else `classic` itself.
pub fn tokens(classic: Tokens) -> Tokens {
    if on() { skin(classic) } else { classic }
}

/// The Tinta palette, light or dark: Tinta Correo's neutrals with the Tinta PDF red.
struct Palette {
    accent: Color32,
    accent_ink: Color32,
    window: Color32,
    band: Color32,
    card: Color32,
    text: Color32,
    text_weak: Color32,
    border: Color32,
}

const LIGHT: Palette = Palette {
    accent: Color32::from_rgb(0xA8, 0x35, 0x2A),
    accent_ink: Color32::from_rgb(0x96, 0x2E, 0x24),
    window: Color32::from_rgb(0xF3, 0xF0, 0xEF),
    band: Color32::from_rgb(0xF1, 0xDB, 0xD7),
    card: Color32::WHITE,
    text: Color32::from_rgb(0x1F, 0x24, 0x30),
    text_weak: Color32::from_rgb(0x5C, 0x5E, 0x66),
    border: Color32::from_rgb(0xE6, 0xE1, 0xE0),
};

const DARK: Palette = Palette {
    accent: Color32::from_rgb(0xE5, 0x7A, 0x6C),
    accent_ink: Color32::from_rgb(0xF2, 0xA0, 0x95),
    window: Color32::from_rgb(0x1D, 0x19, 0x19),
    band: Color32::from_rgb(0x35, 0x21, 0x1F),
    card: Color32::from_rgb(0x2A, 0x26, 0x27),
    text: Color32::from_rgb(0xEC, 0xE8, 0xDF),
    text_weak: Color32::from_rgb(0xB8, 0xB3, 0xB0),
    border: Color32::from_rgb(0x45, 0x3E, 0x3E),
};

/// `a` moved `share` (0 to 1) of the way to `b`.
pub fn mix(a: Color32, b: Color32, share: f32) -> Color32 {
    let s = share.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * s).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// The Tinta look of `classic`: the chrome takes the Tinta palette; the page shadow stays.
pub fn skin(classic: Tokens) -> Tokens {
    let dark = classic.dark();
    let p = if dark { DARK } else { LIGHT };
    Tokens {
        titlebar: mix(p.window, p.band, 0.6),
        chrome: p.window,
        panel: p.card,
        pasteboard: if dark { mix(p.window, Color32::BLACK, 0.3) } else { mix(p.window, p.text_weak, 0.07) },
        card: p.card,
        border: mix(p.border, p.text_weak, 0.15),
        divider: p.border,
        text: p.text,
        text_muted: p.text_weak,
        text_faint: mix(p.text_weak, p.window, 0.04),
        icon: mix(p.text, p.text_weak, 0.35),
        hover: mix(p.window, p.accent, if dark { 0.16 } else { 0.09 }),
        pressed: mix(p.window, p.accent, if dark { 0.26 } else { 0.18 }),
        selected: mix(p.card, p.accent, if dark { 0.22 } else { 0.13 }),
        accent: p.accent,
        accent_text: p.accent_ink,
        accent_soft: mix(p.card, p.accent, if dark { 0.20 } else { 0.13 }),
        field: if dark { mix(p.card, p.window, 0.5) } else { p.card },
        badge_new: p.accent,
        radius: 8,
        ..classic
    }
}

/// Tinta Correo's shapes on egui's visuals for `t` ([`crate::theme::apply`] fills in the colours
/// first): rounder windows and menus, a soft accent tint on buttons, softer shadows.
pub fn visuals(v: &mut Visuals, t: &Tokens) {
    if !on() {
        return;
    }
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, t.divider);
    v.widgets.inactive.weak_bg_fill = mix(t.card, t.accent, 0.05);
    v.window_shadow = egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(if t.dark() { 110 } else { 40 }) };
    v.popup_shadow = egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if t.dark() { 100 } else { 32 }) };
}

/// The checkbox in Preferences › Appearance.
pub(crate) const LABEL: &str = "Tinta Office look";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeKind;

    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let f = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// The rule upstream's palette keeps (`theme::tests::text_is_readable_on_every_surface`), and
    /// the accent's own text on the surfaces it is drawn on.
    #[test]
    fn the_tinta_look_keeps_text_readable() {
        for kind in [ThemeKind::Light, ThemeKind::Dark] {
            let t = skin(Tokens::for_kind(kind));
            for fg in [t.text, t.text_muted, t.text_faint] {
                for bg in [t.titlebar, t.chrome, t.panel, t.card, t.pasteboard, t.hover, t.selected, t.accent_soft] {
                    assert!(contrast(fg, bg) >= 4.5, "{kind:?} {fg:?} on {bg:?}");
                }
            }
            for bg in [t.chrome, t.panel, t.card, t.accent_soft] {
                assert!(contrast(t.accent_text, bg) >= 4.5, "{kind:?} accent text on {bg:?}");
            }
            assert!(contrast(t.accent, t.chrome) >= 3.0, "{kind:?} accent marks");
        }
    }

    /// The label is translated into every language the fork's strings cover.
    #[test]
    fn the_label_is_translated() {
        let rows = include_str!("i18n/tinta.tsv");
        let langs: std::collections::BTreeSet<&str> = rows.lines().filter_map(|l| l.split('\t').next()).collect();
        for lang in langs {
            assert!(rows.lines().any(|l| l.starts_with(&format!("{lang}\t{LABEL}\t"))), "{lang} has no {LABEL:?}");
        }
    }
}
