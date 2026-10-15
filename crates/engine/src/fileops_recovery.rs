//! Repairing a file operation that was interrupted (`data-model.md` §8 and §10.5): every move, copy,
//! rename and delete is entered in the file-operation journal before anything moves and leaves it
//! once its results are in the library (`fileops.rs`), so an entry found when the library opens
//! means an operation stopped half-way (a crash, a power cut, the session closed while it ran, or a
//! carry that could not finish).
//!
//! - **When.** At open, after the journal was read ([`Session::file_ops_left`]): the operations it
//!   holds are repaired by one file job ([`LABEL`], [`JobKind::Files`]) on a worker thread, one
//!   item per operation. The owner thread reads nothing.
//! - **Looking** (on the worker): for each item, what is at its `from` and `to` (a side in an
//!   offline folder, or one that cannot be looked at, is unknown), and when both are there whether
//!   `to` is a complete copy of `from` (byte for byte; a folder file by file). A name that a volume
//!   ignoring case resolves to the other end's file counts only where its folder lists that exact
//!   spelling, so a file is never taken for its own copy; two hard links of one file are a copy of
//!   each other.
//! - **Deciding** through `store`'s [`intents::plan`] rules (`data-model.md` §8's table). An undone
//!   copy (the copy to the trash) is judged as a trash. An item that names one folder at both ends
//!   (Move to New Folder's new folder) has nothing to repair.
//! - **Rolling forward:** a cross-volume move whose verified copy is in place has its source sent
//!   to the OS trash (`transfer::finish_move`; a folder's tree through `trash::send`; a volume
//!   without a trash keeps the source, and the move became a copy); then a photo's originals backup
//!   with its record ([`carry::backups`]) and its sidecar entry ([`carry::entries`]) are carried,
//!   which is harmless when they were carried before (as a copy when the source stays). Rolling
//!   back changes nothing on disk: a partial copy has a temporary name, which the stale-temporary
//!   sweep removes.
//! - **The library follows** on the owner thread, as a change from the files: a moved or renamed
//!   photo the library still shows at its old place moves there with its id and data
//!   (`Op::MovePhotos`), a moved folder likewise (`Op::MoveFolder`, its watched-folder entries
//!   rebased), a trashed photo or folder leaves it; any other end the library shows otherwise than
//!   the disk (a copy, a photo put back) has its folder rescanned when the job ends, which reads
//!   its carried entry.
//! - **Leaving the journal.** An operation leaves the journal once its items are repaired and the
//!   library's ops were sent to the I/O worker (which journals them first). One that waits (a side
//!   unknown, a source that could not go to the trash) or whose carry failed with an I/O error
//!   stays for the next open; a carry refused for good (a damaged sidecar kept aside, a read-only
//!   or vanished folder, a name taken) is reported and not tried again.
//! - **Reporting.** Each item's repair is logged, kept in [`Session::scan_warnings`] (`ui.inspect`
//!   → `notices`) and in [`Session::file_op_repairs`] (`library.inspect` → `fileOpRepairs` once
//!   that method is built), and the job's end is a notice like any file job's.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_fs::Identity;
use dalica_fs::transfer::{self, Finished};
use dalica_fs::trash::{self, Sent};
use dalica_library::Op;
use dalica_store::carry::{self, Carry, CarryError, Mode};
use dalica_store::intents::{self, Action, Facts, Intent, Kind, Seen};
use serde::Serialize;

use crate::filejobs::{Applied, Done, FileItem, FileJob, Notice, Tally};
use crate::jobs::JobKind;
use crate::session::Session;

/// The repair job's label: its progress reads "Repairing interrupted file operations 1 of 2".
pub const LABEL: &str = "Repairing interrupted file operations";
/// The most repairs [`Session::file_op_repairs`] keeps; the oldest go first.
pub const MAX_REPAIRS: usize = 1024;
/// The bytes compared at a time when checking a copy.
const CHUNK: usize = 1 << 20;
/// How many times a carry that failed with an I/O error is tried before its operation waits.
const CARRY_TRIES: usize = 3;

/// How one item of an interrupted operation was repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RepairOutcome {
    /// Rolled back: the item did not happen; nothing changed on disk.
    Back,
    /// Rolled forward: finished (the source to the trash, the entry carried).
    Forward,
    /// Neither end holds the file: nothing to do.
    Lost,
    /// Nothing to repair (an item naming one folder at both ends).
    Nothing,
    /// A side could not be looked at, or finishing failed: the operation stays in the journal for
    /// the next open.
    Waits,
}

