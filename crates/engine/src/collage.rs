//! The open collage (`creating.md` §3, `roadmap.md` M14 → *Collage*): at most one Collage room at a
//! time, its document (`dalica_create::collage::Collage`) and the room's own history (Ctrl+Z and
//! Ctrl+Y in the room; never the library's undo stack). The commands are `cmd::collage`; the
//! room's last style and settings are [`crate::settings::CollageSettings`].
//!
//! The history keeps whole documents (a document is plain data, at most 1 000 clips), so an undo
//! gives back exactly what was there. The placements are `create`'s layouts over the pictures'
//! aspect ratios from the model (never a decode), kept until the document or the library changes.
//! Nothing here touches the disk.

use dalica_create::collage::{Collage as Doc, Placement, layout};
use dalica_edit::Cropping;

use crate::session::Session;

/// The most steps the room's history keeps; the oldest go first.
pub const MAX_HISTORY: usize = 100;

/// The Collage room's state.
#[derive(Debug, Default)]
pub struct Collage {
    room: Option<Room>,
}

#[derive(Debug)]
struct Room {
    doc: Doc,
    /// The document as opened or last written (M14.17): the room has unsaved changes while the
    /// document differs from it.
    baseline: Doc,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    /// The display's size in pixels the window last gave (for Current Display).
    display: Option<(u32, u32)>,
    /// Bumped on every change of the document; with the library's revision, the placements' key.
    generation: u64,
    placements: Option<(u64, u64, Vec<Placement>)>,
}

impl Collage {
    /// Whether a collage is open in the room.
    pub fn is_open(&self) -> bool {
        self.room.is_some()
    }

    /// The steps of the room's history that can be undone and redone.
    pub fn history_depth(&self) -> (usize, usize) {
        self.room.as_ref().map_or((0, 0), |r| (r.undo.len(), r.redo.len()))
    }

    /// The open collage's document.
    pub fn document(&self) -> Option<&Doc> {
        self.room.as_ref().map(|r| &r.doc)
    }

    /// Whether the open collage has changes not yet written (Close asks first).
    pub fn is_changed(&self) -> bool {
        self.room.as_ref().is_some_and(|r| r.doc != r.baseline)
    }

    /// The display's size the window last gave the room.
    pub fn display(&self) -> Option<(u32, u32)> {
        self.room.as_ref().and_then(|r| r.display)
    }

    /// Opens the room on `doc` (an empty history, no unsaved changes), replacing any open one.
    pub(crate) fn open(&mut self, doc: Doc, display: Option<(u32, u32)>) {
        self.room = Some(Room { baseline: doc.clone(), doc, undo: Vec::new(), redo: Vec::new(), display, generation: 0, placements: None });
    }

    /// Closes the room, its document and history gone.
    pub(crate) fn close(&mut self) {
        self.room = None;
    }

    /// Replaces the document as one step of the room's history; answers whether it changed.
    pub(crate) fn change(&mut self, doc: Doc) -> bool {
        let Some(r) = self.room.as_mut() else { return false };
        if r.doc == doc {
            return false;
        }
        let before = std::mem::replace(&mut r.doc, doc);
        r.undo.push(before);
        if r.undo.len() > MAX_HISTORY {
            r.undo.remove(0);
        }
        r.redo.clear();
        r.generation = r.generation.wrapping_add(1);
        true
    }

    /// Undoes the room's last step; answers whether there was one.
    pub(crate) fn undo(&mut self) -> bool {
        let Some(r) = self.room.as_mut() else { return false };
        let Some(doc) = r.undo.pop() else { return false };
        r.redo.push(std::mem::replace(&mut r.doc, doc));
        r.generation = r.generation.wrapping_add(1);
        true
    }

    /// Redoes the room's last undone step; answers whether there was one.
    pub(crate) fn redo(&mut self) -> bool {
        let Some(r) = self.room.as_mut() else { return false };
        let Some(doc) = r.redo.pop() else { return false };
        r.undo.push(std::mem::replace(&mut r.doc, doc));
        r.generation = r.generation.wrapping_add(1);
        true
    }

    /// The document as it is now counts as written (Create Collage and Save Draft, M14.17): Close
    /// no longer asks until the next change.
    pub fn mark_saved(&mut self) {
        if let Some(r) = self.room.as_mut() {
            r.baseline = r.doc.clone();
        }
    }

    pub(crate) fn set_display(&mut self, display: (u32, u32)) {
        if let Some(r) = self.room.as_mut() {
            r.display = Some(display);
        }
    }
}

impl Session {
    /// The Collage room's state (`creating.md` §3).
    pub fn collage(&self) -> &Collage {
        &self.collage
    }

    /// Each clip's aspect ratio (width over height) by its position in the open collage's Clips:
    /// the photo as its recipe shows it (turns, frames and crop counted), from the model's size;
    /// `NaN` (laid out as square) for a size the model does not know or a photo no longer indexed.
    pub fn collage_aspects(&self) -> Vec<f64> {
        let Some(doc) = self.collage.document() else { return Vec::new() };
        let lib = self.library();
        doc.clips
            .iter()
            .map(|c| match lib.photo(c.id) {
                Some(p) => {
                    let (w, h) = dalica_edit::shown_size(&lib.info(c.id).edits, p.width, p.height, Cropping::Counted);
                    if w == 0 || h == 0 { f64::NAN } else { f64::from(w) / f64::from(h) }
                }
                None => f64::NAN,
            })
            .collect()
    }

    /// The open collage's placements, bottom first (`create::collage::layout`): laid out again only
    /// when the document or the library changed since the last ask.
    pub fn collage_placements(&mut self) -> Vec<Placement> {
        let revision = self.library().revision();
        let Some(generation) = self.collage.room.as_ref().map(|r| r.generation) else { return Vec::new() };
        if let Some((g, rev, p)) = self.collage.room.as_ref().and_then(|r| r.placements.as_ref())
            && *g == generation
            && *rev == revision
        {
            return p.clone();
        }
        let aspects = self.collage_aspects();
        let Some(room) = self.collage.room.as_mut() else { return Vec::new() };
        let placements = layout(&room.doc, &aspects);
        room.placements = Some((generation, revision, placements.clone()));
        placements
    }
}
