//! Help ▸ Stats for Nerds… (`menus-and-shortcuts.md` §1.8, `control-protocol.md` §6.18): what
//! Dalica is doing, how much memory and disk it uses and where its files live, as one query the
//! window, the CLI and MCP share, and the cache's one clean-up.
//!
//! - **`app.diagnostics {countBackups?}`** answers at once from memory, never waiting and never
//!   touching the disk on the owner thread: `memory`, `thumbnailCache`, `libraryFolder`,
//!   `photoFolders`, `library`, `work` and `build`. What only the disk knows (the cache's entries
//!   and bytes, the library folder's files, the process's resident memory) comes from a
//!   measurement a helper thread makes; a query starts a new one when the last is older than
//!   [`MEASURE_EVERY`] and none is running, and gives the last one meanwhile (`null` fields and
//!   `measuring: true` before the first lands). The window asks once a second while it is open,
//!   so it shows live values; nothing runs while nobody asks.
//! - **The photo folders' backups** (`.dalica-originals/`, `data-model.md` §6) are counted only on
//!   demand (`countBackups: true`, the window's Count Backups): a job ([`COUNT_LABEL`]) looks in
//!   each folder the library knows (no walk of the disk: the index names the folders) at its
//!   `.dalica-originals/` and its `.dalica.json`, and the answer is kept until the next count.
//!   Nothing here deletes an original.
//! - **The library's counts** (photos, videos, faces, unsaved edits) are worked out once per
//!   library revision, not once per query.
//! - **`app.clearThumbnailCache`** (Clear Thumbnail Cache…) empties the thumbnail cache as a
//!   background job ([`CLEAR_LABEL`]) through the cache's own interface: every entry of every size
//!   made dead, then each size compacted into a new, empty generation. Only the cache's own files
//!   change (`AGENTS.md` §3: a cache lists and deletes only files of its own naming shape); a
//!   stranger's file in the folder stays. Thumbnails come back as photos are looked at: a size a
//!   cell asks for that the cache no longer has is made again from the photo. Sketches live in the
//!   index and stay.
//!
//! The measurements are kept per library folder in this module (a session has no field for
//! them), so two sessions of one process never mix theirs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_scan::rules::Mode;
use dalica_thumbs::pack::{Cache, Size};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::jobs::{Answer, JobKind, JobState, Outcome};
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

/// How old a measurement may be before a query starts another.
pub const MEASURE_EVERY: Duration = Duration::from_secs(1);
/// The clean-up's job label.
pub const CLEAR_LABEL: &str = "Clearing the thumbnail cache";
/// The backup count's job label.
pub const COUNT_LABEL: &str = "Counting backups";
/// The folder backups are kept in, inside each photo folder.
pub const ORIGINALS: &str = ".dalica-originals";
/// A folder's sidecar.
pub const SIDECAR: &str = ".dalica.json";
/// The most names of the library folder a measurement lists.
pub const MAX_LIBRARY_FILES: usize = 512;
/// The most entries a backup count reads in one folder's `.dalica-originals/`.
pub const MAX_BACKUPS_PER_FOLDER: usize = 100_000;
/// The most folders a backup count looks at.
pub const MAX_FOLDERS: usize = 1_000_000;
/// The library folder's own files, in `data-model.md` §4's order (the faces caches and the
/// files kept aside are matched by their shape).
pub const LIBRARY_FILES: &[&str] = &[
    "folders.json",
    "albums.json",
    "people.json",
    "index.log",
    "index.snap",
    "pending-writes.json",
    "damaged-folders.json",
    "file-ops.json",
    "settings.json",
    "lock",
];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "app.diagnostics",
            label: "Stats for Nerds",
            menu: "",
            keys: &[],
            params: r#"{"countBackups"?:bool}: what Stats for Nerds shows, answered at once from memory (fields only the disk knows are the last measurement's, made on a helper thread at most once a second while asked; null before the first): {measuring, memory {residentBytes, thumbnailBytes, thumbnailBudgetBytes}, thumbnailCache {path, temporary, reason, limitBytes, diskBytes, liveBytes, entries, sizes [{size, entries, bytes}], loads, decodes, hitRate, clearing}, libraryFolder {path, files [{name, bytes}], totalBytes, corrupt [{name, bytes}], damagedFolders}, photoFolders {counting, counted, folders, backups {folders, files, bytes}, sidecars}, library {watched, scanOnce, excluded, offline, photos, videos, hidden, faces, people, albums, unsavedEdits, unreadable, thumbnailsFailed, scanning, lastScan {startedMs, durationMs, files, filesPerSecond}}, work {jobs [{job, kind, label, done, total}], thumbnailJobs, prefetching, pendingWrites}, build {version, commit, date, features, os, arch}}; with countBackups the photo folders' .dalica-originals/ and sidecars are counted in a background job ("Counting backups") and photoFolders says so until it lands"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: diagnostics,
        },
        CommandSpec {
            id: "app.clearThumbnailCache",
            label: "Clear Thumbnail Cache",
            menu: "",
            keys: &[],
            params: r#"{}: empties the thumbnail cache in a background job ("Clearing the thumbnail cache"): its entries made dead and its packs compacted, only the cache's own files touched; thumbnails are made again as photos are looked at, and nothing about the photos is lost. Replies {job, pending: true}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: clear_cache,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    #[serde(default, rename = "countBackups")]
    count_backups: bool,
}

