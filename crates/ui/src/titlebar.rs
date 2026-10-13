//! The title bar (`design-system.md` §11.1). On Windows and Linux the window has no OS decorations
//! and the 32 pt menu row *is* the title bar: the app icon, the menus, the window title centred on
//! the window (slid into the free gap), then Minimize, Maximize/Restore and Close (46 × 31 each,
//! above the bar's 1 pt bottom divider) flush in the top-right corner. The free gap drags the
//! window, a double-click there maximizes or restores it, and invisible edges resize it
//! ([`resize_edges`], drawn by the window after everything else).
//!
//! The bar's metrics, the title's placement ([`title_x`]) and the caption buttons' look
//! ([`paint_glyph`]: 10 pt line glyphs, Close turning Windows' red) are adapted from PhotoCraft
//! `crates/ui-egui/src/titlebar.rs` @ a0627311 and `crates/ui-egui/src/panels.rs` (`title_bar`,
//! `title_x`) @ b41e34c2 (MIT OR Apache-2.0); its default (Pro) theme's bar is 32 pt with an 18 pt
//! app mark 10 pt from the left edge.
//!
//! Which chrome the row draws is a [`Chrome`] the window sets once, at creation, from the host OS and
//! the "Use system title bar" setting ([`set_chrome`]). Renders never ask the OS themselves:
//! headless and snapshot renders get the default, [`Chrome::Drawn`], on every OS, so whole-window
//! fingerprints match everywhere.

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Context, CursorIcon, Id, PointerButton, Rect, ResizeDirection, Sense, Stroke, StrokeKind, Ui, UiBuilder, ViewportCommand, pos2, vec2};

use crate::app::App;

/// The title bar's height (the menu row, `layout::MENU_BAR`), its last point the divider line.
pub const HEIGHT: f32 = 32.0;
pub const DIVIDER: f32 = 1.0;
/// A caption button (Minimize, Maximize/Restore, Close): as wide as Windows' own, the bar's height
/// above its divider.
pub const CAPTION_BUTTON: egui::Vec2 = vec2(46.0, HEIGHT - DIVIDER);
/// The app icon's edge (the committed app-icon render, [`crate::app_icon`]), and the gap between
/// the window's left edge and it.
pub const APP_ICON: f32 = 18.0;
pub const ICON_LEFT: f32 = 10.0;
/// The gap between the app icon and the first menu.
const ICON_GAP: f32 = 6.0;
/// The caption glyphs' edge, drawn with 1 pt lines.
const GLYPH: f32 = 10.0;
/// The room the macOS traffic lights take at the left of the strip.
pub const TRAFFIC_LIGHTS: f32 = 78.0;
/// The least padding kept on each side of the window title; a title that does not fit is dropped
/// (`design-system.md` §11.1 "Narrow windows").
const TITLE_PAD: f32 = 16.0;
/// The invisible resize band along each edge, and along each side of a corner.
pub const RESIZE_EDGE: f32 = 5.0;
pub const RESIZE_CORNER: f32 = 12.0;
/// The product name in the window title.
pub const APP_NAME: &str = "Dalica";

/// What the menu row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Chrome {
    /// Windows and Linux (the default, and every headless render): no OS decorations; the row is
    /// the title bar.
    #[default]
    Drawn,
    /// "Use system title bar" (Windows and Linux): the OS decorations above; the row holds only the
    /// menus.
    System,
    /// macOS: the native traffic lights over the strip, the window title centred; the menus live in
    /// the system menu bar.
    Mac,
}

fn chrome_id() -> Id {
    Id::new("dalica-titlebar-chrome")
}

/// Sets the chrome of the window `ctx` draws. The window calls it once, at creation.
pub fn set_chrome(ctx: &Context, chrome: Chrome) {
    ctx.data_mut(|d| d.insert_temp(chrome_id(), chrome));
}

/// The chrome of the window `ctx` draws; [`Chrome::Drawn`] unless the window set another.
pub fn chrome(ctx: &Context) -> Chrome {
    ctx.data(|d| d.get_temp(chrome_id())).unwrap_or_default()
}

