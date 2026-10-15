//! Key cells (`menus-and-shortcuts.md` §2.0, `design-system.md` §6): the shortcut editor's cell
//! showing a command's key or keys as key caps, clicked to record a new one.
//!
//! Keys come as the engine writes them, platform-neutrally (`Ctrl+Shift+S`, `F4`, `Ctrl+,`; `Ctrl`
//! is the primary modifier, Cmd on macOS); [`key_text`] turns one into what the platform shows:
//! `Ctrl+Shift+S` on Windows and Linux, `⇧⌘S` on macOS (the modifiers in Apple's order ⌥ ⇧ ⌘,
//! named keys as their glyphs: ↩ ⎋ ⇥ ⌫ ⌦ and the arrows). The macOS rendering is adapted from
//! LightCraft `crates/ui-egui/src/menubar.rs` (`shortcut_text`) at b3d392b4 (MIT OR Apache-2.0).

use egui::{Align2, CornerRadius, Rect, Response, Stroke, StrokeKind, Ui, WidgetType, pos2, vec2};

use super::{Target, allocate, focus_ring, one_line, register};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// A key cell's default width: room for "Press the new shortcut…", or two caps such as
/// "Ctrl+Shift+S" and "F4" side by side.
pub const KEY_CELL_W: f32 = 160.0;
/// The most keys a cell shows (a command has at most two per context).
pub const KEY_CELL_MAX_KEYS: usize = 2;
/// The space between a key cell's edge and its caps, and between two caps.
pub const KEY_CAP_GAP: f32 = 4.0;
/// The space either side of a cap's text.
pub const KEY_CAP_PAD: f32 = 6.0;
/// What a recording cell says.
pub const RECORDING_TEXT: &str = "Press the new shortcut…";
/// What a cell without a key says (its accessible label; the cell shows "—").
pub const NO_KEY_TEXT: &str = "No shortcut";

/// What a [`key_cell`] shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyCell<'a> {
    /// The keys, as the engine writes them; at most [`KEY_CELL_MAX_KEYS`] are shown.
    pub keys: &'a [&'a str],
    /// Show the keys as macOS does ([`key_text`]).
    pub mac: bool,
    /// Waiting for a key: "Press the new shortcut…" in place of the caps.
    pub recording: bool,
    /// The key is already taken (the editor asks Replace or Cancel): outlined `danger`.
    pub conflict: bool,
    /// The cell's width; a non-finite or tiny width is [`KEY_CELL_W`].
    pub width: f32,
}

impl Default for KeyCell<'_> {
    fn default() -> Self {
        KeyCell { keys: &[], mac: false, recording: false, conflict: false, width: KEY_CELL_W }
    }
}

/// `key` (`Ctrl+Shift+S`, as the engine writes it) as the platform shows it: on Windows and Linux
/// the modifiers in the order Ctrl, Alt, Shift (`Cmd` and `Command` read as Ctrl, `Option` as Alt)
/// joined with `+`; on macOS the glyphs ⌥ ⇧ ⌘ in Apple's order with nothing between them, then
/// the key (named keys as their glyphs). `Ctrl++` is the plus key. Text that names no key is shown
/// as it came.
pub fn key_text(key: &str, mac: bool) -> String {
    let key = key.trim();
    let mut parts: Vec<&str> = key.split('+').collect();
    if key.ends_with("++") {
        parts.truncate(parts.len().saturating_sub(2));
        parts.push("+");
    }
    let Some((code, mods)) = parts.split_last() else { return key.to_owned() };
    let (mut ctrl, mut alt, mut shift) = (false, false, false);
    for m in mods {
        match m.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "cmd" | "control" | "command" => ctrl = true,
            "alt" | "option" | "opt" => alt = true,
            "shift" => shift = true,
            _ => return key.to_owned(),
        }
    }
    let code = code.trim();
    if code.is_empty() {
        return key.to_owned();
    }
    if mac {
        let glyph = match code.to_ascii_lowercase().as_str() {
            "enter" | "return" => "↩",
            "esc" | "escape" => "⎋",
            "tab" => "⇥",
            "backspace" => "⌫",
            "delete" | "del" => "⌦",
            "up" => "↑",
            "down" => "↓",
            "left" => "←",
            "right" => "→",
            "pageup" | "pgup" => "⇞",
            "pagedown" | "pgdn" | "pgdown" => "⇟",
            "home" => "↖",
            "end" => "↘",
            "plus" => "+",
            _ => code,
        };
        let mut out = String::new();
        for (on, g) in [(alt, '⌥'), (shift, '⇧'), (ctrl, '⌘')] {
            if on {
                out.push(g);
            }
        }
        out.push_str(glyph);
        out
    } else {
        let mut out = String::new();
        for (on, name) in [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift, "Shift+")] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(code);
        out
    }
}