/// What this module keeps of one library.
#[derive(Default)]
struct Kept {
    /// The last measurement and when it was taken.
    measured: Option<(Instant, Measured)>,
    measuring: bool,
    /// The last backup count, and whether one runs.
    backups: Option<Backups>,
    counting: bool,
    /// The library's counts and the revision they were worked out at.
    counts: Option<(u64, Value)>,
}

/// What a helper thread measured.
#[derive(Clone, Debug, Default, PartialEq)]
struct Measured {
    resident: Option<u64>,
    cache: Option<CacheFacts>,
    files: Named,
    corrupt: Named,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct CacheFacts {
    disk: u64,
    live: u64,
    entries: u64,
    sizes: Vec<(&'static str, u64, u64)>,
}

/// Files by name, with their sizes in bytes.
type Named = Vec<(String, u64)>;

/// The last backup count.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Backups {
    folders: u64,
    with_backups: u64,
    files: u64,
    bytes: u64,
    sidecars: u64,
}

static KEPT: Mutex<BTreeMap<PathBuf, Kept>> = Mutex::new(BTreeMap::new());

fn with_kept<R>(dir: &Path, f: impl FnOnce(&mut Kept) -> R) -> R {
    let mut all = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    f(all.entry(dir.to_path_buf()).or_default())
}

fn diagnostics(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Params { count_backups } = params(p)?;
    let dir = s.dir().to_path_buf();
    let cache = s.thumbs.screen_source().0;
    // A measurement when the last is stale and none runs.
    let start = with_kept(&dir, |k| {
        let stale = k.measured.as_ref().is_none_or(|(at, _)| at.elapsed() >= MEASURE_EVERY);
        let start = stale && !k.measuring;
        if start {
            k.measuring = true;
        }
        start
    });
    if start {
        let (dir2, cache2) = (dir.clone(), cache.clone());
        let spawned = std::thread::Builder::new().name("dalica-diagnostics".into()).spawn(move || {
            let m = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| measure(&dir2, cache2.as_deref()))).unwrap_or_default();
            with_kept(&dir2, |k| {
                k.measured = Some((Instant::now(), m));
                k.measuring = false;
            });
        });
        if spawned.is_err() {
            with_kept(&dir, |k| k.measuring = false);
        }
    }
    if count_backups {
        start_count(s, &dir)?;
    }
    let counts = library_counts(s, &dir);
    let (measured, measuring, backups, counting) =
        with_kept(&dir, |k| (k.measured.as_ref().map(|(_, m)| m.clone()), k.measuring, k.backups, k.counting));
    let m = measured.as_ref();
    let stats = s.thumbnail_stats();
    let cache_dir = s.thumbnail_cache_dir().map(Path::to_path_buf);
    let temporary = cache_dir.as_deref().is_some_and(is_temporary);
    let reason = s.notices().iter().find(|n| n.contains("thumbnail cache") || n.contains("thumbnails are")).cloned();
    let facts = m.and_then(|m| m.cache.clone());
    let lookups = stats.loads.saturating_add(stats.decodes);
    let hit_rate = (lookups > 0).then(|| stats.loads as f64 / lookups as f64);
    let clearing = s.jobs.answered_running(JobKind::Rebuild, CLEAR_LABEL) > 0;
    let thumbnail_cache = json!({
        "path": cache_dir,
        "temporary": temporary,
        "reason": if temporary { reason } else { None },
        "limitBytes": s.thumbnail_cache_size(),
        "diskBytes": facts.as_ref().map(|f| f.disk),
        "liveBytes": facts.as_ref().map(|f| f.live),
        "entries": facts.as_ref().map(|f| f.entries),
        "sizes": facts.as_ref().map(|f| f.sizes.iter().map(|(size, entries, bytes)| json!({ "size": size, "entries": entries, "bytes": bytes })).collect::<Vec<_>>()),
        "loads": stats.loads,
        "decodes": stats.decodes,
        "hitRate": hit_rate,
        "clearing": clearing,
    });
    let named = |v: &[(String, u64)]| v.iter().map(|(name, bytes)| json!({ "name": name, "bytes": bytes })).collect::<Vec<_>>();
    let library_folder = json!({
        "path": dir,
        "files": m.map(|m| named(&m.files)),
        "totalBytes": m.map(|m| m.files.iter().chain(&m.corrupt).fold(0u64, |a, (_, b)| a.saturating_add(*b))),
        "corrupt": m.map(|m| named(&m.corrupt)),
        "damagedFolders": s.damaged.len(),
    });
    let photo_folders = json!({
        "counting": counting,
        "counted": backups.is_some(),
        "folders": backups.map(|b| b.folders),
        "backups": backups.map(|b| json!({ "folders": b.with_backups, "files": b.files, "bytes": b.bytes })),
        "sidecars": backups.map(|b| b.sidecars),
    });
    let memory = json!({
        "residentBytes": m.and_then(|m| m.resident),
        "thumbnailBytes": stats.memory,
        "thumbnailBudgetBytes": crate::thumbs::MEMORY,
    });
    Ok(json!({
        "measuring": measuring || measured.is_none(),
        "memory": memory,
        "thumbnailCache": thumbnail_cache,
        "libraryFolder": library_folder,
        "photoFolders": photo_folders,
        "library": library(s, counts, &stats),
        "work": work(s, &stats),
        "build": build(),
    }))
}

