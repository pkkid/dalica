//! The Tags pane's "Add a tag:" (`organizing.md` §5.2): the field `field:tags.add` and +
//! (`button:tags.add`). The text is view state ([`crate::state::TagsPane::draft`]); Enter or + adds
//! it to the selected photos through `photo.tags.add` (commas separate tags; one undo step) and
//! empties the field, which keeps the focus after Enter for the next tag.
//!
//! **Autocomplete** (`ui-kit`'s [`widgets::autocomplete`]): the tags in use that start with the
//! piece being typed (the text after the last comma), from the `tags.list {prefix}` query (asked
//! again only when the piece or the library changed). Each suggestion is the whole text with that
//! piece completed ("beach, tr" offers "beach, tram"), so picking one keeps the tags typed before
//! it. ↑ / ↓ move the highlight ([`crate::state::TagsPane::highlight`] follows the list's), Enter
//! adds the highlighted suggestion (or the text as typed when none is), a click on one adds it,
//! and Esc closes the list (a second Esc gives back the text from before the edit).

use dalica_ui_kit::icons::Icon;
use dalica_ui_kit::registry::{self, Role};
use dalica_ui_kit::widgets::{self, Ending};
use egui::{Id, Ui};
use serde_json::{Value, json};

use crate::app::App;
use crate::state::MAX_DRAFT;
use crate::tl;

pub const FIELD_ID: &str = "field:tags.add";
pub const BUTTON_ID: &str = "button:tags.add";
/// The field's hint while empty.
pub const HINT: &str = "Type a tag";
/// + 's edge (points).
const BUTTON: f32 = 24.0;

/// Adds the field's text to the selected photos; the field empties when that worked. A failure's
/// reason ("Select some photos first.", a tag too long) is on the status line, the text kept.
/// Returns whether the tags were added.
pub fn add(app: &mut App) -> bool {
    let state = app.state();
    if state.tags_pane.draft.trim().is_empty() {
        return false;
    }
    let params = json!({ "ids": state.selection.ids, "tags": state.tags_pane.draft });
    let added = app.run("photo.tags.add", &params).is_ok();
    if added {
        let pane = &mut app.state_mut().tags_pane;
        pane.draft.clear();
        pane.highlight = None;
    }
    added
}

/// `text` split where the piece being completed starts: everything up to and including the last
/// comma and the spaces after it, then the piece.
pub fn split_piece(text: &str) -> (&str, &str) {
    let start = text.rfind(',').map_or(0, |at| at + 1);
    let (head, piece) = text.split_at(start);
    let trimmed = piece.trim_start();
    let at = start + (piece.len() - trimmed.len());
    (text.get(..at).unwrap_or(head), trimmed)
}

/// The tags of a `tags.list {prefix}` reply.
pub fn tags_of(reply: &Value) -> Vec<String> {
    let Some(tags) = reply.get("tags").and_then(Value::as_array) else { return Vec::new() };
    tags.iter().filter_map(|t| t.get("tag").and_then(Value::as_str).map(str::to_owned)).collect()
}

/// The tags in use starting with `piece`, and what they were asked for.
#[derive(Clone, Default)]
struct Cached {
    revision: Option<u64>,
    piece: String,
    tags: Vec<String>,
}

/// The suggestions for `text`: the tags in use starting with its last piece (`tags.list
/// {prefix}`, at most 100), each written after the text before that piece. None while the piece is
/// empty or before the library is open.
pub fn suggestions(app: &mut App, ctx: &egui::Context, text: &str) -> Vec<String> {
    let (head, piece) = split_piece(text);
    let piece = piece.trim_end();
    let Some(session) = app.session_mut() else { return Vec::new() };
    if piece.is_empty() {
        return Vec::new();
    }
    let key = Id::new("tags.add.suggestions");
    let revision = session.library().revision();
    let hit = ctx.data(|d| d.get_temp::<Cached>(key)).filter(|c| c.revision == Some(revision) && c.piece == piece).map(|c| c.tags);
    let tags = match hit {
        Some(tags) => tags,
        None => {
            let tags = session.execute("tags.list", &json!({ "prefix": piece })).map(|v| tags_of(&v)).unwrap_or_default();
            ctx.data_mut(|d| d.insert_temp(key, Cached { revision: Some(revision), piece: piece.to_owned(), tags: tags.clone() }));
            tags
        }
    };
    tags.into_iter().map(|t| format!("{head}{t}")).collect()
}

