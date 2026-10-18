//! An effect's parameter panel (`editing.md` §8.1): in place of the effect tab's grid while
//! `edit.tool` is `effect`, the panel of [`crate::state::EditState::effect`], built from the
//! effect's catalogue row ([`controls`]):
//!
//! ```text
//! Tint                                   (title)
//!               Fade                     (sliders: label centred above, ≤ 4)
//! ○─────────────────────────────
//! [■] Lighten                            (checkboxes)
//! Blend mode               Normal ⌄      (choices)
//! [█] Tint color                         (swatches: a click opens the picker popup)
//! [Reset]                [Apply] [Cancel]
//! ```
//!
//! - The sliders are ui-kit's effect sliders (`slider:effect.<param>`; Glow's Radius on a
//!   logarithmic scale; a double-click resets one to its default), the checkboxes
//!   `check:effect.<param>`, the choices dropdowns (`choice:effect.<param>`, items
//!   `choice:effect.<param>/<word>`), the swatches `color:effect.<param>` with their picker popup
//!   (`color:effect.<param>/popup`) and its eyedropper (`color:effect.<param>/eyedropper`), and
//!   Fade the last slider where the effect has it. The buttons are `button:panel.reset`,
//!   `button:panel.apply` and `button:panel.cancel`.
//! - Every change goes to the draft and from it to the preview's candidate
//!   ([`effect_session::sync_preview`]); nothing reaches the recipe before Apply.
//! - The open picker is the draft's `picker` (one at a time), the armed eyedropper its
//!   `eyedropper`: while it is armed the pointer is a crosshair over the photo and a click there
//!   picks the colour under it ([`effect_session::start_pick`], answered off the UI thread).
//! - A panel too short for its rows (left open on another tab of a small window) scrolls them.
//! - An effect may add rows of its own under its controls ([`extra_rows`]): Soften's Paint │
//!   Erase and Brush size ([`super::soften_brush::rows`]).

pub mod controls;
pub mod layout;

use dalica_core::color::{hex_u8, parse_hex_u8};
use dalica_ui_kit::widgets::{self, ToastKind};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Align2, CursorIcon, Id, Layout, Response, ScrollArea, Ui, UiBuilder, vec2};
use serde_json::{Map, Value, json};

use self::controls::{Control, ROW_GAP};
use super::{canvas, soften_brush};
use crate::app::App;
use crate::commands::edit_view;
use crate::commands::effect_session::{self, set_eyedropper, set_picker};
use crate::state::{EffectDraft, EffectValue, effect_row};

/// The panel's buttons.
pub const APPLY_ID: &str = "button:panel.apply";
pub const CANCEL_ID: &str = "button:panel.cancel";
pub const RESET_ID: &str = "button:panel.reset";

/// The rows effect `name` adds under its controls, drawn by its own module: their height and
/// what draws them in the controls' area (scrolling with them).
pub type ExtraRows = (fn(&Tokens) -> f32, fn(&mut App, &mut Ui));

/// The extra rows of effect `name`, if it has any.
pub fn extra_rows(name: &str) -> Option<ExtraRows> {
    (name == soften_brush::EFFECT).then_some((soften_brush::rows_height as fn(&Tokens) -> f32, soften_brush::rows as fn(&mut App, &mut Ui)))
}

/// What a control asked for this pass, applied once every control is drawn.
#[derive(Debug, Clone, PartialEq)]
enum Change {
    Set(String, Value),
    /// The picker of this parameter opened (any other closes).
    OpenPicker(String),
    /// The picker of this parameter closed (if it is the open one).
    ClosePicker(String),
    /// The eyedropper of this parameter armed, or disarmed when it was.
    ToggleEyedropper(String),
}