/// Whether the cache folder is a temporary one (`dalica-thumbs…` in the system's temporary
/// folder): a demo's, a test's, or the fallback when the cache could not be used.
fn is_temporary(dir: &Path) -> bool {
    let named = dir.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("dalica-thumbs"));
    named && dir.parent().is_some_and(|p| p == std::env::temp_dir())
}

/// The counts of the library, worked out again only when its revision changed.
fn library_counts(s: &Session, dir: &Path) -> Value {
    let lib = s.library();
    let revision = lib.revision();
    if let Some(v) = with_kept(dir, |k| k.counts.as_ref().filter(|(r, _)| *r == revision).map(|(_, v)| v.clone())) {
        return v;
    }
    let (mut photos, mut videos, mut hidden, mut faces) = (0u64, 0u64, 0u64, 0u64);
    for p in lib.photos() {
        photos = photos.saturating_add(1);
        if lib.facts(p.id).video {
            videos = videos.saturating_add(1);
        }
        if lib.is_hidden(p.id) || lib.is_folder_hidden(p.folder) {
            hidden = hidden.saturating_add(1);
        }
        faces = faces.saturating_add(u64::try_from(lib.faces(p.id).len()).unwrap_or(u64::MAX));
    }
    let unsaved = lib.unsaved_counts().values().fold(0usize, |a, n| a.saturating_add(*n));
    let v = json!({
        "photos": photos, "videos": videos, "hidden": hidden, "faces": faces,
        "people": lib.people().count(), "albums": lib.albums().count(), "folders": lib.folders().count(), "unsavedEdits": unsaved,
    });
    with_kept(dir, |k| k.counts = Some((revision, v.clone())));
    v
}

