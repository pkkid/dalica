//! The window's geometry (`ui-layout.md` §2, `design-system.md` §4): one pure function from the
//! window rect and the view state to a rect per region, so the panels, the widget registry and the
//! tests all use the same numbers. Every region shown registers its rect as `region:<name>` (the
//! names of `control-protocol.md` §6.6), so agents and tests read the layout from the registry.
//!
//! The splitter between the tree and the light table lives here too: it is part of the layout, not
//! of either pane.

use dalica_ui_kit::Tokens;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Context, CursorIcon, Id, Rect, Sense, Ui, pos2, vec2};

use crate::state::{LIGHT_TABLE_MIN, Pane, TREE_DEFAULT, TREE_MIN, UiState};

/// Menu bar (the title row on Windows and Linux) height.
pub const MENU_BAR: f32 = 24.0;
/// Toolbar height.
pub const TOOLBAR: f32 = 40.0;
/// Splitter width between the tree and the light table, and its grip.
pub const SPLITTER: f32 = 8.0;
pub const GRIP: egui::Vec2 = vec2(8.0, 48.0);
/// Light-table scroll bar width.
pub const SCROLLBAR: f32 = 16.0;
/// Side pane width, and the cap of its large width (min(30 % W, 392)).
pub const SIDE_PANE: f32 = 280.0;
pub const SIDE_PANE_LARGE_MAX: f32 = 392.0;
pub const SIDE_PANE_LARGE_SHARE: f32 = 0.30;
/// Status line height.
pub const STATUS_LINE: f32 = 20.0;
/// Bottom area height, and where it splits: the tray left of 0.365·W, the controls right of it.
pub const BOTTOM: f32 = 96.0;
pub const BOTTOM_SPLIT: f32 = 0.365;
/// The tray box: from x = 6 to 0.365·W − 56, 80 high, centred in the bottom area.
pub const TRAY_LEFT: f32 = 6.0;
pub const TRAY_RIGHT_INSET: f32 = 56.0;
pub const TRAY_HEIGHT: f32 = 80.0;
/// The tray's three stacked buttons (Hold, Clear, Add to Album): 34 × 24 each, 4 apart, so the
/// column is as tall as the box; 8 right of the box.
pub const TRAY_BUTTON_WIDTH: f32 = 34.0;
pub const TRAY_BUTTON_GAP: f32 = 8.0;
/// The bottom controls' top row (star, rotate, loupe, size slider, pane toggles): 24-high buttons
/// with 6 above and below. The action-button row (a 1 pt separator, then 64 × 56 buttons) is the
/// rest of the bottom area.
pub const CONTROLS_ROW: f32 = 36.0;

/// Where every region of the library view is, in window points. `None` for a region not shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub window: Rect,
    pub menu_bar: Rect,
    pub toolbar: Rect,
    /// The room tab strip: only when a second room is open (M11), so never yet.
    pub tab_strip: Option<Rect>,
    pub tree: Rect,
    pub splitter: Rect,
    pub light_table: Rect,
    pub scrollbar: Rect,
    pub side_pane: Option<Rect>,
    pub status_line: Rect,
    /// The whole bottom area: the tray part and the controls part.
    pub bottom: Rect,
    pub tray: Rect,
    pub tray_buttons: Rect,
    pub bottom_controls: Rect,
    pub action_buttons: Rect,
}

/// A rect from `x0..x1` × `y0..y1`, never inside out: a window too small for a region gives it no
/// area rather than a negative one.
fn span(x0: f32, x1: f32, y0: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(x0, y0), pos2(x1.max(x0), y1.max(y0)))
}

/// The tree width the layout uses for a window `width` wide with no side pane: the user's width
/// clamped to `TREE_MIN ..= width − LIGHT_TABLE_MIN` (never below `TREE_MIN`); a width that is not
/// a finite number is the default.
pub fn tree_width(requested: f32, width: f32) -> f32 {
    tree_width_beside(requested, width, 0.0)
}