/// The window title: "Dalica", or "<folder or album> — Dalica" while a section is selected.
pub fn window_title(section: Option<&str>) -> String {
    match section.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => format!("{s} — {APP_NAME}"),
        None => APP_NAME.to_string(),
    }
}

/// Where the parts of the title bar go in the menu row `bar`, given where the menus end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    /// The app icon ([`Chrome::Drawn`]).
    pub icon: Option<Rect>,
    /// Where the menus start.
    pub menus_left: f32,
    /// Minimize, Maximize/Restore, Close ([`Chrome::Drawn`]).
    pub captions: Option<[Rect; 3]>,
    /// The free gap that drags the window (none with [`Chrome::System`]).
    pub drag: Option<Rect>,
}

/// The title bar's geometry in `bar` for `chrome`; `menus_right` is where the menus end (the
/// menus' left edge before they are drawn).
pub fn parts(bar: Rect, chrome: Chrome, menus_right: f32) -> Parts {
    let (top, bottom) = (bar.top(), bar.bottom());
    let span = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, top), pos2(x1.max(x0), bottom));
    match chrome {
        Chrome::Drawn => {
            let icon = Rect::from_center_size(pos2(bar.left() + ICON_LEFT + APP_ICON / 2.0, bar.center().y), vec2(APP_ICON, APP_ICON));
            let close_left = (bar.right() - CAPTION_BUTTON.x).max(bar.left());
            let max_left = (close_left - CAPTION_BUTTON.x).max(bar.left());
            let min_left = (max_left - CAPTION_BUTTON.x).max(bar.left());
            let button = |x0: f32, x1: f32| Rect::from_min_max(pos2(x0, top), pos2(x1.max(x0), (bottom - DIVIDER).max(top)));
            let captions = [button(min_left, max_left), button(max_left, close_left), button(close_left, bar.right())];
            let menus_right = menus_right.clamp(bar.left(), min_left);
            Parts { icon: Some(icon), menus_left: icon.right() + ICON_GAP, captions: Some(captions), drag: Some(span(menus_right, min_left)) }
        }
        Chrome::System => Parts { icon: None, menus_left: bar.left() + ICON_LEFT, captions: None, drag: None },
        Chrome::Mac => {
            let left = (bar.left() + TRAFFIC_LIGHTS).min(bar.right());
            Parts { icon: None, menus_left: left, captions: None, drag: Some(span(left, bar.right())) }
        }
    }
}

/// Draws the menu row `bar` (the region `region:menuBar`) for the window's [`chrome`]: the title
/// bar, or only the menus under OS decorations, or the macOS strip. `section` names the selected
/// folder or album for the window title, which is also given to the OS window.
pub fn show(ui: &mut Ui, app: &mut App, bar: Rect, section: Option<&str>) {
    let ctx = ui.ctx().clone();
    let t = *app.tokens();
    let chrome = chrome(&ctx);
    let title = window_title(section);
    send_title(&ctx, &title);
    let before = parts(bar, chrome, bar.left());
    let menus_right = if chrome == Chrome::Mac {
        before.menus_left
    } else {
        let right = before.captions.map_or(bar.right(), |c| c[0].left());
        let area =
            Rect::from_min_max(pos2(before.menus_left, bar.top()), pos2(right.max(before.menus_left), (bar.bottom() - DIVIDER).max(bar.top())));
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        let mut menus = ui.new_child(UiBuilder::new().id_salt("dalica-titlebar-menus").max_rect(area).layout(layout));
        menus.set_clip_rect(area.intersect(ui.clip_rect()));
        crate::menus::show_bar(&mut menus, app);
        // egui's menu bar takes the whole width it is given: the menus end where the last title
        // registered this pass ends.
        dalica_ui_kit::registry::widgets(&ctx)
            .iter()
            .filter(|w| w.id.starts_with("menu:"))
            .map(|w| w.rect[0] + w.rect[2])
            .fold(before.menus_left, f32::max)
    };
    let p = parts(bar, chrome, menus_right);
    let painter = ui.painter().clone();
    // The bar's bottom point: the divider between it and the toolbar.
    let divider = Rect::from_min_max(pos2(bar.left(), (bar.bottom() - DIVIDER).max(bar.top())), bar.max);
    painter.rect_filled(divider, 0, t.divider);
    if let Some(icon) = p.icon {
        crate::app_icon::paint(&painter, icon);
    }
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    if let Some(drag) = p.drag {
        // The title is centred on the window, slid into the free gap, or dropped when it does not
        // fit there.
        if chrome != Chrome::System {
            let galley = painter.layout_no_wrap(title.clone(), fonts::regular(t.body), t.text_dim);
            if let Some(x) = title_x(bar.center().x, drag.left(), drag.right(), galley.size().x) {
                let y = (bar.top() + (bar.height() - DIVIDER) / 2.0) - galley.size().y / 2.0;
                painter.galley(pos2(x, y).round(), galley, t.text_dim);
            }
        }
        let r = ui.interact(drag, Id::new("dalica-titlebar-drag"), Sense::click_and_drag());
        widgets::register(&ctx, "titlebar:drag", Role::Other, &title, Target::Widget(&r), false);
        if r.double_clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        } else if r.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
    }
    if let Some([min, max, close]) = p.captions {
        let (max_glyph, max_label) = if maximized { (Glyph::Restore, "Restore") } else { (Glyph::Maximize, "Maximize") };
        if caption(ui, &t, min, "button:window.minimize", Glyph::Minimize, "Minimize").clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
        }
        if caption(ui, &t, max, "button:window.maximize", max_glyph, max_label).clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if caption(ui, &t, close, "button:window.close", Glyph::Close, "Close").clicked() {
            // File → Exit: the window closes the way the OS close does, so the library is flushed
            // (`on_exit`) before the process ends.
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
}

