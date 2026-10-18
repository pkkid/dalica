//! dalica-fetch: consented, resumable, hash-pinned downloads (`faces.md` §3.1, `security.md` §8.2).
//!
//! Layer L1; see `docs/architecture.md` §2. One file at a time, only when the user asked for it:
//!
//! - **Checked before anything is fetched.** A [`FileSpec`] (an https address, a local name that
//!   never comes from the address or the server, a size cap, an optional pinned size and SHA-256)
//!   is validated when it is made.
//! - **Consented.** Every download takes a [`Consent`], which only a caller holding the user's
//!   acknowledgement can make, for that one file. Without it nothing is requested or written.
//! - **Whatever the transport.** The caller chooses the [`Transport`] (a ranged GET); `fetch`
//!   builds every [`Request`], so no cookie, token or personal header is ever sent, and the
//!   User-Agent is generic ([`USER_AGENT`]).
//! - **Resumable and pinned.** Bytes go to `<name>.part` next to the final place (through
//!   `dalica-fs`); a pinned partial file resumes where it stopped; the exact size and the SHA-256
//!   are checked before the rename, and a file that fails them is deleted, never kept. A reply past
//!   the cap stops at the cap. Progress is reported as bytes arrive; cancellation takes effect
//!   between reads; the transport keeps the connect and stall timeouts of [`Options`].
//!
//! Runs on whatever thread calls [`download`] (the engine uses a background job; `dalica-fs`
//! refuses the UI thread).
//!
//! Ported from LightCraft `crates/fetch/src/lib.rs` (`download_url`, `fetch_file`, `verified`,
//! `sha256_file`, `parse_content_range`) at f34c7a8a (MIT OR Apache-2.0), without its HTTP code,
//! its mirrors and its bearer token: the transport is the caller's, one address per file, and no
//! credential is ever sent.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod http;
pub mod proxy;
pub mod spec;
pub mod transport;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dalica_fs::PackFile;
use dalica_fs::transfer::{self, Outcome};
use sha2::{Digest, Sha256};

pub use http::HttpsTransport;
pub use proxy::{Proxy, ProxyError};
pub use spec::{FileSpec, MAX_CAP, SpecError, Url};
pub use transport::{Body, HEADER_NAMES, Limits, MemoryTransport, Reply, Request, Seen, Served, Transport, TransportError, USER_AGENT};

/// The most attempts one download makes (each resumes where the last stopped).
pub const MAX_ATTEMPTS: u32 = 16;
/// The most bytes one read asks the transport for.
const READ_BUF: usize = 256 * 1024;

/// The user's acknowledgement of one file's terms (`faces.md` §3.1: Download stays disabled until
/// the box is ticked). It can only be made by [`Consent::acknowledged`] with `true`, it covers
/// exactly the spec it was made for, and it is neither `Clone` nor `Default`.
#[derive(Debug, PartialEq, Eq)]
pub struct Consent {
    spec: FileSpec,
}

impl Consent {
    /// The consent to download `spec`, made only when the user `acknowledged` its terms (the
    /// ticked box; over automation the caller's `acknowledged: true`, which agents never set on
    /// their own). `false` is refused.
    pub fn acknowledged(spec: &FileSpec, acknowledged: bool) -> Result<Consent, FetchError> {
        if acknowledged { Ok(Consent { spec: spec.clone() }) } else { Err(FetchError::NoConsent) }
    }

    /// Whether this consent covers `spec` (the same address, name, cap and pins).
    pub fn covers(&self, spec: &FileSpec) -> bool {
        self.spec == *spec
    }
}

/// Download settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Longest wait to connect.
    pub connect_timeout: Duration,
    /// Longest wait for the next bytes once connected.
    pub stall_timeout: Duration,
    /// Tries (each resumes where the last one stopped), 1 to [`MAX_ATTEMPTS`].
    pub attempts: u32,
}

impl Default for Options {
    fn default() -> Self {
        Options { connect_timeout: Duration::from_secs(15), stall_timeout: Duration::from_secs(30), attempts: 3 }
    }
}

/// Where a download is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Bytes on disk so far (a resumed part included).
    pub done: u64,
    /// The size expected: the pinned one, or the one the server announced.
    pub total: Option<u64>,
}

/// A finished download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Downloaded {
    pub path: PathBuf,
    pub size: u64,
    /// The file's SHA-256, lower-case hexadecimal.
    pub sha256: String,
    /// The byte the last request started at (0: from the start; more: a resumed part).
    pub resumed_from: u64,
    /// `false` when the file was already in place and matched its pins (nothing was requested).
    pub fetched: bool,
}