fn library(s: &Session, counts: Value, stats: &crate::thumbs::ThumbStats) -> Value {
    let (mut watched, mut once, mut excluded) = (0usize, 0usize, 0usize);
    for (_, rule) in s.folder_rules().iter() {
        match rule.mode {
            Some(Mode::Always) => watched = watched.saturating_add(1),
            Some(Mode::Once) => once = once.saturating_add(1),
            Some(Mode::Remove) => excluded = excluded.saturating_add(1),
            None => {}
        }
    }
    let scan = s.scan_progress();
    let last =
        s.jobs().into_iter().filter(|j| j.kind == JobKind::Scan && j.state == JobState::Done).max_by_key(|j| j.finished_ms.unwrap_or(0)).map(|j| {
            let duration = j.finished_ms.unwrap_or(j.started_ms).saturating_sub(j.started_ms);
            let rate = (duration > 0).then(|| j.progress.done as f64 * 1000.0 / duration as f64);
            json!({ "startedMs": j.started_ms, "durationMs": duration, "files": j.progress.done, "filesPerSecond": rate })
        });
    let mut v = json!({
        "watched": watched, "scanOnce": once, "excluded": excluded, "roots": s.roots().len(), "offline": s.offline_roots().len(),
        "unreadable": s.scan_failures().len(), "thumbnailsFailed": stats.failed,
        "scanning": { "running": scan.running, "foldersQueued": scan.folders_queued, "found": scan.found, "indexed": scan.indexed },
        "lastScan": last,
    });
    if let (Some(o), Value::Object(c)) = (v.as_object_mut(), counts) {
        o.extend(c);
    }
    v
}

fn work(s: &Session, stats: &crate::thumbs::ThumbStats) -> Value {
    let jobs: Vec<Value> = s
        .jobs()
        .into_iter()
        .filter(|j| !j.state.is_finished())
        .map(|j| json!({ "job": j.job, "kind": j.kind, "label": j.label, "done": j.progress.done, "total": j.progress.total }))
        .collect();
    let pending = s.pending_writes().map(|p| json!({ "sidecars": p.sidecars, "metadata": p.metadata, "index": p.index }));
    json!({ "jobs": jobs, "thumbnailJobs": stats.jobs, "prefetching": stats.prefetching, "pendingWrites": pending })
}