/// Left edge for a title `width` wide: centred on `center` when that fits in the free gap from
/// `left` to `right` with [`TITLE_PAD`] to spare on each side, else slid into that gap; `None` when
/// the gap is too small for the whole title (PhotoCraft's `title_x`).
pub fn title_x(center: f32, left: f32, right: f32, width: f32) -> Option<f32> {
    let (lo, hi) = (left + TITLE_PAD, right - TITLE_PAD - width);
    if !(lo.is_finite() && hi.is_finite() && width.is_finite()) || hi < lo {
        return None;
    }
    if !center.is_finite() {
        return Some(lo);
    }
    Some((center - width / 2.0).clamp(lo, hi))
}

/// A caption button's glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// One caption button: `hover` on hover, `pressed` while held; Close turns `caption_close` (× 0.8
/// while held) with a white glyph.
fn caption(ui: &mut Ui, t: &Tokens, rect: Rect, id: &str, glyph: Glyph, label: &str) -> egui::Response {
    let r = ui.interact(rect, Id::new(id), Sense::click());
    let close = glyph == Glyph::Close;
    let (down, hovered) = (r.is_pointer_button_down_on(), r.hovered());
    let fill = match (down, hovered) {
        (true, _) if close => Some(t.caption_close.gamma_multiply(0.8)),
        (_, true) if close => Some(t.caption_close),
        (true, _) => Some(t.pressed),
        (_, true) => Some(t.hover),
        _ => None,
    };
    let painter = ui.painter();
    if let Some(fill) = fill {
        painter.rect_filled(rect, 0, fill);
    }
    let ink = if close && (hovered || down) { t.on_accent } else { t.icon };
    paint_glyph(painter, glyph, rect.center(), Stroke::new(1.0, ink));
    widgets::register(ui.ctx(), id, Role::IconButton, label, Target::Widget(&r), false);
    r.on_hover_text(label)
}

/// The 10 pt line glyphs of the Windows caption buttons, centred at `at` (PhotoCraft's
/// `paint_windows_glyph`): a bar, a square, a front square with the corner of the one behind it,
/// an X.
fn paint_glyph(p: &egui::Painter, glyph: Glyph, at: egui::Pos2, s: Stroke) {
    let g = Rect::from_center_size(at, vec2(GLYPH, GLYPH));
    match glyph {
        Glyph::Minimize => {
            p.line_segment([g.left_center(), g.right_center()], s);
        }
        Glyph::Maximize => {
            p.rect_stroke(g, 0, s, StrokeKind::Middle);
        }
        Glyph::Restore => {
            let front = Rect::from_min_max(g.min + vec2(0.0, 2.0), g.max - vec2(2.0, 0.0));
            p.rect_stroke(front, 0, s, StrokeKind::Middle);
            p.line(vec![g.min + vec2(2.0, 2.0), g.min + vec2(2.0, 0.0), g.right_top(), g.max - vec2(0.0, 2.0), g.max - vec2(2.0, 2.0)], s);
        }
        Glyph::Close => {
            p.line_segment([g.left_top(), g.right_bottom()], s);
            p.line_segment([g.right_top(), g.left_bottom()], s);
        }
    }
}

