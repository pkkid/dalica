//! The People pane (`organizing.md` §6.4), in the side pane. Its body is `pane:people` and the
//! region `region:people` (`ui.screenshot {region: "people"}`). It follows the photo in the edit
//! view, else the selection ([`model`]), top to bottom:
//!
//! - **"People in these photos:"**: a row per person on them (`person:<PersonId>`, active while
//!   selected): their face, their name and how many of the photos hold them ("3 photos"). A click
//!   selects the person, Ctrl-click adds or drops one (`people.filter`): the light table then shows
//!   only the photos holding all the people selected, under the banner "Showing photos with Ana
//!   and Rui" with [Show All] (`commands/people_pane.rs`). "None" when nobody is named there.
//! - **"Unnamed faces:"**: a row per face nobody is named on (ignored ones left out): its tile
//!   (`face:<PhotoId>.<n>`) with × at its top-right (`face:<PhotoId>.<n>/ignore`: ignores it) and
//!   "Add a name" (`field:people.name/<PhotoId>.<n>`, autocomplete over the people): Enter names
//!   the face, one undo step, and a name nobody has opens New Person. While the Unnamed grid shows
//!   in the library, its faces are named there (the pane says so instead of listing them twice).
//! - Suggestions as "Name?" with ✓ / × ([`suggestions`], M13.30).
//! - In the edit view, **Add a Person Manually** at the bottom (`button:faces.addManually`, lit
//!   while the box is drawn): the box on the photo (`panels/edit/faces.rs`).
//!
//! **Hovering** a face or a person's row sets `peoplePane.hovered`: its box on the photo in the
//! edit view is highlighted, and hovering a box there highlights its row here ([`track_hover`]).
//! The faces' pictures are the engine's face tiles ([`pictures`]).

pub mod model;
pub mod pictures;
pub mod suggestions;

use std::sync::Arc;

use dalica_core::PersonId;
use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{PeopleIcon, paint_people};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Ending, FaceTile, Target, face_tile_size};
use egui::{Align, Align2, CornerRadius, Id, Layout, Rect, RichText, Sense, Ui, UiBuilder, Vec2, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::commands::people_pane as commands;
use crate::state::{FaceRef, MAX_DRAFT, View};

/// The pane's two parts.
pub const IN_PHOTOS: &str = "People in these photos";
pub const UNNAMED: &str = "Unnamed faces";
/// What a part says when it has nothing.
pub const NONE: &str = "None";
/// What "Unnamed faces" says while the Unnamed grid shows.
pub const IN_GRID: &str = "Name them in the Unnamed view.";
/// The region's name and Add a Person Manually's button.
pub const REGION_ID: &str = "region:people";
pub const ADD_MANUALLY_ID: &str = "button:faces.addManually";
pub const ADD_MANUALLY: &str = "Add a Person Manually";
/// "Add a name"'s hint.
pub const NAME_HINT: &str = "Add a name";
/// A person's row and its face (points).
const PERSON_ROW: f32 = 32.0;
const PERSON_FACE: f32 = 24.0;
/// An unnamed face's picture (points).
const UNNAMED_FACE: f32 = 48.0;
/// Between the parts (points).
const GAP: f32 = 16.0;

/// A person's row: `person:<PersonId>`.
pub fn person_id(id: &PersonId) -> String {
    format!("person:{id}")
}

/// An unnamed face's "Add a name": `field:people.name/<PhotoId>.<n>`.
pub fn name_field_id(face: FaceRef) -> String {
    format!("field:people.name/{face}")
}

/// "1 photo", "3 photos".
pub fn photos_label(n: usize) -> String {
    format!("{n} {}", if n == 1 { "photo" } else { "photos" })
}

/// Where the hovered face was hovered: the pane or a box on the photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverFrom {
    Pane,
    Canvas,
}

