//! The Import room's destination, checked with its links resolved before automation's input can
//! start an import (`control-protocol.md` §5.7, `security.md` §5.7).
//!
//! A click, a key or a pointer event sent over the control channel can press Import All or Enter
//! in the Import room, which runs `import.run` into the destination strip's folder. The app asks
//! [`super::refuse`] first, on the UI thread, where nothing may touch the disk: the folder can only
//! be compared as written there. So before a request (or a batch step) plays input while the room
//! has a source listed, [`before_input`] resolves that folder through its links on a worker, as
//! `engine.execute` resolves `import.run`'s `destination`, and keeps the verdict for the request;
//! [`super::refuse`] then answers from it. A destination it was not given (the draft changed while
//! the input played) is refused, never let through unchecked.
//!
//! The input outcome waits on the UI thread (it is not `Send`) while the worker checks: it is
//! parked in a thread-local slot under a ticket the worker's continuation takes back.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::host::Host;
use super::{Code, Config, Error, Next, Outcome, paths};

/// The destination checked for the request running now, and what the check said.
struct Vetted {
    dest: PathBuf,
    watched: Vec<PathBuf>,
    verdict: Result<(), String>,
}

thread_local! {
    /// The verdict for the request running on this thread (the UI thread), if one was checked.
    static VETTED: RefCell<Option<Vetted>> = const { RefCell::new(None) };
    /// Outcomes waiting for their destination's check, by ticket.
    static PARKED: RefCell<HashMap<u64, Outcome>> = RefCell::new(HashMap::new());
    static NEXT_TICKET: Cell<u64> = const { Cell::new(0) };
}

/// Forgets the last request's verdict: a new request checks again (the disk may have changed).
pub(super) fn forget() {
    let _ = VETTED.try_with(|v| v.try_borrow_mut().map(|mut v| *v = None));
}

/// Keeps `verdict` for `dest` as checked against `watched`, for [`verdict`].
pub(super) fn record(dest: PathBuf, watched: Vec<PathBuf>, verdict: Result<(), String>) {
    let _ = VETTED.try_with(|v| v.try_borrow_mut().map(|mut v| *v = Some(Vetted { dest, watched, verdict })));
}

/// What the check said about `dest` against `watched` in this request; `None` when it was not
/// checked (or checked against other watched folders).
pub(super) fn verdict(dest: &Path, watched: &[PathBuf]) -> Option<Result<(), String>> {
    VETTED
        .try_with(|v| v.try_borrow().ok().and_then(|v| v.as_ref().filter(|v| v.dest == dest && v.watched == watched).map(|v| v.verdict.clone())))
        .ok()
        .flatten()
}

/// The folder `import.run` would import into now, and the watched folders, while the Import room
/// has a source listed (only then can input start an import). A destination that is not absolute
/// and normalised is left to [`super::refuse`], which refuses it without touching the disk.
fn draft_destination(host: &dyn Host) -> Option<(PathBuf, Vec<PathBuf>)> {
    let app = host.app();
    let s = app.session()?;
    s.import_session().source()?;
    let dest = crate::commands::import::place(app, &app.state().import.destination)?;
    paths::lexical(&dest, "destination").ok()?;
    Some((dest, s.roots().to_vec()))
}

/// `outcome`, with the Import room's destination checked on a worker first when it plays input and
/// the room could import; any other outcome as it is.
pub(super) fn before_input(host: &dyn Host, config: &Config, outcome: Outcome) -> Outcome {
    if !matches!(outcome, Outcome::Play(..)) {
        return outcome;
    }
    let Some((dest, watched)) = draft_destination(host) else { return outcome };
    let ticket = Ticket::park(outcome);
    let roots = config.effective_write_roots();
    Outcome::Work(Box::new(move || {
        let verdict = paths::check_output(&dest, &roots, &watched, paths::Output::Folder).map(|_| ()).map_err(|e| e.message);
        Next::Resume(Box::new(move |_| {
            record(dest, watched, verdict);
            ticket.take()
        }))
    }))
}

/// A parked outcome's claim; dropping it unclaimed (the worker's continuation never ran) frees the
/// slot when that happens on the UI thread.
struct Ticket(u64);

impl Ticket {
    fn park(outcome: Outcome) -> Ticket {
        let id = NEXT_TICKET.try_with(|n| {
            let id = n.get();
            n.set(id.wrapping_add(1));
            id
        });
        let id = id.unwrap_or(0);
        let _ = PARKED.try_with(|p| p.try_borrow_mut().map(|mut p| p.insert(id, outcome)));
        Ticket(id)
    }

    /// The parked outcome (on the UI thread, where it was parked).
    fn take(self) -> Outcome {
        let parked = PARKED.try_with(|p| p.try_borrow_mut().ok().and_then(|mut p| p.remove(&self.0))).ok().flatten();
        parked.unwrap_or_else(|| Outcome::Reply(Err(Error::new(Code::Internal, "the input waiting for the import destination's check was lost"))))
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let _ = PARKED.try_with(|p| p.try_borrow_mut().map(|mut p| p.remove(&self.0)));
    }
}