/// Draws the open effect's panel in `ui`'s rect; nothing when no effect is open.
pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    effect_session::poll(app, &ctx);
    let Some(d) = app.state().edit.effect.clone() else { return };
    let Some(row) = effect_row(&d.name) else { return };
    let t: Tokens = *app.tokens();
    let list = controls::controls(row);
    let extra = extra_rows(row.name);
    let need = controls::rows_height(&t, &list) + extra.map_or(0.0, |(height, _)| ROW_GAP + height(&t));
    let l = layout::compute(ui.max_rect(), t.metrics.button_h, need);
    ui.painter().text(l.title.left_center(), Align2::LEFT_CENTER, row.label, fonts::semibold(t.title), t.text);

    let mut changes = Vec::new();
    if l.controls.height() > 0.0 {
        let mut area = ui.new_child(UiBuilder::new().id_salt("effect.controls").max_rect(l.controls).layout(Layout::top_down(Align::Min)));
        area.set_clip_rect(l.controls.intersect(ui.clip_rect()));
        ScrollArea::vertical().id_salt(("effect.controls", row.name)).auto_shrink([false, false]).show(&mut area, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            for (i, c) in list.iter().enumerate() {
                if i > 0 {
                    ui.add_space(ROW_GAP);
                }
                control(ui, &t, &d, c, &mut changes);
            }
            if let Some((_, draw)) = extra {
                ui.add_space(ROW_GAP);
                draw(app, ui);
            }
        });
    }

    let mut clicked = None;
    let mut buttons = ui.new_child(UiBuilder::new().id_salt("effect.buttons").max_rect(l.buttons).layout(Layout::right_to_left(Align::Center)));
    if widgets::text_button(&mut buttons, &t, CANCEL_ID, "Cancel", false).clicked() {
        clicked = Some(("edit.tool.cancel", json!({})));
    }
    if widgets::primary_button(&mut buttons, &t, APPLY_ID, "Apply").clicked() {
        clicked = Some(("edit.tool.apply", json!({})));
    }
    let mut left = ui.new_child(UiBuilder::new().id_salt("effect.reset").max_rect(l.buttons).layout(Layout::left_to_right(Align::Center)));
    if widgets::text_button(&mut left, &t, RESET_ID, "Reset", false).clicked() {
        clicked = Some(("edit.tool.action", json!({ "name": "reset" })));
    }

    apply_changes(app, changes);
    effect_session::sync_preview(app);
    pick_on_photo(app, &ctx);
    if let Some((id, params)) = clicked
        && let Err(e) = app.run(id, &params)
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}

/// Draws control `c` of draft `d` on its row, noting what it changed.
fn control(ui: &mut Ui, t: &Tokens, d: &EffectDraft, c: &Control, changes: &mut Vec<Change>) {
    let id = c.id();
    let p = c.param();
    let value = effect_session::value(d, p.name);
    match *c {
        Control::Slider(p, min, max) => {
            let spec = controls::slider_spec(&id, p, min, max);
            let v = match value {
                Some(EffectValue::Number(n)) => n,
                _ => spec.default,
            };
            let out = widgets::effect_slider(ui, t, &spec, v, controls::scale_of(p));
            if let Some(v) = out.value {
                changes.push(Change::Set(p.name.into(), json!(spec.snap(v))));
            }
        }
        Control::Check(p) => {
            let mut on = matches!(value, Some(EffectValue::Flag(true)));
            row(ui, c.height(t), |ui| {
                if widgets::checkbox(ui, t, &id, p.label, &mut on).changed() {
                    changes.push(Change::Set(p.name.into(), json!(on)));
                }
            });
        }
        Control::Choice(p, words) => {
            let now = match value {
                Some(EffectValue::Text(w)) => w,
                _ => words.first().map_or_else(String::new, |w| (*w).to_owned()),
            };
            let r = row(ui, c.height(t), |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::dropdown(ui, t, &id, &controls::choice_label(&now), |ui| {
                        for w in words {
                            if widgets::dropdown_item(ui, t, &format!("{id}/{w}"), &controls::choice_label(w), "", *w == now).clicked() {
                                changes.push(Change::Set(p.name.into(), json!(w)));
                            }
                        }
                    });
                });
            });
            let color = if ui.is_enabled() { t.text_label } else { t.text_disabled };
            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, p.label, fonts::regular(t.body), color);
        }
        Control::Swatch(p) => {
            let rgb = match &value {
                Some(EffectValue::Text(c)) => parse_hex_u8(c).unwrap_or([0, 0, 0]),
                _ => [0, 0, 0],
            };
            let want = d.picker.as_deref() == Some(p.name);
            if want != widgets::swatch_open(ui.ctx(), &id) {
                if want {
                    widgets::open_swatch(ui.ctx(), &id);
                } else {
                    widgets::close_swatch(ui.ctx(), &id);
                }
            }
            let armed = d.eyedropper.as_deref() == Some(p.name);
            let mut out = None;
            let r = row(ui, c.height(t), |ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    out = Some(widgets::swatch(ui, t, &id, p.label, rgb, armed));
                });
            });
            let Some(out) = out else { return };
            let x = out.response.rect.max.x + 8.0;
            let color = if ui.is_enabled() { t.text_label } else { t.text_disabled };
            ui.painter().text(egui::pos2(x, r.center().y), Align2::LEFT_CENTER, p.label, fonts::regular(t.body), color);
            if let Some(c) = out.color {
                changes.push(Change::Set(p.name.into(), json!(hex_u8(c))));
            }
            if out.open && !want {
                changes.push(Change::OpenPicker(p.name.into()));
            } else if !out.open && want {
                changes.push(Change::ClosePicker(p.name.into()));
            }
            if out.eyedropper {
                changes.push(Change::ToggleEyedropper(p.name.into()));
            }
        }
    }
}