/// One repaired item, as [`Session::file_op_repairs`] lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileOpRepair {
    /// The operation's journal id.
    pub op: u64,
    pub kind: Kind,
    pub direction: intents::Direction,
    pub from: PathBuf,
    pub to: Option<PathBuf>,
    pub outcome: RepairOutcome,
    /// Why, in words: what the disk showed and what was done.
    pub why: String,
}

impl FileOpRepair {
    /// The line the log and the notices give.
    fn line(&self) -> String {
        let what = match self.outcome {
            RepairOutcome::Back => "rolled back",
            RepairOutcome::Forward => "finished",
            RepairOutcome::Lost => "lost",
            RepairOutcome::Nothing => "nothing to repair",
            RepairOutcome::Waits => "waits for the next start",
        };
        let to = self.to.as_ref().map(|t| format!(" → {}", t.display())).unwrap_or_default();
        format!("interrupted file operation {} ({:?}): {}{to}: {what}: {}", self.op, self.kind, self.from.display(), self.why)
    }
}

/// What is at one end of an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum At {
    Absent,
    File(Identity),
    Folder(Identity),
    /// It could not be looked at (an offline folder, an I/O error, a link).
    Unknown,
}

impl At {
    fn seen(self) -> Seen {
        match self {
            At::Absent => Seen::Absent,
            At::File(_) | At::Folder(_) => Seen::Present,
            At::Unknown => Seen::Unknown,
        }
    }

    fn identity(self) -> Option<Identity> {
        match self {
            At::File(id) | At::Folder(id) => Some(id),
            At::Absent | At::Unknown => None,
        }
    }
}

/// Looks at `path` without following a final link.
fn look(path: &Path) -> At {
    match dalica_fs::stat(path) {
        Ok(None) => At::Absent,
        Ok(Some(s)) if s.is_file => At::File(s.identity),
        Ok(Some(s)) => At::Folder(s.identity),
        Err(_) => At::Unknown,
    }
}

/// Whether the folder of `path` lists exactly its name (`None`: it cannot be listed).
fn listed(path: &Path) -> Option<bool> {
    let (dir, name) = (path.parent()?, path.file_name()?);
    let limits = dalica_fs::Limits { max_depth: 1, ..dalica_fs::Limits::default() };
    let walk = dalica_fs::walk(dir, limits, |_| false).ok()?;
    if !walk.warnings.is_empty() {
        return None;
    }
    Some(walk.entries.iter().any(|e| e.path.file_name() == Some(name)))
}

/// What is at both ends of an item: a name that a volume ignoring case resolves to the other end's
/// file counts only where its folder lists that exact spelling (two hard links of one file are
/// both there).
fn ends(from: &Path, to: Option<&Path>, offline: (bool, bool)) -> (At, At) {
    let a = if offline.0 { At::Unknown } else { look(from) };
    let b = match to {
        None => At::Absent,
        Some(_) if offline.1 => At::Unknown,
        Some(t) => look(t),
    };
    match (a.identity(), b.identity(), to) {
        (Some(x), Some(y), Some(t)) if x == y && t != from => match (listed(from), listed(t)) {
            (Some(true), Some(true)) => (a, b),
            (Some(false), Some(true)) => (At::Absent, b),
            (Some(true), Some(false)) => (a, At::Absent),
            _ => (At::Unknown, At::Unknown),
        },
        _ => (a, b),
    }
}

/// Whether the files at `a` and `b` (versions `ia` and `ib`) hold the same bytes, read in chunks;
/// a file that changed while it was read is not a copy.
fn same_bytes(a: &Path, ia: Identity, b: &Path, ib: Identity) -> bool {
    if ia.size() != ib.size() {
        return false;
    }
    if ia == ib {
        // Two links of one file.
        return true;
    }
    let compare = || -> std::io::Result<bool> {
        let (mut fa, mut fb) = (std::fs::File::open(a)?, std::fs::File::open(b)?);
        let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
        loop {
            let n = fill(&mut fa, &mut ba)?;
            let m = fill(&mut fb, &mut bb)?;
            if n != m || ba.get(..n) != bb.get(..m) {
                return Ok(false);
            }
            if n == 0 {
                return Ok(true);
            }
        }
    };
    let unchanged = |p: &Path, id: Identity| matches!(dalica_fs::identity(p), Ok(Some(now)) if now == id);
    compare().unwrap_or(false) && unchanged(a, ia) && unchanged(b, ib)
}

