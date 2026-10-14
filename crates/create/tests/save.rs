//! Save in place, Undo Save and Revert on temp folders (`data-model.md` §6 and §10.9,
//! `security.md` §5.3): Save → Undo Save → Save → Revert returns the exact original bytes; a
//! second Save keeps the first original and renders from it; the saved file keeps the original's
//! quality, gets orientation 1 and carries the metadata; a photo changed since it was read, a link
//! in place of `.dalica-originals`, a mismatched backup, a read-only folder or file and an
//! unknown operation are refused with nothing moved.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use dalica_core::Timestamp;
use dalica_create::output::{self, Purpose, Spec};
use dalica_create::save::{self, Error, Options, Saving, Seen};
use dalica_fs::TempDir;
use dalica_fs::faults::{self, Fault, points};
use dalica_image::{Limits, jpeg_quality, read_header};
use dalica_meta::{Changes, WriteOptions};
use dalica_sidecar::originals::{Entry, Originals, State};

const NAME: &str = "IMG_0001.jpg";
const NOW: Timestamp = Timestamp::from_millis(1_791_000_000_000);

fn folder() -> TempDir {
    TempDir::new("dalica-save-test").unwrap()
}

fn options() -> Options {
    Options { threads: 2, ..Options::new(NOW) }
}

/// A scenes JPEG at quality 80, upright, written as `NAME` in `dir`; its bytes.
fn photo(dir: &Path) -> Vec<u8> {
    let bytes = jpeg(11, 320, 240, 80, 1);
    std::fs::write(dir.join(NAME), &bytes).unwrap();
    bytes
}

fn read(path: impl AsRef<Path>) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

fn backup(dir: &Path) -> PathBuf {
    dir.join(".dalica-originals").join(NAME)
}

fn record_path(dir: &Path) -> PathBuf {
    dir.join(".dalica-originals").join(".dalica.json")
}

/// The record's entry for `NAME`, if there is a record and an entry.
fn entry(dir: &Path) -> Option<Entry> {
    let bytes = std::fs::read(record_path(dir)).ok()?;
    match Originals::parse(&bytes, &dalica_sidecar::Limits::default()).unwrap().entry(NAME) {
        dalica_sidecar::Field::Value(e) => Some(e),
        _ => None,
    }
}

fn seen(path: &Path) -> Seen {
    let meta = std::fs::metadata(path).unwrap();
    Seen { size: meta.len(), mtime: Timestamp::from_system_time(meta.modified().unwrap()) }
}

#[test]
fn save_undo_save_save_revert_returns_the_exact_original_bytes() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    let edits = recipe(&["rotate(90)", "autofix()"]);

    let saved = save::in_place(dir, NAME, &edits, &Changes::default(), &options()).unwrap();
    assert!(saved.first);
    assert_ne!(read(dir.join(NAME)), original, "the edits are in the file");
    assert_eq!(read(backup(dir)), original, "the original is kept byte for byte");
    let e = entry(dir).unwrap();
    assert_eq!(e, saved.entry);
    assert_eq!(e.state, State::Done);
    assert_eq!(e.recipe, ["rotate(90)", "autofix()"]);
    assert_eq!((e.size, e.width, e.height), (original.len() as u64, 320, 240));
    assert_eq!(e.digest, save::digest(&original));
    assert_eq!(e.time, Some(NOW.as_millis()));
    assert_eq!((saved.report.width, saved.report.height), (240, 320), "a quarter turn");

    let undone = save::undo(dir, NAME, &options()).unwrap();
    assert_eq!(undone.recipe.as_deref(), Some(&["rotate(90)".to_string(), "autofix()".to_string()][..]), "the recipe to put back");
    assert_eq!(read(dir.join(NAME)), original, "Undo Save brings the exact bytes back");
    assert!(!backup(dir).exists());
    assert_eq!(entry(dir), None, "the entry is dropped");

    save::in_place(dir, NAME, &edits, &Changes::default(), &options()).unwrap();
    assert_eq!(read(backup(dir)), original);
    let reverted = save::revert(dir, NAME, &options()).unwrap();
    assert_eq!(reverted.recipe, None, "Revert discards the edits");
    assert_eq!(read(dir.join(NAME)), original, "Revert brings the exact bytes back");
    assert!(!backup(dir).exists());
    assert_eq!(entry(dir), None);
}

