//! The metadata write-back end to end (`roadmap.md` M6.33; `data-model.md` §5, `xmp-interop.md`
//! §1–§5) on temporary JPEG, PNG, WebP and TIFF files: captions, tags and dates set by commands
//! reach every file and another reader (`dalica-meta`) sees them; undo writes the old value back;
//! with an option off nothing is written; the star ↔ rating rule; a read-only file gives one
//! failure event and the sidecar keeps the value; values changed elsewhere come back as one
//! undoable import, both changed reaches `photo.metadata`; the writes run as a `metadataWrite` job
//! with progress and Cancel, counted by `pendingWrites.metadata`; the owner thread makes no file
//! system call; the metadata hook sees the photos and fields of a star op, its undo and its redo.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{jpeg, open_with};
use dalica_core::{Clock, DateTime, PhotoId, SystemClock, Timestamp, VirtualClock};
use dalica_engine::settings::MetadataWrites;
use dalica_engine::{Field, Fields, JobKind, JobState, MetadataChange, Origin, Session};
use dalica_fs::Expect;
use dalica_meta::{Changes, Limits, OwnedFields, WriteOptions};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(30);

// Photo files.

fn png(seed: u64) -> Vec<u8> {
    dalica_scenes::png(&dalica_scenes::Scene::new(seed, 48, 32)).unwrap()
}

