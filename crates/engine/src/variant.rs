//! A│A's variant (`editing.md` §4, `roadmap.md` M11.10): the right side of comparing a photo with
//! itself, a scratch history of that photo held in the session only.
//!
//! - **Open** (`edit.variant.open {id}`): the variant starts as a copy of the photo's recipe, with
//!   an empty redo list and no remembered steps. Opening it again for the same photo keeps it as it
//!   is; one variant at a time, so opening one for another photo while a variant is open is refused
//!   (a variant whose photo left the library, or became another file under its id, no longer counts
//!   and is replaced).
//! - **Steps**: `photo.history.apply`, `.undo`, `.redo` and `.state` with `variant: true` walk the
//!   variant's recipe by the photo history's own rules ([`crate::history`], `Side::Variant`); the
//!   preview pane showing the variant renders its recipe ([`crate::preview::PaneShows::Variant`]).
//! - **Close** (`edit.variant.close {keep}`): `left` drops the variant; `right` makes its recipe one
//!   step of the photo's own history (a whole replacement, the step `edit.recipe.set` takes: one
//!   undo gives the recipe before it back), then drops it. A step that fails leaves the variant
//!   open.
//! - **Merge, never replace** (`AGENTS.md` §3): the variant starts as a verbatim copy, so the
//!   entries Dalica cannot read (a newer version's operation, a `legacy:` entry) stay in it, in
//!   place, and keeping it writes them back. Keeping the right side is refused, by name, when it
//!   would drop one the photo's recipe holds now (undone in the variant, or added to the photo
//!   since the variant opened); the variant stays open.
//! - Nothing of a variant reaches a sidecar until it is kept, and a restart has none.

use dalica_core::PhotoId;

use crate::error::CmdError;
use crate::history::{Change, Memory, Side};
use crate::session::Session;

/// The variant open in this session, if any.
#[derive(Debug, Default)]
pub struct Variant {
    open: Option<Scratch>,
}

/// An open variant: the photo it was made of (by id and file name, as the photo histories keep
/// theirs), its recipe and redo list, and its remembered steps.
#[derive(Debug)]
pub(crate) struct Scratch {
    pub(crate) photo: PhotoId,
    pub(crate) name: String,
    pub(crate) edits: Vec<String>,
    pub(crate) redo: Vec<String>,
    pub(crate) memory: Memory,
}

/// Which side `edit.variant.close` keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Keep {
    /// The photo's own recipe: the variant is dropped.
    Left,
    /// The variant's recipe: it becomes one step of the photo's history.
    Right,
}

impl Variant {
    /// The photo whose variant is open.
    pub fn photo(&self) -> Option<PhotoId> {
        self.open.as_ref().map(|v| v.photo)
    }

    /// The open variant's recipe.
    pub fn edits(&self) -> Option<&[String]> {
        self.open.as_ref().map(|v| v.edits.as_slice())
    }

    /// The variant of photo `id` named `name`, when that is the one open.
    pub(crate) fn of(&self, id: PhotoId, name: &str) -> Option<&Scratch> {
        self.open.as_ref().filter(|v| v.photo == id && v.name == name)
    }

    pub(crate) fn of_mut(&mut self, id: PhotoId, name: &str) -> Option<&mut Scratch> {
        self.open.as_mut().filter(|v| v.photo == id && v.name == name)
    }
}

/// Whether Dalica can read `entry`: a known operation whose module (if it has one) takes its
/// parameters (`edit.recipe.set`'s rule).
fn readable(entry: &str) -> bool {
    let op = dalica_edit::Op::parse(entry);
    op.is_known() && !matches!(op.typed(), Some(Err(_)))
}

/// The entries of `old` Dalica cannot read that `new` leaves out (each counted as often as it
/// stands in `old`).
fn dropped_unreadable<'a>(old: &'a [String], new: &[String]) -> Vec<&'a String> {
    let mut kept: Vec<&String> = new.iter().filter(|e| !readable(e)).collect();
    let mut dropped = Vec::new();
    for entry in old.iter().filter(|e| !readable(e)) {
        match kept.iter().position(|k| *k == entry) {
            Some(i) => {
                kept.swap_remove(i);
            }
            None => dropped.push(entry),
        }
    }
    dropped
}

/// The error for a step on a variant that is not open.
pub(crate) fn not_open() -> CmdError {
    CmdError::Disabled("No variant of this photo is open.".into())
}

impl Session {
    /// The recipe the variant of photo `id` holds now, when one is open for it (the preview's
    /// variant pane renders it).
    pub(crate) fn variant_edits(&self, id: PhotoId) -> Option<&[String]> {
        let name = &self.library.photo(id)?.name;
        self.variant.of(id, name).map(|v| v.edits.as_slice())
    }

    /// Opens the variant of photo `id`, a copy of its recipe; returns whether it started (`false`:
    /// that photo's variant was open already and is kept as it is).
    pub fn open_variant(&mut self, id: PhotoId) -> Result<bool, CmdError> {
        let name = self.library.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?.name.clone();
        if let Some(open) = &self.variant.open {
            if open.photo == id && open.name == name {
                return Ok(false);
            }
            let still_there = self.library.photo(open.photo).is_some_and(|p| p.name == open.name);
            if still_there {
                return Err(CmdError::Disabled("A variant of another photo is open; keep one of its sides first.".into()));
            }
        }
        let edits = self.library.info(id).edits.clone();
        self.variant.open = Some(Scratch { photo: id, name, edits, redo: Vec::new(), memory: Memory::default() });
        Ok(true)
    }

    /// Closes the variant, keeping `keep`'s side. Returns its photo and the label of the step that
    /// keeping the right side took (`None`: the left kept, or the variant's recipe was the photo's).
    pub fn close_variant(&mut self, keep: Keep) -> Result<(PhotoId, Option<&'static str>), CmdError> {
        let Some(open) = &self.variant.open else {
            return Err(CmdError::Disabled("No variant is open.".into()));
        };
        let id = open.photo;
        if keep == Keep::Left {
            self.variant.open = None;
            return Ok((id, None));
        }
        if !self.library.photo(id).is_some_and(|p| p.name == open.name) {
            return Err(CmdError::NotFound(format!("no photo {id}: only the left side can be kept")));
        }
        let edits = open.edits.clone();
        let dropped = dropped_unreadable(&self.library.info(id).edits, &edits);
        if !dropped.is_empty() {
            let named: Vec<String> = dropped.iter().take(4).map(|e| format!("{:?}", e.chars().take(64).collect::<String>())).collect();
            return Err(CmdError::Disabled(format!(
                "The right side drops edits this version of Dalica cannot read: {}. Keep the left side, or redo them in the variant.",
                named.join(", ")
            )));
        }
        let label = self.step_on(id, Side::Photo, Change::Replace(edits), None)?;
        self.variant.open = None;
        Ok((id, label))
    }
}