/// The row of the open list that is highlighted, read from the rows the list registered this pass
/// (`<field id>/option/<index>`, active while highlighted), as a place among `text`'s matches.
fn highlighted(ctx: &egui::Context, options: &[String], text: &str) -> Option<usize> {
    let prefix = format!("{FIELD_ID}/option/");
    let lit = registry::widgets(ctx)
        .into_iter()
        .find(|w| w.role == Role::Suggestion && w.active && w.id.starts_with(&prefix))
        .and_then(|w| w.id.get(prefix.len()..).and_then(|i| i.parse::<usize>().ok()))?;
    widgets::autocomplete_matches(options, text).iter().position(|&i| i == lit)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    super::caption(app, ui, &tl!("Add a tag:"));
    let hint = tl!("Type a tag");
    let mut text = app.state().tags_pane.draft.clone();
    let options = suggestions(app, &ctx, &text);
    let width = (ui.available_width() - BUTTON - 4.0).max(24.0);
    let selected = !app.state().selection.ids.is_empty();
    let (out, plus) = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let out =
                widgets::autocomplete(ui, &t, FIELD_ID, &mut text, &options, |ui, text| widgets::text_field(ui, &t, FIELD_ID, text, &hint, width));
            let enabled = !text.trim().is_empty() && selected;
            let plus = widgets::icon_button(ui, &t, BUTTON_ID, Icon::Plus, BUTTON, false, enabled, &tl!("Add the tag to the selected photos"));
            (out, plus)
        })
        .inner;
    if text != app.state().tags_pane.draft {
        let mut text = text.clone();
        if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
            text.truncate(at);
        }
        app.state_mut().tags_pane.draft = text;
    }
    // The highlight follows the list while it is open or the field is being edited.
    if out.open {
        app.state_mut().tags_pane.highlight = highlighted(&ctx, &options, &text);
    } else if out.field.editing || out.field.ending.is_some() {
        app.state_mut().tags_pane.highlight = None;
    }
    let enter = out.field.ending == Some(Ending::Return);
    if (enter || out.picked.is_some() || plus.clicked()) && add(app) && enter {
        // Ready for the next tag.
        widgets::focus_field(&ctx, FIELD_ID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_piece_being_typed_is_the_text_after_the_last_comma() {
        assert_eq!(split_piece("bea"), ("", "bea"));
        assert_eq!(split_piece("beach, tr"), ("beach, ", "tr"));
        assert_eq!(split_piece("beach,tr"), ("beach,", "tr"));
        assert_eq!(split_piece("beach, "), ("beach, ", ""));
        assert_eq!(split_piece("a, b,  ñan"), ("a, b,  ", "ñan"));
        assert_eq!(split_piece(""), ("", ""));
        assert_eq!(split_piece(","), (",", ""));
    }

    #[test]
    fn tags_come_from_the_reply() {
        assert_eq!(tags_of(&json!({ "tags": [{ "tag": "beach", "count": 3 }, { "tag": 4 }, "x"] })), ["beach"]);
        assert_eq!(tags_of(&json!({ "tags": null })), Vec::<String>::new());
    }

    #[test]
    fn suggestions_complete_the_last_piece_and_keep_the_rest() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(2);
        let mut app = App::new(Some(s));
        let n: Vec<u64> = ids.iter().map(|i| i.get()).collect();
        app.run("photo.tags.add", &json!({ "ids": n, "tags": "tram, Trees, beach" })).unwrap();
        let ctx = egui::Context::default();
        assert_eq!(suggestions(&mut app, &ctx, "tr"), ["tram", "Trees"]);
        assert_eq!(suggestions(&mut app, &ctx, "beach, TR"), ["beach, tram", "beach, Trees"]);
        assert_eq!(suggestions(&mut app, &ctx, "beach, "), Vec::<String>::new());
        assert_eq!(suggestions(&mut app, &ctx, "zz"), Vec::<String>::new());
        // A new tag is offered once the library has it.
        app.run("photo.tags.add", &json!({ "ids": [n[0]], "tags": "trolley" })).unwrap();
        assert_eq!(suggestions(&mut app, &ctx, "tr"), ["tram", "Trees", "trolley"]);
        assert_eq!(suggestions(&mut App::new(None), &ctx, "tr"), Vec::<String>::new(), "no library yet");
    }
}
