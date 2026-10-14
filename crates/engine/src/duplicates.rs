//! Find Duplicates (`organizing.md` §8.3): identical files only. Photos grouped by size, the
//! digests of those that share a size computed on workers (MD5, cached in the index by size and
//! modification time), each digest group confirmed byte for byte; the groups (largest total first)
//! enter search mode `duplicates` (`search.rs`), one section each.
//!
//! A stub until M9.16 fills it: the running job's state lives in [`Duplicates`] (the session's
//! `duplicates` field) and its results come in through [`Session::poll_duplicates`], called by
//! `Session::poll`.

use crate::session::Session;

/// The Find Duplicates job running, if one is (M9.16).
#[derive(Debug, Default)]
pub(crate) struct Duplicates {}

impl Session {
    /// Applies what the Find Duplicates workers found (from `poll`). Returns how many things it
    /// handled; nothing yet (M9.16).
    pub(crate) fn poll_duplicates(&mut self) -> usize {
        let _ = &self.duplicates;
        0
    }
}