/// The tree width beside a side pane `pane` wide (0 when closed) in a window `width` wide
/// (`ui-layout.md` §11): the tree keeps the user's width until the light table (with its splitter
/// and scroll bar) would drop below `LIGHT_TABLE_MIN`, then gives up width down to `TREE_MIN`, never
/// further. Only then does a narrowing window take the pane's large width and close the pane
/// (`side_pane::on_resize`), so the tree always shrinks first.
pub fn tree_width_beside(requested: f32, width: f32, pane: f32) -> f32 {
    let requested = if requested.is_finite() { requested } else { TREE_DEFAULT };
    let pane = if pane.is_finite() { pane.max(0.0) } else { 0.0 };
    let max = if width.is_finite() { (width - pane - LIGHT_TABLE_MIN).max(TREE_MIN) } else { TREE_MIN };
    requested.clamp(TREE_MIN, max)
}

/// The side pane's width in a window `width` wide.
pub fn side_pane_width(large: bool, width: f32) -> f32 {
    if large { (SIDE_PANE_LARGE_SHARE * width.max(0.0)).min(SIDE_PANE_LARGE_MAX) } else { SIDE_PANE }
}

/// The tree width the layout gives `state` in a window `width` wide: the user's width, narrowed
/// for the open side pane as [`tree_width_beside`] says. The toolbar places its right-hand group
/// from this too, so the filters start 8 right of the splitter wherever it is.
pub fn effective_tree_width(state: &UiState, width: f32) -> f32 {
    let pane = state.pane.map_or(0.0, |_: Pane| side_pane_width(state.pane_large, width));
    tree_width_beside(state.tree_width, width, pane)
}

/// Lays out the library view in `window` for `state`.
///
/// Narrow windows (`ui-layout.md` §11, `design-system.md` §11.1): no region is ever dropped. The
/// tree shrinks toward its minimum before the light table goes below 480 ([`tree_width_beside`]);
/// a narrowing window then takes the side pane's large width and closes the pane (the side pane
/// does that when it sees the window narrow, so a pane opened on purpose in a narrow window stays,
/// and the light table is then narrower than 480). The chrome inside the rows drops on its own, in
/// the stated order: the window title in the title bar when it does not fit its gap, the toolbar's
/// filter caption below 1100, the search field down to 160; the action buttons go behind More…
/// below 1100. Menus, filter toggles and caption buttons always stay.
///
/// Full screen (F11, [`UiState::full_screen`]) is the whole window filling the screen: it lays out
/// exactly like a window of the screen's size, with every region and control, the menus and the
/// caption buttons included (the menus never go away, and leaving full screen needs no hidden
/// gesture). The window's own frame stops there: the title bar has no resize edges while full
/// screen.
pub fn compute(window: Rect, state: &UiState) -> Layout {
    let window = if window.is_finite() { window } else { Rect::ZERO };
    let (left, right, top, bottom_edge) = (window.min.x, window.max.x.max(window.min.x), window.min.y, window.max.y.max(window.min.y));
    let width = right - left;

    // Rows from the top and from the bottom; the middle gets what is left.
    let menu_bar = span(left, right, top, top + MENU_BAR);
    let toolbar = span(left, right, menu_bar.max.y, menu_bar.max.y + TOOLBAR);
    let bottom = span(left, right, (bottom_edge - BOTTOM).max(toolbar.max.y), bottom_edge);
    let status_line = span(left, right, (bottom.min.y - STATUS_LINE).max(toolbar.max.y), bottom.min.y);
    let (mid_top, mid_bottom) = (toolbar.max.y, status_line.min.y);

    // Columns of the middle: tree, splitter, light table, its scroll bar, the side pane. The tree
    // makes room for the pane first.
    let pane_w = state.pane.map(|_: Pane| side_pane_width(state.pane_large, width));
    let tree = span(left, (left + effective_tree_width(state, width)).min(right), mid_top, mid_bottom);
    let splitter = span(tree.max.x, (tree.max.x + SPLITTER).min(right), mid_top, mid_bottom);
    let pane_left = pane_w.map_or(right, |w| (right - w).max(splitter.max.x));
    let side_pane = pane_w.map(|_| span(pane_left, right, mid_top, mid_bottom));
    let scroll_left = (pane_left - SCROLLBAR).max(splitter.max.x);
    let light_table = span(splitter.max.x, scroll_left, mid_top, mid_bottom);
    let scrollbar = span(scroll_left, pane_left, mid_top, mid_bottom);

    // The bottom area: the tray and its buttons left of the split, two rows of controls right of it.
    let split = (left + BOTTOM_SPLIT * width).min(right);
    let tray_top = bottom.min.y + (BOTTOM - TRAY_HEIGHT) / 2.0;
    let tray = span(left + TRAY_LEFT, split - TRAY_RIGHT_INSET, tray_top, (tray_top + TRAY_HEIGHT).min(bottom.max.y));
    let buttons_left = tray.max.x + TRAY_BUTTON_GAP;
    let tray_buttons = span(buttons_left, (buttons_left + TRAY_BUTTON_WIDTH).min(split), tray.min.y, tray.max.y);
    let bottom_controls = span(split, right, bottom.min.y, (bottom.min.y + CONTROLS_ROW).min(bottom.max.y));
    let action_buttons = span(split, right, bottom_controls.max.y, bottom.max.y);

    Layout {
        window,
        menu_bar,
        toolbar,
        tab_strip: None,
        tree,
        splitter,
        light_table,
        scrollbar,
        side_pane,
        status_line,
        bottom,
        tray,
        tray_buttons,
        bottom_controls,
        action_buttons,
    }
}

