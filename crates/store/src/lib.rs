//! dalica-store: Persistence of the model: index journal + snapshot, model ↔ sidecar mapping, the debounced sidecar writer, the metadata write queue, rebuild from folders.
//!
//! Layer L3; see `docs/architecture.md` §2 and §4. The index (`index.snap` + `index.log` in the
//! library directory) is a cache of the model: [`Store::open`] loads the snapshot and replays the
//! journal after it, and recovers from damage without ever failing on it. Every recovery is
//! reported as a [`Recovery`]. Damaged files are kept aside as `<name>.corrupt-<time>`. When the
//! index cannot be trusted at all, the library opens empty with [`Recovery::RebuildNeeded`] and
//! the caller rebuilds it from the folders (the sidecars are the source of truth).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod albums;
pub mod carry;
pub mod faces_cache;
pub mod intents;
pub mod journal;
pub mod mapping;
pub mod metawrite;
pub mod people;
pub mod rebuild;
pub mod snapshot;
pub mod stale;
pub mod writer;

use std::path::{Path, PathBuf};

use dalica_core::{DateTime, Timestamp};
use dalica_library::{Library, Op};

pub use journal::Journal;
pub use mapping::FolderState;
pub use metawrite::{FileResult, MetaWriteQueue};
pub use rebuild::{Import, import_sidecars};
pub use writer::{CarryReport, Outcome, SidecarWriter, write_folder};

/// The largest index file read (journals are snapshotted long before this).
pub const MAX_FILE: u64 = 1 << 30;
/// A snapshot is due after this many ops or bytes in the journal.
pub const SNAPSHOT_OPS: u64 = 10_000;
pub const SNAPSHOT_BYTES: u64 = 64 << 20;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Fs(#[from] dalica_fs::Error),
    #[error("JSON: {0}")]
    Json(String),
    #[error("a journal record of {0} bytes is over the limit")]
    RecordTooLarge(usize),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Something [`Store::open`] repaired, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recovery {
    /// The journal ended in a torn or damaged record; the ops before it were kept and the rest
    /// removed from the file.
    JournalTail { kept: u64, dropped_bytes: u64, reason: String },
    /// The snapshot could not be used and was kept aside; the journal had every op, so nothing was lost.
    SnapshotSetAside { kept_as: PathBuf, reason: String },
    /// The journal ended before the snapshot it belongs to; it starts again after the snapshot.
    JournalReset { reason: String },
    /// The index cannot be trusted: it was kept aside and the library starts empty. Rebuild it
    /// from the folders.
    RebuildNeeded { reason: String },
}

/// The open index of one library directory.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    journal: Journal,
    snapshot_seq: u64,
}

/// What [`Store::open`] gives back.
#[derive(Debug)]
pub struct Opened {
    pub store: Store,
    pub library: Library,
    pub report: Vec<Recovery>,
}

/// `corrupt-20261010T051500Z` for the time `now`.
fn tag(kind: &str, now: Timestamp) -> String {
    let stamp = DateTime::from_unix_secs(now.as_secs()).map(|d| d.to_string().replace(['-', ':'], "")).unwrap_or_else(|| now.as_millis().to_string());
    format!("{kind}-{stamp}Z")
}