/// Reads until `buf` is full or the file ends; how many bytes.
fn fill(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let Some(rest) = buf.get_mut(n..) else { break };
        match f.read(rest)? {
            0 => break,
            k => n = n.saturating_add(k),
        }
    }
    Ok(n)
}

/// Whether the folder `b` is a complete copy of the folder `a`: the same names, the same kinds and,
/// file by file, the same bytes. A link, or anything the walk could not list, is not a copy.
fn same_tree(a: &Path, b: &Path) -> bool {
    let list = |root: &Path| -> Option<BTreeMap<PathBuf, dalica_fs::Entry>> {
        let walk = dalica_fs::walk(root, dalica_fs::Limits::default(), |_| false).ok()?;
        if !walk.warnings.is_empty() || walk.entries.iter().any(|e| e.is_link) {
            return None;
        }
        Some(walk.entries.into_iter().filter_map(|e| Some((e.path.strip_prefix(root).ok()?.to_path_buf(), e))).collect())
    };
    let (Some(la), Some(lb)) = (list(a), list(b)) else { return false };
    if la.len() != lb.len() || la.keys().ne(lb.keys()) {
        return false;
    }
    la.iter().all(|(rel, ea)| match lb.get(rel) {
        Some(eb) if ea.is_dir && eb.is_dir => true,
        Some(eb) if ea.is_file && eb.is_file && ea.size == eb.size => match (dalica_fs::identity(&ea.path), dalica_fs::identity(&eb.path)) {
            (Ok(Some(ia)), Ok(Some(ib))) => same_bytes(&ea.path, ia, &eb.path, ib),
            _ => false,
        },
        _ => false,
    })
}

/// The folder and the plain name of `path`.
fn split(path: &Path) -> Option<(&Path, &str)> {
    Some((path.parent()?, path.file_name()?.to_str()?))
}

/// What was at a moving item's ends before its driver ran (`fileops.rs`): when the driver fails,
/// the disk says whether the file or folder moved all the same (a step after the rename failed:
/// the folders' `fsync`, the check after it), so the library can follow it.
pub(crate) struct Before {
    from: PathBuf,
    to: PathBuf,
    src: At,
    dst: At,
}

impl Before {
    /// Looks at an item of `kind` before it runs (on its job's thread); `None` for an item that
    /// moves nothing (a copy, a trash, one naming one folder twice).
    pub(crate) fn look(kind: crate::fileops::FileOpKind, from: &Path, to: Option<&Path>) -> Option<Before> {
        use crate::fileops::FileOpKind as K;
        let to = to.filter(|t| *t != from)?;
        if !matches!(kind, K::Move | K::Rename | K::MoveFolder | K::RenameFolder) {
            return None;
        }
        Some(Before { from: from.to_path_buf(), to: to.to_path_buf(), src: look(from), dst: look(to) })
    }

    /// After the driver failed: the move it made all the same, if the source is gone and the
    /// destination (`retargeted`, the name a clash gave, else the item's own, which was free) holds
    /// it (for a file, the same file).
    pub(crate) fn moved_anyway(self, retargeted: Option<&Path>) -> Option<Moved> {
        let (to, was_free) = match retargeted {
            Some(t) => (t.to_path_buf(), true),
            None => (self.to, self.dst == At::Absent),
        };
        if !was_free {
            return None;
        }
        let (a, b) = ends(&self.from, Some(&to), (false, false));
        match (self.src, a, b) {
            (At::File(x), At::Absent, At::File(y)) if x == y => Some(Moved { from: self.from, to, a, b }),
            (At::Folder(_), At::Absent, At::Folder(_)) => Some(Moved { from: self.from, to, a, b }),
            _ => None,
        }
    }
}

/// A file or folder that moved although its driver failed.
pub(crate) struct Moved {
    from: PathBuf,
    to: PathBuf,
    a: At,
    b: At,
}