impl Layout {
    /// Every region by its protocol name (`control-protocol.md` §6.6), in that order, with a label;
    /// `None` for one not shown. The edit view's regions join with the edit view (M7).
    pub fn regions(&self) -> [(&'static str, &'static str, Option<Rect>); 13] {
        [
            ("menuBar", "Menu bar", Some(self.menu_bar)),
            ("toolbar", "Toolbar", Some(self.toolbar)),
            ("tabStrip", "Room tabs", self.tab_strip),
            ("tree", "Library tree", Some(self.tree)),
            ("splitter", "Splitter", Some(self.splitter)),
            ("lightTable", "Light table", Some(self.light_table)),
            ("scrollbar", "Light-table scroll bar", Some(self.scrollbar)),
            ("sidePane", "Side pane", self.side_pane),
            ("statusLine", "Status line", Some(self.status_line)),
            ("tray", "Photo tray", Some(self.tray)),
            ("trayButtons", "Tray buttons", Some(self.tray_buttons)),
            ("bottomControls", "Bottom controls", Some(self.bottom_controls)),
            ("actionButtons", "Action buttons", Some(self.action_buttons)),
        ]
    }

    /// Registers every region shown as `region:<name>` for this pass. A region with no area (a
    /// window below its minimum size) is not listed.
    pub fn register(&self, ctx: &Context) {
        for (name, label, rect) in self.regions() {
            if let Some(rect) = rect {
                widgets::register(ctx, &format!("region:{name}"), Role::Other, label, Target::Painted { rect, clip: self.window }, false);
            }
        }
    }
}