/// Gives the OS window `title` when it changed since the last frame.
fn send_title(ctx: &Context, title: &str) {
    let id = Id::new("dalica-titlebar-title");
    let last: Option<String> = ctx.data(|d| d.get_temp(id));
    if last.as_deref() != Some(title) {
        ctx.data_mut(|d| d.insert_temp(id, title.to_string()));
        ctx.send_viewport_cmd(ViewportCommand::Title(title.to_string()));
    }
}

/// The resize edges of a `window` (its content rect) and where each one resizes toward: 5 pt bands
/// along the sides, 12 pt along each side of a corner. Corners first, so they win.
pub fn edges(window: Rect) -> Vec<(&'static str, ResizeDirection, Rect)> {
    let (l, r, t, b) = (window.left(), window.right(), window.top(), window.bottom());
    let (e, c) = (RESIZE_EDGE, RESIZE_CORNER);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0), y1.max(y0)));
    vec![
        ("resize:nw", ResizeDirection::NorthWest, rect(l, t, l + c, t + e)),
        ("resize:nw", ResizeDirection::NorthWest, rect(l, t, l + e, t + c)),
        ("resize:ne", ResizeDirection::NorthEast, rect(r - c, t, r, t + e)),
        ("resize:ne", ResizeDirection::NorthEast, rect(r - e, t, r, t + c)),
        ("resize:sw", ResizeDirection::SouthWest, rect(l, b - e, l + c, b)),
        ("resize:sw", ResizeDirection::SouthWest, rect(l, b - c, l + e, b)),
        ("resize:se", ResizeDirection::SouthEast, rect(r - c, b - e, r, b)),
        ("resize:se", ResizeDirection::SouthEast, rect(r - e, b - c, r, b)),
        ("resize:n", ResizeDirection::North, rect(l + c, t, r - c, t + e)),
        ("resize:s", ResizeDirection::South, rect(l + c, b - e, r - c, b)),
        ("resize:w", ResizeDirection::West, rect(l, t + c, l + e, b - c)),
        ("resize:e", ResizeDirection::East, rect(r - e, t + c, r, b - c)),
    ]
}

/// Where `p` resizes the window `window` toward, if it is on a resize edge.
pub fn edge_at(window: Rect, p: egui::Pos2) -> Option<ResizeDirection> {
    edges(window).into_iter().find(|(_, _, r)| r.contains(p)).map(|(_, d, _)| d)
}

fn cursor(d: ResizeDirection) -> CursorIcon {
    match d {
        ResizeDirection::North => CursorIcon::ResizeNorth,
        ResizeDirection::South => CursorIcon::ResizeSouth,
        ResizeDirection::East => CursorIcon::ResizeEast,
        ResizeDirection::West => CursorIcon::ResizeWest,
        ResizeDirection::NorthEast => CursorIcon::ResizeNorthEast,
        ResizeDirection::SouthEast => CursorIcon::ResizeSouthEast,
        ResizeDirection::NorthWest => CursorIcon::ResizeNorthWest,
        ResizeDirection::SouthWest => CursorIcon::ResizeSouthWest,
    }
}

