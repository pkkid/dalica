//! Repairing a Save that was interrupted (`data-model.md` §6 and §10.5): a Save writes its record
//! into the folder's `.dalica-originals/.dalica.json` as pending before anything moves and marks
//! it done last, so a pending record found later means a Save stopped half-way.
//!
//! The engine looks for them at two moments: when the session opens
//! ([`Session::recover_saves_at_open`]) and when a scan brings a folder's originals record into the
//! model ([`Session::note_save_records`], called by [`Session::apply`] for every op from the files).
//! Both are stubs until M8.18 fills them: then a pending record is repaired on a worker
//! (`create`'s `save::recover`, rolled forward or back), the recipe put back when it rolled back,
//! and the repair reported in the log and `library.inspect`.

use dalica_library::Op;

use crate::session::Session;

impl Session {
    /// At session open: the pending Save records of the library's folders are repaired (M8.18).
    /// Does nothing yet.
    pub(crate) fn recover_saves_at_open(&mut self) {}

    /// After an op from the files (a scan's, the watcher's) was applied: the pending Save records
    /// it brought are repaired (M8.18). Does nothing yet.
    pub(crate) fn note_save_records(&mut self, _op: &Op) {}
}