impl Moved {
    /// The photo's entry and backup to carry (a file's), as a move.
    pub(crate) fn carry(&self) -> Option<Carry> {
        let (At::File(_), Some((fd, fname)), Some((td, tname))) = (self.b, split(&self.from), split(&self.to)) else { return None };
        Some(Carry::one(Mode::Move, fd, fname, td, tname))
    }
}

/// The kind an item is judged as: undoing a copy sends the copy to the trash.
fn judged(op: &Intent) -> Kind {
    match (op.kind, op.direction) {
        (Kind::Copy, intents::Direction::Undo) => Kind::Trash,
        (kind, _) => kind,
    }
}

/// One operation to repair: everything its work needs.
struct Item {
    intent: Intent,
    /// Per item: whether its `from` and its `to` lie in an offline folder of the library.
    offline: Vec<(bool, bool)>,
    now: Timestamp,
    /// The journal's path, for the progress and the failures.
    file: PathBuf,
}

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.file
    }
}

/// One item as its repair left it.
struct Ended {
    repair: FileOpRepair,
    /// What is at its ends now.
    from: At,
    to: At,
}

/// What the repair of one operation did.
struct Fixed {
    intent: Intent,
    items: Vec<Ended>,
    /// Whether it stays in the journal (an item waits, or a carry failed).
    keep: bool,
}

/// What finishing one item on disk did.
struct Finish {
    outcome: RepairOutcome,
    why: String,
    keep: bool,
}

impl Finish {
    fn new(outcome: RepairOutcome, why: impl Into<String>) -> Finish {
        Finish { outcome, why: why.into(), keep: outcome == RepairOutcome::Waits }
    }
}

/// Repairs one item on disk (on the job's worker).
fn repair_item(kind: Kind, state: intents::State, from: &Path, to: Option<&Path>, offline: (bool, bool), now: Timestamp) -> Finish {
    if to == Some(from) {
        return Finish::new(RepairOutcome::Nothing, "it names one folder at both ends");
    }
    let (a, b) = ends(from, to, offline);
    let same = match (a, b, to) {
        (At::File(ia), At::File(ib), Some(t)) => same_bytes(from, ia, t, ib),
        (At::Folder(_), At::Folder(_), Some(t)) => same_tree(from, t),
        _ => false,
    };
    let decision = intents::decide(kind, state, Facts { from: a.seen(), to: b.seen(), same });
    let (finish, carry) = match decision.action {
        Action::Back => return Finish::new(RepairOutcome::Back, decision.why),
        Action::Lost => return Finish::new(RepairOutcome::Lost, decision.why),
        Action::Wait => return Finish::new(RepairOutcome::Waits, decision.why),
        Action::Forward { finish, carry } => (finish, carry),
    };
    let Some(to) = to else { return Finish::new(RepairOutcome::Forward, decision.why) };
    let mut why = decision.why.to_string();
    // A source that stays (its volume has no trash, or the item was done that way) has its entry
    // copied, not moved.
    let mut source_stays = matches!(a, At::File(_) | At::Folder(_));
    if finish {
        let sent = match (a, b, split(from), split(to)) {
            (At::File(ia), At::File(ib), Some((fd, fname)), Some((td, tname))) => {
                transfer::finish_move(fd, fname, ia, td, tname, ib).map(|f| matches!(f, Finished::Trashed(_)))
            }
            (At::Folder(_), At::Folder(_), _, _) => trash::send(from).map(|s| matches!(s, Sent::Trashed(_))),
            _ => return Finish::new(RepairOutcome::Waits, format!("{why}; its names are not plain text")),
        };
        match sent {
            Ok(true) => {
                source_stays = false;
                why.push_str("; the source went to the trash");
            }
            Ok(false) => why.push_str("; the source stays: its drive has no trash, so the move became a copy"),
            Err(e) => return Finish::new(RepairOutcome::Waits, format!("{why}; the source could not go to the trash: {e}")),
        }
    }
    // Only a photo's file carries anything: a folder's sidecar is inside it.
    let (Some(mode), At::File(_)) = (carry, b) else { return Finish::new(RepairOutcome::Forward, why) };
    let (Some((fd, fname)), Some((td, tname))) = (split(from), split(to)) else {
        return Finish::new(RepairOutcome::Forward, format!("{why}; its names are not plain text, so nothing was carried"));
    };
    let c = Carry::one(if source_stays { Mode::Copy } else { mode }, fd, fname, td, tname);
    let mut keep = false;
    match again(|| carry::backups(&c, now)) {
        Ok(out) => {
            if out.changed() {
                why.push_str("; its originals backup was carried");
            }
            for r in out.refused {
                why.push_str(&format!("; its originals backup stayed: {}", r.reason));
            }
        }
        Err(e) => keep |= carry_failed(&e, &mut why, "its originals backup"),
    }
    match again(|| carry::entries(&c, now)) {
        Ok(out) => {
            if out.changed() {
                why.push_str("; its entry was carried");
            }
            for r in out.refused {
                why.push_str(&format!("; its entry stayed: {}", r.reason));
            }
        }
        Err(e) => keep |= carry_failed(&e, &mut why, "its entry"),
    }
    let outcome = if keep { RepairOutcome::Waits } else { RepairOutcome::Forward };
    Finish { outcome, why, keep }
}

