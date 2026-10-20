//! The inline caption editor (`organizing.md` §5.3): Picture ▸ Edit Caption opens a one-line
//! field (ui-kit's [`TextField`], registered as [`FIELD_ID`], placeholder [`PLACEHOLDER`]) under
//! the photo's cell, ready for typing. Enter or a click elsewhere commits through the engine's
//! `photo.caption.set` (one undo step, nothing when the text is unchanged); Esc closes it and leaves
//! the caption as it was; scrolling the cell out of view commits what was typed.
//!
//! **Boundary.** [`super::cell::show`] calls [`show`] for the cell whose photo the view state's
//! [`CaptionEdit`] names, after painting the cell, so the field sits on top of it. The field
//! fills the row gap under the cell (the caption line's place, ending at the next row's top) and is
//! as wide as the cell, so no other cell paints over it. What the user did is kept in [`Editor`]
//! and applied once per frame by [`apply`] (through [`super::cell::apply`]), when the library is
//! no longer borrowed: the typed text goes to [`CaptionEdit::draft`] (the view state is the text's
//! one home, so `ui.set` can type too), a commit runs the command, and an editor that was on
//! screen and is not any more commits.
//!
//! While the field has the focus the window's shortcuts are those of the Text input context, then
//! Global ([`crate::keys::dispatch`]): Ctrl+8 typed into it stars nothing.

use dalica_core::PhotoId;
use dalica_ui_kit::widgets::{self, TextField};
use egui::{Rect, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use super::Frame;
use crate::app::App;
use crate::state::{CaptionEdit, MAX_DRAFT};
use crate::tl;

/// The editor's automation id (`control-protocol.md` §7).
pub const FIELD_ID: &str = "field:caption.inline";
/// What the empty field shows, in English (drawn through the catalogs).
pub const PLACEHOLDER: &str = "Add a caption";

/// What the user did in the editor this frame.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Action {
    /// The text changed (typing, a paste, Esc's restore).
    Typed(String),
    /// Enter or a click elsewhere: set the caption to this text.
    Commit(String),
    /// Esc: close, changing nothing.
    Cancel,
}

/// What the editor keeps between frames.
#[derive(Debug, Default)]
pub struct Editor {
    /// The photo the editor was drawn for since it opened.
    shown: Option<PhotoId>,
    /// The photo the editor has given the focus to (once per opening).
    focused: Option<PhotoId>,
    /// The pass the editor was last drawn in.
    drawn: Option<u64>,
    /// This frame's action, for [`apply`].
    action: Option<Action>,
}

impl Editor {
    /// Whether the editor was drawn in pass `pass`.
    pub fn drawn_in(&self, pass: u64) -> bool {
        self.drawn == Some(pass)
    }

    fn reset(&mut self) {
        *self = Editor::default();
    }
}

/// Where the field goes under a cell `cell` of a grid whose rows are `row_gap` apart: as wide as
/// the cell, `height` high, ending at the next row's top (so it covers the caption line).
pub fn field_rect(cell: Rect, row_gap: f32, height: f32) -> Rect {
    let gap = if row_gap.is_finite() { row_gap.max(0.0) } else { 0.0 };
    let top = cell.bottom() + gap - height;
    Rect::from_min_size(pos2(cell.left(), top), vec2(cell.width(), height))
}

/// `text` cut to at most [`MAX_DRAFT`] characters (on a character boundary).
fn cut(text: &mut String) {
    if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
        text.truncate(at);
    }
}

