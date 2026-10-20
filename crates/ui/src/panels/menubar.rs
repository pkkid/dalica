//! The menu bar (`menus-and-shortcuts.md` §1): on Windows and Linux the title bar
//! ([`crate::titlebar`], `design-system.md` §11.1) around the in-window menus of the menu model
//! ([`crate::menus::show_bar`]), every menu bounded by the window.
//!
//! **From the keyboard** ([`keyboard`], `ui-layout.md` → Keyboard and screen readers): F10, or Alt
//! pressed and released alone (not on macOS, where Option is no menu key), opens the first menu
//! with the focus on its title, and closes an open menu. While a menu of the bar is open, Left and
//! Right open the menu beside it (wrapping), Down and Up move the focus through its rows (into an
//! open submenu's rows, which follow their row), Enter or Space runs the focused row or opens its
//! submenu, Esc closes the menu. The rows are egui buttons: the focused one draws egui's focus.

use egui::{Context, Event, FocusDirection, Id, Key, Modifiers, Ui};

use super::Panels;
use crate::app::App;
use crate::tl;
use dalica_ui_kit::registry::{self, Role};

pub const HEIGHT: f32 = crate::titlebar::HEIGHT;

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    // The region's whole row (the ui given here is inset for the menus): the caption buttons sit
    // flush in its corner.
    let bar = ui.clip_rect();
    let section = app.state().folder.and_then(|f| p.sections.list.iter().find(|s| s.folder == Some(f))).map(|s| s.title.clone());
    crate::titlebar::show(ui, app, bar, section.as_deref());
}

/// The title a menu of the bar shows, from its English title (`menus::title`), in the language in
/// effect: the in-window bar and the macOS menu both draw it.
pub fn menu_title(english: &str) -> String {
    match english {
        "File" => tl!(ctx = "menu", "File"),
        "Edit" => tl!(ctx = "menu", "Edit"),
        "View" => tl!(ctx = "menu", "View"),
        "Folder" => tl!(ctx = "menu", "Folder"),
        "Album" => tl!(ctx = "menu", "Album"),
        "Picture" => tl!(ctx = "menu", "Picture"),
        "Create" => tl!(ctx = "menu", "Create"),
        "Tools" => tl!(ctx = "menu", "Tools"),
        "Help" => tl!(ctx = "menu", "Help"),
        "Dalica" => tl!(ctx = "menu", "Dalica"),
        other => crate::i18n::tr_ctx("menu", other),
    }
}

/// The label a submenu's row shows, from its English label (the menu catalog's).
pub fn submenu_label(english: &str) -> String {
    crate::i18n::tr(english)
}

fn alt_key() -> Id {
    Id::new("dalica.menubar.alt")
}

/// Watching for Alt pressed and released alone.
#[derive(Clone, Copy, Debug, Default)]
struct AltWatch {
    /// The modifiers of the last frame.
    before: Modifiers,
    /// Alt went down alone and nothing else happened since.
    armed: bool,
}

/// Whether this frame releases an Alt that was pressed alone (no key, no click, no other modifier
/// while it was down). Never on macOS.
fn lone_alt(ctx: &Context) -> bool {
    if cfg!(target_os = "macos") {
        return false;
    }
    let (now, other) = ctx.input(|i| {
        let other = i.events.iter().any(|e| matches!(e, Event::Key { .. } | Event::PointerButton { .. } | Event::Text(_)));
        (i.modifiers, other)
    });
    let mut w = ctx.data(|d| d.get_temp::<AltWatch>(alt_key())).unwrap_or_default();
    let alone = now.alt && !now.ctrl && !now.shift && !now.command && !now.mac_cmd;
    let mut fire = false;
    if other || (now.alt && !alone) {
        w.armed = false;
    } else if alone && !w.before.alt {
        w.armed = true;
    } else if !now.alt && w.before.alt && w.armed {
        fire = true;
        w.armed = false;
    }
    w.before = now;
    ctx.data_mut(|d| d.insert_temp(alt_key(), w));
    fire
}

/// The bar's menu titles of the last frame, left to right, with their egui ids.
fn titles(ctx: &Context) -> Vec<Id> {
    registry::widgets(ctx).into_iter().filter(|w| w.role == Role::Menu && w.id.starts_with("menu:")).filter_map(|w| w.egui_id).collect()
}

/// The popup of the menu whose title is `title`: egui's id for a menu button's popup
/// (`Popup::default_response_id`).
fn popup(title: Id) -> Id {
    title.with("popup")
}

/// Opens menu `index` of `titles` with the focus on its title.
fn open(ctx: &Context, titles: &[Id], index: usize) {
    if let Some(title) = titles.get(index) {
        egui::Popup::open_id(ctx, popup(*title));
        ctx.memory_mut(|m| m.request_focus(*title));
    }
}