/// The invisible resize edges of an undecorated window ([`Chrome::Drawn`]), while it is neither
/// maximized nor full screen. Call it after everything else is drawn, so the edges win over the
/// widgets under them; a press on one hands the resize to the OS. Each band registers as
/// `resize:<n|s|e|w|ne|nw|se|sw>`.
pub fn resize_edges(ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    let (maximized, fullscreen) = ctx.input(|i| (i.viewport().maximized.unwrap_or(false), i.viewport().fullscreen.unwrap_or(false)));
    if chrome(&ctx) != Chrome::Drawn || maximized || fullscreen {
        return;
    }
    let pressed = ctx.input(|i| i.pointer.primary_pressed());
    for (i, (id, dir, rect)) in edges(ctx.content_rect()).into_iter().enumerate() {
        let r = ui.interact(rect, Id::new(("dalica-resize", i)), Sense::drag()).on_hover_cursor(cursor(dir));
        // A corner has two bands under one id: the registry keeps the first.
        if i % 2 == 0 || i >= 8 {
            widgets::register(&ctx, id, Role::Other, "Resize the window", Target::Widget(&r), false);
        }
        if pressed && r.is_pointer_button_down_on() {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headless::Driver;
    use dalica_ui_kit::registry;

    #[test]
    fn the_title_names_the_section() {
        assert_eq!(window_title(None), "Dalica");
        assert_eq!(window_title(Some("  ")), "Dalica");
        assert_eq!(window_title(Some("Lisbon 2024")), "Lisbon 2024 — Dalica");
    }

    #[test]
    fn the_caption_buttons_sit_flush_in_the_top_right_corner() {
        let bar = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, HEIGHT));
        let p = parts(bar, Chrome::Drawn, 300.0);
        let [min, max, close] = p.captions.unwrap();
        // 46 × 31: the bar's last point is its divider, which the buttons leave visible.
        assert_eq!(close, Rect::from_min_max(pos2(1354.0, 0.0), pos2(1400.0, 31.0)));
        assert_eq!((min.size(), max.size(), close.size()), (CAPTION_BUTTON, CAPTION_BUTTON, CAPTION_BUTTON));
        assert_eq!(CAPTION_BUTTON, vec2(46.0, 31.0));
        assert_eq!((min.right(), max.right()), (max.left(), close.left()));
        assert_eq!(p.drag.unwrap(), Rect::from_min_max(pos2(300.0, 0.0), pos2(1262.0, 32.0)));
        // The 18 pt icon 10 pt from the left edge, centred in the 32 pt row; the menus 6 pt after it.
        assert_eq!(p.icon.unwrap(), Rect::from_min_size(pos2(10.0, 7.0), vec2(18.0, 18.0)));
        assert_eq!(p.menus_left, 34.0);
        // Menus wider than the room: the gap is empty, never inside out.
        assert_eq!(parts(bar, Chrome::Drawn, 5000.0).drag.unwrap().width(), 0.0);
        // A window narrower than the buttons keeps them inside it; a bar shorter than the divider
        // gives empty buttons, never inside-out ones.
        let tiny = parts(Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, 0.5)), Chrome::Drawn, 0.0);
        assert!(tiny.captions.unwrap().iter().all(|c| c.height() == 0.0));
        let tiny = parts(Rect::from_min_size(pos2(0.0, 0.0), vec2(50.0, HEIGHT)), Chrome::Drawn, 0.0);
        assert!(tiny.captions.unwrap().iter().all(|c| c.left() >= 0.0 && c.right() <= 50.0));
        let sys = parts(bar, Chrome::System, 300.0);
        assert_eq!((sys.icon, sys.captions, sys.drag), (None, None, None));
        let mac = parts(bar, Chrome::Mac, 0.0);
        assert_eq!(mac.drag.unwrap().left(), TRAFFIC_LIGHTS);
        assert_eq!(TRAFFIC_LIGHTS, 78.0, "the Craft apps' inset for the traffic lights");
    }

    #[test]
    fn the_title_is_centred_on_the_window_and_slides_into_the_gap() {
        // Room on both sides of the window's centre: centred on it.
        assert_eq!(title_x(700.0, 300.0, 1262.0, 100.0), Some(650.0));
        // The menus reach past the centre's left half: slid right, 16 pt clear of them.
        assert_eq!(title_x(450.0, 420.0, 1262.0, 100.0), Some(436.0));
        // The caption buttons crowd it: slid left, 16 pt clear of them.
        assert_eq!(title_x(1200.0, 300.0, 1262.0, 100.0), Some(1146.0));
        // Exactly room for it and its padding, and one point less.
        assert_eq!(title_x(0.0, 0.0, 132.0, 100.0), Some(16.0));
        assert_eq!(title_x(0.0, 0.0, 131.0, 100.0), None);
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(title_x(700.0, bad, 1262.0, 100.0), None, "{bad}");
            assert_eq!(title_x(700.0, 300.0, 1262.0, bad), None, "{bad}");
            assert_eq!(title_x(bad, 300.0, 1262.0, 100.0), Some(316.0), "{bad}");
        }
    }

    #[test]
    fn the_edges_resize_toward_their_side_and_corners_win() {
        let w = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 600.0));
        assert_eq!(edge_at(w, pos2(2.0, 2.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(w, pos2(10.0, 2.0)), Some(ResizeDirection::NorthWest));
        assert_eq!(edge_at(w, pos2(450.0, 2.0)), Some(ResizeDirection::North));
        assert_eq!(edge_at(w, pos2(898.0, 300.0)), Some(ResizeDirection::East));
        assert_eq!(edge_at(w, pos2(2.0, 590.0)), Some(ResizeDirection::SouthWest));
        assert_eq!(edge_at(w, pos2(450.0, 599.0)), Some(ResizeDirection::South));
        assert_eq!(edge_at(w, pos2(895.0, 598.0)), Some(ResizeDirection::SouthEast));
        assert_eq!(edge_at(w, pos2(450.0, 300.0)), None);
        assert_eq!(edge_at(w, pos2(6.0, 300.0)), None);
    }

    #[test]
    fn the_window_chrome_is_drawn_unless_the_window_says_otherwise() {
        let ctx = Context::default();
        assert_eq!(chrome(&ctx), Chrome::Drawn);
        set_chrome(&ctx, Chrome::Mac);
        assert_eq!(chrome(&ctx), Chrome::Mac);
    }

    fn ids(d: &Driver) -> Vec<String> {
        d.widgets().into_iter().map(|w| w.id).collect()
    }

    /// The title bar at both sizes: caption buttons flush in the corner, the drag gap between the
    /// menus and them, the title centred in it at 1400 wide; snapshots in `target/test-output/ui/`.
    #[test]
    fn headless_snapshots_of_the_title_bar_at_both_sizes() {
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
        std::fs::create_dir_all(&out).unwrap();
        for (w, h) in [(1400u32, 900u32), (900, 600)] {
            let (_dir, s, _ids) = crate::app::tests::session_with_photos(2);
            let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
            d.settle(600).unwrap();
            let find = |d: &Driver, id: &str| d.widgets().into_iter().find(|x| x.id == id).unwrap_or_else(|| panic!("{id} at {w}: {:?}", ids(d)));
            let close = find(&d, "button:window.close");
            assert_eq!(close.rect, [w as f32 - 46.0, 0.0, 46.0, 31.0], "{w}");
            assert_eq!(find(&d, "button:window.maximize").rect, [w as f32 - 92.0, 0.0, 46.0, 31.0]);
            assert_eq!(find(&d, "button:window.minimize").rect, [w as f32 - 138.0, 0.0, 46.0, 31.0]);
            assert_eq!(find(&d, "button:window.maximize").label, "Maximize");
            let drag = find(&d, "titlebar:drag");
            let help = find(&d, "menu:Help");
            assert!((drag.rect[0] - (help.rect[0] + help.rect[2])).abs() <= 8.0, "the gap starts after the menus: {drag:?} {help:?}");
            assert_eq!(drag.rect[0] + drag.rect[2], w as f32 - 138.0);
            let file = find(&d, "menu:File");
            assert!(file.rect[0] >= ICON_LEFT + APP_ICON, "the app icon comes first");
            // The menu titles sit centred in the row above the divider.
            assert!((file.rect[1] + file.rect[3] / 2.0 - (HEIGHT - DIVIDER) / 2.0).abs() <= 1.0, "{file:?}");
            assert!(d.app().panels().unwrap().unregistered_widgets().is_empty());
            let shot = d.screenshot().unwrap();
            // The top 32 rows: the title bar.
            let rows = shot.rgba.get(..shot.width as usize * HEIGHT as usize * 4).unwrap().to_vec();
            let bar = dalica_ui_kit::Image { width: shot.width, height: HEIGHT as u32, rgba: rows };
            let png = dalica_scenes::fingerprint::png_rgba(bar.width, bar.height, &bar.rgba);
            std::fs::write(out.join(format!("titlebar-{w}x{h}.png")), png).unwrap();
            // The title is drawn in the gap: some pixel there is not the bar's fill.
            let t = Tokens::default();
            let row = 15 * bar.width as usize * 4;
            let gap = (drag.rect[0] as usize + 1)..((drag.rect[0] + drag.rect[2]) as usize - 1);
            let inked = gap.filter(|x| bar.rgba.get(row + x * 4..row + x * 4 + 3) != Some(&t.chrome.to_array()[..3])).count();
            assert!(inked > 0, "the title is drawn at {w}");
            // The bar's last row is the divider, under the caption buttons too.
            let last = (HEIGHT as usize - 1) * bar.width as usize * 4;
            for x in [0, w as usize / 2, w as usize - 1] {
                assert_eq!(bar.rgba.get(last + x * 4..last + x * 4 + 3), Some(&t.divider.to_array()[..3]), "the divider at x {x}, {w}");
            }
        }
    }

    /// The title bar's top-left corner shows the app icon (`assets/app-icon/`), not a glyph: the
    /// 18 pt icon is drawn from the 24 px render at 1× and the 48 px one at 2× (the smallest that
    /// covers its pixels), filling its square, the bar's fill around it.
    #[test]
    fn the_title_bar_shows_the_app_icon_at_its_top_left() {
        let t = Tokens::default();
        let fill = t.chrome.to_array();
        let is_orange = |p: [u8; 4]| p[..3].iter().zip([0xd6, 0x5d, 0x0e]).all(|(&g, o): (&u8, i32)| (i32::from(g) - o).abs() <= 8);
        for (ppp, render) in [(1.0f32, 24u32), (2.0, 48)] {
            let (_dir, s, _ids) = crate::app::tests::session_with_photos(1);
            let mut d = Driver::new(App::new(Some(s)), (1400.0 * ppp) as u32, (900.0 * ppp) as u32, ppp);
            d.settle(600).unwrap();
            assert_eq!(crate::app_icon::render_for(APP_ICON * ppp).0, render);
            let shot = d.screenshot().unwrap();
            let edge = (APP_ICON * ppp) as u32;
            let (x0, y0) = ((ICON_LEFT * ppp) as u32, ((HEIGHT - APP_ICON) / 2.0 * ppp) as u32);
            let mut orange = 0;
            for y in 0..edge {
                for x in 0..edge {
                    orange += usize::from(is_orange(shot.pixel(x0 + x, y0 + y).unwrap()));
                }
            }
            assert!(orange >= (edge * edge / 4) as usize, "at {ppp}×, the icon is the orange app icon ({orange} orange pixels)");
            // Nothing of it outside its square: the bar's fill one pixel beyond each side.
            for (x, y) in [(x0 - 1, y0 + edge / 2), (x0 + edge, y0 + edge / 2), (x0 + edge / 2, y0 - 1), (x0 + edge / 2, y0 + edge)] {
                assert_eq!(&shot.pixel(x, y).unwrap()[..3], &fill[..3], "at {ppp}×, ({x}, {y}) is the bar's fill");
            }
        }
    }

    #[test]
    fn under_system_decorations_only_the_menus_are_drawn() {
        let (_dir, s, _ids) = crate::app::tests::session_with_photos(1);
        let mut d = Driver::new(App::new(Some(s)), 900, 600, 1.0);
        set_chrome(d.ctx(), Chrome::System);
        d.settle(60).unwrap();
        let ids = ids(&d);
        assert!(ids.iter().any(|i| i == "menu:File"), "{ids:?}");
        assert!(!ids.iter().any(|i| i.starts_with("button:window.") || i == "titlebar:drag"), "{ids:?}");
        set_chrome(d.ctx(), Chrome::Mac);
        d.settle(60).unwrap();
        let ids: Vec<String> = registry::widgets(d.ctx()).into_iter().map(|w| w.id).collect();
        assert!(!ids.iter().any(|i| i.starts_with("menu:")), "macOS: the menus are in the system menu bar: {ids:?}");
        assert!(ids.iter().any(|i| i == "titlebar:drag"));
    }
}
