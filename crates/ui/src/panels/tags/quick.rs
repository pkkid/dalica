//! The Tags pane's quick tags (`organizing.md` §5.2): the caption with its gear
//! (`button:quicktags.configure`, opening Configure Quick Tags through `quicktags.configure`) and
//! ten buttons in rows of 2/3/2/3 (`quicktag:1` … `quicktag:10`), each as wide as its row allows.
//!
//! A button shows the tag it applies (`tags.quick.get` → `shown`: the stored slots, with buttons
//! 1 and 2 following the most recently added tags while "Use the top two buttons for recently used
//! tags" is on), cut short with "…" when too long, or "+" while it has none. A click on a tag
//! toggles it on the selected photos (`photo.tags.toggle`, one undo step): off when every one of
//! them holds it, else on for all. The button looks pressed while every selected photo holds its
//! tag, and is greyed with nothing selected. A click on "+" opens Configure Quick Tags, where it
//! can be given a tag. Ctrl+Alt+1 … Ctrl+Alt+0 apply the same ten through the engine's
//! `tags.quick.1` … `tags.quick.10`, on the tray's photos ([`crate::keys::TRAY_COMMANDS`]).
//!
//! What the buttons show is read from the settings each frame: ten slots and ten recent tags, the
//! same small amount of work whatever the size of the library.

use dalica_engine::Context;
use dalica_engine::cmd::tags::quick_tags_shown;
use dalica_engine::keymap::Platform;
use dalica_engine::settings::QUICK_TAGS;
use dalica_library::tags::fold;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::text::{LayoutJob, TextWrapping};
use egui::{CornerRadius, Id, Rect, Response, Sense, Stroke, StrokeKind, Ui, WidgetType, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::tl;

pub const CONFIGURE_ID: &str = "button:quicktags.configure";
/// The buttons per row, top to bottom.
pub const ROWS: [usize; 4] = [2, 3, 2, 3];
/// What a button with no tag shows.
pub const UNASSIGNED: &str = "+";
/// The gear's edge (points).
const GEAR: f32 = 24.0;
/// Between two buttons of a row, and between rows (points).
const GAP: f32 = 4.0;
/// The extra room under the first row, which sets the recent tags apart (points).
const RECENT_GAP: f32 = 4.0;

/// Button `n`'s automation id: `quicktag:<n>` (1 … 10).
pub fn button_id(n: usize) -> String {
    format!("quicktag:{n}")
}

/// What each button applies (`None`: "+"), as the engine's `tags.quick.get` → `shown` gives it;
/// all `None` before the library is open.
pub fn shown(app: &App) -> [Option<String>; QUICK_TAGS] {
    app.session().map(quick_tags_shown).unwrap_or_default()
}

/// The ten buttons' rects in a grid `width` wide whose top left is `min`: rows of 2/3/2/3, each
/// button as wide as its row allows, `height` tall, the first row set apart.
pub fn grid(min: egui::Pos2, width: f32, height: f32) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(QUICK_TAGS);
    let mut y = min.y;
    for (row, &n) in ROWS.iter().enumerate() {
        let gaps = GAP * n.saturating_sub(1) as f32;
        let w = ((width - gaps) / n as f32).max(1.0);
        for i in 0..n {
            let x = min.x + i as f32 * (w + GAP);
            rects.push(Rect::from_min_max(pos2(x.round(), y), pos2((x + w).round(), y + height)));
        }
        y += height + GAP + if row == 0 { RECENT_GAP } else { 0.0 };
    }
    rects
}

/// The grid's height for buttons `height` tall.
pub fn grid_height(height: f32) -> f32 {
    ROWS.len() as f32 * height + (ROWS.len() - 1) as f32 * GAP + RECENT_GAP
}

/// The tags every selected photo holds (folded), from the list above (its cache, so no second
/// query): the buttons that look pressed.
fn held_by_all(app: &mut App, ctx: &egui::Context) -> Vec<String> {
    super::list::rows(app, ctx).iter().filter(|r| !r.partial()).map(|r| fold(&r.tag)).collect()
}

/// The tooltip of button `n` showing `tag`: what a click does, with the shortcut in effect.
fn tooltip(app: &App, n: usize, tag: Option<&str>) -> String {
    let key = app.session().and_then(|s| s.keymap().keys_for(&format!("tags.quick.{n}"), Context::Library).into_iter().next()).map(|k| {
        let k = k.to_string();
        if Platform::current() == Platform::Mac { k.replace("Ctrl+", "Cmd+") } else { k }
    });
    match (tag, key) {
        (Some(tag), Some(key)) => tl!("Add or remove “{tag}” on the selected photos ({key})", tag = tag, key = key),
        (Some(tag), None) => tl!("Add or remove “{tag}” on the selected photos", tag = tag),
        (None, _) => tl!("Quick tag {n} has no tag: click to choose quick tags", n = n),
    }
}

