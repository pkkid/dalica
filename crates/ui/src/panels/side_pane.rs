//! The side pane (`ui-layout.md` §7): right of the light table's scroll bar when a pane is open,
//! 280 wide (large: min(30 % W, 392)), pushing the light table. The frame: a 30-high header with
//! « at the left (`button:pane.widen`: toggles the large width), the pane's title centred in
//! SemiBold, × at the right (`button:pane.close`), then the pane body (`pane:<name>`): the
//! Properties pane ([`super::properties`]), the Tags pane ([`super::tags`]), the People pane
//! ([`super::people_pane`]) and the Places pane ([`super::places`]). When the window narrows so far that the light table would drop below 480
//! the pane gives up its large width, then closes itself.

use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Id, Rect, Ui, UiBuilder, pos2, vec2};

use super::layout::side_pane_width;
use crate::app::App;
use crate::state::{LIGHT_TABLE_MIN, Pane, TREE_MIN, UiState};
use crate::tl;

/// Header height and its two icon buttons' edge and inset (`design-system.md` §4).
pub const HEADER: f32 = 30.0;
pub const BUTTON: f32 = 24.0;
const INSET: f32 = 3.0;

/// The pane's title and its protocol name.
pub fn title(pane: Pane) -> (String, &'static str) {
    match pane {
        Pane::People => (tl!("People"), "people"),
        Pane::Places => (tl!("Places"), "places"),
        Pane::Tags => (tl!("Tags"), "tags"),
        Pane::Properties => (tl!("Properties"), "properties"),
    }
}

/// Whether a pane `large` or not leaves the light table its 480 points in a window `width` wide,
/// once the tree has shrunk to its minimum (`ui-layout.md` §11; the 480 counts the splitter and
/// the scroll bar, as the tree's own limit does).
pub fn fits(width: f32, large: bool) -> bool {
    width.is_finite() && width - TREE_MIN - side_pane_width(large, width) >= LIGHT_TABLE_MIN
}

/// What a window that narrowed from `before` to `width` does to the open pane in `state`: nothing
/// while it still fits; else the large pane takes its normal width if that fits, or the pane
/// closes. A window that did not narrow changes nothing, so a pane opened on purpose in a narrow
/// window stays open. Returns whether anything changed.
pub fn on_resize(state: &mut UiState, before: f32, width: f32) -> bool {
    let narrowed = width < before;
    if state.pane.is_none() || !narrowed || fits(width, state.pane_large) {
        return false;
    }
    if state.pane_large && fits(width, false) {
        state.pane_large = false;
    } else {
        state.pane = None;
        state.pane_large = false;
    }
    true
}