#[test]
fn a_second_save_keeps_the_first_original_and_renders_from_it() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap();
    let first_saved = read(dir.join(NAME));

    let later = Options { now: Timestamp::from_millis(NOW.as_millis() + 60_000), ..options() };
    let again = save::in_place(dir, NAME, &recipe(&["autofix()"]), &Changes::default(), &later).unwrap();
    assert!(!again.first);
    assert_eq!(read(backup(dir)), original, "the first original stays the one kept");
    let e = entry(dir).unwrap();
    assert_eq!(e.recipe, ["rotate(90)", "autofix()"], "the whole recipe is recorded");
    assert_eq!((e.size, e.digest.as_str()), (original.len() as u64, save::digest(&original).as_str()), "still the original's facts");
    assert_eq!(e.time, Some(later.now.as_millis()));

    // Rendered from the kept original with the whole recipe, not from the first saved file.
    let spec = Spec { thumbnail: true, threads: 2, ..Spec::new(Purpose::InPlace) };
    let expected = output::render(&original, &recipe(&["rotate(90)", "autofix()"]), &spec).unwrap();
    let on_disk = read(dir.join(NAME));
    assert_eq!(on_disk, expected.bytes, "the saved file is the original rendered with the whole recipe");
    let compounding = output::render(&first_saved, &recipe(&["autofix()"]), &spec).unwrap();
    assert_ne!(on_disk, compounding.bytes, "not a re-encode of the first saved file");

    // Undo Save gives back the whole recipe and the original.
    let undone = save::undo(dir, NAME, &options()).unwrap();
    assert_eq!(undone.recipe.unwrap(), ["rotate(90)", "autofix()"]);
    assert_eq!(read(dir.join(NAME)), original);
}

#[test]
fn the_saved_file_keeps_the_quality_gets_orientation_1_and_carries_the_metadata() {
    let tmp = folder();
    let dir = tmp.path();
    // A 320 × 240 picture stored turned (240 × 320, orientation 6).
    let original = jpeg(12, 320, 240, 75, 6);
    std::fs::write(dir.join(NAME), &original).unwrap();
    let estimate = jpeg_quality::quality(&original).unwrap();

    let changes = Changes { caption: Some(Some("Harbour at dusk".into())), tags: Some(vec!["boats".into(), "evening".into()]), ..Changes::default() };
    let opts = Options { write: WriteOptions::default(), ..options() };
    let saved = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &changes, &opts).unwrap();

    let bytes = read(dir.join(NAME));
    let quality = jpeg_quality::quality(&bytes).unwrap();
    println!("original estimated at {estimate}, saved at {quality}");
    assert!(quality.abs_diff(estimate) <= 2 && quality.abs_diff(75) <= 2, "saved at {quality}, the original at {estimate}");
    let header = read_header(&bytes, &Limits::default()).unwrap();
    assert_eq!(header.orientation.get(), 1);
    assert_eq!((header.width, header.height), (240, 320), "a quarter turn of the 320 × 240 picture, stored upright");
    let e = entry(dir).unwrap();
    assert_eq!((e.width, e.height), (320, 240), "the record keeps the original's displayed size");

    let meta = dalica_meta::read(&bytes, &dalica_meta::Limits::default()).unwrap();
    assert_eq!(meta.summary.make.as_deref(), Some("Dalicam"), "EXIF carried");
    assert_eq!(meta.summary.model.as_deref(), Some("Model 7"));
    assert_eq!(meta.summary.orientation, Some(1));
    assert_eq!(meta.owned.caption.as_deref(), Some("Harbour at dusk"), "the owned fields merged");
    assert_eq!(meta.owned.tags, ["boats", "evening"]);
    assert!(saved.report.thumbnail, "a fresh EXIF thumbnail");
    assert!(header.thumbnail.is_some());
}

