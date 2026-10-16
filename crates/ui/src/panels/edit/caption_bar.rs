//! The caption bar under the photo (`editing.md` §2): 36 high on `canvas`, the show-captions toggle
//! at the left (`button:caption.toggle`, for the slideshow, drawn disabled until M11), the photo's
//! caption in a centred field with the placeholder "Add a caption" (`field:caption`), and the trash
//! button at the right (`button:edit.trash`, drawn disabled until M9).
//!
//! The field shows the open photo's caption. What is typed is kept while the field has the focus;
//! Return or a click away sets it through `photo.caption.set` (one library undo step, nothing when
//! the text is unchanged); Esc gives back the caption as it was. Moving to another photo commits
//! what was typed for the one it was typed for. The draft is kept in the view state
//! (`UiState::caption_bar`), so closing the app commits it too ([`crate::App::shutdown`]).

use dalica_core::PhotoId;
use dalica_ui_kit::icons::EditIcon;
use dalica_ui_kit::widgets::TextField;
use egui::{Id, Rect, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use super::top_bar;
use crate::app::App;
use crate::commands::edit_view;
use crate::state::{CaptionEdit, MAX_DRAFT};

pub const FIELD_ID: &str = "field:caption";
pub const PLACEHOLDER: &str = "Add a caption";
pub const TOGGLE_ID: &str = "button:caption.toggle";
pub const TOGGLE_TIP: &str = "Show captions in the slideshow";
pub const TRASH_ID: &str = "button:edit.trash";
pub const TRASH_TIP: &str = "Delete this photo";

/// Where the field's focus last frame is kept in the context's data, for [`editing`].
const EDITING: &str = "caption:editing";

/// The field's widest, and the room kept at each end for the buttons (margin, button, gap).
pub const FIELD_MAX_W: f32 = 360.0;
pub const BUTTON: f32 = 28.0;
pub const MARGIN: f32 = 8.0;

/// Draws the caption bar into `ui`'s rect (the `captionBar` region).
pub fn show(app: &mut App, ui: &mut Ui) {
    let Some(photo) = edit_view::open_photo(app) else { return };
    let t = *app.tokens();
    let bar = ui.max_rect();
    let y = bar.center().y;
    let button = |x: f32| Rect::from_center_size(pos2(x, y), vec2(BUTTON, BUTTON));
    // Disabled until the slideshow (M11) and delete (M9).
    top_bar::glyph_button(ui, &t, TOGGLE_ID, EditIcon::CaptionToggle, button(bar.min.x + MARGIN + BUTTON / 2.0), false, TOGGLE_TIP);
    top_bar::glyph_button(ui, &t, TRASH_ID, EditIcon::Trash, button(bar.max.x - MARGIN - BUTTON / 2.0), false, TRASH_TIP);

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
    let out = TextField::new(FIELD_ID, PLACEHOLDER, field.width()).show(&mut child, &t, &mut text);
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