/// What a key cell says to a screen reader and the automation registry: "Press the new
/// shortcut…" while recording, else its keys as [`key_text`] shows them, joined with ", ", or
/// "No shortcut".
pub fn key_cell_label(cell: &KeyCell<'_>) -> String {
    if cell.recording {
        return RECORDING_TEXT.to_owned();
    }
    let keys: Vec<String> = cell.keys.iter().filter(|k| !k.trim().is_empty()).take(KEY_CELL_MAX_KEYS).map(|k| key_text(k, cell.mac)).collect();
    if keys.is_empty() { NO_KEY_TEXT.to_owned() } else { keys.join(", ") }
}

/// The height of a key cell: the form line (`text_field_h`, 24), taller when the font needs it.
pub fn key_cell_height(t: &Tokens) -> f32 {
    t.metrics.text_field_h.max((t.count * 1.25 + 10.0).ceil())
}

/// A key cell (`design-system.md` §6): `cell.width` × [`key_cell_height`], the whole cell
/// clickable. Each key is a cap: 12 pt `text` on a `button` plate with a 1 pt `button_border`
/// outline (radius 3), [`KEY_CAP_PAD`] either side of its text, [`KEY_CAP_GAP`] apart; a cap too
/// wide for what is left is cut short with "…", one that no longer fits is left out. Without a key
/// the cell shows "—" in `text_dim`. Hovered the cell is filled `hover × 0.7`; recording it is a
/// `field` with a 1.5 pt `accent` outline saying "Press the new shortcut…" in `text`; a conflict
/// outlines it 1.5 pt `danger` (the caps still shown). Disabled (a hold gesture: listed, not
/// editable) the caps' text is `text_disabled` and it ignores clicks. The caller starts and ends
/// recording on `clicked()`. Registered as `id` (role `button`, active while recording, labelled
/// by [`key_cell_label`]).
pub fn key_cell(ui: &mut Ui, t: &Tokens, id: &str, cell: &KeyCell<'_>) -> Response {
    let width = if cell.width.is_finite() && cell.width >= 24.0 { cell.width.min(4096.0) } else { KEY_CELL_W };
    let enabled = ui.is_enabled();
    let r = allocate(ui, id, vec2(width, key_cell_height(t)), enabled);
    let rect = r.rect;
    let p = ui.painter();
    let radius = CornerRadius::same(t.radius);
    let font = fonts::regular(t.count);
    if cell.recording {
        p.rect(rect, radius, t.field, Stroke::new(1.5, t.accent), StrokeKind::Inside);
        let galley = one_line(p, RECORDING_TEXT, font, t.text, rect.width() - 2.0 * KEY_CAP_PAD);
        p.galley(pos2(rect.min.x + KEY_CAP_PAD, rect.center().y - galley.size().y / 2.0).round(), galley, t.text);
    } else {
        if enabled && (r.hovered() || r.is_pointer_button_down_on()) {
            let fill = if r.is_pointer_button_down_on() { t.pressed } else { t.hover.gamma_multiply(0.7) };
            p.rect_filled(rect, radius, fill);
        }
        let keys: Vec<String> = cell.keys.iter().filter(|k| !k.trim().is_empty()).take(KEY_CELL_MAX_KEYS).map(|k| key_text(k, cell.mac)).collect();
        if keys.is_empty() {
            p.text(pos2(rect.min.x + KEY_CAP_PAD, rect.center().y), Align2::LEFT_CENTER, "—", font.clone(), t.text_dim);
        }
        let color = if enabled { t.text } else { t.text_disabled };
        let cap_h = (rect.height() - 6.0).max(1.0);
        let mut x = rect.min.x + KEY_CAP_GAP;
        for key in &keys {
            let room = rect.max.x - KEY_CAP_GAP - x;
            if room < 2.0 * KEY_CAP_PAD + 8.0 {
                break;
            }
            let galley = one_line(p, key, font.clone(), color, room - 2.0 * KEY_CAP_PAD);
            let cap = Rect::from_min_size(pos2(x, rect.center().y - cap_h / 2.0), vec2((galley.size().x + 2.0 * KEY_CAP_PAD).ceil(), cap_h));
            p.rect(
                cap,
                CornerRadius::same(3),
                t.button,
                Stroke::new(1.0, if enabled { t.button_border } else { t.field_border }),
                StrokeKind::Inside,
            );
            p.galley(pos2(cap.min.x + KEY_CAP_PAD, cap.center().y - galley.size().y / 2.0).round(), galley, color);
            x = cap.max.x + KEY_CAP_GAP;
        }
        if cell.conflict {
            p.rect_stroke(rect, radius, Stroke::new(1.5, t.danger), StrokeKind::Inside);
        }
    }
    focus_ring(p, t, &r, rect, t.radius);
    let label = key_cell_label(cell);
    register(ui.ctx(), id, Role::Button, &label, Target::Widget(&r), cell.recording);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, cell.recording, &label));
    r
}