#[test]
fn a_seen_file_saves_and_a_changed_one_is_refused_with_nothing_moved() {
    let tmp = folder();
    let dir = tmp.path();
    let path = dir.join(NAME);
    photo(dir);
    let before = seen(&path);

    // Changed after the library read it: another program rewrote it.
    let rewritten = jpeg(13, 200, 100, 90, 1);
    std::fs::write(&path, &rewritten).unwrap();
    let opts = Options { seen: Some(before), ..options() };
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &opts).unwrap_err();
    assert!(matches!(err, Error::Changed(_)), "{err:?}");
    assert_eq!(read(&path), rewritten);
    assert!(!dir.join(".dalica-originals").exists(), "nothing made");

    // The version the caller saw saves.
    let opts = Options { seen: Some(seen(&path)), ..options() };
    save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &opts).unwrap();
    assert_eq!(read(backup(dir)), rewritten);

    // Undo Save of a saved file changed since: refused, nothing moved.
    let saved_seen = seen(&path);
    let changed = jpeg(14, 100, 100, 90, 1);
    std::fs::write(&path, &changed).unwrap();
    let err = save::undo(dir, NAME, &Options { seen: Some(saved_seen), ..options() }).unwrap_err();
    assert!(matches!(err, Error::Changed(_)), "{err:?}");
    assert_eq!(read(&path), changed);
    assert_eq!(read(backup(dir)), rewritten);
    assert_eq!(entry(dir).unwrap().state, State::Done);
}

#[test]
fn a_photo_changed_right_before_it_moves_is_refused_with_nothing_moved() {
    let tmp = folder();
    let dir = tmp.path();
    let path = dir.join(NAME);
    photo(dir);
    let stranger = jpeg(15, 64, 64, 90, 1);
    let (p, s) = (path.clone(), stranger.clone());
    faults::run_at(points::BACKUP_CHECK, move || std::fs::write(&p, &s).unwrap());
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    faults::disarm_all();
    assert!(matches!(err, Error::Fs(dalica_fs::Error::Changed(_))), "{err:?}");
    assert_eq!(read(&path), stranger, "the other program's version stays");
    assert!(!backup(dir).exists(), "nothing moved");
    assert_eq!(entry(dir), None, "the pending entry is taken back");
}

#[test]
fn a_saved_file_changed_during_a_second_save_is_refused_and_the_record_kept() {
    let tmp = folder();
    let dir = tmp.path();
    let path = dir.join(NAME);
    let original = photo(dir);
    save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap();
    let first = entry(dir).unwrap();

    let stranger = jpeg(16, 64, 64, 90, 1);
    let (p, s) = (path.clone(), stranger.clone());
    faults::run_at(points::BACKUP_OPEN_DIR, move || std::fs::write(&p, &s).unwrap());
    let err = save::in_place(dir, NAME, &recipe(&["autofix()"]), &Changes::default(), &options()).unwrap_err();
    faults::disarm_all();
    assert!(matches!(err, Error::Fs(dalica_fs::Error::Changed(_))), "{err:?}");
    assert_eq!(read(&path), stranger, "never written over");
    assert_eq!(read(backup(dir)), original);
    assert_eq!(entry(dir), Some(first), "the entry is the first Save's again");
}

#[cfg(unix)]
#[test]
fn a_link_in_place_of_the_originals_folder_is_refused_with_nothing_moved() {
    let tmp = folder();
    let elsewhere = folder();
    let dir = tmp.path();
    let original = photo(dir);
    std::os::unix::fs::symlink(elsewhere.path(), dir.join(".dalica-originals")).unwrap();
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(err, Error::Fs(dalica_fs::Error::Link(_))), "{err:?}");
    assert_eq!(read(dir.join(NAME)), original);
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0, "nothing written through the link");
    assert!(matches!(save::undo(dir, NAME, &options()).unwrap_err(), Error::Fs(dalica_fs::Error::Link(_))));
}