/// Runs a carry up to [`CARRY_TRIES`] times while it fails in a way trying again may mend (a
/// sidecar the session's writer replaced between the carry's read and its write): carrying again
/// is harmless.
fn again(f: impl Fn() -> Result<carry::Carried, CarryError>) -> Result<carry::Carried, CarryError> {
    let mut last = f();
    for _ in 1..CARRY_TRIES {
        match &last {
            Err(CarryError::Fs(_) | CarryError::SourceKept { .. }) => last = f(),
            _ => break,
        }
    }
    last
}

/// Notes a carry that failed in `why`; whether trying again at the next open may finish it (an
/// I/O error, an entry left in both files) rather than never (a damaged sidecar kept aside, a
/// read-only folder, a folder that is gone).
fn carry_failed(e: &CarryError, why: &mut String, what: &str) -> bool {
    why.push_str(&format!("; {what} was not carried: {e}"));
    matches!(e, CarryError::Fs(_) | CarryError::SourceKept { .. })
}

/// One operation, on the job's worker.
fn work(item: Item) -> Result<Done<Fixed>, String> {
    let Item { intent, offline, now, .. } = item;
    let kind = judged(&intent);
    let mut keep = false;
    let mut items = Vec::with_capacity(intent.items.len());
    for (i, it) in intent.items.iter().enumerate() {
        let off = offline.get(i).copied().unwrap_or((false, false));
        let f = repair_item(kind, it.state, &it.from, it.to.as_deref(), off, now);
        keep |= f.keep;
        let (from, to) = ends(&it.from, it.to.as_deref(), off);
        let repair = FileOpRepair {
            op: intent.id,
            kind: intent.kind,
            direction: intent.direction,
            from: it.from.clone(),
            to: it.to.clone(),
            outcome: f.outcome,
            why: f.why,
        };
        items.push(Ended { repair, from, to });
    }
    Ok(Done { changes: Vec::new(), value: Fixed { intent, items, keep } })
}

impl Session {
    /// At session open: the operations an earlier session left in the journal are repaired by a
    /// file job (nothing starts when there are none). The owner thread reads nothing.
    pub(crate) fn recover_file_ops_at_open(&mut self) {
        let left = self.file_ops_left().to_vec();
        if left.is_empty() {
            return;
        }
        let now = self.clock.now();
        let file = self.dir.join(intents::FILE);
        let items: Vec<Item> = left
            .into_iter()
            .map(|intent| {
                let offline =
                    intent.items.iter().map(|it| (self.offline_at(&it.from), it.to.as_deref().is_some_and(|t| self.offline_at(t)))).collect();
                Item { intent, offline, now, file: file.clone() }
            })
            .collect();
        let rescan: Arc<Mutex<BTreeSet<PathBuf>>> = Arc::default();
        let said: Arc<Mutex<Vec<FileOpRepair>>> = Arc::default();
        let (rescan_end, said_end) = (Arc::clone(&rescan), Arc::clone(&said));
        let job = FileJob::new(JobKind::Files, LABEL, items, work)
            .on_applied(move |s: &mut Session, a: Applied<Fixed>| {
                let (repairs, folders) = s.follow_repairs(a.value);
                rescan.lock().unwrap_or_else(PoisonError::into_inner).extend(folders);
                said.lock().unwrap_or_else(PoisonError::into_inner).extend(repairs);
                Ok(Vec::new())
            })
            .on_finish(move |s: &mut Session, tally: &Tally| {
                let folders = std::mem::take(&mut *rescan_end.lock().unwrap_or_else(PoisonError::into_inner));
                // A folder inside another one rescanned here is left to that scan.
                for f in folders.iter().filter(|f| !folders.iter().any(|o| o != *f && f.starts_with(o))) {
                    if s.library().folder_by_path(f).is_some() || s.folder_rules().indexes(f) {
                        s.start_region_scan(f);
                    }
                }
                Notice { message: message(&said_end.lock().unwrap_or_else(PoisonError::into_inner), tally), folder: None }
            });
        self.start_file_job(job);
    }