/// Writes bits least significant first, as VP8L reads them.
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, bits: u32) {
        self.acc |= u64::from(v) << self.n;
        self.n += bits;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// A simple-format lossless WebP (RFC 9649): no transforms, no colour cache, one group of simple
/// prefix codes drawing a two-green pattern.
fn webp(w: u32, h: u32) -> Vec<u8> {
    let mut b = Bits { out: vec![0x2f], acc: 0, n: 0 };
    b.put(w - 1, 14);
    b.put(h - 1, 14);
    b.put(0, 1); // no alpha
    b.put(0, 3); // version
    b.put(0, 1); // no transform
    b.put(0, 1); // no colour cache
    b.put(0, 1); // no meta prefix codes
    // Green: a simple code of two 8-bit symbols.
    for (v, n) in [(1, 1), (1, 1), (1, 1), (0x20, 8), (0xc0, 8)] {
        b.put(v, n);
    }
    // Red, blue, alpha: one 8-bit symbol each; distance: one 1-bit symbol.
    for v in [0x40, 0x90, 0xff] {
        b.put(1, 1);
        b.put(0, 1);
        b.put(1, 1);
        b.put(v, 8);
    }
    for (v, n) in [(1, 1), (0, 1), (0, 1), (0, 1)] {
        b.put(v, n);
    }
    for y in 0..h {
        for x in 0..w {
            b.put(u32::from((x * 3 + y * 5) % 7 < 3), 1);
        }
    }
    let vp8l = b.finish();
    let mut body = b"WEBP".to_vec();
    body.extend(b"VP8L");
    body.extend((vp8l.len() as u32).to_le_bytes());
    body.extend(&vp8l);
    if vp8l.len() % 2 == 1 {
        body.push(0);
    }
    let mut out = b"RIFF".to_vec();
    out.extend((body.len() as u32).to_le_bytes());
    out.extend(body);
    out
}

/// A baseline little-endian TIFF: one strip of 8-bit grey, no metadata.
fn tiff(w: u16, h: u16) -> Vec<u8> {
    let short = |tag: u16, v: u16| (tag, 3u16, u32::from(v));
    let pixels = u32::from(w) * u32::from(h);
    let entries = [
        short(256, w),
        short(257, h),
        short(258, 8),
        short(259, 1),
        short(262, 1),
        (273, 4, 8 + 2 + 9 * 12 + 4),
        short(277, 1),
        short(278, h),
        (279, 4, pixels),
    ];
    let mut out = b"II".to_vec();
    out.extend(42u16.to_le_bytes());
    out.extend(8u32.to_le_bytes());
    out.extend((entries.len() as u16).to_le_bytes());
    for (tag, kind, value) in entries {
        out.extend(tag.to_le_bytes());
        out.extend(kind.to_le_bytes());
        out.extend(1u32.to_le_bytes());
        out.extend(value.to_le_bytes());
    }
    out.extend(0u32.to_le_bytes());
    out.extend((0..pixels).map(|i| (i * 37 % 251) as u8));
    out
}

/// `bytes` (a JPEG) with an XMP packet another application wrote: a rating of `rating`.
fn with_rating(bytes: &[u8], rating: i8) -> Vec<u8> {
    let packet = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
         xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" \
         xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"{rating}\"/></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    );
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend(packet.as_bytes());
    let mut out = bytes[..2].to_vec();
    out.extend([0xff, 0xe1]);
    out.extend(((payload.len() + 2) as u16).to_be_bytes());
    out.extend(payload);
    out.extend(&bytes[2..]);
    out
}

/// A library watching one folder `Lisbon` holding `files`; ids by name.
struct Lib {
    _library: tempfile::TempDir,
    _disk: tempfile::TempDir,
    folder: PathBuf,
    s: Session,
}

impl Lib {
    fn new(files: &[(&str, Vec<u8>)]) -> Lib {
        Lib::with_clock(files, Arc::new(SystemClock))
    }

    fn with_clock(files: &[(&str, Vec<u8>)], clock: Arc<dyn Clock>) -> Lib {
        let (library, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let folder = disk.path().join("Lisbon");
        dalica_fs::create_dir_all(&folder).unwrap();
        for (name, bytes) in files {
            dalica_fs::replace(&folder.join(name), bytes, Expect::Absent).unwrap();
        }
        let mut s = open_with(library.path(), clock);
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
        s.wait_idle();
        assert_eq!(s.library().photos().count(), files.len(), "every file is a photo: {:?}", s.scan_warnings());
        Lib { _library: library, _disk: disk, folder, s }
    }

    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap().id
    }

    fn run(&mut self, command: &str, params: Value) -> Value {
        self.s.execute(command, &params).unwrap()
    }

    fn flush(&mut self) {
        assert!(self.s.flush_metadata(WAIT), "the writes landed in time");
        assert_eq!(self.s.pending_metadata_writes(), 0);
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    fn read(&self, name: &str) -> OwnedFields {
        dalica_meta::read(&fs::read(self.path(name)).unwrap(), &Limits::default()).unwrap().owned
    }

    fn set_writes(&mut self, f: impl FnOnce(&mut MetadataWrites)) {
        let mut w = self.s.settings().metadata_writes();
        f(&mut w);
        self.s.change_settings(|st| st.set_metadata_writes(&w));
    }

    fn sidecar(&mut self) -> Value {
        self.s.flush().unwrap();
        fs::read(self.folder.join(".dalica.json")).map_or(Value::Null, |b| serde_json::from_slice(&b).unwrap())
    }

    fn metadata_jobs(&self) -> Vec<dalica_engine::JobEntry> {
        self.s.jobs().into_iter().filter(|j| j.kind == JobKind::MetadataWrite).collect()
    }
}

fn dt(s: &str) -> DateTime {
    s.parse().unwrap()
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| t.to_string()).collect()
}

/// Another application writes `changes` into the file at `path`.
fn other_app(path: &Path, changes: &Changes) {
    let bytes = fs::read(path).unwrap();
    let new = dalica_meta::write_owned(&bytes, changes, &WriteOptions::default()).unwrap();
    let id = dalica_fs::identity(path).unwrap().unwrap();
    dalica_fs::replace(path, &new, Expect::Unchanged(id)).unwrap();
}

fn set_read_only(p: &Path, read_only: bool) {
    let mut perms = fs::metadata(p).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(if read_only { 0o444 } else { 0o644 });
    }
    #[cfg(not(unix))]
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(read_only);
    fs::set_permissions(p, perms).unwrap();
}

