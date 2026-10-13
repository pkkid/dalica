//! The caption bar under the photo (`editing.md` §2): 36 high on `canvas`, the show-captions toggle
//! at the left (`button:caption.toggle`, for the slideshow, drawn disabled until M11), the photo's
//! caption in a centred field with the placeholder "Add a caption" (`field:caption`), and the trash
//! button at the right (`button:edit.trash`, drawn disabled until M9).
//!
//! The field shows the open photo's caption. What is typed is kept while the field has the focus;
//! Return or a click away sets it through `photo.caption.set` (one library undo step, nothing when
//! the text is unchanged); Esc gives back the caption as it was. Moving to another photo commits
//! what was typed for the one it was typed for.

use dalica_core::PhotoId;
use dalica_ui_kit::icons::EditIcon;
use dalica_ui_kit::widgets::TextField;
use egui::{Id, Rect, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use super::top_bar;
use crate::app::App;
use crate::commands::edit_view;

pub const FIELD_ID: &str = "field:caption";
pub const PLACEHOLDER: &str = "Add a caption";
pub const TOGGLE_ID: &str = "button:caption.toggle";
pub const TOGGLE_TIP: &str = "Show captions in the slideshow";
pub const TRASH_ID: &str = "button:edit.trash";
pub const TRASH_TIP: &str = "Delete this photo";

/// Where the draft is kept in the context's data.
const DRAFT: &str = "caption:draft";

/// The field's widest, and the room kept at each end for the buttons (margin, button, gap).
pub const FIELD_MAX_W: f32 = 360.0;
pub const BUTTON: f32 = 28.0;
pub const MARGIN: f32 = 8.0;

/// The caption being typed: for which photo, and the text.
#[derive(Clone, Default)]
struct Draft {
    photo: Option<PhotoId>,
    text: String,
    /// The field had the focus last frame.
    editing: bool,
}

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
    let id = Id::new(DRAFT);
    let mut draft = ui.ctx().data_mut(|d| d.remove_temp::<Draft>(id)).unwrap_or_default();
    // Typed for another photo (a step while the field had the focus): that photo keeps it.
    if draft.editing && draft.photo.is_some_and(|p| p != photo) {
        commit(app, &draft);
        draft.editing = false;
    }
    if !draft.editing {
        draft.photo = Some(photo);
        draft.text = caption(app, photo);
    }
    let h = t.metrics.text_field_h;
    let field = Rect::from_min_size(pos2((bar.center().x - width / 2.0).round(), (y - h / 2.0).round()), vec2(width.round(), h));
    let mut child = ui.new_child(UiBuilder::new().id_salt("captionBar").max_rect(field));
    let out = TextField::new(FIELD_ID, PLACEHOLDER, field.width()).show(&mut child, &t, &mut draft.text);
    if out.committed() {
        commit(app, &draft);
    }
    draft.editing = out.editing && out.ending.is_none();
    ui.ctx().data_mut(|d| d.insert_temp(id, draft));
}

/// Whether the caption field had the keyboard last frame. egui drops the focus as soon as Esc is
/// pressed, before the frame's shortcuts run, so the edit view's dispatch asks this to leave an Esc
/// that ends a caption edit to the field.
pub fn editing(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Draft>(Id::new(DRAFT)).is_some_and(|d| d.editing))
}

/// Photo `id`'s caption ("" when the library does not have it).
fn caption(app: &App, id: PhotoId) -> String {
    app.session().map(|s| s.library().caption(id).to_owned()).unwrap_or_default()
}

/// Sets the draft's photo's caption to what was typed, unless it is that already.
fn commit(app: &mut App, draft: &Draft) {
    let Some(photo) = draft.photo else { return };
    let text = draft.text.trim();
    if caption(app, photo) != text {
        let _ = app.run("photo.caption.set", &json!({ "ids": [photo.get()], "caption": text }));
    }
}