impl Store {
    /// Opens the index in `dir` (the library directory, which must exist and be locked by the
    /// caller). `now` names any file set aside.
    pub fn open(dir: &Path, now: Timestamp) -> Result<Opened> {
        let snap_path = dir.join(snapshot::FILE);
        let log_path = dir.join(journal::FILE);
        let mut report = Vec::new();
        let aside = |path: &Path, kind: &str| dalica_fs::set_aside(path, &tag(kind, now));

        // 1. The snapshot: absent (an empty model at op 0), usable, or damaged.
        let snap = match read_file(&snap_path)? {
            None => Some((Library::new(), 0)),
            Some(Ok(bytes)) => match snapshot::read(&bytes) {
                Ok((lib, seq)) => Some((lib, seq)),
                Err(reason) => {
                    let kept_as = aside(&snap_path, "corrupt")?;
                    report.push(Recovery::SnapshotSetAside { kept_as, reason });
                    None
                }
            },
            Some(Err(reason)) => {
                let kept_as = aside(&snap_path, "corrupt")?;
                report.push(Recovery::SnapshotSetAside { kept_as, reason });
                None
            }
        };

        // 2. The journal.
        let bytes = match read_file(&log_path)? {
            None => None,
            Some(Ok(b)) => Some(b),
            Some(Err(reason)) => return rebuild(dir, &aside, report, reason),
        };
        let Some(bytes) = bytes else {
            return match snap {
                Some((library, seq)) => Ok(Opened { store: Store::fresh(dir, seq)?, library, report }),
                None => rebuild(dir, &aside, report, "the snapshot was damaged and there is no journal".into()),
            };
        };
        let contents = journal::read(&bytes);
        let Some(header) = contents.header else {
            if let (true, Some((library, seq))) = (bytes.is_empty(), snap) {
                return Ok(Opened { store: Store::fresh(dir, seq)?, library, report });
            }
            let reason = match &contents.stop {
                Some(journal::Stop::Corrupt { reason, .. }) => format!("journal header: {reason}"),
                _ => "the journal has no header".into(),
            };
            return rebuild(dir, &aside, report, reason);
        };
        let (mut library, snap_seq) = match snap {
            Some(s) => s,
            // Damaged snapshot: the journal can stand in only if it holds every op from the start.
            None if header.base == 0 => (Library::new(), 0),
            None => return rebuild(dir, &aside, report, format!("the snapshot was damaged and the journal starts after op {}", header.base)),
        };
        if header.base > snap_seq {
            return rebuild(dir, &aside, report, format!("the journal starts after op {} but the snapshot holds only {snap_seq}", header.base));
        }

        // 3. Replay the ops after the snapshot, stopping at the first that does not apply.
        let mut kept = 0u64;
        let mut stop_reason = contents.stop.as_ref().map(|s| match s {
            journal::Stop::Torn => "the last record was cut off".to_string(),
            journal::Stop::Corrupt { index, reason } => format!("record {index}: {reason}"),
        });
        for (i, op) in contents.ops.into_iter().enumerate() {
            let seq = header.base + i as u64 + 1;
            if seq > snap_seq
                && let Err(e) = library.apply(op)
            {
                stop_reason = Some(format!("op {seq} does not apply: {e}"));
                break;
            }
            kept += 1;
        }
        if header.base + kept < snap_seq {
            report
                .push(Recovery::JournalReset { reason: format!("the journal ends at op {} but the snapshot holds {snap_seq}", header.base + kept) });
            return Ok(Opened { store: Store::fresh(dir, snap_seq)?, library, report });
        }
        let valid_len = contents.ends.get(kept as usize).copied().unwrap_or(0);
        let rewrite = valid_len != bytes.len();
        if let Some(reason) = stop_reason.filter(|_| rewrite) {
            report.push(Recovery::JournalTail { kept, dropped_bytes: (bytes.len() - valid_len) as u64, reason });
        }
        let journal = Journal::resume(&log_path, bytes.get(..valid_len).unwrap_or_default(), header.base, kept, rewrite)?;
        Ok(Opened { store: Store { dir: dir.to_path_buf(), journal, snapshot_seq: snap_seq }, library, report })
    }

    /// A store with a new journal starting after op `seq`.
    fn fresh(dir: &Path, seq: u64) -> Result<Store> {
        Ok(Store { dir: dir.to_path_buf(), journal: Journal::create(&dir.join(journal::FILE), seq)?, snapshot_seq: seq })
    }

    /// Records an applied op; it is on disk when this returns.
    pub fn append(&mut self, op: &Op) -> Result<u64> {
        self.journal.append(op)
    }

    /// Writes a snapshot of `library` (which must hold every op appended so far) and starts a new,
    /// empty journal after it.
    pub fn snapshot(&mut self, library: &Library) -> Result<()> {
        let seq = self.journal.seq();
        snapshot::write(&self.dir.join(snapshot::FILE), library, seq)?;
        self.journal = Journal::create(&self.dir.join(journal::FILE), seq)?;
        self.snapshot_seq = seq;
        Ok(())
    }

    /// Whether the journal is long enough that a snapshot is worth writing.
    pub fn wants_snapshot(&self) -> bool {
        self.journal.len() >= SNAPSHOT_OPS || self.journal.bytes() >= SNAPSHOT_BYTES
    }

    /// The number of the last op recorded.
    pub fn seq(&self) -> u64 {
        self.journal.seq()
    }

