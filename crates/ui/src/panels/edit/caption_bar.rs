//! The caption bar under the photo (`editing.md` §2): 36 high on `canvas`, the show-captions toggle
//! at the left (`button:caption.toggle`, for the slideshow: `slideshow.captions`, enabled as that
//! command is, M11.21; drawn selected, as ui-kit's icon button is when active, while captions are
//! on, and registered active then), the photo's
//! caption in a centred field with the placeholder "Add a caption" (`field:caption`), and the trash
//! button at the right (`button:edit.trash`).
//!
//! The trash button deletes the photo shown as Ctrl+Delete does in the library: `photo.delete` for
//! it, through the delete question (`organizing.md` §9; skipped with "Delete from disk without
//! asking"). Cancel changes nothing. Once Move to Trash has taken the photo out of the library the
//! edit view shows the next photo of its section, else the previous one, else goes back to the
//! library ([`edit_view::successors`], listed when the button is clicked and kept in the context's
//! data until then; [`edit_view::left_behind`]).
//!
//! The field shows the open photo's caption. What is typed is kept while the field has the focus;
//! Return or a click away sets it through `photo.caption.set` (one library undo step, nothing when
//! the text is unchanged); Esc gives back the caption as it was. Moving to another photo commits
//! what was typed for the one it was typed for. The draft is kept in the view state
//! (`UiState::caption_bar`), so closing the app commits it too ([`crate::App::shutdown`]).

use std::sync::Arc;

use dalica_core::PhotoId;
use dalica_ui_kit::Tokens;
use dalica_ui_kit::icons::{EditIcon, paint_edit};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, TextField, focus_ring};
use egui::{CornerRadius, Id, Rect, Response, Sense, Ui, UiBuilder, Vec2, WidgetType, pos2, vec2};
use serde_json::json;

use super::top_bar;
use crate::app::App;
use crate::commands::edit_view;
use crate::state::{CaptionEdit, MAX_DRAFT};
use crate::tl;

pub const FIELD_ID: &str = "field:caption";
pub const PLACEHOLDER: &str = "Add a caption";
pub const TOGGLE_ID: &str = "button:caption.toggle";
pub const TOGGLE_TIP: &str = "Show captions in the slideshow";
pub const TRASH_ID: &str = "button:edit.trash";
pub const TRASH_TIP: &str = "Delete this photo";

/// The field's placeholder or a button's tooltip in the language in effect (the constants keep the
/// English, which the catalogs key on).
pub fn say(english: &str) -> String {
    match english {
        "Add a caption" => tl!("Add a caption"),
        "Show captions in the slideshow" => tl!("Show captions in the slideshow"),
        "Delete this photo" => tl!("Delete this photo"),
        other => crate::i18n::tr(other),
    }
}

/// Where the field's focus last frame is kept in the context's data, for [`editing`].
const EDITING: &str = "caption:editing";
/// Where the photo the trash button asked about is kept in the context's data, with where the
/// view goes once it is gone.
const TRASHED: &str = "caption:trashed";

/// The photo the trash button asked about, and its [`edit_view::successors`].
#[derive(Clone)]
struct Trashed {
    photo: PhotoId,
    then: Arc<[PhotoId]>,
}

/// The field's widest, and the room kept at each end for the buttons (margin, button, gap).
pub const FIELD_MAX_W: f32 = 360.0;
pub const BUTTON: f32 = 28.0;
pub const MARGIN: f32 = 8.0;

