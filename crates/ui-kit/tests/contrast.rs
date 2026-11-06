//! Token contrast (`design-system.md` §9): every text/background pair the widgets paint, measured
//! with the WCAG 2 formula (`theme::contrast`). Body text needs 4.5:1 (AA); glyphs and other
//! non-text marks 3:1. Translucent plates (toast, backdrop) are measured as they show on screen,
//! blended over what lies under them (`theme::over`).
//!
//! Disabled text (`text_disabled`) is exempt by WCAG and not listed; it only has to read as
//! dimmer than every enabled text, which is checked too.
//!
//! Every check runs for every theme of `ThemeKind::ALL` (M16.3). Dark keeps its pinned pairs; the
//! other themes have none: a palette that would fail a pair overrides the derived token in its
//! file instead (`design-system.md` §2, Themes). A theme still wearing Dark's values (its palette
//! not ported yet) is checked as Dark is.

use dalica_ui_kit::theme::{EGUI_TEXT_CURSOR, contrast, over};
use dalica_ui_kit::{ThemeKind, Tokens};
use egui::Color32;

/// `(what, foreground, background)`.
type Pair = (&'static str, Color32, Color32);

fn text_pairs(t: &Tokens) -> Vec<Pair> {
    let toast_on_table = over(t.toast_bg, t.grid_bg);
    let toast_on_white = over(t.toast_bg, Color32::WHITE);
    vec![
        // §9's three named pairs.
        ("text_label on chrome", t.text_label, t.chrome),
        ("text_dim on chrome (status line, tooltip detail, window title)", t.text_dim, t.chrome),
        ("text_dim on grid_bg", t.text_dim, t.grid_bg),
        // Panels, menus, tooltips, dialogs (chrome).
        ("text on chrome", t.text, t.chrome),
        // Buttons, segments, toggles in every state.
        ("text on button", t.text, t.button),
        ("text on hover", t.text, t.hover),
        ("text on pressed", t.text, t.pressed),
        ("text on tool_active", t.text, t.tool_active),
        ("text_label on button", t.text_label, t.button),
        ("text_label on hover", t.text_label, t.hover),
        ("text_dim on button", t.text_dim, t.button),
        // The primary button.
        ("on_accent on accent", t.on_accent, t.accent),
        ("on_accent on accent_hover", t.on_accent, t.accent_hover),
        ("on_accent on accent_pressed", t.on_accent, t.accent_pressed),
        // Fields: text and placeholder, at rest and focused.
        ("text on field", t.text, t.field),
        ("text on canvas", t.text, t.canvas),
        ("text_dim on field", t.text_dim, t.field),
        ("text_dim on canvas", t.text_dim, t.canvas),
        // The light table, its folder headers, the tray, the filter banner.
        ("text on grid_bg", t.text, t.grid_bg),
        ("text on folder_header", t.text, t.folder_header),
        ("text_label on folder_header", t.text_label, t.folder_header),
        ("text_dim on folder_header", t.text_dim, t.folder_header),
        ("text_label on tray_bg", t.text_label, t.tray_bg),
        ("text_dim on tray_bg", t.text_dim, t.tray_bg),
        ("text on filter_banner", t.text, t.filter_banner),
        ("text_label on filter_banner", t.text_label, t.filter_banner),
        ("text_label on inset", t.text_label, t.inset),
        // Toasts on the light table, and on the brightest photo.
        ("text on toast over grid_bg", t.text, toast_on_table),
        ("text on toast over white", t.text, toast_on_white),
        // A selected text range.
        ("text on selection over field", t.text, over(t.accent.gamma_multiply(0.6), t.field)),
    ]
}

fn glyph_pairs(t: &Tokens) -> Vec<Pair> {
    vec![
        ("icon on chrome", t.icon, t.chrome),
        ("icon on button", t.icon, t.button),
        ("text glyph on hover", t.text, t.hover),
        ("pick on tool_active", t.pick, t.tool_active),
        ("star on chrome", t.star, t.chrome),
        ("star on badge plate over grid_bg", t.star, over(t.badge_plate, t.grid_bg)),
        ("thumb on chrome", t.thumb, t.chrome),
        ("thumb_hover on chrome", t.thumb_hover, t.chrome),
        ("caution on chrome", t.caution, t.chrome),
        ("caution on grid_bg", t.caution, t.grid_bg),
        ("danger on chrome", t.danger, t.chrome),
        ("on_accent on danger", t.on_accent, t.danger),
        ("on_accent on caption_close (the Close button hovered)", t.on_accent, t.caption_close),
        ("on_accent on caption_close × 0.8 (the Close button held)", t.on_accent, over(t.caption_close.gamma_multiply(0.8), t.chrome)),
        ("photo_focus on grid_bg", t.photo_focus, t.grid_bg),
        ("photo_selected on grid_bg", t.photo_selected, t.grid_bg),
    ]
}

/// Pairs the tokens of `design-system.md` §2 put below their threshold, pinned here so they cannot
/// get worse. The maintainer settled them at the M3 review: `text_dim` was raised to `#949494` so it
/// reaches 4.5:1 on `chrome`; on a hovered row it stays below (a hovered row would need about
/// `#a8a8a8`, too close to `text_label`). `accent` on `chrome` stays below the 3:1 that WCAG 2.2
/// asks of a drop-target outline (drawn over an `accent × 0.15` fill); the focus ring uses the
/// brighter `widgets::focus_color` since M15.14, checked in `tests/accessibility.rs`. The slider `track` stays a guide under the
/// ring (`thumb`, 5.3:1), not the control itself.
fn known_below(t: &Tokens) -> Vec<(&'static str, Color32, Color32, f64, f64)> {
    if wears_dark(t) { dark_pins(t) } else { Vec::new() }
}

/// The pairs pinned in Dark, measured in `t`: every other theme passes them at their threshold.
fn dark_pins(t: &Tokens) -> Vec<(&'static str, Color32, Color32, f64, f64)> {
    // (what, foreground, background, pinned floor, threshold it misses)
    vec![
        ("text_dim on hover (counts in a hovered row)", t.text_dim, t.hover, 3.75, 4.5),
        ("accent drop-target outline on chrome", t.accent, t.chrome, 2.56, 3.0),
        ("track on chrome", t.track, t.chrome, 2.00, 3.0),
    ]
}

/// Dark's colours (Dark itself, or a theme whose palette has not landed yet).
fn wears_dark(t: &Tokens) -> bool {
    Tokens { kind: ThemeKind::Dark, ..*t } == Tokens::DARK
}

/// Every theme's tokens.
fn themes() -> impl Iterator<Item = Tokens> {
    ThemeKind::ALL.into_iter().map(Tokens::for_kind)
}

/// The pairs of `pairs` below `threshold`: `"<theme>: <pair>: <ratio>"`.
fn below(t: &Tokens, pairs: Vec<Pair>, threshold: f64) -> Vec<String> {
    pairs
        .into_iter()
        .filter_map(|(name, fg, bg)| {
            let c = contrast(fg, bg);
            (c < threshold).then(|| format!("{}: {name}: {c:.2}:1", t.kind.id()))
        })
        .collect()
}

#[test]
fn every_text_pair_is_at_least_4_5_to_1() {
    let fails: Vec<String> = themes().flat_map(|t| below(&t, text_pairs(&t), 4.5)).collect();
    assert!(fails.is_empty(), "below 4.5:1: {fails:?}");
}

/// Only Dark's colours have pinned pairs: in any other theme the pairs Dark pins pass at their
/// threshold.
#[test]
fn the_pairs_dark_pins_pass_in_every_other_theme() {
    assert_eq!(known_below(&Tokens::DARK).len(), 3);
    let mut fails = Vec::new();
    for t in themes().filter(|t| !wears_dark(t)) {
        assert!(known_below(&t).is_empty());
        for (name, fg, bg, _, threshold) in dark_pins(&t) {
            let c = contrast(fg, bg);
            if c < threshold {
                fails.push(format!("{}: {name}: {c:.2}:1 (needs {threshold}:1)", t.kind.id()));
            }
        }
    }
    assert!(fails.is_empty(), "{fails:?}");
}

/// The M3 review's answer (`design-system.md` §9): dim text on the chrome is body text too, so the
/// status line, a tooltip's detail line and the window title reach 4.5:1 (`#8e8e8e` gave 4.20:1).
#[test]
fn text_dim_on_chrome_is_at_least_4_5_to_1() {
    for t in themes() {
        let c = contrast(t.text_dim, t.chrome);
        assert!(c >= 4.5, "{}: text_dim on chrome: {c:.2}:1", t.kind.id());
    }
}

#[test]
fn every_glyph_pair_is_at_least_3_to_1() {
    let fails: Vec<String> = themes().flat_map(|t| below(&t, glyph_pairs(&t), 3.0)).collect();
    assert!(fails.is_empty(), "below 3:1: {fails:?}");
}

/// The text cursor against a field at rest (`field`) and a focused one (`canvas`): a non-text mark,
/// so 3:1 (M16.30).
fn cursor_pairs(t: &Tokens, cursor: Color32) -> Vec<Pair> {
    vec![("text cursor on field", cursor, t.field), ("text cursor on canvas", cursor, t.canvas)]
}

#[test]
fn the_text_cursor_is_at_least_3_to_1_in_every_theme() {
    assert_eq!(themes().count(), 15);
    let fails: Vec<String> = themes().flat_map(|t| below(&t, cursor_pairs(&t, t.text_cursor()), 3.0)).collect();
    assert!(fails.is_empty(), "below 3:1: {fails:?}");
    assert_eq!(Tokens::DARK.text_cursor(), EGUI_TEXT_CURSOR, "Dark keeps egui's cursor");
    // The check bites: egui's default, which every theme wore before M16.30, fails the light ones.
    let old: Vec<String> = themes().flat_map(|t| below(&t, cursor_pairs(&t, EGUI_TEXT_CURSOR), 3.0)).collect();
    assert!(old.iter().any(|f| f.starts_with("proLight:")), "{old:?}");
}

#[test]
fn pairs_below_their_threshold_are_pinned_and_never_get_worse() {
    for t in themes() {
        for (name, fg, bg, floor, threshold) in known_below(&t) {
            let c = contrast(fg, bg);
            assert!(c >= floor - 0.005, "{}: {name}: {c:.2}:1 fell below its pinned {floor}:1", t.kind.id());
            assert!(c < threshold, "{name} now passes ({c:.2}:1): move it to the checked pairs");
        }
    }
}

#[test]
fn disabled_text_is_dimmer_than_every_enabled_text() {
    for t in themes() {
        for bg in [t.chrome, t.button, t.field, t.canvas] {
            for fg in [t.text, t.text_label, t.text_dim] {
                assert!(contrast(t.text_disabled, bg) < contrast(fg, bg), "{}: {fg:?} on {bg:?}", t.kind.id());
            }
            assert!(contrast(t.text_disabled, bg) > 1.3, "{}: disabled text is still visible on {bg:?}", t.kind.id());
        }
    }
}

/// The checks cover the font-size variants too: font size never changes a colour.
#[test]
fn font_sizes_keep_the_colours() {
    for t in themes() {
        for size in [dalica_ui_kit::theme::FontSize::Medium, dalica_ui_kit::theme::FontSize::Large] {
            let s = t.with_font_size(size);
            assert_eq!(text_pairs(&s), text_pairs(&t));
        }
    }
}