    /// The repairs of interrupted file operations made since the session opened, oldest first (at
    /// most [`MAX_REPAIRS`]; `library.inspect` → `fileOpRepairs` once that method is built).
    pub fn file_op_repairs(&self) -> impl Iterator<Item = &FileOpRepair> {
        self.fileops.repairs.iter()
    }

    /// Whether `path` lies in a library folder that is offline (its folder, or itself).
    fn offline_at(&self, path: &Path) -> bool {
        let lib = self.library();
        [Some(path), path.parent()].into_iter().flatten().filter_map(|p| lib.folder_by_path(p)).any(|f| self.is_folder_offline(f))
    }

    /// Brings one repaired operation into the library (its ops applied as changes from the
    /// files), reports its items, and ends it in the journal unless it stays. Returns the repairs
    /// and the folders to rescan when the job ends.
    fn follow_repairs(&mut self, fixed: Fixed) -> (Vec<FileOpRepair>, BTreeSet<PathBuf>) {
        let Fixed { intent, items, keep } = fixed;
        let kind = judged(&intent);
        let mut rescan = BTreeSet::new();
        let mut repairs = Vec::with_capacity(items.len());
        for Ended { repair, from, to } in items {
            let line = repair.line();
            match repair.outcome {
                RepairOutcome::Back | RepairOutcome::Forward | RepairOutcome::Nothing => log::info!("{line}"),
                RepairOutcome::Lost | RepairOutcome::Waits => log::warn!("{line}"),
            }
            self.scan_warnings.push(line);
            if repair.outcome != RepairOutcome::Waits {
                self.follow_item(kind, &repair.from, repair.to.as_deref(), from, to, &mut rescan);
            }
            repairs.push(repair);
        }
        for r in &repairs {
            if self.fileops.repairs.len() >= MAX_REPAIRS {
                self.fileops.repairs.pop_front();
            }
            self.fileops.repairs.push_back(r.clone());
        }
        if keep {
            log::warn!("interrupted file operation {}: stays in the file-operation journal for the next start", intent.id);
        } else {
            // After the library's ops, which the I/O worker journals first.
            self.end_left_file_op(intent.id);
        }
        (repairs, rescan)
    }

    /// A running operation's item moved although its driver failed (`fileops.rs`): the library
    /// follows it as a change from the files, as a repair does.
    pub(crate) fn follow_moved(&mut self, kind: Kind, m: &Moved) {
        log::warn!("{} moved to {} although the step after it failed; the library follows it", m.from.display(), m.to.display());
        let mut rescan = BTreeSet::new();
        self.follow_item(kind, &m.from, Some(&m.to), m.a, m.b, &mut rescan);
        for f in rescan {
            if self.library().folder_by_path(&f).is_some() || self.folder_rules().indexes(&f) {
                self.start_region_scan(&f);
            }
        }
    }

    /// The photo the library shows at `path`.
    fn photo_at(&self, path: &Path) -> Option<PhotoId> {
        let (dir, name) = split(path)?;
        Some(self.library().photo_by_name(self.library().folder_by_path(dir)?, name)?.id)
    }