/// Paints the side pane into `ui`'s rect (the side-pane region of [`super::layout`]); called only
/// while a pane is open.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let ctx = ui.ctx().clone();
    // The window's width the last pass, to see it narrow (only if the pane was drawn then: a pane
    // just opened compares with nothing).
    let width = ctx.content_rect().width();
    let pass = ctx.cumulative_pass_nr();
    let seen = Id::new("pane:windowWidth");
    let before = match ctx.data(|d| d.get_temp::<(u64, f32)>(seen)) {
        Some((at, w)) if at.saturating_add(1) == pass => w,
        _ => width,
    };
    ctx.data_mut(|d| d.insert_temp(seen, (pass, width)));
    if on_resize(app.state_mut(), before, width) {
        ctx.request_repaint();
    }
    let Some(pane) = app.state().pane else { return };
    let (name, id) = title(pane);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0, t.chrome);
    painter.rect_filled(Rect::from_min_max(rect.min, pos2(rect.min.x + 1.0, rect.max.y)), 0, t.divider);

    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER));
    painter.rect_filled(Rect::from_min_max(pos2(header.min.x, header.max.y - 1.0), header.max), 0, t.divider);
    let button = |x: f32| Rect::from_min_size(pos2(x, header.min.y + (HEADER - BUTTON) / 2.0), vec2(BUTTON, BUTTON));
    let large = app.state().pane_large;
    let widen_tip = if large { tl!("Make the pane narrower") } else { tl!("Make the pane wider") };
    let mut left = ui.new_child(UiBuilder::new().max_rect(button(header.min.x + INSET)));
    let widen = widgets::icon_button(&mut left, &t, "button:pane.widen", Icon::ChevronsLeft, BUTTON, large, true, &widen_tip);
    let mut right = ui.new_child(UiBuilder::new().max_rect(button(header.max.x - INSET - BUTTON)));
    let close = widgets::icon_button(&mut right, &t, "button:pane.close", Icon::Close, BUTTON, false, true, &tl!("Close the pane"));
    let room = (header.width() - 2.0 * (INSET + BUTTON + 4.0)).max(0.0);
    let title_rect = Rect::from_center_size(header.center(), vec2(room, HEADER));
    widgets::label_in(
        &painter.with_clip_rect(title_rect.intersect(rect)),
        title_rect,
        Align2::CENTER_CENTER,
        &name,
        fonts::semibold(t.body),
        t.text_label,
    );

    // The body: each pane draws its own.
    let body = Rect::from_min_max(pos2(rect.min.x, header.max.y), rect.max);
    widgets::register(&ctx, &format!("pane:{id}"), Role::Other, &name, Target::Painted { rect: body, clip: rect }, false);
    if body.is_positive() {
        let mut inner = ui.new_child(UiBuilder::new().id_salt(("pane", id)).max_rect(body));
        inner.set_clip_rect(body.intersect(ui.clip_rect()));
        match pane {
            Pane::Properties => super::properties::show(app, &mut inner),
            Pane::Tags => super::tags::show(app, &mut inner),
            Pane::People => super::people_pane::show(app, &mut inner),
            Pane::Places => super::places::show(app, &mut inner),
        }
    }

    if widen.clicked() {
        app.state_mut().pane_large = !large;
    }
    if close.clicked() {
        let state = app.state_mut();
        state.pane = None;
        state.pane_large = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pane_fits_while_the_light_table_keeps_480() {
        assert!(fits(1400.0, false) && fits(1400.0, true));
        assert!(fits(960.0, false), "200 + 480 + 280");
        assert!(!fits(959.0, false));
        assert!(!fits(900.0, false));
        assert!(fits(1100.0, true), "large is 330 there: 200 + 480 + 330 = 1010");
        // Large is 30 % of W up to 1306 wide: it fits from 0.7 · W ≥ 680, W ≥ 971.4.
        assert!(fits(972.0, true) && !fits(971.0, true) && fits(971.0, false));
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            assert!(!fits(bad, false), "{bad}");
        }
    }

    #[test]
    fn a_narrowing_window_shrinks_then_closes_the_pane() {
        let open = |large| UiState { pane: Some(Pane::Tags), pane_large: large, ..UiState::default() };
        let mut s = open(true);
        assert!(!on_resize(&mut s, 1400.0, 1200.0), "still fits");
        assert_eq!(s, open(true));
        assert!(on_resize(&mut s, 1200.0, 970.0), "large no longer fits, normal does");
        assert_eq!((s.pane, s.pane_large), (Some(Pane::Tags), false));
        assert!(on_resize(&mut s, 970.0, 900.0));
        assert_eq!((s.pane, s.pane_large), (None, false), "closed");

        let mut s = open(false);
        assert!(!on_resize(&mut s, 900.0, 900.0), "opened on purpose in a narrow window: stays");
        assert!(!on_resize(&mut s, 800.0, 900.0), "widening never closes it");
        assert_eq!(s, open(false));
        assert!(!on_resize(&mut s, f32::NAN, 900.0) && !on_resize(&mut s, 900.0, f32::NAN), "a hostile width changes nothing");
        let mut closed = UiState::default();
        assert!(!on_resize(&mut closed, 1400.0, 900.0));
    }
}
