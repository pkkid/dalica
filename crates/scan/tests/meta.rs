//! What a scan reads from the photo files and what the diff makes of it (`xmp-interop.md` §5, §7;
//! `roadmap.md` M6.32) on temp folders: the owned fields of new and changed photos only, from
//! JPEG, PNG and a RAW's `.xmp` sidecar; a first scan's import in the `AddPhotos` batch; a value
//! changed elsewhere imported by the sync op and a field changed on both sides left to Dalica (and
//! reported by `meta::conflicts`); a sync op decided before a model change decided again by a new
//! diff; the place never imported; unreadable and truncated files bring nothing and never panic.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::common::apply_all;
use dalica_core::{DateTime, PhotoId};
use dalica_library::sync::Field;
use dalica_library::{Library, Op};
use dalica_meta::xmp_sidecar::{self, SidecarWrite};
use dalica_meta::{Changes, DateTaken, Place, WriteOptions};
use dalica_scan::meta::{conflicts, sync_parts, undo_of_import};
use dalica_scan::{FoundPhoto, Known, Options, ReadMetadata, Scan, diff, scan_with};

fn date(s: &str) -> DateTime {
    s.parse().unwrap()
}

fn time(n: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(981_173_106 + n)
}

fn options() -> WriteOptions {
    WriteOptions { stars_as_ratings: true, ..WriteOptions::default() }
}

fn caption(c: &str) -> Changes {
    Changes { caption: Some(Some(c.into())), ..Changes::default() }
}

fn plain_jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 64, 48), 85, None).unwrap()
}

/// A JPEG another tool tagged: caption, keywords, rating 5, a place and a date taken.
fn tagged_jpeg(seed: u64) -> Vec<u8> {
    let changes = Changes {
        caption: Some(Some("  Elétrico 28 ".into())),
        tags: Some(vec!["lisbon".into(), "tram".into(), "Lisbon".into()]),
        star: Some(true),
        place: Some(Place::new(38.71, -9.14)),
        date_taken: Some(DateTaken::local(date("2026-06-04T19:42:10"))),
    };
    dalica_meta::write_owned(&plain_jpeg(seed), &changes, &options()).unwrap()
}

/// Writes `bytes` at `path` with modification time `time(n)`.
fn put(path: &Path, bytes: &[u8], n: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    std::fs::File::options().write(true).open(path).unwrap().set_modified(time(n)).unwrap();
}

/// Another application edits `path`'s metadata; the file gets time `time(n)`.
fn other_app(path: &Path, changes: Changes, n: u64) {
    let bytes = std::fs::read(path).unwrap();
    put(path, &dalica_meta::write_owned(&bytes, &changes, &options()).unwrap(), n);
}

const CR3: &[u8] = b"\0\0\0\x18ftypcrx \0\0\0\x01 and the rest of a Canon RAW file";

/// `dir` with `tagged.jpg`, `plain.jpg`, `caption.png`, `broken.jpg` (a JPEG's start and junk),
/// `IMG_0001.CR3` with its `IMG_0001.xmp` and `IMG_0002.CR3` with none.
fn folder(dir: &Path) {
    put(&dir.join("tagged.jpg"), &tagged_jpeg(1), 0);
    put(&dir.join("plain.jpg"), &plain_jpeg(2), 0);
    let png = dalica_scenes::png(&dalica_scenes::Scene::new(3, 64, 48)).unwrap();
    put(&dir.join("caption.png"), &dalica_meta::write_owned(&png, &caption("A PNG"), &options()).unwrap(), 0);
    let mut broken = plain_jpeg(4);
    broken.truncate(40);
    broken.extend_from_slice(&[0xff; 64]);
    put(&dir.join("broken.jpg"), &broken, 0);
    put(&dir.join("IMG_0001.CR3"), CR3, 0);
    put(&dir.join("IMG_0002.CR3"), CR3, 0);
    let raw = Changes { tags: Some(vec!["raw".into()]), star: Some(true), ..caption("From the sidecar") };
    let Ok(SidecarWrite::Write(xmp)) = xmp_sidecar::merge(None, None, &raw, &options()) else { panic!("a new sidecar") };
    put(&dir.join("IMG_0001.xmp"), &xmp, 0);
}