/// The splitter between the tree and the light table (`splitter:tree`): drag it to change the tree
/// width, clamped as [`tree_width_beside`] says for the side pane shown. Paints the bar and its
/// centred grip.
pub fn splitter(ui: &mut Ui, t: &Tokens, layout: &Layout, state: &mut UiState) {
    let rect = layout.splitter;
    if !rect.is_positive() {
        return;
    }
    let id = Id::new("splitter:tree");
    let response = ui.interact(rect, id, Sense::drag());
    let width = layout.window.width();
    let pane = layout.side_pane.map_or(0.0, |r| r.width());
    // The tree follows the pointer from where the press began, so a drag loses nothing to egui's
    // drag threshold: the width at the press plus how far the pointer has moved since.
    if response.drag_started() {
        let start = layout.tree.width();
        ui.ctx().data_mut(|d| d.insert_temp(id, start));
    }
    if response.dragged() {
        let start = ui.ctx().data(|d| d.get_temp::<f32>(id)).unwrap_or(layout.tree.width());
        let moved = ui.input(|i| i.pointer.press_origin().zip(i.pointer.latest_pos()).map(|(from, to)| to.x - from.x));
        if let Some(dx) = moved.filter(|dx| dx.is_finite()) {
            state.tree_width = tree_width_beside(start + dx, width, pane);
        }
    }
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, t.chrome);
    let grip = Rect::from_center_size(rect.center(), GRIP);
    let color = if response.dragged() || response.hovered() { t.hover } else { t.divider };
    painter.rect_filled(grip.shrink2(vec2(3.0, 0.0)), 1, color);
    widgets::register(ui.ctx(), "splitter:tree", Role::Other, "Resize the library tree", Target::Widget(&response), response.dragged());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(w, h))
    }

    fn xywh(r: Rect) -> [f32; 4] {
        [r.min.x, r.min.y, r.width(), r.height()]
    }

    #[test]
    fn the_default_window_is_laid_out_as_the_spec_says() {
        let l = compute(window(1400.0, 900.0), &UiState::default());
        assert_eq!(xywh(l.menu_bar), [0.0, 0.0, 1400.0, 24.0]);
        assert_eq!(xywh(l.toolbar), [0.0, 24.0, 1400.0, 40.0]);
        assert_eq!(xywh(l.tree), [0.0, 64.0, 240.0, 900.0 - 64.0 - 116.0]);
        assert_eq!(xywh(l.splitter), [240.0, 64.0, 8.0, 720.0]);
        assert_eq!(xywh(l.light_table), [248.0, 64.0, 1400.0 - 248.0 - 16.0, 720.0]);
        assert_eq!(xywh(l.scrollbar), [1384.0, 64.0, 16.0, 720.0]);
        assert_eq!(l.side_pane, None);
        assert_eq!(l.tab_strip, None);
        assert_eq!(xywh(l.status_line), [0.0, 784.0, 1400.0, 20.0]);
        assert_eq!(xywh(l.bottom), [0.0, 804.0, 1400.0, 96.0]);
        let split = 0.365 * 1400.0;
        assert_eq!(xywh(l.tray), [6.0, 812.0, split - 56.0 - 6.0, 80.0]);
        assert_eq!(xywh(l.tray_buttons), [split - 48.0, 812.0, 34.0, 80.0]);
        assert_eq!(xywh(l.bottom_controls), [split, 804.0, 1400.0 - split, 36.0]);
        assert_eq!(xywh(l.action_buttons), [split, 840.0, 1400.0 - split, 60.0]);
    }

    #[test]
    fn the_side_pane_pushes_the_light_table_and_has_a_large_width() {
        let mut s = UiState { pane: Some(Pane::Tags), ..UiState::default() };
        let l = compute(window(1400.0, 900.0), &s);
        assert_eq!(l.side_pane.map(xywh), Some([1120.0, 64.0, 280.0, 720.0]));
        assert_eq!(l.scrollbar.max.x, 1120.0);
        assert_eq!(l.light_table.width(), 1120.0 - 16.0 - 248.0);
        s.pane_large = true;
        assert_eq!(compute(window(1400.0, 900.0), &s).side_pane.map(|r| r.width()), Some(392.0), "capped at 392");
        assert_eq!(compute(window(1100.0, 700.0), &s).side_pane.map(|r| r.width()), Some(330.0), "30 % of 1100");
    }

    #[test]
    fn the_tree_width_is_clamped_between_its_minimum_and_w_minus_480() {
        assert_eq!(tree_width(240.0, 1400.0), 240.0);
        assert_eq!(tree_width(100.0, 1400.0), 200.0);
        assert_eq!(tree_width(1000.0, 1400.0), 920.0);
        assert_eq!(tree_width(500.0, 900.0), 420.0);
        assert_eq!(tree_width(500.0, 600.0), 200.0, "never below the minimum");
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(tree_width(bad, 1400.0), 240.0, "{bad}");
            assert_eq!(tree_width(300.0, bad), 200.0, "{bad}");
        }
        let s = UiState { tree_width: 300.0, ..UiState::default() };
        let l = compute(window(1400.0, 900.0), &s);
        assert_eq!((l.tree.width(), l.splitter.min.x, l.light_table.min.x), (300.0, 300.0, 308.0));
    }

    #[test]
    fn the_tree_shrinks_for_the_side_pane_before_the_light_table_drops_below_480() {
        assert_eq!(tree_width_beside(240.0, 1400.0, 280.0), 240.0, "room for both");
        assert_eq!(tree_width_beside(400.0, 1100.0, 280.0), 340.0, "1100 − 280 − 480");
        assert_eq!(tree_width_beside(240.0, 1000.0, 300.0), 220.0, "a large pane at 1000 (30 %)");
        assert_eq!(tree_width_beside(240.0, 960.0, 280.0), 200.0, "the minimum, the light table keeps 480");
        assert_eq!(tree_width_beside(240.0, 900.0, 280.0), 200.0, "never below the minimum");
        assert_eq!(tree_width_beside(240.0, 1400.0, 0.0), tree_width(240.0, 1400.0), "no pane: the plain clamp");
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -100.0] {
            let w = tree_width_beside(240.0, 1400.0, bad);
            assert!((TREE_MIN..=1400.0).contains(&w), "{bad}: {w}");
        }

        // In the layout: with the pane open the tree gives up width first; the columns still tile.
        let s = |pane, large, tree| UiState { pane, pane_large: large, tree_width: tree, ..UiState::default() };
        for (w, pane, large, tree, want) in [
            (1400.0, Some(Pane::Tags), false, 240.0, 240.0),
            (1100.0, Some(Pane::Tags), false, 400.0, 340.0),
            (1100.0, None, false, 400.0, 400.0),
            (1000.0, Some(Pane::People), true, 240.0, 220.0),
            (960.0, Some(Pane::People), false, 240.0, 200.0),
            (900.0, Some(Pane::People), false, 240.0, 200.0),
            (900.0, None, false, 240.0, 240.0),
        ] {
            let l = compute(window(w, 700.0), &s(pane, large, tree));
            assert_eq!(l.tree.width(), want, "{w} {pane:?} {large} {tree}");
            let light = l.light_table.width() + SPLITTER + SCROLLBAR;
            assert!(light >= LIGHT_TABLE_MIN || want == TREE_MIN, "{w}: the light table keeps 480 unless the tree is at its minimum");
        }
    }

    #[test]
    fn full_screen_lays_out_like_a_window_of_the_screens_size() {
        for (w, h) in [(1400.0, 900.0), (1100.0, 700.0), (900.0, 600.0), (1920.0, 1080.0), (3840.0, 2160.0)] {
            for pane in [None, Some(Pane::Places)] {
                let windowed = UiState { pane, ..UiState::default() };
                let full = UiState { full_screen: true, ..windowed.clone() };
                assert_eq!(compute(window(w, h), &full), compute(window(w, h), &windowed), "{w}×{h} {pane:?}");
            }
        }
    }

    #[test]
    fn hostile_windows_give_empty_regions_never_inside_out_ones() {
        let all = |l: &Layout| l.regions().into_iter().filter_map(|(_, _, r)| r).collect::<Vec<_>>();
        for w in [window(0.0, 0.0), window(10.0, 10.0), window(-50.0, 300.0), window(f32::NAN, 900.0), window(1e9, 1e9)] {
            for (pane, full_screen) in [(None, false), (Some(Pane::People), false), (Some(Pane::People), true)] {
                let l = compute(w, &UiState { pane, pane_large: true, tree_width: f32::NAN, full_screen, ..UiState::default() });
                for r in all(&l) {
                    assert!(r.width() >= 0.0 && r.height() >= 0.0 && !r.any_nan(), "{w:?}: {r:?}");
                }
            }
        }
    }

    #[test]
    fn regions_tile_the_window_without_overlapping() {
        for (w, h) in [(1400.0, 900.0), (1100.0, 700.0), (900.0, 600.0)] {
            for (pane, tree_width) in [(None, 240.0), (Some(Pane::Places), 240.0), (Some(Pane::Places), 600.0)] {
                let l = compute(window(w, h), &UiState { pane, tree_width, ..UiState::default() });
                let rows = [l.menu_bar, l.toolbar, l.tree, l.status_line, l.bottom];
                assert_eq!(rows.iter().map(|r| r.height()).sum::<f32>(), h, "{w}×{h}: the rows add up to the height");
                let mut cols = vec![l.tree, l.splitter, l.light_table, l.scrollbar];
                cols.extend(l.side_pane);
                assert_eq!(cols.iter().map(|r| r.width()).sum::<f32>(), w, "{w}×{h}: the columns add up to the width");
                for pair in cols.windows(2) {
                    assert_eq!(pair[0].max.x, pair[1].min.x);
                }
                for r in [l.tray, l.tray_buttons, l.bottom_controls, l.action_buttons] {
                    assert!(l.bottom.contains_rect(r), "{r:?} in {:?}", l.bottom);
                }
                assert!(l.tray.max.x < l.tray_buttons.min.x && l.tray_buttons.max.x <= l.bottom_controls.min.x);
            }
        }
    }
}
