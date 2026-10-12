//! The Tags pane's "Add a tag:" (`organizing.md` §5.2): the field `field:tags.add` and +
//! (`button:tags.add`). The text is view state ([`crate::state::TagsPane::draft`]); Enter or + adds
//! it to the selected photos through `photo.tags.add` (commas separate tags) and empties the field.
//!
//! For now the field has no autocomplete list; it arrives later in this milestone with the
//! highlight ([`crate::state::TagsPane::highlight`]).

use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::widgets;
use egui::Ui;
use serde_json::json;

use crate::app::App;
use crate::state::MAX_DRAFT;

pub const FIELD_ID: &str = "field:tags.add";
pub const BUTTON_ID: &str = "button:tags.add";
/// The field's hint while empty.
pub const HINT: &str = "Type a tag";
/// + 's edge (points).
const BUTTON: f32 = 24.0;

/// Whether + can add now: some text and some photos selected.
pub fn can_add(app: &App) -> bool {
    !app.state().tags_pane.draft.trim().is_empty() && !app.state().selection.ids.is_empty()
}

/// Adds the field's text to the selected photos; the field empties when that worked (a failure's
/// reason is on the status line, the text kept).
pub fn add(app: &mut App) {
    if !can_add(app) {
        return;
    }
    let state = app.state();
    let params = json!({ "ids": state.selection.ids, "tags": state.tags_pane.draft });
    if app.run("photo.tags.add", &params).is_ok() {
        let pane = &mut app.state_mut().tags_pane;
        pane.draft.clear();
        pane.highlight = None;
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    super::caption(app, ui, "Add a tag:");
    let mut text = app.state().tags_pane.draft.clone();
    let width = (ui.available_width() - BUTTON - 4.0).max(24.0);
    let (field, plus) = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let field = widgets::text_field(ui, &t, FIELD_ID, &mut text, HINT, width);
            let enabled = !text.trim().is_empty() && !app.state().selection.ids.is_empty();
            let plus = widgets::icon_button(ui, &t, BUTTON_ID, Icon::Plus, BUTTON, false, enabled, "Add the tag to the selected photos");
            (field, plus)
        })
        .inner;
    if text != app.state().tags_pane.draft {
        let mut text = text;
        if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
            text.truncate(at);
        }
        app.state_mut().tags_pane.draft = text;
    }
    if field.ending == Some(widgets::Ending::Return) || plus.clicked() {
        add(app);
    }
}