/// Why a download failed. Each message says what happened to the file.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// No consent, or a consent for another file: nothing was requested or written.
    #[error("the download needs the user's consent to the file's terms; nothing was downloaded")]
    NoConsent,
    /// Cancelled; a pinned partial file is kept to resume, any other is deleted.
    #[error("the download was cancelled")]
    Cancelled,
    /// A local file problem (folder not writable, disk full, a link in the way).
    #[error("{0}")]
    Disk(String),
    /// The transport failed (connect, stall, secure connection, a broken connection).
    #[error("{0}")]
    Transport(TransportError),
    /// The server answered a status that is not the file.
    #[error("the server answered HTTP {0}")]
    Status(u16),
    /// The server resumed at another place, or for another file; the partial file was deleted.
    #[error("the server resumed the download at the wrong place ({0}); the partial file was deleted")]
    Range(String),
    /// The file has another size than the pinned one; it was deleted.
    #[error("the downloaded file is {got} bytes, not the expected {expected}; it was deleted")]
    WrongSize { expected: u64, got: u64 },
    /// The server sent an empty file; it was deleted.
    #[error("the server sent an empty file; it was deleted")]
    Empty,
    /// The server sent (or announced) more than the cap; the download stopped there and the
    /// partial file was deleted.
    #[error("the server sent more than the expected {limit} bytes; the download was stopped and deleted")]
    TooLarge { limit: u64 },
    /// The file's SHA-256 is not the pinned one; it was deleted.
    #[error("the downloaded file is damaged or not the expected one (SHA-256 {got}, expected {expected}); it was deleted")]
    WrongHash { expected: String, got: String },
    /// The download ended early; the pinned partial file is kept to resume.
    #[error("the download stopped at {have} of {size} bytes; it resumes from there next time")]
    Incomplete { have: u64, size: u64 },
    /// Another file already has the name (one that does not match the pins, a folder or a link);
    /// nothing was requested or replaced.
    #[error("{} already exists and is not the expected file; nothing was replaced", .0.display())]
    Exists(PathBuf),
}

impl From<dalica_fs::Error> for FetchError {
    fn from(e: dalica_fs::Error) -> Self {
        FetchError::Disk(e.to_string())
    }
}

/// One attempt's failure: try again (resuming), or stop.
enum Step {
    Retry(FetchError),
    Stop(FetchError),
}

impl From<dalica_fs::Error> for Step {
    fn from(e: dalica_fs::Error) -> Self {
        Step::Stop(e.into())
    }
}

/// Downloads `spec` into `dir` over `transport`, with the user's `consent` for it. A file already
/// in place that matches the pins is kept (nothing is requested); any other file of that name is
/// refused, never replaced. `progress` is called as bytes arrive (often: throttle in it); raising
/// `cancel` stops the download before its next read.
pub fn download(
    spec: &FileSpec,
    consent: &Consent,
    dir: &Path,
    transport: &dyn Transport,
    opts: &Options,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Downloaded, FetchError> {
    if !consent.covers(spec) {
        return Err(FetchError::NoConsent);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(FetchError::Cancelled);
    }
    dalica_fs::create_dir_all(dir)?;
    let target = dir.join(spec.name());
    if let Some(done) = already_there(spec, &target)? {
        return Ok(done);
    }
    let limits = Limits { connect: opts.connect_timeout, stall: opts.stall_timeout, cancel };
    let mut last = FetchError::Cancelled;
    for _ in 0..opts.attempts.clamp(1, MAX_ATTEMPTS) {
        match attempt(spec, dir, transport, &limits, progress) {
            Ok(done) => return Ok(done),
            Err(Step::Stop(e)) => return Err(e),
            Err(Step::Retry(e)) => last = e,
        }
    }
    Err(last)
}

/// The file at `target` when it is already the right one; `Exists` when something else has the
/// name; `None` when the name is free.
fn already_there(spec: &FileSpec, target: &Path) -> Result<Option<Downloaded>, FetchError> {
    let meta = match std::fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(FetchError::Disk(format!("{}: {e}", target.display()))),
    };
    if !meta.is_file() || meta.len() == 0 || meta.len() > spec.limit() || spec.size().is_some_and(|s| s != meta.len()) {
        return Err(FetchError::Exists(target.to_path_buf()));
    }
    let got = sha256_file(target, spec.limit())?;
    if spec.sha256().is_some_and(|want| *want != got) {
        return Err(FetchError::Exists(target.to_path_buf()));
    }
    Ok(Some(Downloaded { path: target.to_path_buf(), size: meta.len(), sha256: spec::hex(&got), resumed_from: 0, fetched: false }))
}

