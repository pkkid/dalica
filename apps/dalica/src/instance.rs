//! One Dalica per user (`viewing.md` §3.4): a later launch hands its files to the running Dalica,
//! which shows them in its photo viewer, instead of starting a second copy.
//!
//! Adapted from PhotoCraft `apps/photocraft/src/single_instance.rs` @ d95fdf31 (MIT OR Apache-2.0).
//!
//! - **Who takes part.** Only a launch on the per-user settings folder (the platform's library
//!   folder: no `--library`, `--demo`, `DALICA_HOME` or portable copy, any of which may be shared
//!   between users or meant as a separate Dalica) whose file names fit the hand-off ([`skip_reason`]).
//! - **The first launch** takes an exclusive OS lock on [`LOCK_FILE`] in that folder (the OS drops
//!   it when the process ends, crash included), listens on 127.0.0.1 only and writes the port and a
//!   fresh 256-bit token to [`INFO_FILE`] (only the user may read it, written through `fs`).
//! - **A later launch** that finds the lock taken sends its paths with the token in one request
//!   (at most [`MAX_PATHS`] paths of at most [`MAX_PATH_BYTES`] bytes, one line of at most
//!   [`MAX_REQUEST`] bytes, within [`Timing::request`]), and exits once the first says it took
//!   them. The first answers `ok` only once its UI has taken the paths ([`Handoffs::take`]): a
//!   launch that hears anything else (the first is still opening, hung, or quitting) keeps trying
//!   the lock, and when its time runs out starts on its own; a file is never acknowledged and then
//!   dropped. [`Primary::shutdown`] (or dropping it) refuses new files and frees the lock.
//! - **Security** (`security.md`, M15.36): loopback only; the token is compared in constant time;
//!   a request without it, too long, too slow, with more paths than allowed, or with a path that is
//!   not absolute is refused whole and nothing opens; at most [`MAX_CLIENTS`] launches are served
//!   at once, each on its own thread with its own deadline. The token file is `0600` on Unix and
//!   keeps the per-user ACL of the settings folder on Windows, so another user's process cannot
//!   hand files in.
//!
//! The running Dalica's side, which opens the viewer and returns the window to where it was, is
//! [`receive`]; macOS's open-documents events reach it through [`apple`].

#[cfg(target_os = "macos")]
pub mod apple;
pub mod receive;
mod wire;

use std::fs::File;
use std::io::{BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

pub use receive::Receiver;
pub use wire::{INFO_FILE, MAX_PATH_BYTES, MAX_PATHS, MAX_REQUEST};

/// The lock file in the settings folder; it stays (removing it would race with the next launch).
pub const LOCK_FILE: &str = "instance.lock";
/// Launches served at once; a stalled one holds only its own slot, never the next launch.
pub const MAX_CLIENTS: usize = 8;
/// Kept out of the hand-off's budget, so the first answers before the later launch gives up.
const REPLY_MARGIN: Duration = Duration::from_millis(500);
/// Between two tries of the lock and the running Dalica.
const RETRY: Duration = Duration::from_millis(100);

/// How long each side waits (shortened in tests).
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    /// A later launch's whole budget for handing its paths over (or taking the lock over).
    pub handoff: Duration,
    /// How long the first waits for its UI to take a launch's paths before refusing them.
    pub ack: Duration,
    /// The time a connection gets to send its request line.
    pub request: Duration,
}

/// Generous enough for a first Dalica that is still opening its library.
pub const TIMING: Timing = Timing { handoff: Duration::from_secs(10), ack: Duration::from_secs(8), request: Duration::from_secs(5) };

/// What [`claim`] decided.
pub enum Claim {
    /// This process is the running Dalica: keep the [`Primary`] until the app ends.
    Primary(Primary),
    /// The running Dalica took the paths: this process exits.
    Forwarded,
    /// Neither (the reason given): this process runs on its own, as without the hand-off.
    Standalone(String),
}

/// Why this launch runs on its own without trying the hand-off, or `None` to take part.
/// `per_user`: the library is the platform's per-user one (see the module docs); `paths`: the
/// files it hands over.
pub fn skip_reason(per_user: bool, paths: &[PathBuf]) -> Option<String> {
    if !per_user {
        return Some("the settings folder is not the per-user one (--library, --demo, DALICA_HOME or a portable copy)".into());
    }
    if paths.len() > MAX_PATHS {
        return Some(format!("more than {MAX_PATHS} files"));
    }
    for p in paths {
        let Some(s) = p.to_str() else { return Some("a file name is not valid Unicode".into()) };
        if s.len() > MAX_PATH_BYTES {
            return Some(format!("a file name is longer than {MAX_PATH_BYTES} bytes"));
        }
        if !p.is_absolute() {
            return Some("a file name is not absolute".into());
        }
    }
    None
}