/// Sets `peoplePane.hovered` from `from`'s pointer: the face under it, or, when the pointer left
/// what `from` hovered, none. A face hovered elsewhere (or set by `ui.set`) is left alone.
pub fn track_hover(app: &mut App, ctx: &egui::Context, from: HoverFrom, over: Option<FaceRef>) {
    let key = Id::new("peoplePane.hoverFrom");
    let owner = ctx.data(|d| d.get_temp::<HoverFrom>(key));
    match over {
        Some(face) => {
            if app.state().people_pane.hovered != Some(face) {
                app.state_mut().people_pane.hovered = Some(face);
            }
            ctx.data_mut(|d| d.insert_temp(key, from));
        }
        None if owner == Some(from) => {
            app.state_mut().people_pane.hovered = None;
            ctx.data_mut(|d| d.remove::<HoverFrom>(key));
        }
        None => {}
    }
}

/// Every person's name, by name ignoring case (the autocomplete of "Add a name"), read again only
/// when the library changed.
pub fn people_names(app: &App, ctx: &egui::Context) -> Arc<Vec<String>> {
    let Some(session) = app.session() else { return Arc::default() };
    let revision = session.library().revision();
    let key = Id::new("peoplePane.names");
    if let Some((r, names)) = ctx.data(|d| d.get_temp::<(u64, Arc<Vec<String>>)>(key))
        && r == revision
    {
        return names;
    }
    let mut names: Vec<String> = session.library().people().map(|(_, p)| p.name.clone()).collect();
    names.sort_by_key(|n| n.to_lowercase());
    let names = Arc::new(names);
    ctx.data_mut(|d| d.insert_temp(key, (revision, Arc::clone(&names))));
    names
}

/// What a frame of the pane asks for once it is drawn.
#[derive(Default)]
struct Acts {
    over: Option<FaceRef>,
    select: Option<Vec<PersonId>>,
    name: Option<(FaceRef, String)>,
    ignore: Option<FaceRef>,
    manual: bool,
}

fn caption(app: &App, ui: &mut Ui, text: &str) {
    let t = app.tokens();
    ui.label(RichText::new(format!("{text}:")).font(fonts::regular(t.body)).color(t.text_label));
    ui.add_space(4.0);
}

fn dim(app: &App, ui: &mut Ui, text: &str) {
    let t = app.tokens();
    ui.label(RichText::new(text).font(fonts::regular(t.body)).color(t.text_dim));
}

/// Draws the pane's body into `ui`'s rect.
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let body = ui.max_rect();
    widgets::register(&ctx, REGION_ID, Role::Other, "People", Target::Painted { rect: body, clip: ui.clip_rect() }, false);
    pictures::begin(app, &ctx);
    let model = model::model(app, &ctx);
    let edit = app.state().view == View::Edit;
    let rect = body.shrink(8.0);
    let bottom = if edit { t.metrics.button_h + 8.0 } else { 0.0 };
    let list = Rect::from_min_max(rect.min, pos2(rect.max.x, (rect.max.y - bottom).max(rect.min.y)));
    let mut acts = Acts::default();
    let mut col = ui.new_child(UiBuilder::new().id_salt("people.pane.list").max_rect(list).layout(Layout::top_down(Align::Min)));
    // The wheel scrolls the parts; the bar only shows where they are (a bar that takes drags would
    // be a control without an automation id).
    egui::ScrollArea::vertical()
        .id_salt("people.pane")
        .max_height(list.height())
        .auto_shrink([false, true])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(&mut col, |ui| {
            caption(app, ui, IN_PHOTOS);
            if model.people.is_empty() {
                dim(app, ui, NONE);
            }
            for p in &model.people {
                person_row(app, ui, p, &mut acts);
            }
            ui.add_space(GAP);
            caption(app, ui, UNNAMED);
            if !edit && app.state().unnamed.open && !model.unnamed.is_empty() {
                dim(app, ui, IN_GRID);
            } else {
                if model.unnamed.is_empty() {
                    dim(app, ui, NONE);
                }
                let names = people_names(app, &ctx);
                for u in &model.unnamed {
                    unnamed_row(app, ui, u, &names, &mut acts);
                }
                if model.more > 0 {
                    dim(app, ui, &format!("and {} more", model.more));
                }
            }
            suggestions::show(app, ui);
        });
    if edit {
        let row = Rect::from_min_max(pos2(rect.min.x, rect.max.y - t.metrics.button_h), rect.max);
        let mut b = ui.new_child(UiBuilder::new().id_salt("people.pane.manual").max_rect(row).layout(Layout::left_to_right(Align::Center)));
        let on = app.state().people_pane.manual.is_some();
        acts.manual = widgets::text_button(&mut b, &t, ADD_MANUALLY_ID, ADD_MANUALLY, on).clicked();
    }
    pictures::finish(app, &ctx);
    track_hover(app, &ctx, HoverFrom::Pane, acts.over);
    act(app, &ctx, acts);
}