/// The build: its version, commit and date (`dalica_core::build`), the optional features compiled
/// in, and the system.
pub fn build() -> Value {
    let mut features = Vec::new();
    if cfg!(feature = "heif") {
        features.push("heif");
    }
    if cfg!(feature = "hevc") {
        features.push("hevc");
    }
    if cfg!(feature = "camera-usb") {
        features.push("camera-usb");
    }
    json!({
        "version": dalica_core::build::VERSION,
        "commit": dalica_core::build::commit(),
        "date": dalica_core::build::build_date(),
        "features": features,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

// --- On helper threads ---------------------------------------------------------------------------

/// What only the disk knows, measured off the owner thread.
fn measure(dir: &Path, cache: Option<&Cache>) -> Measured {
    let cache = cache.map(|c| {
        let mut facts = CacheFacts { disk: c.disk_bytes(), ..CacheFacts::default() };
        for size in Size::ALL {
            let st = c.stats(size);
            facts.live = facts.live.saturating_add(st.live_bytes);
            facts.entries = facts.entries.saturating_add(st.entries);
            facts.sizes.push((size.name(), st.entries, st.disk_bytes));
        }
        facts
    });
    let (files, corrupt) = library_files(dir);
    Measured { resident: resident(), cache, files, corrupt }
}

/// The library folder's own files with their sizes (in `data-model.md` §4's order, the faces
/// caches after them), and the files kept aside as `.corrupt-<time>`.
fn library_files(dir: &Path) -> (Named, Named) {
    let mut listed: Vec<(String, u64)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten().take(MAX_LIBRARY_FILES) {
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() {
                listed.push((e.file_name().to_string_lossy().into_owned(), meta.len()));
            }
        }
    }
    let mut files: Vec<(String, u64)> = LIBRARY_FILES.iter().filter_map(|n| listed.iter().find(|(name, _)| name == n).cloned()).collect();
    let mut faces: Vec<(String, u64)> =
        listed.iter().filter(|(n, _)| n.starts_with("faces-") && n.ends_with(".cache") && !n.contains(".corrupt-")).cloned().collect();
    faces.sort();
    files.extend(faces);
    let mut corrupt: Vec<(String, u64)> = listed.into_iter().filter(|(n, _)| n.contains(".corrupt-")).collect();
    corrupt.sort();
    (files, corrupt)
}

/// The process's resident memory: `VmRSS` of `/proc/self/status` on Linux; `None` elsewhere (no
/// pure-Rust way to ask the system there).
fn resident() -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.trim_start_matches("VmRSS:").trim().trim_end_matches("kB").trim().parse().ok()?;
    kb.checked_mul(1024)
}

/// Starts a backup count unless one runs.
fn start_count(s: &mut Session, dir: &Path) -> Result<(), CmdError> {
    if with_kept(dir, |k| std::mem::replace(&mut k.counting, true)) {
        return Ok(());
    }
    let folders: Vec<PathBuf> = s.library().folders().take(MAX_FOLDERS).map(|f| f.path.clone()).collect();
    let now = s.clock.now();
    let (_, cancel, answer) = s.jobs.start_answered(JobKind::Analysis, COUNT_LABEL, true, now);
    let dir2 = dir.to_path_buf();
    let spawned = std::thread::Builder::new().name("dalica-count-backups".into()).spawn(move || {
        let counted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| count_backups(&folders, &cancel)));
        let answered: Answer = match counted {
            Ok(Some(b)) => {
                with_kept(&dir2, |k| {
                    k.backups = Some(b);
                    k.counting = false;
                });
                (
                    Outcome::Done(json!({ "folders": b.with_backups, "files": b.files, "bytes": b.bytes, "sidecars": b.sidecars })),
                    "Counted the backups".into(),
                )
            }
            Ok(None) => {
                with_kept(&dir2, |k| k.counting = false);
                (Outcome::Cancelled(json!({})), "Backup count cancelled".into())
            }
            Err(_) => {
                with_kept(&dir2, |k| k.counting = false);
                (Outcome::Failed("the count stopped unexpectedly".into()), "Could not count the backups".into())
            }
        };
        // The session may have closed meanwhile; nobody is waiting then.
        let _ = answer.send(answered);
    });
    if let Err(e) = spawned {
        with_kept(dir, |k| k.counting = false);
        return Err(CmdError::Limit(format!("cannot start a worker: {e}")));
    }
    Ok(())
}