    /// The op number the snapshot holds.
    pub fn snapshot_seq(&self) -> u64 {
        self.snapshot_seq
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Keeps the index files aside and starts an empty one.
fn rebuild(dir: &Path, aside: &dyn Fn(&Path, &str) -> dalica_fs::Result<PathBuf>, mut report: Vec<Recovery>, reason: String) -> Result<Opened> {
    for name in [snapshot::FILE, journal::FILE] {
        let path = dir.join(name);
        if dalica_fs::identity(&path)?.is_some() {
            aside(&path, "corrupt")?;
        }
    }
    report.push(Recovery::RebuildNeeded { reason });
    Ok(Opened { store: Store::fresh(dir, 0)?, library: Library::new(), report })
}

/// The bytes of an index file: `None` if absent, `Some(Err)` if it cannot be used (too large).
fn read_file(path: &Path) -> Result<Option<std::result::Result<Vec<u8>, String>>> {
    match dalica_fs::read(path, MAX_FILE) {
        Ok(None) => Ok(None),
        Ok(Some(r)) => Ok(Some(Ok(r.bytes))),
        Err(dalica_fs::Error::TooLarge { limit, .. }) => Ok(Some(Err(format!("larger than {limit} bytes")))),
        Err(e) => Err(e.into()),
    }
}

/// CRC-32 (IEEE 802.3, reflected, polynomial 0xEDB88320).
pub fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut t = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                k += 1;
            }
            t[i] = c;
            i += 1;
        }
        t
    };
    !bytes.iter().fold(!0u32, |c, &b| TABLE.get(((c ^ u32::from(b)) & 0xff) as usize).copied().unwrap_or(0) ^ (c >> 8))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use dalica_core::{FolderId, PhotoId, Timestamp};
    use dalica_library::{Folder, Library, Model, Op, Photo};

    use super::*;

    const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

    fn photo(id: u64, folder: FolderId, name: &str) -> Photo {
        Photo {
            id: PhotoId::new(id),
            folder,
            name: name.into(),
            size: 1000 + id,
            mtime: Timestamp::from_millis(1_700_000_000_000),
            width: 480,
            height: 320,
            date: None,
            star: false,
        }
    }

    /// A fixed history of ops and the model after each (`models[k]` = after `k` ops).
    fn history() -> (Vec<Op>, Vec<Model>) {
        let f = FolderId::new(1);
        let ops = vec![
            Op::AddFolder { folder: Folder { id: f, path: PathBuf::from("/photos/lisbon") } },
            Op::AddPhotos { photos: vec![photo(1, f, "IMG_0001.jpg"), photo(2, f, "a]b \"c\";=.jpg")] },
            Op::SetStars { stars: vec![(PhotoId::new(1), true)] },
            Op::AddPhotos { photos: vec![photo(3, f, "東京.jpg")] },
            Op::SetStars { stars: vec![(PhotoId::new(3), true), (PhotoId::new(2), true)] },
            Op::SetStars { stars: vec![(PhotoId::new(2), false)] },
            Op::RemovePhotos { ids: vec![PhotoId::new(1)] },
            Op::Batch {
                ops: vec![
                    Op::AddFolder { folder: Folder { id: FolderId::new(2), path: PathBuf::from("/photos/porto") } },
                    Op::AddPhotos { photos: vec![photo(4, FolderId::new(2), "p.jpg")] },
                ],
            },
        ];
        let mut lib = Library::new();
        let mut models = vec![lib.model().clone()];
        for op in &ops {
            lib.apply(op.clone()).unwrap();
            models.push(lib.model().clone());
        }
        (ops, models)
    }

    /// A library directory whose journal holds `ops`, and that journal's bytes.
    fn journal_of(ops: &[Op]) -> (tempfile::TempDir, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let mut opened = Store::open(dir.path(), NOW).unwrap();
        for op in ops {
            opened.library.apply(op.clone()).unwrap();
            opened.store.append(op).unwrap();
        }
        let bytes = std::fs::read(dir.path().join(journal::FILE)).unwrap();
        (dir, bytes)
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn journal_and_replay_reproduce_the_model() {
        let (ops, models) = history();
        let (dir, _) = journal_of(&ops);
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.report, []);
        assert_eq!(opened.library.model(), models.last().unwrap());
        assert_eq!(opened.store.seq(), ops.len() as u64);
        assert_eq!(files(dir.path()), [journal::FILE]);
    }

    /// Every prefix of a journal replays to its last complete op (`security.md` §4.2), and the file
    /// is cut back to that op, so the next open is clean. Every prefix is read in memory (the
    /// reader `Store::open` uses); the opens themselves run at the lengths where the answer changes
    /// (empty, inside the header, and one byte either side of every record's end), and at every
    /// length at full strength (`DALICA_FULL_TESTS=1`).
    #[test]
    fn truncation_at_every_byte_replays_to_the_last_complete_op() {
        let (ops, models) = history();
        let (_full, bytes) = journal_of(&ops);
        let ends = journal::read(&bytes).ends;
        let complete_at = |len: usize| ends.iter().filter(|&&e| e <= len).count();
        for len in 0..=bytes.len() {
            let read = journal::read(&bytes[..len]);
            let complete = complete_at(len);
            let k = complete.saturating_sub(1); // the header is not an op
            assert_eq!(read.ops, ops[..k], "prefix {len}");
            assert_eq!(read.valid_len, ends.iter().copied().filter(|&e| e <= len).max().unwrap_or(0), "prefix {len}");
            let whole = len == 0 || ends.contains(&len);
            assert_eq!(read.stop, (!whole).then_some(journal::Stop::Torn), "prefix {len}");
            let mut lib = Library::new();
            for op in read.ops {
                lib.apply(op).unwrap();
            }
            assert_eq!(lib.model(), &models[k], "prefix {len}");
        }
        let full = std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1");
        let mut lengths: Vec<usize> = if full {
            (0..=bytes.len()).collect()
        } else {
            let mut l = vec![0, ends[0] / 2];
            l.extend(ends.iter().flat_map(|&e| [e - 1, e, e + 1]));
            l
        };
        lengths.retain(|&l| l <= bytes.len());
        lengths.sort_unstable();
        lengths.dedup();
        for len in lengths {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(journal::FILE), &bytes[..len]).unwrap();
            let opened = Store::open(dir.path(), NOW).unwrap();
            let complete = complete_at(len);
            let k = complete.saturating_sub(1);
            assert_eq!(opened.library.model(), &models[k], "prefix {len}");
            match (len, complete) {
                (0, _) => assert_eq!(opened.report, [], "prefix {len}"),
                (_, 0) => assert!(matches!(opened.report[..], [Recovery::RebuildNeeded { .. }]), "prefix {len}: {:?}", opened.report),
                _ if ends.contains(&len) => assert_eq!(opened.report, [], "prefix {len}"),
                _ => {
                    assert!(
                        matches!(&opened.report[..], [Recovery::JournalTail { kept, .. }] if *kept == k as u64),
                        "prefix {len}: {:?}",
                        opened.report
                    )
                }
            }
            drop(opened);
            let again = Store::open(dir.path(), NOW).unwrap();
            assert_eq!(again.report, [], "prefix {len}, second open");
            assert_eq!(again.library.model(), &models[k]);
        }
    }

    #[test]
    fn a_damaged_record_stops_replay_there() {
        let (ops, models) = history();
        let (dir, mut bytes) = journal_of(&ops);
        let ends = journal::read(&bytes).ends;
        // Flip a byte inside op 4's JSON: its checksum fails.
        let at = ends[3] + 20;
        bytes[at] ^= 0x20;
        std::fs::write(dir.path().join(journal::FILE), &bytes).unwrap();
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.library.model(), &models[3]);
        assert!(matches!(&opened.report[..], [Recovery::JournalTail { kept: 3, reason, .. }] if reason.contains("checksum")), "{:?}", opened.report);
        // An op that does not apply (a star for a photo that is not there) stops replay too.
        let (dir, _) = journal_of(&ops[..2]);
        let mut opened = Store::open(dir.path(), NOW).unwrap();
        opened.store.append(&Op::SetStars { stars: vec![(PhotoId::new(99), true)] }).unwrap();
        opened.store.append(&ops[2]).unwrap();
        drop(opened);
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.library.model(), &models[2]);
        assert!(
            matches!(&opened.report[..], [Recovery::JournalTail { kept: 2, reason, .. }] if reason.contains("does not apply")),
            "{:?}",
            opened.report
        );
    }

    #[test]
    fn snapshots_hold_the_model_and_restart_the_journal() {
        let (ops, models) = history();
        let dir = tempfile::tempdir().unwrap();
        let mut o = Store::open(dir.path(), NOW).unwrap();
        for op in &ops[..5] {
            o.library.apply(op.clone()).unwrap();
            o.store.append(op).unwrap();
        }
        o.store.snapshot(&o.library).unwrap();
        assert_eq!((o.store.seq(), o.store.snapshot_seq()), (5, 5));
        for op in &ops[5..] {
            o.library.apply(op.clone()).unwrap();
            o.store.append(op).unwrap();
        }
        drop(o);
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.report, []);
        assert_eq!(opened.library.model(), models.last().unwrap());
        assert_eq!(opened.store.seq(), ops.len() as u64);
        assert_eq!(journal::read(&std::fs::read(dir.path().join(journal::FILE)).unwrap()).header.unwrap().base, 5);
    }

    #[test]
    fn a_damaged_snapshot_falls_back_to_the_journal_when_it_has_every_op() {
        let (ops, models) = history();
        // A crash between writing the snapshot and restarting the journal: the journal still starts at 0.
        let (dir, _) = journal_of(&ops);
        let mut lib = Library::new();
        for op in &ops[..4] {
            lib.apply(op.clone()).unwrap();
        }
        snapshot::write(&dir.path().join(snapshot::FILE), &lib, 4).unwrap();
        // Without damage, the snapshot is used and the journal's first 4 ops are skipped.
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!((opened.report.clone(), opened.library.model()), (vec![], models.last().unwrap()));
        drop(opened);
        // Damaged: kept aside, and the journal alone rebuilds the same model.
        let snap = dir.path().join(snapshot::FILE);
        let mut b = std::fs::read(&snap).unwrap();
        let mid = b.len() / 2;
        b[mid] ^= 1;
        std::fs::write(&snap, b).unwrap();
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.library.model(), models.last().unwrap());
        assert!(
            matches!(&opened.report[..], [Recovery::SnapshotSetAside { kept_as, .. }] if kept_as.file_name().unwrap() == "index.snap.corrupt-20261003T040000Z"),
            "{:?}",
            opened.report
        );
        assert!(!snap.exists());
    }

    #[test]
    fn a_damaged_snapshot_after_compaction_means_a_rebuild() {
        let (ops, _) = history();
        let dir = tempfile::tempdir().unwrap();
        let mut o = Store::open(dir.path(), NOW).unwrap();
        for op in &ops {
            o.library.apply(op.clone()).unwrap();
            o.store.append(op).unwrap();
        }
        o.store.snapshot(&o.library).unwrap();
        drop(o);
        std::fs::write(dir.path().join(snapshot::FILE), b"garbage\n").unwrap();
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.library.model(), &Model::default());
        assert!(
            matches!(&opened.report[..], [Recovery::SnapshotSetAside { .. }, Recovery::RebuildNeeded { reason }] if reason.contains("starts after op 8")),
            "{:?}",
            opened.report
        );
        assert_eq!(files(dir.path()), ["index.log", "index.log.corrupt-20261003T040000Z", "index.snap.corrupt-20261003T040000Z"]);
        // The fresh index works.
        drop(opened);
        let again = Store::open(dir.path(), NOW).unwrap();
        assert_eq!((again.report, again.store.seq()), (vec![], 0));
    }

    #[test]
    fn version_mismatch_and_journal_behind_the_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let header = br#"{"journal":2,"base":0}"#;
        std::fs::write(dir.path().join(journal::FILE), journal::encode(header)).unwrap();
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert!(matches!(&opened.report[..], [Recovery::RebuildNeeded { reason }] if reason.contains("version 2")), "{:?}", opened.report);

        let (ops, models) = history();
        let (dir, _) = journal_of(&ops[..3]);
        let mut lib = Library::new();
        for op in &ops[..6] {
            lib.apply(op.clone()).unwrap();
        }
        snapshot::write(&dir.path().join(snapshot::FILE), &lib, 6).unwrap();
        let opened = Store::open(dir.path(), NOW).unwrap();
        assert_eq!(opened.library.model(), &models[6]);
        assert!(matches!(&opened.report[..], [Recovery::JournalReset { .. }]), "{:?}", opened.report);
        assert_eq!(opened.store.seq(), 6);
    }

    #[test]
    fn oversized_records_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut o = Store::open(dir.path(), NOW).unwrap();
        let huge = Op::AddFolder { folder: Folder { id: FolderId::new(1), path: PathBuf::from("x".repeat(journal::MAX_RECORD)) } };
        assert!(matches!(o.store.append(&huge), Err(Error::RecordTooLarge(_))));
        assert_eq!(o.store.seq(), 0);
    }
}