/// Whether `name` is a partial download's name (the only kind of file `fetch` deletes).
fn part_shape(name: &str) -> bool {
    name.ends_with(spec::PART_SUFFIX)
}

/// Deletes the partial download at `part`, if any.
fn remove_part(part: &Path) -> Result<(), FetchError> {
    dalica_fs::remove_shaped(part, part_shape)?;
    Ok(())
}

/// One try: open (or resume) `<name>.part`, fetch the rest, check it, move it into place.
fn attempt(
    spec: &FileSpec,
    dir: &Path,
    transport: &dyn Transport,
    limits: &Limits<'_>,
    progress: &mut dyn FnMut(&Progress),
) -> Result<Downloaded, Step> {
    let part_name = spec.part_name();
    let part = dir.join(&part_name);
    let limit = spec.limit();
    // Resume only a plain partial file of a resumable spec that fits; delete any other one. A
    // link or a folder at the name is refused (`remove_shaped` and `PackFile::open` never follow one).
    match std::fs::symlink_metadata(&part) {
        Ok(m) if m.is_file() && spec.resumable() && m.len() <= limit => {}
        Ok(_) => remove_part(&part).map_err(Step::Stop)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Step::Stop(FetchError::Disk(format!("{}: {e}", part.display())))),
    }
    let pack = PackFile::open(&part)?;
    let resumed_from = pack.len();
    let have = if spec.size() == Some(resumed_from) { resumed_from } else { receive(spec, &part, pack, transport, limits, progress)? };
    check_and_place(spec, dir, &part_name, have, resumed_from)
}

/// Requests the bytes after what `pack` holds and appends them, within the limit; answers the
/// part's length. The part is closed before it is deleted or checked.
fn receive(
    spec: &FileSpec,
    part: &Path,
    mut pack: PackFile,
    transport: &dyn Transport,
    limits: &Limits<'_>,
    progress: &mut dyn FnMut(&Progress),
) -> Result<u64, Step> {
    let limit = spec.limit();
    let from = pack.len();
    let request = Request::new(spec.url().clone(), from);
    let reply = transport.get(&request, limits).map_err(transport_step)?;
    match reply.status {
        200 => {
            if let Some(n) = reply.length {
                if n > limit {
                    drop(pack);
                    remove_part(part).map_err(Step::Stop)?;
                    return Err(Step::Stop(FetchError::TooLarge { limit }));
                }
                if let Some(size) = spec.size()
                    && size != n
                {
                    drop(pack);
                    remove_part(part).map_err(Step::Stop)?;
                    return Err(Step::Stop(FetchError::WrongSize { expected: size, got: n }));
                }
            }
            // The whole file, from the start (also when a range was asked for).
            pack.truncate(0)?;
        }
        206 if from > 0 => {
            let range = reply.content_range.clone().unwrap_or_default();
            let fits = parse_content_range(&range)
                .is_some_and(|(start, total)| start == from && total.is_none_or(|t| t <= limit && spec.size().is_none_or(|s| s == t)));
            if !fits {
                drop(pack);
                remove_part(part).map_err(Step::Stop)?;
                return Err(Step::Stop(FetchError::Range(range)));
            }
        }
        416 => {
            // The partial file does not fit what the server has: start over.
            drop(pack);
            remove_part(part).map_err(Step::Stop)?;
            return Err(Step::Retry(FetchError::Range("HTTP 416".into())));
        }
        s if s >= 500 => return Err(Step::Retry(FetchError::Status(s))),
        s => return Err(Step::Stop(FetchError::Status(s))),
    }
    let mut body = reply.body;
    let mut have = pack.len();
    let total = spec.size().or_else(|| reply.length.map(|n| n.saturating_add(have)));
    progress(&Progress { done: have, total });
    let mut buf = vec![0u8; READ_BUF];
    loop {
        if limits.cancel.load(Ordering::Relaxed) {
            return Err(stopped(spec, part, pack, Step::Stop(FetchError::Cancelled)));
        }
        // One byte past the limit is enough to know the reply is too long.
        let room = limit.saturating_sub(have).saturating_add(1);
        let want = usize::try_from(room).unwrap_or(READ_BUF).min(READ_BUF);
        let n = match body.read(buf.get_mut(..want).unwrap_or_default(), limits) {
            Ok(n) => n,
            Err(e) => return Err(stopped(spec, part, pack, transport_step(e))),
        };
        if n == 0 {
            break;
        }
        let n64 = u64::try_from(n).unwrap_or(u64::MAX);
        if have.saturating_add(n64) > limit {
            drop(pack);
            remove_part(part).map_err(Step::Stop)?;
            return Err(Step::Stop(FetchError::TooLarge { limit }));
        }
        pack.append(buf.get(..n).unwrap_or_default())?;
        have = pack.len();
        progress(&Progress { done: have, total });
    }
    pack.sync()?;
    Ok(have)
}