    /// The library follows one item's ends as the repair left them (`a` at `from`, `b` at `to`).
    fn follow_item(&mut self, kind: Kind, from: &Path, to: Option<&Path>, a: At, b: At, rescan: &mut BTreeSet<PathBuf>) {
        let parents =
            |paths: &[Option<&Path>]| -> Vec<PathBuf> { paths.iter().flatten().filter_map(|p| p.parent().map(Path::to_path_buf)).collect() };
        let mut ops = Vec::new();
        match (kind, a, b, to) {
            // A photo moved or renamed: the library's photo follows with its id and data.
            (Kind::Move | Kind::Rename, At::Absent, At::File(_), Some(t)) if self.photo_at(t).is_none() => {
                let dest = t.parent().and_then(|d| self.library().folder_by_path(d));
                match (self.photo_at(from), dest, split(t)) {
                    (Some(id), Some(dest), Some((_, name))) => ops.push(Op::MovePhotos { moves: vec![(id, dest, name.to_string())] }),
                    _ => rescan.extend(parents(&[Some(from), Some(t)])),
                }
            }
            // A folder moved or renamed: the library's folder and those under it follow with their
            // ids, and so do its watched-folder entries.
            (Kind::Move | Kind::Rename | Kind::MoveFolder | Kind::RenameFolder, At::Absent, At::Folder(_), Some(t)) => {
                match (self.library().folder_by_path(from), self.library().folder_by_path(t)) {
                    (Some(id), None) => {
                        ops.push(Op::MoveFolder { id, path: t.to_path_buf() });
                        self.rebase_folders(from, t);
                    }
                    // Already there (or the folder a merge emptied): the source the library still
                    // shows went away.
                    (Some(id), Some(_)) => ops.extend(self.folders_at_and_under(id)),
                    (None, _) => {}
                }
            }
            // In the trash: it leaves the library.
            (Kind::Trash | Kind::TrashFolder, At::Absent, _, _) => {
                if let Some(id) = self.photo_at(from) {
                    ops.push(Op::RemovePhotos { ids: vec![id] });
                } else if let Some(id) = self.library().folder_by_path(from) {
                    ops.extend(self.folders_at_and_under(id));
                }
            }
            _ => {}
        }
        let now = self.clock.now();
        for op in ops {
            let op = self.library().stamped(op, now);
            if let Err(e) = self.apply(op) {
                log::warn!("an interrupted file operation's change could not reach the library: {e}");
                rescan.extend(parents(&[Some(from), to]));
            }
        }
        // Any end the library still shows otherwise than the disk has its folder rescanned.
        for (path, at) in [(Some(from), a), (to, b)] {
            let Some(path) = path else { continue };
            let differs = match at {
                At::File(_) => self.photo_at(path).is_none(),
                At::Absent => self.photo_at(path).is_some() || self.library().folder_by_path(path).is_some(),
                At::Folder(_) => self.library().folder_by_path(path).is_none() && self.folder_rules().indexes(path),
                At::Unknown => false,
            };
            if differs {
                rescan.extend(parents(&[Some(path)]));
            }
        }
    }

    /// The ops that take library folder `id` and every library folder under it out, deepest first.
    fn folders_at_and_under(&self, id: FolderId) -> Vec<Op> {
        let lib = self.library();
        let Some(root) = lib.folder(id).map(|f| f.path.clone()) else { return Vec::new() };
        let mut under: Vec<(usize, FolderId)> =
            lib.folders().filter(|f| f.path.starts_with(&root)).map(|f| (f.path.components().count(), f.id)).collect();
        under.sort_by(|x, y| y.cmp(x));
        under.into_iter().map(|(_, id)| Op::RemoveFolder { id }).collect()
    }
}