/// Draws the field for `edit` under the photo's cell `cell` (see the module's boundary) and keeps
/// what the user did for [`apply`]. `pass` is egui's pass number.
pub fn show(ui: &mut Ui, f: &Frame<'_>, editor: &mut Editor, edit: &CaptionEdit, cell: Rect, pass: u64) {
    let t = f.t;
    let rect = field_rect(cell, f.layout.spacing().row_gap, t.metrics.text_field_h);
    if editor.shown.is_some_and(|p| p != edit.photo) {
        editor.reset();
    }
    // A new opening: the field takes the focus, ready for typing. Not on a pass where a button
    // goes up (the menu item's click that opened it): egui ends a focus on a click elsewhere.
    let clicking = ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released());
    if editor.focused != Some(edit.photo) && !clicking {
        widgets::focus_field(ui.ctx(), FIELD_ID);
        editor.focused = Some(edit.photo);
    }
    editor.shown = Some(edit.photo);
    let mut text = edit.draft.clone();
    let mut child = ui.new_child(UiBuilder::new().id_salt("captionEdit").max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let out = TextField::new(FIELD_ID, &tl!("Add a caption"), rect.width()).show(&mut child, t, &mut text);
    cut(&mut text);
    editor.drawn = Some(pass);
    editor.action = if out.cancelled() {
        Some(Action::Cancel)
    } else if out.committed() {
        Some(Action::Commit(text))
    } else if text != edit.draft {
        Some(Action::Typed(text))
    } else {
        None
    };
}

/// Applies what the editor did this frame (see the module's boundary). `last_pass` is the pass the
/// light table last drew cells in: an editor drawn before it but not in it was scrolled away.
pub fn apply(app: &mut App, editor: &mut Editor, last_pass: Option<u64>) {
    let Some(edit) = app.state().caption_edit.clone() else {
        editor.reset();
        return;
    };
    if editor.shown.is_some_and(|p| p != edit.photo) {
        // Opened on another photo (by the command or `ui.set`) and not drawn since: start afresh.
        editor.reset();
    }
    let gone = app.session().is_none_or(|s| s.library().photo(edit.photo).is_none());
    let action = match editor.action.take() {
        Some(a) => Some(a),
        None if gone => Some(Action::Cancel),
        // Shown, then scrolled out of view: what was typed is kept.
        None if editor.shown == Some(edit.photo) && editor.drawn.is_some() && last_pass.is_some() && editor.drawn != last_pass => {
            Some(Action::Commit(edit.draft.clone()))
        }
        None => None,
    };
    match action {
        None => {}
        Some(Action::Typed(text)) => {
            if let Some(e) = app.state_mut().caption_edit.as_mut() {
                e.draft = text;
            }
        }
        Some(Action::Cancel) => close(app, editor),
        Some(Action::Commit(text)) => {
            close(app, editor);
            commit(app, edit.photo, &text);
        }
    }
}

/// Closes the editor.
fn close(app: &mut App, editor: &mut Editor) {
    app.state_mut().caption_edit = None;
    editor.reset();
}

/// Sets `photo`'s caption to `text` through `photo.caption.set`, unless it is that already. A
/// refusal's reason is on the status line ([`App::run`]).
pub fn commit(app: &mut App, photo: PhotoId, text: &str) {
    let same = app.session().and_then(|s| s.library().photo(photo).map(|_| s.library().caption(photo) == text));
    if same == Some(false) {
        let _ = app.run("photo.caption.set", &json!({ "ids": [photo.get()], "caption": text }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_field_fills_the_row_gap_under_the_cell() {
        let cell = Rect::from_min_size(pos2(24.0, 100.0), vec2(144.0, 144.0));
        let r = field_rect(cell, 22.0, 24.0);
        assert_eq!(r, Rect::from_min_size(pos2(24.0, 242.0), vec2(144.0, 24.0)));
        assert_eq!(r.bottom(), cell.bottom() + 22.0, "it ends at the next row's top");
        assert_eq!(field_rect(cell, f32::NAN, 24.0).bottom(), cell.bottom(), "a bad gap is no gap");
    }

    #[test]
    fn drafts_are_cut_on_a_character_boundary() {
        let mut s = "é".repeat(MAX_DRAFT + 3);
        cut(&mut s);
        assert_eq!(s.chars().count(), MAX_DRAFT);
        let mut short = "Tram 28".to_string();
        cut(&mut short);
        assert_eq!(short, "Tram 28");
    }
}