fn reading(metadata: ReadMetadata) -> Options {
    Options { metadata, ..Options::default() }
}

fn found<'a>(scan: &'a Scan, name: &str) -> &'a FoundPhoto {
    scan.folders.iter().flat_map(|f| &f.photos).find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed"))
}

fn id_of(lib: &Library, dir: &Path, name: &str) -> PhotoId {
    lib.photo_by_name(lib.folder_by_path(dir).unwrap(), name).unwrap().id
}

#[test]
fn the_owned_fields_of_new_and_changed_photos_are_read_and_only_theirs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Lisbon");
    folder(&dir);
    let roots = [tmp.path().to_path_buf()];

    // Off (the default): nothing read.
    let off = scan_with(&roots, &Options::default());
    assert!(off.folders.iter().flat_map(|f| &f.photos).all(|p| p.file.is_none()));

    let scan = scan_with(&roots, &reading(ReadMetadata::all()));
    let v = &found(&scan, "tagged.jpg").file.as_ref().unwrap().0;
    assert_eq!((v.caption.as_str(), v.tags.as_slice(), v.star), ("Elétrico 28", &["lisbon".to_owned(), "tram".to_owned()][..], true));
    assert_eq!(v.date, Some(date("2026-06-04T19:42:10")));
    let (lat, lon) = v.place.unwrap();
    assert!((lat - 38.71).abs() < 1e-6 && (lon + 9.14).abs() < 1e-6);
    assert_eq!(found(&scan, "plain.jpg").file.as_ref().unwrap().0, Default::default());
    assert_eq!(found(&scan, "caption.png").file.as_ref().unwrap().0.caption, "A PNG");
    assert_eq!(found(&scan, "broken.jpg").file, None, "unreadable: nothing, and the scan goes on");
    let raw = &found(&scan, "IMG_0001.CR3").file.as_ref().unwrap().0;
    assert_eq!((raw.caption.as_str(), raw.tags.as_slice(), raw.star), ("From the sidecar", &["raw".to_owned()][..], true));
    assert_eq!(found(&scan, "IMG_0002.CR3").file.as_ref().unwrap().0, Default::default(), "a RAW with nothing to read");

    // What the model knows at its size and time is not read again; a changed file is.
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan);
    apply_all(&mut lib, ops);
    other_app(&dir.join("plain.jpg"), caption("Now with a caption"), 5);
    let again = scan_with(&roots, &reading(ReadMetadata::NewOrChanged(Arc::new(Known::of(&lib)))));
    for p in again.folders.iter().flat_map(|f| &f.photos) {
        assert_eq!(p.file.is_some(), p.name == "plain.jpg", "{}", p.name);
    }
    assert_eq!(found(&again, "plain.jpg").file.as_ref().unwrap().0.caption, "Now with a caption");

    // A cancelled scan stops before reading.
    let cancelled = Options { cancel: Some(Arc::new(AtomicBool::new(true))), ..reading(ReadMetadata::all()) };
    assert!(scan_with(&roots, &cancelled).folders.is_empty());
}