const NAMES: [&str; 4] = ["a.jpg", "b.png", "c.webp", "d.tif"];

fn four() -> Lib {
    Lib::new(&[(NAMES[0], jpeg(1)), (NAMES[1], png(2)), (NAMES[2], webp(9, 7)), (NAMES[3], tiff(12, 8))])
}

#[test]
fn caption_tags_and_a_date_reach_every_container_and_another_reader_sees_them() {
    let mut l = four();
    let ids: Vec<PhotoId> = NAMES.iter().map(|n| l.id(n)).collect();
    let times: Vec<_> = NAMES.iter().map(|n| fs::metadata(l.path(n)).unwrap().modified().unwrap()).collect();
    l.run("photo.caption.set", json!({ "ids": ids, "caption": "Tram 28 à Alfama" }));
    l.run("photo.tags.add", json!({ "ids": ids, "tags": ["Lisbon", "tram/yellow"] }));
    l.run("photo.dates.set", json!({ "ids": ids, "set": "2024-05-06T07:08:09" }));
    assert_eq!(l.s.pending_metadata_writes(), 4, "one write per photo, every field batched");
    assert_eq!(l.s.pending_writes().map(|p| p.metadata), Some(4));
    l.flush();
    assert_eq!(l.s.pending_writes(), None);
    for (i, name) in NAMES.iter().enumerate() {
        let f = l.read(name);
        assert_eq!(f.caption.as_deref(), Some("Tram 28 à Alfama"), "{name}");
        assert_eq!(f.tags, tags(&["Lisbon", "tram/yellow"]), "{name}");
        assert_eq!(f.date_taken.map(|d| d.date_time()), Some(dt("2024-05-06T07:08:09")), "{name}");
        // The modification time is put back; the photo takes the file's new size, so a rescan
        // sees nothing new; the last-synced hashes are the file's.
        let meta = fs::metadata(l.path(name)).unwrap();
        assert_eq!(meta.modified().unwrap(), times[i], "{name}");
        let p = l.s.library().photo(ids[i]).unwrap();
        assert_eq!(p.size, meta.len(), "{name}");
        assert!(l.s.library().synced(ids[i]).is_some_and(|s| s.caption.is_some() && s.tags.is_some()), "{name}");
    }
    // One job did it, and says so.
    let jobs = l.metadata_jobs();
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    assert_eq!((jobs[0].state, jobs[0].label.as_str()), (JobState::Done, "Metadata written"));
    assert_eq!(jobs[0].result.as_ref().unwrap()["written"], json!(4));
    assert_eq!((jobs[0].progress.done, jobs[0].progress.total), (4, 4));
    assert_eq!(l.s.metadata_events().count(), 0);
    // Nothing comes back from the files: the model is what the commands made it.
    assert_eq!(l.s.undo_stack().len(), 3);
    assert!(ids.iter().all(|id| l.s.library().caption(*id) == "Tram 28 à Alfama"));
}

#[test]
fn undo_writes_the_old_value_back_and_redo_the_new_one() {
    let mut l = four();
    let ids: Vec<PhotoId> = NAMES.iter().map(|n| l.id(n)).collect();
    l.run("photo.caption.set", json!({ "ids": ids, "caption": "First" }));
    l.flush();
    l.run("photo.caption.set", json!({ "ids": ids, "caption": "Second" }));
    l.run("photo.tags.add", json!({ "ids": ids, "tags": ["beach"] }));
    l.flush();
    l.run("edit.undo", json!({}));
    l.run("edit.undo", json!({}));
    l.flush();
    for name in NAMES {
        let f = l.read(name);
        assert_eq!((f.caption.as_deref(), f.tags.len()), (Some("First"), 0), "{name}");
    }
    l.run("edit.redo", json!({}));
    l.flush();
    assert!(NAMES.iter().all(|n| l.read(n).caption.as_deref() == Some("Second")));
    // Undoing the first caption clears it from the files.
    l.run("edit.undo", json!({}));
    l.run("edit.undo", json!({}));
    l.flush();
    assert!(NAMES.iter().all(|n| l.read(n).caption.is_none()));
}