/// The menu bar's keys for this frame (see the module docs). Runs before the shortcuts; the keys it
/// uses are held back from them while a menu is open (`crate::a11y::keyboard`).
pub fn keyboard(app: &App, ctx: &Context) {
    let toggled = lone_alt(ctx) || pressed(ctx, Key::F10);
    // Most frames: no menu key and no menu open, nothing to look up.
    if !(toggled || egui::Popup::is_any_open(ctx)) || !app.state().dialogs.is_empty() {
        return;
    }
    let titles = titles(ctx);
    if titles.is_empty() {
        return;
    }
    let open_now = titles.iter().position(|t| egui::Popup::is_id_open(ctx, popup(*t)));
    if toggled {
        match open_now {
            Some(_) => {
                egui::Popup::close_all(ctx);
                if let Some(f) = ctx.memory(|m| m.focused()) {
                    ctx.memory_mut(|m| m.surrender_focus(f));
                }
            }
            None => open(ctx, &titles, 0),
        }
        return;
    }
    let Some(at) = open_now else { return };
    let n = titles.len();
    if pressed(ctx, Key::ArrowRight) {
        ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
        open(ctx, &titles, (at + 1) % n);
    } else if pressed(ctx, Key::ArrowLeft) {
        ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
        open(ctx, &titles, (at + n - 1) % n);
    } else if pressed(ctx, Key::ArrowDown) || pressed(ctx, Key::ArrowUp) {
        ctx.memory_mut(|m| m.move_focus(FocusDirection::None));
        let down = pressed(ctx, Key::ArrowDown);
        let rows = rows(&registry::widgets(ctx));
        if rows.is_empty() {
            return;
        }
        let focused = ctx.memory(|m| m.focused());
        let k = rows.len();
        let next = match (focused.and_then(|f| rows.iter().position(|r| *r == f)), down) {
            (Some(i), true) => (i + 1) % k,
            (Some(i), false) => (i + k - 1) % k,
            (None, true) => 0,
            (None, false) => k - 1,
        };
        if let Some(row) = rows.get(next) {
            ctx.memory_mut(|m| m.request_focus(*row));
        }
    } else if pressed(ctx, Key::Escape) {
        egui::Popup::close_all(ctx);
        if let Some(title) = titles.get(at) {
            ctx.memory_mut(|m| m.request_focus(*title));
        }
    }
}

/// The open menu's rows that take the focus, in the order Down walks them: each level top to
/// bottom, an open submenu's rows right after its row. (A submenu registers its row after its
/// rows, which are drawn while the row is; the levels are told apart by where they are: each one
/// a column to the right of the one it opens from.)
fn rows(widgets: &[dalica_ui_kit::WidgetInfo]) -> Vec<Id> {
    let rect = |w: &dalica_ui_kit::WidgetInfo| egui::Rect::from_min_size(egui::pos2(w.rect[0], w.rect[1]), egui::vec2(w.rect[2], w.rect[3]));
    let all: Vec<(&dalica_ui_kit::WidgetInfo, egui::Rect)> =
        widgets.iter().filter(|w| w.role == Role::MenuItem && w.egui_id.is_some()).map(|w| (w, rect(w))).collect();
    // The levels: rows grouped by their left edge, left to right.
    let mut lefts: Vec<f32> = Vec::new();
    for (_, r) in &all {
        if !lefts.iter().any(|l| (l - r.left()).abs() < 1.0) {
            lefts.push(r.left());
        }
    }
    lefts.sort_by(f32::total_cmp);
    let level = |r: &egui::Rect| lefts.iter().position(|l| (l - r.left()).abs() < 1.0).unwrap_or(0);
    let mut out = Vec::new();
    walk_level(&all, 0, &level, &mut out);
    out
}

fn walk_level(all: &[(&dalica_ui_kit::WidgetInfo, egui::Rect)], depth: usize, level: &dyn Fn(&egui::Rect) -> usize, out: &mut Vec<Id>) {
    const MAX_LEVELS: usize = 8;
    if depth > MAX_LEVELS {
        return;
    }
    let mut here: Vec<&(&dalica_ui_kit::WidgetInfo, egui::Rect)> = all.iter().filter(|(_, r)| level(r) == depth).collect();
    here.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
    for (w, _) in here {
        if w.enabled
            && let Some(id) = w.egui_id
        {
            out.push(id);
        }
        // An open submenu: its rows next.
        if w.id.starts_with("submenu:") && w.active {
            walk_level(all, depth + 1, level, out);
        }
    }
}

/// Whether `key` was pressed with no modifier this frame.
fn pressed(ctx: &Context, key: Key) -> bool {
    ctx.input(|i| i.events.iter().any(|e| matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.is_none())))
}