/// `xmp-interop.md` §5 rule 1: a photo Dalica has nothing for brings every value its file has in
/// its `AddPhotos` batch; the sync op sets every read photo's synced hashes and imports nothing;
/// applied, a second diff gives nothing.
#[test]
fn a_first_scan_imports_in_the_add_batch_and_the_sync_op_sets_the_hashes() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Lisbon");
    folder(&dir);
    // A sidecar entry for caption.png: Dalica has something for it, so its file is not imported.
    std::fs::write(dir.join(".dalica.json"), br#"{"dalica":{"version":1},"photos":{"caption.png":{"caption":"Ours"}}}"#).unwrap();
    let scan = scan_with(&[tmp.path().to_path_buf()], &reading(ReadMetadata::all()));
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan);
    let Some(Op::Batch { ops: add }) = ops.iter().find(|o| matches!(o, Op::Batch { ops } if matches!(ops.first(), Some(Op::AddPhotos { .. }))))
    else {
        panic!("an add batch: {ops:?}")
    };
    let [Op::AddPhotos { photos }, Op::SetPhotoInfo { photos: infos }] = add.as_slice() else { panic!("{add:?}") };
    let starred: Vec<&str> = photos.iter().filter(|p| p.star).map(|p| p.name.as_str()).collect();
    assert_eq!(starred, ["IMG_0001.CR3", "tagged.jpg"], "a rating of 5 is a star");
    let caption_of = |name: &str| {
        let id = photos.iter().find(|p| p.name == name).unwrap().id;
        infos.iter().find(|(i, _)| *i == id).map(|(_, info)| info.caption.clone())
    };
    assert_eq!(caption_of("tagged.jpg").as_deref(), Some("Elétrico 28"));
    assert_eq!(caption_of("caption.png").as_deref(), Some("Ours"), "the sidecar's value, not the file's");
    assert_eq!(caption_of("plain.jpg"), None);
    let sync = ops.last().unwrap();
    let parts = sync_parts(sync).unwrap();
    assert!(parts.imports.is_empty(), "a first scan is no undoable import");
    assert_eq!(parts.synced.len(), 5, "every photo read: six listed, broken.jpg unreadable");

    apply_all(&mut lib, ops);
    let tagged = id_of(&lib, &dir, "tagged.jpg");
    assert_eq!(lib.tags_of(tagged), ["lisbon", "tram"]);
    assert_eq!(lib.info(tagged).date, None, "the file's own date is not a user date");
    assert_eq!(lib.effective_date(tagged), Some(date("2026-06-04T19:42:10")));
    assert!(lib.synced(tagged).is_some());
    assert_eq!(diff(&mut lib, &scan), vec![], "applied, diffed again: nothing");
}