/// One button in `rect`: the plain button look (`button` / `hover` / `pressed`, a `button_border`
/// outline), pressed while `on`, its label cut to fit.
#[allow(clippy::too_many_arguments)] // one button's facts
fn button(ui: &mut Ui, t: &Tokens, rect: Rect, id: &str, label: &str, on: bool, enabled: bool, tip: &str) -> Response {
    let enabled = enabled && ui.is_enabled();
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let fill = if enabled && (on || r.is_pointer_button_down_on()) {
        t.pressed
    } else if enabled && r.hovered() {
        t.hover
    } else {
        t.button
    };
    let p = ui.painter();
    p.rect(rect, CornerRadius::same(t.radius), fill, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if !enabled {
        t.text_disabled
    } else if label == UNASSIGNED {
        t.text_dim
    } else {
        t.text
    };
    let mut job = LayoutJob::simple_singleline(label.to_owned(), fonts::semibold(t.button_label), color);
    job.wrap = TextWrapping::truncate_at_width((rect.width() - 8.0).max(1.0));
    let galley = p.layout_job(job);
    p.galley((rect.center() - galley.size() / 2.0).round(), galley, color);
    widgets::register(ui.ctx(), id, Role::Button, label, Target::Widget(&r), on);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, enabled, on, label));
    r.on_hover_text(tip)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let gear = ui
        .horizontal(|ui| {
            super::caption(app, ui, &tl!("Quick tags"));
            ui.add_space((ui.available_width() - GEAR).max(0.0));
            widgets::icon_button(ui, &t, CONFIGURE_ID, Icon::More, GEAR, false, true, &tl!("Configure Quick Tags"))
        })
        .inner;
    if gear.clicked() {
        // A refusal (the dialog is already open) is on the status line.
        let _ = app.run("quicktags.configure", &json!({}));
    }
    let tags = shown(app);
    let held = held_by_all(app, &ctx);
    let selected = !app.state().selection.ids.is_empty();
    // The design system's button height; the list above leaves room for the four rows
    // (`list.rs` → `QUICK_ROOM`).
    let height = t.metrics.button_h;
    let width = ui.available_width();
    let (_, area) = ui.allocate_space(vec2(width, grid_height(height)));
    let mut clicked: Option<usize> = None;
    for (i, rect) in grid(area.min, width, height).into_iter().enumerate() {
        let n = i + 1;
        let tag = tags.get(i).cloned().flatten();
        let on = tag.as_deref().is_some_and(|tag| held.contains(&fold(tag)));
        let label = tag.as_deref().unwrap_or(UNASSIGNED);
        let enabled = tag.is_none() || selected;
        let tip = tooltip(app, n, tag.as_deref());
        if button(ui, &t, rect, &button_id(n), label, on, enabled, &tip).clicked() {
            clicked = Some(i);
        }
    }
    let Some(i) = clicked else { return };
    // A refusal's reason is on the status line.
    let _ = match tags.get(i).cloned().flatten() {
        Some(tag) => {
            let ids = app.state().selection.ids.clone();
            app.run("photo.tags.toggle", &json!({ "ids": ids, "tag": tag }))
        }
        None => app.run("quicktags.configure", &json!({})),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_has_rows_of_two_three_two_three_filling_the_width() {
        let rects = grid(pos2(10.0, 20.0), 200.0, 24.0);
        assert_eq!(rects.len(), QUICK_TAGS);
        let rows: Vec<&[Rect]> = rects.chunk_by(|a, b| a.min.y == b.min.y).collect();
        assert_eq!(rows.iter().map(|r| r.len()).collect::<Vec<_>>(), ROWS);
        for row in &rows {
            // Each row reaches both edges; buttons of a row never overlap.
            assert_eq!(row.first().map(|r| r.min.x), Some(10.0));
            assert_eq!(row.last().map(|r| r.max.x), Some(210.0));
            assert!(row.windows(2).all(|w| w[0].max.x < w[1].min.x));
            assert!(row.iter().all(|r| r.height() == 24.0));
        }
        let bottom = rects.last().map_or(0.0, |r| r.max.y);
        assert_eq!(bottom - 20.0, grid_height(24.0));
        assert_eq!((button_id(1), button_id(10)), ("quicktag:1".to_string(), "quicktag:10".to_string()));
        // A pane too narrow for its buttons still lays out ten.
        assert_eq!(grid(pos2(0.0, 0.0), 0.0, 24.0).len(), QUICK_TAGS);
    }

    #[test]
    fn the_buttons_show_the_engines_quick_tags() {
        assert_eq!(shown(&App::new(None)), <[Option<String>; QUICK_TAGS]>::default(), "no library yet");
        let (_dir, s, _ids) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        app.run("tags.quick.set", &json!({ "slots": ["a", "b", "beach"], "useRecent": false })).unwrap();
        let tags = shown(&app);
        assert_eq!((tags[0].as_deref(), tags[2].as_deref(), tags[3].as_deref()), (Some("a"), Some("beach"), None));
        assert!(tooltip(&app, 3, Some("beach")).contains("“beach”") && tooltip(&app, 3, Some("beach")).contains("+3)"));
        assert!(tooltip(&app, 4, None).starts_with("Quick tag 4 has no tag"));
    }
}
