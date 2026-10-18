//! "Search for a place:" (`organizing.md` §7): Enter in the field `field:places.search` runs
//! `map.search` (typed coordinates found without the network, a name sent to the geocoding
//! service); this part takes the search's answer when it arrives and lists the places found in a
//! list over the bottom of the map, just above the caption, a click on one moving the pin there
//! (`map.showResult`).
//!
//! - Each pass the pane is drawn, a finished search (`Session::take_place_search`) fills the view
//!   state's `places.results` (at most 50, `crate::commands::map::results_of`); a failed one puts
//!   its reason on the status line, one that found nothing says "No places found.". While a search
//!   is on its way the pass polls again shortly.
//! - The list (`list:places.results`, labelled "3 places found") holds a row per place
//!   (`field:places.search/result/<n>`, labelled with the place), the first [`MAX_ROWS`] (fewer when
//!   the map is short; the geocoding service's first answers are its best); × at its top right
//!   (`button:places.results.close`) closes it. Picking a place closes it too.

use std::time::Duration;

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, list_row, list_row_height, list_text};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Align2, CornerRadius, Id, Layout, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::commands::map::results_of;

pub const LIST_ID: &str = "list:places.results";
pub const CLOSE_ID: &str = "button:places.results.close";
/// A result's row id.
pub fn result_id(n: usize) -> String {
    format!("field:places.search/result/{n}")
}
/// The status line when a search found nothing.
pub const NONE_FOUND: &str = "No places found.";
/// The most rows shown.
pub const MAX_ROWS: usize = 8;
/// How often a pass looks for the answer while a search is on its way.
const POLL: Duration = Duration::from_millis(50);
/// The list's padding, the close button's side, and the room kept above the caption (points).
const PAD: f32 = 4.0;
const CLOSE: f32 = 18.0;
const ABOVE: f32 = 4.0;

/// Takes the answer of the search on its way, if it arrived: its places into the view state.
pub fn poll(app: &mut App, ui: &Ui) {
    let Some(s) = app.session_mut() else { return };
    let pending = s.place_search_pending();
    let Some(done) = s.take_place_search() else {
        if pending {
            ui.ctx().request_repaint_after(POLL);
        }
        return;
    };
    match done.result {
        Ok(places) => {
            let found = results_of(&json!(places.iter().map(dalica_engine::geocode::Place::to_json).collect::<Vec<_>>()));
            let state = app.state_mut();
            if found.is_empty() {
                state.status = NONE_FOUND.into();
            }
            state.places.results = found;
        }
        Err(why) => app.state_mut().status = why,
    }
}

/// Takes a finished search and draws the list of places found over the map, its bottom just above
/// the field row `ui` lays out (the caption's height above it).
pub fn show(app: &mut App, ui: &mut Ui) {
    poll(app, ui);
    if app.state().places.results.is_empty() {
        return;
    }
    let t = *app.tokens();
    let field = ui.max_rect();
    let caption_h = 18.0;
    let names: Vec<String> = app.state().places.results.iter().map(|r| r.name.clone()).collect();
    let row_h = list_row_height(&t);
    let bottom = field.min.y - caption_h - ABOVE;
    // As many rows as the room above the caption holds (the pane's body is the clip).
    let room = ((bottom - ui.clip_rect().min.y - 2.0 * PAD - CLOSE - 40.0) / row_h).floor().max(1.0) as usize;
    let shown = names.len().min(MAX_ROWS).min(room);
    let h = 2.0 * PAD + CLOSE + row_h * shown as f32;
    let rect = Rect::from_min_max(pos2(field.min.x, bottom - h), pos2(field.max.x, bottom));
    if let Some(picked) = list(ui, &t, rect, &names[..shown], names.len()) {
        // A refusal's reason is on the status line.
        let _ = match picked {
            Some(n) => app.run("map.showResult", &json!({ "n": n })),
            None => {
                app.state_mut().places.results.clear();
                Ok(json!({}))
            }
        };
    }
}

/// "3 places found".
pub fn found(count: usize) -> String {
    if count == 1 { "1 place found".into() } else { format!("{count} places found") }
}

/// The list in `rect`: its frame, "N places found" (of `count`) and ×, a row for each of `names`.
/// Returns a click: `Some(n)` on row `n`, `None` on ×.
fn list(ui: &mut Ui, t: &Tokens, rect: Rect, names: &[String], count: usize) -> Option<Option<usize>> {
    let clip = ui.clip_rect();
    // The list takes the clicks on it, so none reaches the map under it.
    let r = ui.interact(rect, Id::new(LIST_ID), Sense::click());
    let p = ui.painter().with_clip_rect(clip);
    let radius = CornerRadius::same(6);
    p.add(t.popup_shadow.as_shape(rect, radius));
    p.rect(rect, radius, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
    let title = found(count);
    let head = Rect::from_min_size(rect.min + vec2(PAD, PAD), vec2(rect.width() - 2.0 * PAD, CLOSE));
    widgets::label_in(&p, head.shrink2(vec2(6.0, 0.0)), Align2::LEFT_CENTER, &title, fonts::regular(t.small), t.text_dim);
    let mut clicked = None;
    let close = Rect::from_min_size(pos2(head.max.x - CLOSE, head.min.y), vec2(CLOSE, CLOSE));
    let x = ui.interact(close, Id::new(CLOSE_ID), Sense::click());
    if x.hovered() {
        p.rect_filled(close, CornerRadius::same(t.radius), if x.is_pointer_button_down_on() { t.pressed } else { t.hover });
    }
    let arm = close.shrink(5.0);
    let stroke = Stroke::new(1.5, if x.hovered() { t.text } else { t.text_label });
    p.line_segment([arm.left_top(), arm.right_bottom()], stroke);
    p.line_segment([arm.right_top(), arm.left_bottom()], stroke);
    widgets::register(ui.ctx(), CLOSE_ID, Role::Button, "Close", Target::Widget(&x), false);
    if x.on_hover_text("Close the list").clicked() {
        clicked = Some(None);
    }
    let rows = Rect::from_min_max(pos2(rect.min.x + PAD, head.max.y), pos2(rect.max.x - PAD, rect.max.y - PAD));
    let mut child = ui.new_child(UiBuilder::new().max_rect(rows).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rows.intersect(clip));
    child.spacing_mut().item_spacing.y = 0.0;
    for (n, name) in names.iter().enumerate() {
        let row = list_row(&mut child, t, &result_id(n), name, &[f32::INFINITY], false, false, |ui, cell| {
            list_text(ui, t, cell, name, false);
        });
        if row.clicked() {
            clicked = Some(Some(n));
        }
    }
    widgets::register(ui.ctx(), LIST_ID, Role::Other, &title, Target::Widget(&r), false);
    clicked
}