#[test]
fn a_mismatched_backup_is_refused_with_nothing_moved() {
    // A backup no record names: the first original is the one kept, so nothing replaces it.
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    let stray = b"someone else's file".to_vec();
    std::fs::create_dir(dir.join(".dalica-originals")).unwrap();
    std::fs::write(backup(dir), &stray).unwrap();
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(err, Error::Fs(dalica_fs::Error::Changed(_))), "{err:?}");
    assert_eq!(read(dir.join(NAME)), original);
    assert_eq!(read(backup(dir)), stray);
    assert!(!record_path(dir).exists());

    // A kept original that no longer matches its record (same size, one byte changed).
    let tmp = folder();
    let dir = tmp.path();
    photo(dir);
    save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap();
    let saved = read(dir.join(NAME));
    let mut tampered = read(backup(dir));
    let middle = tampered.len() / 2;
    tampered[middle] ^= 0x55;
    std::fs::write(backup(dir), &tampered).unwrap();
    let recorded = entry(dir).unwrap();
    for err in [
        save::in_place(dir, NAME, &recipe(&["autofix()"]), &Changes::default(), &options()).unwrap_err(),
        save::undo(dir, NAME, &options()).unwrap_err(),
        save::revert(dir, NAME, &options()).unwrap_err(),
    ] {
        assert!(matches!(err, Error::Fs(dalica_fs::Error::Changed(_))), "{err:?}");
    }
    assert_eq!(read(dir.join(NAME)), saved, "the saved file stays");
    assert_eq!(read(backup(dir)), tampered, "the backup is left alone");
    assert_eq!(entry(dir), Some(recorded));
}

#[cfg(unix)]
#[test]
fn a_read_only_folder_fails_with_its_reason_and_nothing_changed() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    // Running as root writes anyway; the check means nothing there.
    let root = std::fs::write(dir.join("probe"), b"x").is_ok();
    let result = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options());
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if root {
        return;
    }
    let err = result.unwrap_err();
    println!("read-only folder: {}", err.reason());
    assert!(err.reason().contains("ermission denied"), "{}", err.reason());
    assert_eq!(read(dir.join(NAME)), original);
    assert!(!dir.join(".dalica-originals").exists());
}

#[test]
fn a_read_only_file_fails_with_its_reason_and_nothing_changed() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    let path = dir.join(NAME);
    let mut perms = std::fs::metadata(&path).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&path, perms.clone()).unwrap();
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&path, perms).unwrap();
    assert!(matches!(err, Error::ReadOnly(_)), "{err:?}");
    assert!(err.reason().contains("read-only"), "{}", err.reason());
    assert_eq!(read(&path), original);
    assert!(!dir.join(".dalica-originals").exists());
}

#[test]
fn an_unknown_operation_is_refused_by_name() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    for op in ["frobnicate(3)", "legacy:redeye(1)", "sepia()", "tuning(nonsense)"] {
        assert!(!save::renders(op), "{op}");
        let err = save::in_place(dir, NAME, &recipe(&["rotate(90)", op]), &Changes::default(), &options()).unwrap_err();
        assert!(matches!(&err, Error::Unrenderable(text) if text == op), "{err:?}");
        assert!(err.reason().contains(op), "{}", err.reason());
        let err = save::save(dir, NAME, &recipe(&[op]), &Changes::default(), &options()).unwrap_err();
        assert!(matches!(err, Error::Unrenderable(_)), "{err:?}");
    }
    for op in ["rotate(90)", "flip_h()", "autofix()"] {
        assert!(save::renders(op), "{op}");
    }
    assert_eq!(read(dir.join(NAME)), original);
    assert!(!dir.join(".dalica-originals").exists());
}

#[test]
fn an_interrupted_save_is_left_pending_for_the_repair() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    faults::arm(points::BACKUP_MOVE, Fault::Crash);
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    faults::disarm_all();
    assert!(matches!(err, Error::Fs(dalica_fs::Error::Crashed(_))), "{err:?}");
    assert_eq!(entry(dir).unwrap().state, State::Pending, "journaled before anything moved");
    assert_eq!(read(dir.join(NAME)), original);
    for err in [
        save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err(),
        save::undo(dir, NAME, &options()).unwrap_err(),
        save::revert(dir, NAME, &options()).unwrap_err(),
    ] {
        assert!(matches!(err, Error::Pending(_)), "{err:?}");
    }
}

#[test]
fn a_failed_commit_of_a_first_save_moves_the_original_back() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    // Once the original is in the backup, the new file's rename fails.
    faults::run_at(points::BACKUP_VERIFY, || faults::arm(points::RENAME, Fault::Error));
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    faults::disarm_all();
    assert!(matches!(err, Error::Fs(dalica_fs::Error::Io { op: "rename", .. })), "{err:?}");
    assert_eq!(read(dir.join(NAME)), original, "the original is back at its name");
    assert!(!backup(dir).exists());
    assert_eq!(entry(dir), None, "and the entry is taken back");
}