#[test]
fn with_an_option_off_nothing_is_written() {
    let mut l = four();
    let ids: Vec<PhotoId> = NAMES.iter().map(|n| l.id(n)).collect();
    let bytes: Vec<Vec<u8>> = NAMES.iter().map(|n| fs::read(l.path(n)).unwrap()).collect();
    l.set_writes(|w| {
        w.captions_tags_places = false;
        w.dates = false;
    });
    l.run("photo.caption.set", json!({ "ids": ids, "caption": "Not in the files" }));
    l.run("photo.tags.add", json!({ "ids": ids, "tags": ["kept"] }));
    l.run("photo.dates.set", json!({ "ids": ids, "set": "2020-01-02T03:04:05" }));
    assert_eq!(l.s.pending_metadata_writes(), 0, "nothing is queued");
    l.flush();
    for (i, name) in NAMES.iter().enumerate() {
        assert_eq!(fs::read(l.path(name)).unwrap(), bytes[i], "{name} unchanged");
    }
    assert!(l.metadata_jobs().is_empty());
    // The sidecar holds the values, and the hook still saw the changes.
    let sidecar = l.sidecar();
    assert_eq!(sidecar["photos"]["a.jpg"]["caption"], json!("Not in the files"));
    let (changes, _) = l.s.take_metadata_changes();
    assert_eq!(changes.len(), 3);
    // Dates on again: only the date goes into the files at the next date change.
    l.set_writes(|w| w.dates = true);
    l.run("photo.dates.set", json!({ "ids": [ids[0]], "set": "2021-01-02T03:04:05" }));
    l.flush();
    let f = l.read(NAMES[0]);
    assert_eq!((f.caption, f.date_taken.map(|d| d.date_time())), (None, Some(dt("2021-01-02T03:04:05"))));
}

#[test]
fn the_star_rating_rule() {
    let mut l = Lib::new(&[("a.jpg", jpeg(1)), ("b.jpg", with_rating(&jpeg(2), 3))]);
    let (a, b) = (l.id("a.jpg"), l.id("b.jpg"));
    let rating = |l: &Lib, n: &str| l.read(n).rating;
    assert_eq!(rating(&l, "b.jpg"), Some(3));
    // Off (the default): a star stays in the sidecar.
    l.run("photo.star.set", json!({ "ids": [a, b], "on": true }));
    assert_eq!(l.s.pending_metadata_writes(), 0);
    l.flush();
    assert_eq!((rating(&l, "a.jpg"), rating(&l, "b.jpg")), (None, Some(3)));
    // On: unstarring a photo whose rating Dalica never set leaves another application's 3.
    l.set_writes(|w| w.stars_as_ratings = true);
    l.run("photo.star.set", json!({ "ids": [b], "on": false }));
    l.flush();
    assert_eq!(rating(&l, "b.jpg"), Some(3), "another application's rating survives");
    // Starring writes 5; unstarring takes back only the 5 Dalica wrote.
    l.run("photo.star.set", json!({ "ids": [a, b], "on": false }));
    l.run("photo.star.set", json!({ "ids": [a], "on": true }));
    l.flush();
    assert_eq!(rating(&l, "a.jpg"), Some(5));
    l.run("photo.star.toggle", json!({ "ids": [a] }));
    l.flush();
    assert_eq!(rating(&l, "a.jpg"), Some(0));
    // Undo of the unstar writes the star back.
    l.run("edit.undo", json!({}));
    l.flush();
    assert_eq!(rating(&l, "a.jpg"), Some(5));
}