/// Runs what the frame asked for; a refusal's reason is on the status line.
fn act(app: &mut App, ctx: &egui::Context, acts: Acts) {
    if let Some(people) = acts.select {
        let _ = app.run("people.filter", &json!({ "people": people }));
    }
    if let Some((face, name)) = acts.name
        && commands::name_face(app, face, &name).is_ok()
    {
        drafts(ctx, |d| {
            d.remove(&face);
        });
    }
    if let Some(face) = acts.ignore {
        let _ = commands::ignore_face(app, face);
    }
    if acts.manual {
        let _ = app.run("faces.addManually", &json!({}));
    }
}

/// The names typed in the unnamed faces' fields, kept while the window runs.
fn drafts<R>(ctx: &egui::Context, f: impl FnOnce(&mut std::collections::HashMap<FaceRef, String>) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<std::collections::HashMap<FaceRef, String>>(Id::new("peoplePane.drafts"))))
}

/// A face's picture `side` points wide at `at`: its tile when it has arrived, else the placeholder.
fn paint_face(ui: &Ui, app: &App, at: Rect, face: FaceRef, rect: dalica_core::Rect01) {
    let t = app.tokens();
    let p = ui.painter();
    match pictures::picture(ui.ctx(), face, rect, pictures::side_px(ui.ctx(), at.width())) {
        Some(pic) => {
            p.image(pic.texture, at, pic.uv, egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(at, 0, t.placeholder);
            paint_people(p, Rect::from_center_size(at.center(), Vec2::splat((at.width() * 0.6).round())), PeopleIcon::Person, t.text_disabled);
        }
    }
}

fn person_row(app: &App, ui: &mut Ui, p: &model::Person, acts: &mut Acts) {
    let t = app.tokens();
    let width = ui.available_width();
    let (rect, r) = ui.allocate_exact_size(vec2(width, PERSON_ROW), Sense::click());
    let selected = app.state().people_pane.selected.contains(&p.id);
    let lit = r.hovered() || app.state().people_pane.hovered == Some(p.face);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(t.radius), t.cell_selected);
    } else if lit {
        painter.rect_filled(rect, CornerRadius::same(t.radius), t.hover);
    }
    let face = Rect::from_min_size(pos2(rect.min.x + 4.0, rect.center().y - PERSON_FACE / 2.0), Vec2::splat(PERSON_FACE));
    paint_face(ui, app, face, p.face, p.rect);
    let count = photos_label(p.photos);
    let count_g = painter.layout_no_wrap(count.clone(), fonts::regular(t.count), t.text_dim);
    let count_at = pos2(rect.max.x - 6.0 - count_g.size().x, rect.center().y - count_g.size().y / 2.0);
    let name_rect = Rect::from_min_max(pos2(face.max.x + 8.0, rect.min.y), pos2(count_at.x - 6.0, rect.max.y));
    if name_rect.width() > 0.0 {
        let mut job = egui::text::LayoutJob::simple_singleline(p.name.clone(), fonts::regular(t.body), t.text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(name_rect.width().max(1.0));
        let g = painter.layout_job(job);
        let at = Align2::LEFT_CENTER.align_size_within_rect(g.size(), name_rect).min.round();
        painter.galley(at, g, t.text);
    }
    painter.galley(count_at.round(), count_g, t.text_dim);
    let label = format!("{}, {count}", p.name);
    widgets::register(ui.ctx(), &person_id(&p.id), Role::Other, &label, Target::Widget(&r), selected);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, &label));
    if r.hovered() {
        acts.over = Some(p.face);
    }
    if r.clicked() {
        let mut people = app.state().people_pane.selected.clone();
        let add = ui.input(|i| i.modifiers.command);
        if add {
            match people.iter().position(|x| *x == p.id) {
                Some(at) => {
                    people.remove(at);
                }
                None => people.push(p.id.clone()),
            }
        } else if people == [p.id.clone()] {
            people.clear();
        } else {
            people = vec![p.id.clone()];
        }
        acts.select = Some(people);
    }
}