/// `xmp-interop.md` §5 rule 3 on a rescan: a value changed elsewhere is imported by the sync op
/// (its undo is the import alone); changed on both sides, Dalica's value stays and
/// `meta::conflicts` names the field; the place is never imported nor reported.
#[test]
fn a_rescan_imports_what_changed_elsewhere_and_leaves_a_conflict_to_dalica() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Lisbon");
    folder(&dir);
    let roots = [tmp.path().to_path_buf()];
    let mut lib = Library::new();
    let first = scan_with(&roots, &reading(ReadMetadata::all()));
    let ops = diff(&mut lib, &first);
    apply_all(&mut lib, ops);
    let tagged = id_of(&lib, &dir, "tagged.jpg");

    // Changed elsewhere: caption, rating and place.
    let changes = Changes { star: Some(false), place: Some(Place::new(41.15, -8.61)), ..caption("Tram 28 at night") };
    other_app(&dir.join("tagged.jpg"), changes, 10);
    let rescan = |lib: &Library| scan_with(&roots, &reading(ReadMetadata::NewOrChanged(Arc::new(Known::of(lib)))));
    let scan = rescan(&lib);
    let ops = diff(&mut lib, &scan);
    assert!(matches!(ops.first(), Some(Op::UpdatePhotos { .. })), "{ops:?}");
    let sync = ops.last().unwrap().clone();
    let parts = sync_parts(&sync).unwrap();
    assert_eq!(
        parts.imports,
        &[Op::SetCaptions { captions: vec![(tagged, "Tram 28 at night".into())] }, Op::SetStars { stars: vec![(tagged, false)] }][..],
        "no place: the model holds none"
    );
    let file = parts.synced.iter().find(|(i, _)| *i == tagged).unwrap().1.unwrap();
    assert_eq!(conflicts(&lib, tagged, &file), vec![], "only the file changed");
    let old_synced = *lib.synced(tagged).unwrap();
    let mut inverse = None;
    for op in ops {
        inverse = Some(lib.apply(op).unwrap());
    }
    assert_eq!((lib.caption(tagged), lib.photo(tagged).unwrap().star), ("Tram 28 at night", false));
    assert_ne!(lib.synced(tagged), Some(&old_synced));
    // Undoing the import changes the values back and leaves the synced hashes the file's.
    let undo = undo_of_import(&inverse.unwrap()).unwrap();
    let synced = *lib.synced(tagged).unwrap();
    lib.apply(undo).unwrap();
    assert_eq!((lib.caption(tagged), lib.photo(tagged).unwrap().star), ("Elétrico 28", true));
    assert_eq!(lib.synced(tagged), Some(&synced));
    assert_eq!(diff(&mut lib, &scan), vec![], "no import again: Dalica's value is now the one to write");

    // Both changed: Dalica's caption stays and the conflict is named; tags changed only outside
    // still come in.
    lib.apply(Op::SetCaptions { captions: vec![(tagged, "Ours".into())] }).unwrap();
    other_app(&dir.join("tagged.jpg"), Changes { tags: Some(vec!["night".into()]), ..caption("Theirs") }, 20);
    let scan = rescan(&lib);
    let ops = diff(&mut lib, &scan);
    let sync = ops.last().unwrap().clone();
    let parts = sync_parts(&sync).unwrap();
    assert_eq!(parts.imports, &[Op::SetTags { tags: vec![(tagged, vec!["night".into()])] }][..]);
    let file = parts.synced.iter().find(|(i, _)| *i == tagged).unwrap().1.unwrap();
    apply_all(&mut lib, ops.into_iter().take_while(|o| sync_parts(o).is_none()).collect());
    assert_eq!(conflicts(&lib, tagged, &file), vec![Field::Caption], "judged before the sync op");
    lib.apply(sync).unwrap();
    assert_eq!((lib.caption(tagged), lib.tags_of(tagged)), ("Ours", &["night".to_owned()][..]));
    assert_eq!(diff(&mut lib, &scan), vec![]);
}

/// A sync op decided before the model changed (a command run between a folder's diff and its sync
/// op) is decided again by a new diff: the user's edit stands, and the field becomes a conflict.
#[test]
fn a_sync_op_decided_before_a_model_change_is_decided_again() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Lisbon");
    folder(&dir);
    let roots = [tmp.path().to_path_buf()];
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan_with(&roots, &reading(ReadMetadata::all())));
    apply_all(&mut lib, ops);
    let tagged = id_of(&lib, &dir, "tagged.jpg");
    other_app(&dir.join("tagged.jpg"), caption("Theirs"), 10);
    let scan = scan_with(&roots, &reading(ReadMetadata::NewOrChanged(Arc::new(Known::of(&lib)))));
    let ops = diff(&mut lib, &scan);
    let stale = ops.last().cloned().unwrap();
    assert_eq!(sync_parts(&stale).unwrap().imports, &[Op::SetCaptions { captions: vec![(tagged, "Theirs".into())] }][..]);
    apply_all(&mut lib, ops.into_iter().take_while(|o| sync_parts(o).is_none()).collect());
    // The user's edit lands in between.
    lib.apply(Op::SetCaptions { captions: vec![(tagged, "Mine".into())] }).unwrap();
    let again = diff(&mut lib, &scan);
    assert_eq!(again.len(), 1, "only the sync op is left: {again:?}");
    let parts = sync_parts(&again[0]).unwrap();
    assert!(parts.imports.is_empty(), "the user's caption is not overwritten");
    let file = parts.synced.iter().find(|(i, _)| *i == tagged).unwrap().1.unwrap();
    assert_eq!(conflicts(&lib, tagged, &file), vec![Field::Caption]);
    apply_all(&mut lib, again);
    assert_eq!(lib.caption(tagged), "Mine");
    assert_eq!(diff(&mut lib, &scan), vec![]);
}