#[test]
fn a_read_only_file_fails_once_and_the_sidecar_keeps_the_value() {
    let mut l = Lib::new(&[("a.jpg", jpeg(1)), ("b.jpg", jpeg(2))]);
    let (a, b) = (l.id("a.jpg"), l.id("b.jpg"));
    let bytes = fs::read(l.path("a.jpg")).unwrap();
    set_read_only(&l.path("a.jpg"), true);
    l.run("photo.caption.set", json!({ "ids": [a, b], "caption": "Kept" }));
    l.flush();
    let events: Vec<_> = l.s.metadata_events().cloned().collect();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!((events[0].photo, events[0].fields.as_slice()), (a, [Field::Caption].as_slice()));
    let message = events[0].message();
    assert!(message.starts_with("Could not write the caption into a.jpg: ") && message.contains("read-only"), "{message}");
    assert_eq!(fs::read(l.path("a.jpg")).unwrap(), bytes, "the file is as it was");
    assert_eq!(l.read("b.jpg").caption.as_deref(), Some("Kept"), "the other photo is written");
    assert_eq!(l.sidecar()["photos"]["a.jpg"]["caption"], json!("Kept"), "the sidecar keeps the value");
    assert_eq!(l.s.library().caption(a), "Kept");
    let job = &l.metadata_jobs()[0];
    assert_eq!(job.result.as_ref().unwrap()["failed"], json!(1));
    // Polling again reports nothing new.
    l.s.poll();
    assert_eq!(l.s.metadata_events().count(), 1);
    set_read_only(&l.path("a.jpg"), false);
}

#[test]
fn a_value_changed_elsewhere_comes_back_as_one_undoable_import() {
    let mut l = Lib::new(&[("a.jpg", jpeg(1))]);
    let a = l.id("a.jpg");
    l.run("photo.caption.set", json!({ "ids": [a], "caption": "Ours" }));
    l.flush();
    other_app(&l.path("a.jpg"), &Changes { tags: Some(tags(&["theirs"])), ..Changes::default() });
    l.run("photo.caption.set", json!({ "ids": [a], "caption": "Ours again" }));
    l.flush();
    assert_eq!(l.s.library().tags_of(a), tags(&["theirs"]), "imported");
    assert_eq!(l.s.undo_stack().undo_label(), Some("Import Metadata"));
    let f = l.read("a.jpg");
    assert_eq!((f.caption.as_deref(), f.tags.clone()), (Some("Ours again"), tags(&["theirs"])));
    assert_eq!(l.metadata_jobs().last().unwrap().result.as_ref().unwrap()["imported"], json!(1));
    // Nothing went back for the import; undoing it is a command, written back like any other.
    assert_eq!(l.s.pending_metadata_writes(), 0);
    l.run("edit.undo", json!({}));
    assert!(l.s.library().tags_of(a).is_empty());
    l.flush();
    assert!(l.read("a.jpg").tags.is_empty());
}

#[test]
fn both_changed_reaches_photo_metadata_and_dalica_s_value_is_kept() {
    let mut l = Lib::new(&[("a.jpg", jpeg(1))]);
    let a = l.id("a.jpg");
    l.run("photo.caption.set", json!({ "ids": [a], "caption": "A" }));
    l.flush();
    other_app(&l.path("a.jpg"), &Changes { caption: Some(Some("B".into())), ..Changes::default() });
    l.run("photo.caption.set", json!({ "ids": [a], "caption": "C" }));
    l.flush();
    assert_eq!(l.read("a.jpg").caption.as_deref(), Some("C"), "Dalica's value is written");
    assert_eq!(l.s.library().caption(a), "C");
    l.run("photo.metadata", json!({ "id": a }));
    assert!(l.s.wait_photo_metadata(WAIT));
    let v = l.run("photo.metadata", json!({ "id": a }));
    assert_eq!(v["changedElsewhere"], json!({ "fields": ["caption"], "note": "Changed in another application; Dalica's value kept" }));
    assert_eq!(l.metadata_jobs().last().unwrap().result.as_ref().unwrap()["conflicts"], json!(1));
}