fn unnamed_row(app: &App, ui: &mut Ui, u: &model::Unnamed, names: &[String], acts: &mut Acts) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let size = face_tile_size(UNNAMED_FACE, false, false);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, size.y), Sense::hover());
    if !rect.intersects(ui.clip_rect()) {
        return;
    }
    if app.state().people_pane.hovered == Some(u.face) {
        ui.painter().rect_filled(rect, CornerRadius::same(t.radius), t.hover);
    }
    let id = u.face.to_string();
    let picture = pictures::picture(&ctx, u.face, u.rect, pictures::side_px(&ctx, UNNAMED_FACE));
    let tile = FaceTile { side: UNNAMED_FACE, picture, ignore: true, ..FaceTile::new(&id) };
    let mut left = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(rect.min, size)));
    let out = widgets::face_tile(&mut left, &t, &tile);
    if out.response.contains_pointer() {
        acts.over = Some(u.face);
    }
    if out.ignore {
        acts.ignore = Some(u.face);
    }
    let field_h = t.metrics.button_h;
    let field =
        Rect::from_min_max(pos2(rect.min.x + size.x + 6.0, rect.center().y - field_h / 2.0), pos2(rect.max.x - 2.0, rect.center().y + field_h / 2.0));
    if field.width() <= 0.0 {
        return;
    }
    let fid = name_field_id(u.face);
    let mut text = drafts(&ctx, |d| d.get(&u.face).cloned().unwrap_or_default());
    let mut right = ui.new_child(UiBuilder::new().max_rect(field));
    let out =
        widgets::autocomplete(&mut right, &t, &fid, &mut text, names, |ui, text| widgets::text_field(ui, &t, &fid, text, NAME_HINT, field.width()));
    if out.field.rect.contains(ctx.pointer_hover_pos().unwrap_or(pos2(f32::NAN, f32::NAN))) {
        acts.over = Some(u.face);
    }
    let text: String = text.chars().take(MAX_DRAFT).collect();
    let enter = out.field.ending == Some(Ending::Return) || out.picked.is_some();
    if enter && !text.trim().is_empty() {
        acts.name = Some((u.face, text.clone()));
    }
    drafts(&ctx, |d| {
        if text.is_empty() {
            d.remove(&u.face);
        } else {
            d.insert(u.face, text);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_counts_read_as_the_protocol_says() {
        let face = FaceRef { photo: dalica_core::PhotoId::new(12), index: 3 };
        assert_eq!(name_field_id(face), "field:people.name/12.3");
        assert_eq!(person_id(&PersonId::parse("p00a1").unwrap()), "person:p00a1");
        assert_eq!((photos_label(1), photos_label(3)), ("1 photo".to_string(), "3 photos".to_string()));
    }

    #[test]
    fn the_hover_follows_the_pointer_of_where_it_was_set() {
        let mut app = App::new(None);
        let ctx = egui::Context::default();
        let f = FaceRef { photo: dalica_core::PhotoId::new(1), index: 0 };
        track_hover(&mut app, &ctx, HoverFrom::Canvas, Some(f));
        assert_eq!(app.state().people_pane.hovered, Some(f));
        track_hover(&mut app, &ctx, HoverFrom::Pane, None);
        assert_eq!(app.state().people_pane.hovered, Some(f), "the pane did not set it");
        track_hover(&mut app, &ctx, HoverFrom::Canvas, None);
        assert_eq!(app.state().people_pane.hovered, None);
    }
}