/// Draws the caption bar into `ui`'s rect (the `captionBar` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    let trashed = ui.ctx().data(|d| d.get_temp::<Trashed>(Id::new(TRASHED)));
    if let Some(gone) = trashed.filter(|g| g.photo == photo)
        && edit_view::left_behind(app, gone.photo, &gone.then)
    {
        ui.ctx().data_mut(|d| d.remove::<Trashed>(Id::new(TRASHED)));
        ui.ctx().request_repaint();
        return;
    }
    let t = *app.tokens();
    let bar = ui.max_rect();
    let y = bar.center().y;
    let button = |x: f32| Rect::from_center_size(pos2(x, y), vec2(BUTTON, BUTTON));
    // The slideshow's captions setting, live as `slideshow.captions` is, selected while it is on.
    let live = crate::commands::enabled(app, "slideshow.captions", &json!({})).is_ok();
    let on = app.session().is_some_and(|s| s.settings().slideshow().captions);
    if toggle(ui, &t, button(bar.min.x + MARGIN + BUTTON / 2.0), on, live).clicked() {
        let _ = app.run("slideshow.captions", &json!({}));
    }
    if top_bar::glyph_button(ui, &t, TRASH_ID, EditIcon::Trash, button(bar.max.x - MARGIN - BUTTON / 2.0), true, &say(TRASH_TIP)).clicked() {
        let then: Arc<[PhotoId]> = edit_view::successors(app, photo).into();
        ui.ctx().data_mut(|d| d.insert_temp(Id::new(TRASHED), Trashed { photo, then }));
        // The delete question for the photo shown; a refusal's reason goes to the status line.
        let _ = app.run("photo.delete", &json!({ "ids": [photo.get()] }));
    }

    let room = bar.width() - 2.0 * (MARGIN + BUTTON + MARGIN);
    let width = room.min(FIELD_MAX_W);
    if width.is_nan() || width < 24.0 {
        return;
    }
    // The draft lives in the view state, where closing the app finds it (`App::shutdown`).
    let draft = app.state_mut().caption_bar.take();
    let had_draft = draft.as_ref().is_some_and(|d| d.photo == photo);
    let mut text = match draft {
        // Typed for another photo (a step while the field had the focus): that photo keeps it.
        Some(d) if d.photo != photo => {
            commit(app, d.photo, &d.draft);
            caption(app, photo)
        }
        Some(d) => d.draft,
        None => caption(app, photo),
    };
    let h = t.metrics.text_field_h;
    let field = Rect::from_min_size(pos2((bar.center().x - width / 2.0).round(), (y - h / 2.0).round()), vec2(width.round(), h));
    let mut child = ui.new_child(UiBuilder::new().id_salt("captionBar").max_rect(field));
    let out = TextField::new(FIELD_ID, &say(PLACEHOLDER), field.width()).show(&mut child, &t, &mut text);
    // At most `MAX_DRAFT` characters, as the view state keeps it (the light table's editor too).
    if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
        text.truncate(at);
    }
    let editing = out.editing && out.ending.is_none();
    // Return or a click away commits; so does a draft whose field no longer has the focus with no
    // ending seen (the view left and come back to, or a draft handed over by `ui.set`): what was
    // typed is kept, never dropped.
    if out.committed() || (had_draft && !editing && !out.cancelled()) {
        commit(app, photo, &text);
    }
    app.state_mut().caption_bar = editing.then_some(CaptionEdit { photo, draft: text });
    ui.ctx().data_mut(|d| d.insert_temp(Id::new(EDITING), editing));
}

/// The show-captions toggle over `rect`, as ui-kit's icon button draws one with an edit glyph: a
/// `tool_active` plate and a `pick` glyph while `on`; otherwise a `hover × 0.7` plate when hovered
/// and the glyph `icon` at rest, `text` hovered; `text_disabled` when not `live`. Registered as
/// [`TOGGLE_ID`] (role `iconButton`), active while `on`.
fn toggle(ui: &mut Ui, t: &Tokens, rect: Rect, on: bool, live: bool) -> Response {
    let c = top_bar::child(ui, rect, live);
    let r = c.interact(rect, Id::new(TOGGLE_ID), Sense::click());
    let p = c.painter();
    if on {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.tool_active);
    } else if live && r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    focus_ring(p, t, &r, rect, t.radius);
    let glyph = if !live {
        t.text_disabled
    } else if on {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.icon
    };
    let side = (rect.width().min(rect.height()) * 0.6).round();
    paint_edit(p, Rect::from_center_size(rect.center(), Vec2::splat(side)), EditIcon::CaptionToggle, glyph);
    let tip = say(TOGGLE_TIP);
    widgets::register(c.ctx(), TOGGLE_ID, Role::IconButton, &tip, Target::Widget(&r), on);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Button, live, on, &tip));
    if live { r.on_hover_text(tip) } else { r.on_disabled_hover_text(tip) }
}

/// Whether the caption field had the keyboard last frame. egui drops the focus as soon as Esc is
/// pressed, before the frame's shortcuts run, so the edit view's dispatch asks this to leave an Esc
/// that ends a caption edit to the field.
pub fn editing(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(Id::new(EDITING)).unwrap_or(false))
}

/// Photo `id`'s caption ("" when the library does not have it).
fn caption(app: &App, id: PhotoId) -> String {
    app.session().map(|s| s.library().caption(id).to_owned()).unwrap_or_default()
}

/// Sets `photo`'s caption to what was typed, unless it is that already: as Return or a click away
/// does, and as closing the app does with a draft still open ([`crate::App::shutdown`]).
pub fn commit(app: &mut App, photo: PhotoId, text: &str) {
    let text = text.trim();
    if caption(app, photo) != text {
        let _ = app.run("photo.caption.set", &json!({ "ids": [photo.get()], "caption": text }));
    }
}