/// The job's message.
fn message(said: &[FileOpRepair], tally: &Tally) -> String {
    let ops: BTreeSet<u64> = said.iter().map(|r| r.op).collect();
    let waiting: BTreeSet<u64> = said.iter().filter(|r| r.outcome == RepairOutcome::Waits).map(|r| r.op).collect();
    match (ops.len(), waiting.len(), tally.failures.len()) {
        (0, _, 0) => "No interrupted file operation needed repair".into(),
        (0, _, _) => "Interrupted file operations could not be looked at".into(),
        (1, 0, _) => "Repaired an interrupted file operation".into(),
        (n, 0, _) => format!("Repaired {n} interrupted file operations"),
        (n, w, _) => format!("Repaired {} of {n} interrupted file operations; {w} wait for the next start", n.saturating_sub(w)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at0() -> Timestamp {
        Timestamp::from_millis(0)
    }

    #[test]
    fn an_undone_copy_is_judged_as_a_trash() {
        let op = |kind, direction| Intent { id: 1, kind, direction, items: Vec::new() };
        assert_eq!(judged(&op(Kind::Copy, intents::Direction::Undo)), Kind::Trash);
        assert_eq!(judged(&op(Kind::Copy, intents::Direction::Redo)), Kind::Copy);
        assert_eq!(judged(&op(Kind::Move, intents::Direction::Undo)), Kind::Move);
    }

    #[test]
    fn the_message_counts_the_operations() {
        let r = |op, outcome| FileOpRepair {
            op,
            kind: Kind::Move,
            direction: intents::Direction::Do,
            from: PathBuf::from("/a"),
            to: None,
            outcome,
            why: String::new(),
        };
        let t = Tally::default();
        assert_eq!(message(&[], &t), "No interrupted file operation needed repair");
        assert_eq!(message(&[r(1, RepairOutcome::Forward), r(1, RepairOutcome::Back)], &t), "Repaired an interrupted file operation");
        assert_eq!(
            message(&[r(1, RepairOutcome::Forward), r(2, RepairOutcome::Waits)], &t),
            "Repaired 1 of 2 interrupted file operations; 1 wait for the next start"
        );
    }

    /// The same path at both ends, a spelling the folder does not list, and two links of one file:
    /// a file is never sent to the trash as its own copy.
    #[test]
    fn a_file_is_never_its_own_copy_and_a_case_alias_is_one_end() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("IMG_0001.jpg");
        std::fs::write(&a, b"photo").unwrap();
        let f = repair_item(Kind::Move, intents::State::Pending, &a, Some(&a), (false, false), at0());
        assert_eq!(f.outcome, RepairOutcome::Nothing);
        assert!(a.exists());
        // On a volume that ignores case this name opens IMG_0001.jpg; it is still not there.
        let upper = dir.path().join("img_0001.JPG");
        let (x, y) = ends(&a, Some(&upper), (false, false));
        assert!(matches!(x, At::File(_)), "{x:?}");
        assert_eq!(y, At::Absent);
        let f = repair_item(Kind::Rename, intents::State::Pending, &a, Some(&upper), (false, false), at0());
        assert_eq!(f.outcome, RepairOutcome::Back, "{}", f.why);
        // And the other way round: renamed to the other spelling, the old one is gone.
        let f = repair_item(Kind::Rename, intents::State::Pending, &upper, Some(&a), (false, false), at0());
        assert_eq!(f.outcome, RepairOutcome::Forward, "{}", f.why);
        assert_eq!(std::fs::read(&a).unwrap(), b"photo");
        // Two links of one file are a copy of each other.
        let b = dir.path().join("linked.jpg");
        if std::fs::hard_link(&a, &b).is_ok() {
            let (x, y) = ends(&a, Some(&b), (false, false));
            assert!(matches!((x, y), (At::File(p), At::File(q)) if p == q), "{x:?} {y:?}");
        }
        // An end in an offline folder is unknown: the item waits.
        let f = repair_item(Kind::Move, intents::State::Pending, &a, Some(&upper), (true, false), at0());
        assert_eq!((f.outcome, f.keep), (RepairOutcome::Waits, true));
    }

    #[test]
    fn copies_and_trees_compare_byte_for_byte() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (dir.path().join("a"), dir.path().join("b"), dir.path().join("c"));
        std::fs::write(&a, vec![7u8; CHUNK + 5]).unwrap();
        std::fs::write(&b, vec![7u8; CHUNK + 5]).unwrap();
        let mut other = vec![7u8; CHUNK + 5];
        if let Some(x) = other.last_mut() {
            *x = 8;
        }
        std::fs::write(&c, other).unwrap();
        let id = |p: &Path| dalica_fs::identity(p).unwrap().unwrap();
        assert!(same_bytes(&a, id(&a), &b, id(&b)));
        assert!(!same_bytes(&a, id(&a), &c, id(&c)));
        let (t1, t2) = (dir.path().join("t1"), dir.path().join("t2"));
        for t in [&t1, &t2] {
            std::fs::create_dir_all(t.join("sub")).unwrap();
            std::fs::write(t.join("sub").join("x.jpg"), b"x").unwrap();
            std::fs::write(t.join(".dalica.json"), b"{}").unwrap();
        }
        assert!(same_tree(&t1, &t2));
        std::fs::write(t2.join("sub").join("x.jpg"), b"y").unwrap();
        assert!(!same_tree(&t1, &t2));
        std::fs::write(t2.join("sub").join("x.jpg"), b"x").unwrap();
        std::fs::write(t2.join("extra.txt"), b"e").unwrap();
        assert!(!same_tree(&t1, &t2));
    }
}