/// Truncated and damaged files of every kind the scan reads: no panic; what is read is what a
/// whole-file read says, or nothing.
#[test]
fn truncated_and_damaged_files_bring_nothing_and_never_panic() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("cut");
    let jpeg = tagged_jpeg(5);
    let png = dalica_meta::write_owned(&dalica_scenes::png(&dalica_scenes::Scene::new(6, 64, 48)).unwrap(), &caption("A PNG"), &options()).unwrap();
    let mut n = 0;
    for (ext, bytes) in [("jpg", &jpeg), ("png", &png)] {
        let len = bytes.len();
        for cut in (0..len).step_by((len / 40).max(1)) {
            put(&dir.join(format!("{n:04}.{ext}")), &bytes[..cut], 0);
            n += 1;
        }
    }
    // Damaged RAW sidecars: not XML, cut short, no rdf:RDF.
    for (i, xmp) in [&b"not xml at all"[..], b"<x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF", b"<a/>"].into_iter().enumerate() {
        put(&dir.join(format!("raw{i}.CR3")), CR3, 0);
        put(&dir.join(format!("raw{i}.xmp")), xmp, 0);
    }
    let scan = scan_with(&[tmp.path().to_path_buf()], &reading(ReadMetadata::all()));
    let limits = dalica_meta::Limits::default();
    for p in scan.folders.iter().flat_map(|f| &f.photos) {
        let path: PathBuf = dir.join(&p.name);
        if p.name.ends_with(".CR3") {
            assert_eq!(p.file.as_ref().map(|f| f.0.clone()), Some(Default::default()), "{}: the damaged sidecar is not read", p.name);
            continue;
        }
        let whole = dalica_meta::read(&std::fs::read(&path).unwrap(), &limits).ok().map(|m| dalica_scan::FileValues::of(&m.owned));
        assert_eq!(p.file, whole, "{}", p.name);
    }
    // And the diff of all of it applies.
    let mut lib = Library::new();
    let ops = diff(&mut lib, &scan);
    apply_all(&mut lib, ops);
}

/// The owned fields of a JPEG, PNG, WebP or TIFF come from the bytes its header read already holds,
/// on the reading thread: the file is not read again (the first-thumbnails budget). A planted
/// bytes reader handing back another file's bytes shows it; without one (a planted header reader)
/// the fields are read from the file itself.
#[test]
fn the_owned_fields_come_from_the_bytes_the_header_read() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("Lisbon");
    put(&dir.join("plain.jpg"), &plain_jpeg(7), 0);
    let tagged = dalica_meta::write_owned(&plain_jpeg(8), &caption("From the header's bytes"), &options()).unwrap();
    let bytes_reader: dalica_scan::HeaderBytesReader = Arc::new(move |path: &Path, limits: &dalica_image::Limits| {
        let (header, _) = dalica_image::read_header_file(path, limits)?;
        Ok((header, tagged.clone()))
    });
    let roots = [tmp.path().to_path_buf()];
    let planted = Options { bytes_reader: Some(bytes_reader), ..reading(ReadMetadata::all()) };
    let scan = scan_with(&roots, &planted);
    assert_eq!(found(&scan, "plain.jpg").file.as_ref().unwrap().0.caption, "From the header's bytes", "not read again");
    let without = Options { bytes_reader: None, ..reading(ReadMetadata::all()) };
    let scan = scan_with(&roots, &without);
    assert_eq!(found(&scan, "plain.jpg").file.as_ref().unwrap().0, Default::default(), "read from the file itself");
}