#[test]
fn writes_run_as_a_job_with_progress_that_cancel_stops() {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)));
    let files: Vec<(String, Vec<u8>)> = (0..24).map(|i| (format!("IMG_{i:04}.jpg"), jpeg(i))).collect();
    let refs: Vec<(&str, Vec<u8>)> = files.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    let mut l = Lib::with_clock(&refs, clock.clone());
    let ids: Vec<PhotoId> = l.s.library().photos().map(|p| p.id).collect();
    l.run("photo.caption.set", json!({ "ids": ids, "caption": "Batch" }));
    assert_eq!(l.s.pending_metadata_writes(), 24);
    // Debounced: nothing is due before a second of quiet, so no job yet.
    poll_for(&mut l.s, Duration::from_millis(300));
    assert!(l.metadata_jobs().is_empty());
    clock.advance(Duration::from_secs(2));
    let start = Instant::now();
    let job = loop {
        l.s.poll();
        if let Some(j) = l.metadata_jobs().pop() {
            break j;
        }
        assert!(start.elapsed() < WAIT, "a job starts");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!((job.label.as_str(), job.cancellable, job.progress.total), ("Writing metadata", true, 24));
    assert_eq!(l.s.cancel_jobs(Some(job.job)).unwrap(), [job.job]);
    let end = l.s.wait_job(job.job, WAIT).unwrap();
    assert_eq!(end.state, JobState::Cancelled);
    let not_written = end.result.as_ref().unwrap()["notWritten"].as_u64().unwrap();
    assert!(not_written > 0, "{end:?}");
    let written = files.iter().filter(|(n, _)| l.read(n).caption.is_some()).count() as u64;
    assert_eq!(written + not_written, 24);
    assert_eq!(l.s.pending_metadata_writes(), 0, "cancelled writes leave the queue; the sidecar keeps the values");
    assert!(l.s.library().photos().all(|p| l.s.library().caption(p.id) == "Batch"));
    // The next change starts a new job, which runs to the end with its progress.
    l.run("photo.caption.set", json!({ "ids": [ids[0]], "caption": "One" }));
    clock.advance(Duration::from_secs(2));
    let start = Instant::now();
    let done = loop {
        l.s.poll();
        if let Some(j) = l.metadata_jobs().into_iter().find(|j| j.job != job.job && j.state.is_finished()) {
            break j;
        }
        assert!(start.elapsed() < WAIT, "the job ends");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!((done.state, done.progress.done, done.progress.total), (JobState::Done, 1, 1));
    assert_eq!(l.read(&files[0].0).caption.as_deref(), Some("One"));
}

fn poll_for(s: &mut Session, d: Duration) {
    let start = Instant::now();
    while start.elapsed() < d {
        s.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_owner_thread_makes_no_file_system_call() {
    let mut l = four();
    let ids: Vec<PhotoId> = NAMES.iter().map(|n| l.id(n)).collect();
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    l.run("photo.caption.set", json!({ "ids": ids, "caption": "Owner" }));
    l.run("photo.tags.add", json!({ "ids": ids, "tags": ["t"] }));
    l.run("photo.dates.set", json!({ "ids": ids, "set": "2023-03-03T03:03:03" }));
    l.s.poll();
    assert!(l.s.flush_metadata(WAIT));
    l.run("edit.undo", json!({}));
    assert!(l.s.flush_metadata(WAIT));
    l.s.poll();
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "no fs call was refused on the owner thread");
    for name in NAMES {
        let f = l.read(name);
        // A date is never removed from a file (`xmp-interop.md` §5): undoing it leaves the file's.
        let date = f.date_taken.map(|d| d.date_time());
        assert_eq!((f.caption.as_deref(), f.tags.clone(), date), (Some("Owner"), tags(&["t"]), Some(dt("2023-03-03T03:03:03"))), "{name}");
    }
}

#[test]
fn a_write_whose_folder_is_gone_waits_and_lands_when_it_returns() {
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_700_000_000_000)));
    let mut l = Lib::with_clock(&[("a.jpg", jpeg(1))], clock.clone());
    let a = l.id("a.jpg");
    let away = l.folder.with_file_name("Lisbon (away)");
    l.run("photo.caption.set", json!({ "ids": [a], "caption": "Later" }));
    fs::rename(&l.folder, &away).unwrap();
    assert!(l.s.flush_metadata(WAIT));
    assert_eq!(l.s.pending_metadata_writes(), 1, "still queued");
    assert_eq!(l.s.pending_writes().map(|p| p.metadata), Some(1));
    assert_eq!(l.metadata_jobs()[0].result.as_ref().unwrap()["waiting"], json!(1));
    assert_eq!(l.s.metadata_events().count(), 0, "waiting is not a failure");
    fs::rename(&away, &l.folder).unwrap();
    clock.advance(Duration::from_secs(11));
    let start = Instant::now();
    while l.s.pending_metadata_writes() > 0 {
        assert!(start.elapsed() < WAIT, "the write lands");
        l.s.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(l.read("a.jpg").caption.as_deref(), Some("Later"));
    assert_eq!(l.s.library().caption(a), "Later");
}