/// Each folder's `.dalica-originals/` (its files, the originals record left out) and whether it
/// has a sidecar; `None` when cancelled.
fn count_backups(folders: &[PathBuf], cancel: &AtomicBool) -> Option<Backups> {
    let mut b = Backups { folders: u64::try_from(folders.len()).unwrap_or(u64::MAX), ..Backups::default() };
    for folder in folders {
        if cancel.load(Ordering::SeqCst) {
            return None;
        }
        if std::fs::symlink_metadata(folder.join(SIDECAR)).is_ok_and(|m| m.is_file()) {
            b.sidecars = b.sidecars.saturating_add(1);
        }
        let Ok(entries) = std::fs::read_dir(folder.join(ORIGINALS)) else { continue };
        let mut any = false;
        for e in entries.flatten().take(MAX_BACKUPS_PER_FOLDER) {
            if e.file_name() == SIDECAR {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_file() {
                any = true;
                b.files = b.files.saturating_add(1);
                b.bytes = b.bytes.saturating_add(meta.len());
            }
        }
        if any {
            b.with_backups = b.with_backups.saturating_add(1);
        }
    }
    Some(b)
}

fn clear_cache(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let _: Empty = params(p)?;
    let cache = s.thumbs.screen_source().0.ok_or_else(|| CmdError::Disabled("Thumbnails are off: there is no cache to clear.".into()))?;
    if s.jobs.answered_running(JobKind::Rebuild, CLEAR_LABEL) > 0 {
        return Err(CmdError::Disabled("The thumbnail cache is being cleared.".into()));
    }
    let now = s.clock.now();
    let (job, _cancel, answer) = s.jobs.start_answered(JobKind::Rebuild, CLEAR_LABEL, false, now);
    let spawned = std::thread::Builder::new().name("dalica-clear-thumbs".into()).spawn(move || clear_work(&cache, &answer));
    if let Err(e) = spawned {
        // The worker's sender went with the closure: the job ends as failed at the next poll.
        return Err(CmdError::Limit(format!("cannot start a worker: {e}")));
    }
    Ok(json!({ "job": job, "pending": true }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

fn clear_work(cache: &Arc<Cache>, answer: &Sender<Answer>) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| clear(cache)))
        .unwrap_or_else(|_| Err("the clean-up stopped unexpectedly".to_string()));
    let answered = match outcome {
        Ok(v) => (Outcome::Done(v), "Cleared the thumbnail cache".to_string()),
        Err(e) => (Outcome::Failed(e), "Could not clear the thumbnail cache".to_string()),
    };
    let _ = answer.send(answered);
}

/// Empties `cache` through its own interface: every entry made dead, then each size compacted
/// into a new, empty generation (the old pack and index go as compaction removes them).
fn clear(cache: &Cache) -> Result<Value, String> {
    let before = cache.disk_bytes();
    let mut removed = 0u64;
    for size in Size::ALL {
        for (key, _) in cache.entries(size) {
            if cache.remove(size, key).map_err(|e| e.to_string())? {
                removed = removed.saturating_add(1);
            }
        }
        cache.compact(size).map_err(|e| e.to_string())?;
    }
    cache.sync().map_err(|e| e.to_string())?;
    let after = cache.disk_bytes();
    Ok(json!({ "removed": removed, "freedBytes": before.saturating_sub(after), "diskBytes": after }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_files_list_ours_in_order_and_the_kept_aside() {
        let dir = tempfile::tempdir().unwrap();
        for (name, n) in [("settings.json", 3usize), ("index.log", 5), ("faces-b.cache", 2), ("albums.json.corrupt-20261106", 7), ("notes.txt", 1)] {
            std::fs::write(dir.path().join(name), vec![b'x'; n]).unwrap();
        }
        let (files, corrupt) = library_files(dir.path());
        assert_eq!(files, vec![("index.log".to_string(), 5), ("settings.json".to_string(), 3), ("faces-b.cache".to_string(), 2)]);
        assert_eq!(corrupt, vec![("albums.json.corrupt-20261106".to_string(), 7)]);
    }

    #[test]
    fn backups_count_files_not_the_record_and_stop_when_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(a.join(ORIGINALS)).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join(ORIGINALS).join("IMG_0001.jpg"), [0u8; 10]).unwrap();
        std::fs::write(a.join(ORIGINALS).join(SIDECAR), [0u8; 4]).unwrap();
        std::fs::write(b.join(SIDECAR), b"{}").unwrap();
        let got = count_backups(&[a.clone(), b, dir.path().join("gone")], &AtomicBool::new(false)).unwrap();
        assert_eq!(got, Backups { folders: 3, with_backups: 1, files: 1, bytes: 10, sidecars: 1 });
        assert_eq!(count_backups(&[a], &AtomicBool::new(true)), None);
    }
}
