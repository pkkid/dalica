//! The light table's pointer input on cells and empty space (`ui-layout.md` §5.4–§5.5): for now
//! a click selects a photo (`select.set`) and Ctrl-click toggles it; the selected photo's folder
//! becomes the selected section.
//!
//! **Boundary.** Two calls a frame and one after it. [`begin`] runs before any cell is laid out
//! (an interaction it creates on the empty space sits under the cells, so cells win their clicks);
//! [`finish`] runs after the last cell with the cells' [`CellOut`]s and what [`begin`] returned, and
//! collects what to do as [`Pending`]; the parent passes that to [`apply`] once the library is no
//! longer borrowed. What input keeps between frames (a rubber band, a Shift anchor) lives in
//! [`Input`].

use dalica_core::{FolderId, PhotoId};
use egui::{Response, Ui};
use serde_json::{Value, json};

use super::Frame;
use super::cell::CellOut;
use crate::app::App;

/// What the input keeps between frames.
#[derive(Debug, Default)]
pub struct Input {}

/// A command to run, and the folder to select when it succeeds.
#[derive(Debug)]
pub struct Action {
    pub command: String,
    pub params: Value,
    pub then_folder: Option<FolderId>,
}

/// What this frame's input asks for, in order.
#[derive(Debug, Default)]
pub struct Pending {
    pub actions: Vec<Action>,
}

/// Before the cells: the empty space's interaction, if any (none yet).
pub fn begin(_ui: &mut Ui, _f: &Frame<'_>, _input: &mut Input) -> Option<Response> {
    None
}

/// After the cells: what the clicks ask for.
pub fn finish(ui: &mut Ui, f: &Frame<'_>, _input: &mut Input, _background: Option<Response>, cells: &[CellOut]) -> Pending {
    let mut pending = Pending::default();
    let toggle = ui.input(|i| i.modifiers.command);
    if let Some(c) = cells.iter().find(|c| c.response.clicked()) {
        let id: PhotoId = c.photo;
        let mode = if toggle { "toggle" } else { "replace" };
        let folder = f.session.library().photo(id).map(|p| p.folder);
        pending.actions.push(Action { command: "select.set".into(), params: json!({ "ids": [id.get()], "mode": mode }), then_folder: folder });
    }
    pending
}

/// Runs what [`finish`] collected.
pub fn apply(app: &mut App, pending: Pending) {
    for a in pending.actions {
        if app.run(&a.command, &a.params).is_ok()
            && let Some(folder) = a.then_folder
        {
            app.state_mut().folder = Some(folder);
        }
    }
}
