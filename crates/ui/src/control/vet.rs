//! The folders the app's own buttons write into, checked with their links resolved before
//! automation's input can press them (`control-protocol.md` §5.7, `security.md` §5.7).
//!
//! A click, a key or a pointer event sent over the control channel can press Import All or Enter
//! in the Import room, which runs `import.run` into the destination strip's folder, or Create
//! Collage, Save Draft or the close question's Save Draft in the Collage room, which write into
//! `Session::collage_destination`, or Save Frame as Picture in the edit view's video tab, which
//! writes into `Session::video_frames_dir`. The app asks [`super::refuse_folder`] first, on the UI thread,
//! where nothing may touch the disk: the folder can only be compared as written there. So before a
//! request (or a batch step) plays input while a room could write, [`before_input`] resolves its
//! folder through its links on a worker, as `engine.execute` resolves a command's path, and keeps
//! the verdict for the request; [`super::refuse_folder`] then answers from it. A folder it was not
//! given (the draft changed while the input played) is refused, never let through unchecked.
//!
//! The input outcome waits on the UI thread (it is not `Send`) while the worker checks: it is
//! parked in a thread-local slot under a ticket the worker's continuation takes back.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::host::Host;
use super::{Code, Config, Error, Next, Outcome, Resume, paths};

/// A folder checked for the request running now, and what the check said.
struct Vetted {
    dest: PathBuf,
    watched: Vec<PathBuf>,
    verdict: Result<(), String>,
}

thread_local! {
    /// The verdicts for the request running on this thread (the UI thread): at most one per room.
    static VETTED: RefCell<Vec<Vetted>> = const { RefCell::new(Vec::new()) };
    /// Outcomes waiting for their destination's check, by ticket.
    static PARKED: RefCell<HashMap<u64, Outcome>> = RefCell::new(HashMap::new());
    static NEXT_TICKET: Cell<u64> = const { Cell::new(0) };
}

/// The most verdicts kept for one request (one per room that writes; the oldest go first).
const MAX_VETTED: usize = 4;

/// Forgets the last request's verdicts: a new request checks again (the disk may have changed).
pub(super) fn forget() {
    let _ = VETTED.try_with(|v| v.try_borrow_mut().map(|mut v| v.clear()));
}

/// Keeps `verdict` for `dest` as checked against `watched`, for [`verdict`].
pub(super) fn record(dest: PathBuf, watched: Vec<PathBuf>, verdict: Result<(), String>) {
    let _ = VETTED.try_with(|v| {
        v.try_borrow_mut().map(|mut v| {
            v.retain(|k| k.dest != dest || k.watched != watched);
            if v.len() >= MAX_VETTED {
                v.remove(0);
            }
            v.push(Vetted { dest, watched, verdict });
        })
    });
}

/// What the check said about `dest` against `watched` in this request; `None` when it was not
/// checked (or checked against other watched folders).
pub(super) fn verdict(dest: &Path, watched: &[PathBuf]) -> Option<Result<(), String>> {
    VETTED
        .try_with(|v| v.try_borrow().ok().and_then(|v| v.iter().find(|v| v.dest == dest && v.watched == watched).map(|v| v.verdict.clone())))
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

/// The folder Create Collage and Save Draft would write into now (`Session::collage_destination`:
/// beside the collage the room reopened, else the Collages folder), and the watched folders, while
/// the Collage room shows a collage. A folder that is not absolute and normalised is left to
/// [`super::refuse_folder`].
fn collage_destination(host: &dyn Host) -> Option<(PathBuf, Vec<PathBuf>)> {
    let app = host.app();
    crate::commands::collage::collage_open(app, &serde_json::Value::Null).ok()?;
    let s = app.session()?;
    let dest = s.collage_destination(None).ok()?;
    paths::lexical(&dest, "path").ok()?;
    Some((dest, s.roots().to_vec()))
}

/// The folder Save Frame as Picture would write into now (`Session::video_frames_dir`), and the
/// watched folders, while the edit view shows a video (its video tab's Save Frame can be pressed).
fn video_frames_destination(host: &dyn Host) -> Option<(PathBuf, Vec<PathBuf>)> {
    let app = host.app();
    crate::commands::video::shown(app)?;
    let s = app.session()?;
    let dest = s.video_frames_dir()?;
    paths::lexical(&dest, "path").ok()?;
    Some((dest, s.roots().to_vec()))
}

/// `outcome`, with the folders the Import and Collage rooms and the video tab would write into
/// checked on a worker first when it plays input and one could write; any other outcome as it is.
pub(super) fn before_input(host: &dyn Host, config: &Config, outcome: Outcome) -> Outcome {
    if !matches!(outcome, Outcome::Play(..)) {
        return outcome;
    }
    let folders: Vec<_> = draft_destination(host).into_iter().chain(collage_destination(host)).chain(video_frames_destination(host)).collect();
    if folders.is_empty() {
        return outcome;
    }
    let ticket = Ticket::park(outcome);
    let roots = config.effective_write_roots();
    Outcome::Work(Box::new(move || {
        let checked: Vec<_> = folders
            .into_iter()
            .map(|(dest, watched)| {
                let verdict = paths::check_output(&dest, &roots, &watched, paths::Output::Folder).map(|_| ()).map_err(|e| e.message);
                (dest, watched, verdict)
            })
            .collect();
        Next::Resume(Box::new(move |_| {
            for (dest, watched, verdict) in checked {
                record(dest, watched, verdict);
            }
            ticket.take()
        }))
    }))
}

/// `then`, run once the Collage room's folder is checked on a worker, as `engine.execute` checks
/// `collage.create`'s: a folder outside the write roots and the watched folders is the reply
/// (`path_denied`) and `then` never runs; a folder that passes is kept for
/// [`super::refuse_folder`]. While the room shows no collage, `then` runs at once.
pub(super) fn collage_first(host: &mut dyn Host, config: &Config, then: Resume) -> Outcome {
    let Some((dest, watched)) = collage_destination(host) else { return then(host) };
    let roots = config.effective_write_roots();
    Outcome::Work(Box::new(move || match paths::check_output(&dest, &roots, &watched, paths::Output::Folder) {
        Err(e) => Next::Reply(Err(e)),
        Ok(_) => Next::Resume(Box::new(move |host| {
            record(dest, watched, Ok(()));
            then(host)
        })),
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
