//! The quick full-screen preview (`viewing.md` §2): holding Ctrl+Alt (Cmd+Option) over a thumbnail,
//! or in the edit view, fills the window with that photo until the keys are released. A hold
//! gesture is listed in the shortcut editor but not rebound, and Ctrl+Alt alone is no key the
//! keymap can bind, so `peek` has no default key: the preview's panel watches the modifiers itself
//! ([`crate::panels::peek::watch`]) and an agent holds them with `ui.key {state: "down"}`, or runs
//! `peek` directly.
//!
//! - `peek {id}` shows photo `id`; `peek {id: null}` ends the preview, and the photo it showed
//!   becomes the focused one ([`end`]). The view state's `peek` is the photo shown.
//! - [`step`]: the wheel's steps through the section's photos, stopping at its ends.
//! - [`end`]: the last photo shown is focused (the anchor when it is selected, else selected alone)
//!   and scrolled into view; in the edit view, a photo other than the open one opens.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, edit_view, parse};
use crate::app::App;
use crate::selection::{self, Table};
use crate::state::{Section, Selection, View};

pub const COMMANDS: &[UiCommand] = &[UiCommand {
    id: "peek",
    label: "Quick Preview (hold Ctrl+Alt)",
    menu: "",
    keys: &[],
    params: "{id: PhotoId | null}: show a photo full screen while Ctrl+Alt is held; null ends the preview and focuses the photo shown",
    run: peek,
}];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Peek {
    /// Required: a photo, or `null` to end the preview.
    #[serde(deserialize_with = "required")]
    id: Option<PhotoId>,
}

/// An `Option` field that must be given (`null` for none): missing is an error, not `None`.
fn required<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<PhotoId>, D::Error> {
    Option::<PhotoId>::deserialize(d)
}

fn peek(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Peek = parse(params)?;
    match p.id {
        Some(id) => {
            if !app.session().is_some_and(|s| s.library().photo(id).is_some()) {
                return Err(CmdError::NotFound(format!("no photo {id}")));
            }
            app.state_mut().peek = Some(id);
        }
        None => end(app, None),
    }
    Ok(json!({ "peek": p.id }))
}

/// The section the preview steps through for `photo`: `section` when the light table shows the
/// photo in it, else the one the selection model places it in (the selection's album, its folder).
fn place(table: &Table<'_>, photo: PhotoId, section: Option<Section>) -> Option<Section> {
    section.filter(|s| table.scope(*s).is_some_and(|sc| sc.index(photo).is_some())).or_else(|| table.section_of(photo))
}

/// Moves the preview `by` photos through `section` (see [`place`]), stopping at its ends; returns
/// the photo shown. Nothing happens without a preview, or for a photo no shown section holds.
pub fn step(app: &mut App, section: Option<Section>, by: i32) -> Option<PhotoId> {
    let photo = app.state().peek?;
    let to = {
        let table = Table::of(app)?;
        let scope = table.scope(place(&table, photo, section)?)?;
        let k = scope.index(photo)?;
        let last = scope.photos.len().saturating_sub(1);
        let n = usize::try_from(by.unsigned_abs()).unwrap_or(usize::MAX);
        let j = if by >= 0 { k.saturating_add(n).min(last) } else { k.saturating_sub(n) };
        scope.photos.get(j).copied().unwrap_or(photo)
    };
    app.state_mut().peek = Some(to);
    Some(to)
}

/// The photos beside the one shown in `section` (next first), for prefetching.
pub fn neighbours(app: &App, section: Option<Section>) -> Vec<PhotoId> {
    let Some(photo) = app.state().peek else { return Vec::new() };
    let Some(table) = Table::of(app) else { return Vec::new() };
    let Some(scope) = place(&table, photo, section).and_then(|s| table.scope(s)) else { return Vec::new() };
    let Some(k) = scope.index(photo) else { return Vec::new() };
    let after = k.checked_add(1).and_then(|i| scope.photos.get(i));
    let before = k.checked_sub(1).and_then(|i| scope.photos.get(i));
    after.into_iter().chain(before).copied().collect()
}

/// Ends the preview: the photo it showed becomes the focused one in `section` (see [`place`]) and
/// is scrolled into view; in the edit view a photo other than the open one opens.
pub fn end(app: &mut App, section: Option<Section>) {
    let Some(photo) = app.state_mut().peek.take() else { return };
    if app.session().is_none_or(|s| s.library().photo(photo).is_none()) {
        return;
    }
    if app.state().view == View::Edit {
        if edit_view::open_photo(app) != Some(photo) {
            let _ = app.run("view.edit", &json!({ "id": photo }));
        }
        return;
    }
    let (section, reveal) = match Table::of(app) {
        Some(table) => {
            let s = place(&table, photo, section);
            (s, s.and_then(|s| table.reveal(s, photo)))
        }
        None => (None, None),
    };
    let state = app.state_mut();
    if state.selection.contains(photo) {
        state.selection.anchor = Some(photo);
    } else {
        selection::commit(state, Selection::new(vec![photo], Some(photo), section, false));
    }
    if let Some(anchor) = reveal {
        state.scroll = anchor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_shows_a_photo_of_the_library_and_null_ends_it() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(1);
        let mut app = App::new(Some(s));
        assert_eq!(app.run("peek", &json!({ "id": ids[0] })).unwrap(), json!({ "peek": ids[0] }));
        assert_eq!(app.state().peek, Some(ids[0]));
        assert!(matches!(app.run("peek", &json!({ "id": 999_999 })), Err(CmdError::NotFound(_))));
        assert_eq!(app.state().peek, Some(ids[0]), "a refused peek changes nothing");
        app.run("peek", &json!({ "id": null })).unwrap();
        assert_eq!(app.state().peek, None);
        assert_eq!(app.state().selection.anchor, Some(ids[0]), "the photo shown is focused");
        assert!(matches!(app.run("peek", &json!({})), Err(CmdError::BadParams(_))), "`id` is required");
    }

    #[test]
    fn steps_stop_at_the_section_ends() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(3);
        let mut app = App::new(Some(s));
        app.run("peek", &json!({ "id": ids[1] })).unwrap();
        let first = step(&mut app, None, -99).unwrap();
        assert_eq!(neighbours(&app, None).len(), 1, "the first photo has only a next neighbour");
        assert_eq!(step(&mut app, None, -1), Some(first), "the first photo stays");
        let second = step(&mut app, None, 1).unwrap();
        assert_ne!(second, first);
        assert_eq!(neighbours(&app, None).len(), 2);
        let last = step(&mut app, None, 99).unwrap();
        assert_eq!(step(&mut app, None, 1), Some(last), "the last photo stays");
        assert_eq!(step(&mut app, None, -2), Some(first));
    }
}