/// The running Dalica: holds the lock and takes other launches' paths.
pub struct Primary {
    lock: Mutex<Option<File>>,
    shared: Arc<Shared>,
}

/// What the listener's threads and the UI share.
struct Shared {
    inbox: Mutex<Inbox>,
    /// Set by [`Primary::shutdown`]: new connections are dropped, waiting ones refused.
    closed: AtomicBool,
}

/// Wakes the UI when a launch's paths arrive.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct Inbox {
    pending: Vec<Pending>,
    wake: Option<Wake>,
    next_id: u64,
}

/// Paths waiting for the UI; `taken` tells the connection that sent them.
struct Pending {
    id: u64,
    paths: Vec<PathBuf>,
    taken: SyncSender<()>,
}

impl Primary {
    /// The queue the UI drains every frame; `wake` is called when a launch's paths arrive.
    pub fn connect(&self, wake: Option<Wake>) -> Handoffs {
        let mut inbox = self.shared.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(w) = &wake
            && !inbox.pending.is_empty()
        {
            w();
        }
        inbox.wake = wake;
        Handoffs { shared: Arc::clone(&self.shared) }
    }

    /// Stops taking files and releases the lock: launches waiting on this one are refused and take
    /// the lock over themselves. Called once the app has ended (and on drop).
    pub fn shutdown(&self) {
        self.shared.closed.store(true, Ordering::Release);
        // Dropping the senders refuses every connection still waiting for the UI.
        self.shared.inbox.lock().unwrap_or_else(PoisonError::into_inner).pending.clear();
        drop(self.lock.lock().unwrap_or_else(PoisonError::into_inner).take());
    }
}

impl Drop for Primary {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The UI's end of the hand-off: each later launch's paths, in the order they arrived.
pub struct Handoffs {
    shared: Arc<Shared>,
}

impl Handoffs {
    /// Everything queued, acknowledged to the senders while the inbox is locked (see
    /// [`Shared::hand_over`]). An empty list is a launch without files: bring the window forward.
    pub fn take(&self) -> Vec<Vec<PathBuf>> {
        let mut inbox = self.shared.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        inbox
            .pending
            .drain(..)
            .map(|p| {
                let _ = p.taken.try_send(());
                p.paths
            })
            .collect()
    }
}

impl Shared {
    /// Queues `paths` for the UI and waits up to `wait` for it to take them. False (and nothing
    /// left queued) when it did not: the sender then retries or starts on its own.
    fn hand_over(&self, paths: Vec<PathBuf>, wait: Duration) -> bool {
        let (taken, rx) = sync_channel(1);
        let id = {
            let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) {
                return false;
            }
            let id = inbox.next_id;
            inbox.next_id = id.wrapping_add(1);
            inbox.pending.push(Pending { id, paths, taken });
            if let Some(wake) = &inbox.wake {
                wake();
            }
            id
        };
        if rx.recv_timeout(wait).is_ok() {
            return true;
        }
        // Timed out or refused: withdrawn, unless the UI took them meanwhile (it acknowledges while
        // holding the lock, so the answer is settled here).
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        if rx.try_recv().is_ok() {
            return true;
        }
        inbox.pending.retain(|p| p.id != id);
        false
    }
}

/// Becomes the running Dalica of the settings folder `dir`, or hands `paths` (absolute; see
/// [`skip_reason`]) to the one that is.
pub fn claim(dir: &Path, paths: &[PathBuf]) -> Claim {
    claim_with(dir, paths, TIMING)
}

/// [`claim`] with explicit timeouts. Until `timing.handoff` runs out, each round tries the lock
/// (free once a quitting or crashed Dalica is gone) and then the running one.
pub fn claim_with(dir: &Path, paths: &[PathBuf], timing: Timing) -> Claim {
    let lock = match open_lock(dir) {
        Ok(lock) => lock,
        Err(e) => return Claim::Standalone(e),
    };
    let request = match wire::Request::new(paths) {
        Ok(r) => r,
        Err(e) => return Claim::Standalone(e),
    };
    let deadline = Instant::now() + timing.handoff;
    let mut last = format!("no {INFO_FILE}");
    loop {
        match lock.try_lock() {
            Ok(()) => {
                return match serve(dir, lock, timing) {
                    Ok(primary) => Claim::Primary(primary),
                    Err(e) => Claim::Standalone(e),
                };
            }
            Err(std::fs::TryLockError::WouldBlock) => {}
            Err(std::fs::TryLockError::Error(e)) => return Claim::Standalone(format!("{}: {e}", dir.join(LOCK_FILE).display())),
        }
        match wire::read_info(dir) {
            Some((port, token)) => match send(port, &token, &request, deadline) {
                Ok(()) => return Claim::Forwarded,
                Err(e) => last = e,
            },
            None => last = format!("no readable {INFO_FILE}"),
        }
        if Instant::now() >= deadline {
            return Claim::Standalone(format!("the running Dalica did not take the files ({last})"));
        }
        std::thread::sleep(RETRY);
    }
}