// The metadata hook (M6.5), which the write-back follows.

/// A session over one folder of `n` photos, its ids in name order, the folder.
fn starred_library(n: usize) -> (tempfile::TempDir, Session, Vec<PhotoId>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64), Expect::Absent).unwrap();
    }
    let mut s = crate::common::open(&dir.path().join("library"));
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut photos: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    photos.sort();
    (dir, s, photos.into_iter().map(|(_, id)| id).collect(), folder)
}

#[test]
fn the_metadata_hook_sees_the_photos_and_fields_of_a_star_op_its_undo_and_its_redo() {
    let (_dir, mut s, ids, _) = starred_library(4);
    let (_, dropped) = s.take_metadata_changes();
    assert_eq!(dropped, 0);
    let star = Fields::of(&[Field::Star]);
    let expect = |photos: &[PhotoId]| vec![MetadataChange { origin: Origin::Command, photos: photos.iter().map(|id| (*id, star)).collect() }];

    // ids[1] is starred first, so the toggle below changes only ids[0] and ids[2].
    s.execute("photo.star.set", &json!({ "ids": [ids[1]], "on": true })).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[1]]), 0));
    s.execute("photo.star.set", &json!({ "ids": [ids[0], ids[1], ids[2]], "on": true })).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[0], ids[2]]), 0), "only the photos whose star changed");

    // Undo and redo reach the hook too, with the same photos and fields.
    s.execute("edit.undo", &json!({})).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[0], ids[2]]), 0));
    assert!(!s.library().photo(ids[0]).unwrap().star);
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.take_metadata_changes(), (expect(&[ids[0], ids[2]]), 0));

    // What changes nothing tells the hook nothing; queries and polling neither.
    s.execute("photo.star.set", &json!({ "ids": [ids[0]], "on": true })).unwrap();
    s.execute("library.query", &json!({ "starred": true })).unwrap();
    s.poll();
    assert_eq!(s.take_metadata_changes(), (Vec::new(), 0));

    // Changes queue up until taken, oldest first; `poll` leaves them for the writer (M6.33).
    s.execute("photo.star.toggle", &json!({ "ids": [ids[3]] })).unwrap();
    s.execute("edit.undo", &json!({})).unwrap();
    s.poll();
    let (changes, _) = s.take_metadata_changes();
    assert_eq!(changes, [expect(&[ids[3]]), expect(&[ids[3]])].concat());
}
