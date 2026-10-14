//! `albums.json` in the library directory (`data-model.md` §3.2, §4): the albums' definitions (id,
//! name, description, date, place, cover) and a copy of their order, beside the memberships each
//! folder sidecar holds. It is read when the library opens and written through the I/O worker after
//! every applied op that touches albums; when it is missing, a rebuild recovers memberships, order
//! and names from the sidecars.
//!
//! Stubs until M9.10 fills them: [`Session::read_albums_at_open`] (called by `Session::open`) and
//! [`Session::albums_applied`] (called by `Session::apply` after every op, which decides whether
//! the op touched albums).

use dalica_library::Op;

use crate::session::Session;

impl Session {
    /// At session open: `albums.json` is read, a damaged one set aside and reported (M9.10). Does
    /// nothing yet.
    pub(crate) fn read_albums_at_open(&mut self) {}

    /// After `op` was applied: when it touched albums, `albums.json` is written through the I/O
    /// worker (M9.10). Does nothing yet.
    pub(crate) fn albums_applied(&mut self, _op: &Op) {}
}