/// The lock file of `dir`, made if missing (through `fs`, never replacing one that appeared) and
/// opened for reading: an OS lock needs no write access.
fn open_lock(dir: &Path) -> Result<File, String> {
    dalica_fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(LOCK_FILE);
    match std::fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() => return Err(format!("{} is not a plain file", path.display())),
        Ok(_) => {}
        Err(_) => match dalica_fs::exact_file(dir, LOCK_FILE, b"") {
            // Made by a launch started at the same moment: the same file.
            Ok(_) | Err(dalica_fs::Error::Exists(_)) => {}
            Err(e) => return Err(e.to_string()),
        },
    }
    File::open(&path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Listens on a loopback port, writes the port and a fresh token to the info file and serves the
/// later launches on threads of their own.
fn serve(dir: &Path, lock: File, timing: Timing) -> Result<Primary, String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("loopback listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("loopback listener: {e}"))?.port();
    let token = wire::Secret::generate()?;
    wire::write_info(dir, port, &token)?;
    let shared = Arc::new(Shared { inbox: Mutex::default(), closed: AtomicBool::new(false) });
    let listening = Arc::clone(&shared);
    let spawned = std::thread::Builder::new().name("dalica-instance".into()).spawn(move || {
        let token = Arc::new(token);
        let clients = Arc::new(AtomicUsize::new(0));
        loop {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                // Back off instead of spinning when accept keeps failing (out of descriptors).
                Err(_) => {
                    std::thread::sleep(RETRY);
                    continue;
                }
            };
            // Closing: the sender sees the connection drop and takes the lock over.
            if listening.closed.load(Ordering::Acquire) {
                continue;
            }
            // Past the cap a connection is dropped and that launch tries again.
            if clients.fetch_add(1, Ordering::AcqRel) >= MAX_CLIENTS {
                clients.fetch_sub(1, Ordering::AcqRel);
                continue;
            }
            let (token, shared, slot) = (Arc::clone(&token), Arc::clone(&listening), Arc::clone(&clients));
            let spawned = std::thread::Builder::new().name("dalica-instance-client".into()).spawn(move || {
                serve_client(stream, &token, &shared, timing);
                slot.fetch_sub(1, Ordering::AcqRel);
            });
            if spawned.is_err() {
                clients.fetch_sub(1, Ordering::AcqRel);
            }
        }
    });
    spawned.map_err(|e| format!("the hand-off listener thread: {e}"))?;
    Ok(Primary { lock: Mutex::new(Some(lock)), shared })
}

/// Reads one request, hands its paths to the UI and answers `ok` only if the UI took them.
fn serve_client(stream: TcpStream, token: &wire::Secret, shared: &Shared, timing: Timing) {
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let deadline = Instant::now() + timing.request;
    let line = match crate::control::transport::read_line(&mut reader, MAX_REQUEST, timing.request, Some(deadline)) {
        Ok(Some(line)) => line,
        // Too long, too slow or gone: refused, and the connection closes.
        Ok(None) | Err(_) => return reply(stream, timing, false),
    };
    let taken = match wire::parse_request(&line, token) {
        Ok((paths, wait)) => shared.hand_over(paths, wait.min(timing.ack)),
        Err(why) => {
            log::warn!("dalica: a hand-off request was refused: {why}");
            false
        }
    };
    reply(stream, timing, taken);
}

fn reply(mut stream: TcpStream, timing: Timing, taken: bool) {
    let _ = stream.set_write_timeout(Some(timing.request));
    let _ = writeln!(stream, "{}", wire::reply(taken)).and_then(|()| stream.flush());
}

/// Offers the request to the running Dalica on `port`; `Ok` once its UI took the paths.
fn send(port: u16, token: &str, request: &wire::Request, deadline: Instant) -> Result<(), String> {
    let left = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero()).ok_or("out of time")?;
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&addr, left).map_err(|e| e.to_string())?;
    // The first must answer before this deadline, so it waits a little less.
    let wait = left.saturating_sub(REPLY_MARGIN);
    stream.set_write_timeout(Some(left)).map_err(|e| e.to_string())?;
    writeln!(stream, "{}", request.line(token, wait)).and_then(|()| stream.flush()).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    match crate::control::transport::read_line(&mut reader, wire::MAX_REPLY, left, Some(deadline)) {
        Ok(Some(line)) if wire::is_taken(&line) => Ok(()),
        Ok(Some(_)) => Err("not taken".into()),
        Ok(None) => Err("no reply".into()),
        Err(e) => Err(format!("{e:?}")),
    }
}

#[cfg(test)]
mod tests;