#[test]
fn a_save_stopped_after_its_new_file_landed_stays_pending_with_the_original_kept() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    // The new file is renamed into place, then its read-back fails.
    faults::run_at(points::BACKUP_VERIFY, || faults::arm(points::VERIFY, Fault::Error));
    let err = save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    faults::disarm_all();
    assert!(matches!(err, Error::Incomplete { .. }), "{err:?}");
    println!("stopped part-way: {}", err.reason());
    assert_eq!(read(backup(dir)), original, "the original is kept");
    let e = entry(dir).unwrap();
    assert_eq!(e.state, State::Pending, "for the repair to finish or take back");
    assert_eq!(e.digest, save::digest(&original));
}

#[test]
fn undo_and_revert_without_a_kept_original_and_hostile_names_are_refused() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    assert!(matches!(save::undo(dir, NAME, &options()).unwrap_err(), Error::NotSaved(_)));
    assert!(matches!(save::revert(dir, NAME, &options()).unwrap_err(), Error::NotSaved(_)));
    for name in ["", ".", "..", "../x.jpg", "a/b.jpg", "CON", ".dalica-originals", ".dalica.json"] {
        let r = recipe(&["rotate(90)"]);
        assert!(matches!(save::in_place(dir, name, &r, &Changes::default(), &options()).unwrap_err(), Error::Name(_)), "{name:?}");
        assert!(matches!(save::undo(dir, name, &options()).unwrap_err(), Error::Name(_)), "{name:?}");
        assert!(matches!(save::revert(dir, name, &options()).unwrap_err(), Error::Name(_)), "{name:?}");
    }
    let missing = save::in_place(dir, "gone.jpg", &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(missing, Error::Missing(_)), "{missing:?}");
    std::fs::create_dir(dir.join("folder.jpg")).unwrap();
    let not_a_file = save::in_place(dir, "folder.jpg", &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(not_a_file, Error::NotAFile(_)), "{not_a_file:?}");
    std::fs::write(dir.join("junk.jpg"), b"not a photo").unwrap();
    let junk = save::in_place(dir, "junk.jpg", &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(junk, Error::Output(_)), "{junk:?}");
    assert_eq!(read(dir.join(NAME)), original);
    assert!(!dir.join(".dalica-originals").exists(), "no refusal made anything");
}

#[test]
fn a_damaged_record_is_set_aside_and_the_save_starts_a_new_one() {
    let tmp = folder();
    let dir = tmp.path();
    let original = photo(dir);
    std::fs::create_dir(dir.join(".dalica-originals")).unwrap();
    std::fs::write(record_path(dir), b"{ not json").unwrap();
    save::in_place(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap();
    assert_eq!(read(backup(dir)), original);
    assert_eq!(entry(dir).unwrap().state, State::Done);
    let aside: Vec<String> = std::fs::read_dir(dir.join(".dalica-originals"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".dalica.json.corrupt-"))
        .collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    assert_eq!(read(dir.join(".dalica-originals").join(&aside[0])), b"{ not json", "kept as it was");
}

#[test]
fn the_dispatch_saves_jpegs_in_place_and_refuses_what_cannot_be_written_in_place() {
    let tmp = folder();
    let dir = tmp.path();
    photo(dir);
    let saving = save::save(dir, NAME, &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap();
    assert!(matches!(saving, Saving::InPlace(ref s) if s.first), "{saving:?}");

    // A PNG with transparency goes beside it (M8.14); until then nothing is written.
    let png = png_rgba(&with_alpha(3, 64, 48));
    std::fs::write(dir.join("alpha.png"), &png).unwrap();
    let err = save::save(dir, "alpha.png", &recipe(&["rotate(90)"]), &Changes::default(), &options()).unwrap_err();
    assert!(matches!(err, Error::Output(output::Error::NotInPlace(_))), "{err:?}");
    assert_eq!(read(dir.join("alpha.png")), png);
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 3, "IMG, alpha.png and .dalica-originals only");

    let copy = save::copy::copy(dir, NAME, &recipe(&[]), &Changes::default(), &save::copy::Destination::Numbered, &options());
    assert!(matches!(copy, Err(Error::NotYet(_))));
}