/// After a cancel or a broken connection: a resumable part is synced and kept, any other closed
/// and deleted. Answers `step`, or the disk error that kept the part from being handled.
fn stopped(spec: &FileSpec, part: &Path, pack: PackFile, step: Step) -> Step {
    let handled = if spec.resumable() {
        pack.sync().map_err(FetchError::from)
    } else {
        drop(pack);
        remove_part(part)
    };
    match handled {
        Ok(()) => step,
        Err(disk) => Step::Stop(disk),
    }
}

/// Size, emptiness and hash of the finished part; then the rename into place, never replacing.
fn check_and_place(spec: &FileSpec, dir: &Path, part_name: &str, have: u64, resumed_from: u64) -> Result<Downloaded, Step> {
    let part = dir.join(part_name);
    if let Some(size) = spec.size()
        && have != size
    {
        // Shorter than pinned (the limit keeps it from being longer): resume next time.
        return Err(Step::Retry(FetchError::Incomplete { have, size }));
    }
    if have == 0 {
        remove_part(&part).map_err(Step::Stop)?;
        return Err(Step::Stop(FetchError::Empty));
    }
    let got = sha256_file(&part, spec.limit()).map_err(Step::Stop)?;
    if let Some(want) = spec.sha256()
        && *want != got
    {
        remove_part(&part).map_err(Step::Stop)?;
        let e = FetchError::WrongHash { expected: spec::hex(want), got: spec::hex(&got) };
        // Bytes kept from an earlier run may be the damaged ones: one more try from the start.
        return Err(if resumed_from > 0 { Step::Retry(e) } else { Step::Stop(e) });
    }
    let Some(identity) = dalica_fs::identity(&part)? else {
        return Err(Step::Retry(FetchError::Disk(format!("{} vanished before it was moved into place", part.display()))));
    };
    match transfer::move_file(dir, part_name, identity, dir, spec.name())? {
        Outcome::Done(_) => Ok(Downloaded { path: dir.join(spec.name()), size: have, sha256: spec::hex(&got), resumed_from, fetched: true }),
        Outcome::Exists => Err(Step::Stop(FetchError::Exists(dir.join(spec.name())))),
    }
}

fn transport_step(e: TransportError) -> Step {
    match e {
        TransportError::Cancelled => Step::Stop(FetchError::Cancelled),
        e if e.retryable() => Step::Retry(FetchError::Transport(e)),
        e => Step::Stop(FetchError::Transport(e)),
    }
}

/// The SHA-256 of the file at `path`, reading at most `limit` bytes (a longer file is refused).
fn sha256_file(path: &Path, limit: u64) -> Result<[u8; 32], FetchError> {
    let disk = |e: std::io::Error| FetchError::Disk(format!("{}: {e}", path.display()));
    let mut file = std::fs::File::open(path).map_err(disk)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut read = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(disk)?;
        if n == 0 {
            break;
        }
        read = read.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
        if read > limit {
            return Err(FetchError::Disk(format!("{} grew past {limit} bytes while it was checked", path.display())));
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
    Ok(hasher.finalize().into())
}

/// `bytes <first>-<last>/<total>` → (first, total); a `*` total → `None`.
fn parse_content_range(v: &str) -> Option<(u64, Option<u64>)> {
    let v = v.trim().strip_prefix("bytes")?.trim();
    let (range, total) = v.split_once('/')?;
    let (a, b) = range.split_once('-')?;
    let (a, b) = (a.trim().parse::<u64>().ok()?, b.trim().parse::<u64>().ok()?);
    if b < a {
        return None;
    }
    let total = match total.trim() {
        "*" => None,
        t => {
            let t = t.parse::<u64>().ok()?;
            if t <= b {
                return None;
            }
            Some(t)
        }
    };
    Some((a, total))
}