/// A row `h` high across the controls, holding what `add` draws; its rect.
fn row(ui: &mut Ui, h: f32, add: impl FnOnce(&mut Ui)) -> egui::Rect {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width().max(1.0), h));
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
    add(&mut child);
    rect
}

/// Applies this pass's changes to the draft: the values in one `set` (refused values change
/// nothing, and say why), then the picker and the eyedropper.
fn apply_changes(app: &mut App, changes: Vec<Change>) {
    let mut values = Map::new();
    for c in &changes {
        if let Change::Set(name, v) = c {
            values.insert(name.clone(), v.clone());
        }
    }
    if !values.is_empty()
        && let Err(e) = effect_session::set_values(app, &values)
    {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
    for c in changes {
        let d = app.state().edit.effect.clone();
        let Some(d) = d else { return };
        match c {
            Change::Set(..) => {}
            Change::OpenPicker(name) => set_picker(app, Some(&name)),
            Change::ClosePicker(name) => {
                if d.picker.as_deref() == Some(name.as_str()) {
                    set_picker(app, None);
                }
            }
            Change::ToggleEyedropper(name) => {
                let armed = d.eyedropper.as_deref() == Some(name.as_str());
                set_eyedropper(app, if armed { None } else { Some(&name) });
            }
        }
    }
}

/// While the eyedropper is armed: a crosshair over the photo, and a click there (the photo area's
/// click of the last pass; the picker popup takes its own clicks) picks the colour under it.
fn pick_on_photo(app: &mut App, ctx: &egui::Context) {
    let Some(param) = app.state().edit.effect.as_ref().and_then(|d| d.eyedropper.clone()) else { return };
    let Some(photo) = edit_view::open_photo(app) else { return };
    let Some(view) = canvas::view(ctx).filter(|v| v.photo == photo) else { return };
    let visible = view.image.intersect(view.area);
    let response: Option<Response> = ctx.read_response(Id::new(canvas::ID));
    if response.as_ref().and_then(Response::hover_pos).is_some_and(|p| visible.contains(p)) {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
    }
    let Some(at) = response.filter(Response::clicked).and_then(|r| r.interact_pointer_pos()).filter(|p| visible.contains(*p)) else { return };
    let (w, h) = (view.image.width(), view.image.height());
    if !(w > 0.0 && h > 0.0) {
        return;
    }
    let shown = [((at.x - view.image.min.x) / w).clamp(0.0, 1.0), ((at.y - view.image.min.y) / h).clamp(0.0, 1.0)];
    let Some([x, y]) = effect_session::stored_point(app, photo, shown) else { return };
    if let Err(e) = effect_session::start_pick(app, Some(&param), x, y) {
        app.toast(&e.to_string(), ToastKind::Caution);
    }
}
