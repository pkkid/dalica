//! One metadata write: re-read, decide per field, merge-write, replace atomically, put the time
//! back, hand back the synced hashes (see the module above). A photo file (JPEG, PNG, WebP, TIFF)
//! is rewritten in place; a RAW gets its `.xmp` sidecar.

use std::path::{Path, PathBuf};

use dalica_core::{DateTime, Timestamp};
use dalica_fs::{Error as FsError, Expect, Identity, Stat, Times};
use dalica_image::Format;
use dalica_library::sync::{Decision, reconcile};
use dalica_library::{clean_caption, clean_tags};
use dalica_meta::xmp_sidecar::{self, SidecarWrite, Sidecars};
use dalica_meta::{Changes, DateTaken, OwnedFields, Place, WriteOptions};

use super::{Field, FieldOutcome, FileResult, Options, Request, Retry, Status, Values};

/// Writes one photo now: [`MetaWriteQueue::write`](super::MetaWriteQueue::write) without a queue
/// (no offline roots, no retries). Any thread but the UI thread.
pub fn write_file(request: &Request, options: &Options, now: Timestamp) -> FileResult {
    run(request, options, now, None).0
}

/// One write. `remembered`: the times the file had before an earlier try of this queue that did
/// not finish. Returns the result and the times still to put back (to remember for a retry).
pub(crate) fn run(request: &Request, options: &Options, now: Timestamp, remembered: Option<Times>) -> (FileResult, Option<Times>) {
    let path = &request.path;
    let stat = match dalica_fs::stat(path) {
        Ok(Some(s)) => s,
        Ok(None) => return (absent(request), None),
        Err(FsError::Link(_)) => return (failed(request, "it is a symbolic link; Dalica does not write through links".into()), None),
        Err(e) => return (failed(request, e.to_string()), remembered),
    };
    if !stat.is_file {
        return (skipped(request, "it is not a regular file".into()), None);
    }
    let read = match dalica_fs::read(path, options.max_file) {
        Ok(Some(r)) => r,
        Ok(None) => return (absent(request), None),
        Err(FsError::TooLarge { limit, .. }) => {
            return (failed(request, format!("it is larger than {limit} bytes; Dalica does not rewrite it")), None);
        }
        Err(FsError::Changed(_)) => return (requeue(request, Retry::Changed, "it changed while it was read".into()), remembered),
        Err(e) => return (failed(request, e.to_string()), remembered),
    };
    if read.identity != stat.identity {
        return (requeue(request, Retry::Changed, "it changed while it was read".into()), remembered);
    }
    match dalica_image::sniff(&read.bytes) {
        Some(Format::Raw(_)) => (raw(request, options, now, &read.bytes), None),
        Some(Format::Jpeg | Format::Png | Format::WebP | Format::Tiff) => photo(request, options, now, &stat, read.bytes, read.identity, remembered),
        Some(other) => {
            (skipped(request, format!("Dalica does not write metadata into {} files; the folder sidecar keeps the values", other.name())), None)
        }
        None => (skipped(request, "Dalica does not write metadata into this kind of file; the folder sidecar keeps the values".into()), None),
    }
}

/// A JPEG, PNG, WebP or TIFF file, rewritten in place.
fn photo(
    request: &Request,
    options: &Options,
    now: Timestamp,
    stat: &Stat,
    bytes: Vec<u8>,
    identity: Identity,
    remembered: Option<Times>,
) -> (FileResult, Option<Times>) {
    let path = &request.path;
    let meta = match dalica_meta::read(&bytes, &options.limits) {
        Ok(m) => m,
        Err(e) => return (failed(request, format!("its metadata cannot be read ({e}); it is not rewritten")), None),
    };
    let file = values_of(&meta.owned);
    let mut plan = plan(request, &file, &meta.owned, options, None);
    plan.result.file = Some(file.clone());
    plan.result.synced = Some(file.hashes());
    let mut result = plan.result;
    if plan.changes.is_empty() {
        // An earlier try wrote the file and could not put its time back: do it now, if the file
        // still says what Dalica wrote and nobody else changed it since.
        let ours = !result.fields.iter().any(|(_, o)| matches!(o, FieldOutcome::Imported | FieldOutcome::Conflict { .. }));
        if let Some(times) = remembered.filter(|t| ours && !options.update_modification_time && t.modified != stat.times.modified) {
            return match dalica_fs::set_times(path, &times, stat.identity) {
                Ok(_) => (result, None),
                Err(FsError::Changed(_)) => (result, None),
                Err(e) => {
                    result.status = Status::Requeued(format!("its modification time is not back yet: {e}"));
                    result.retry = Some(Retry::Time);
                    (result, Some(times))
                }
            };
        }
        return (result, None);
    }
    if !stat.writable {
        return (fail_writes(result, &plan.to_write, "the file is read-only; the folder sidecar keeps the value".into()), None);
    }
    let write_options = write_options(options, now);
    let new = match dalica_meta::write_owned(&bytes, &plan.changes, &write_options) {
        Ok(new) => new,
        Err(e) => return (fail_writes(result, &plan.to_write, format!("{e}; the folder sidecar keeps the value")), None),
    };
    if new == bytes {
        return (written(result, &plan.to_write, Status::Unchanged), None);
    }
    let original = remembered.unwrap_or(stat.times);
    let id = match dalica_fs::replace(path, &new, Expect::Unchanged(identity)) {
        Ok(id) => id,
        Err(e) => match landed(path, &new, e, options) {
            Ok(id) => id,
            Err((retry, why)) => {
                let keep = (retry != Retry::Changed).then_some(original);
                return (requeue_with(result, &plan.to_write, retry, why), keep);
            }
        },
    };
    // The file is written: the synced hashes are those of what it now says.
    if let Ok(m) = dalica_meta::read(&new, &options.limits) {
        result.synced = Some(values_of(&m.owned).hashes());
    }
    let mut result = written(result, &plan.to_write, Status::Written(path.clone()));
    if options.update_modification_time {
        return (result, None);
    }
    match dalica_fs::set_times(path, &original, id) {
        Ok(_) | Err(FsError::Changed(_)) => (result, None),
        Err(e) => {
            result.status = Status::Requeued(format!("it is written; its modification time is not back yet: {e}"));
            result.retry = Some(Retry::Time);
            (result, Some(original))
        }
    }
}

/// A RAW file: its fields go into its `.xmp` sidecar (with the option on); the RAW is never
/// written.
fn raw(request: &Request, options: &Options, now: Timestamp, raw: &[u8]) -> FileResult {
    let path = &request.path;
    let (Some(folder), Some(raw_name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return failed(request, "its name cannot be given an .xmp sidecar".into());
    };
    let names = match folder_names(folder) {
        Ok(n) => n,
        Err(e) => return failed(request, format!("its folder cannot be listed: {e}")),
    };
    let sidecars = Sidecars::new(&names, |n| n == raw_name || request.raws.iter().any(|r| r == n));
    let Some(name) = sidecars.get(raw_name) else {
        return skipped(request, "no .xmp sidecar name is free for it; the folder sidecar keeps the values".into());
    };
    let max_xmp = u64::try_from(options.limits.max_xmp).unwrap_or(u64::MAX);
    // The existing sidecar, read (one that cannot be read at all is left alone: the write fails).
    let existing = match &name.existing {
        None => None,
        Some(n) => {
            let p = folder.join(n);
            let stat = match dalica_fs::stat(&p) {
                Ok(Some(s)) => s,
                Ok(None) => return requeue(request, Retry::Changed, format!("{n} vanished while it was read")),
                Err(e) => return failed(request, format!("its sidecar cannot be read: {e}")),
            };
            match dalica_fs::read(&p, max_xmp) {
                Ok(Some(r)) => Some((p, r, stat.writable)),
                Ok(None) | Err(FsError::Changed(_)) => return requeue(request, Retry::Changed, format!("{n} changed while it was read")),
                Err(e) => return failed(request, format!("its sidecar cannot be read: {e}")),
            }
        }
    };
    let sidecar = existing.as_ref().map(|(_, r, _)| r.bytes.as_slice());
    let read = xmp_sidecar::read(Some(raw), sidecar, &options.limits);
    let file = values_of(&read.metadata.owned);
    let off = (!options.raw_xmp_sidecars).then_some("Write XMP sidecars for RAW files is off; the folder sidecar keeps the value");
    let plan = plan(request, &file, &read.metadata.owned, options, off);
    let mut result = plan.result;
    result.file = Some(file.clone());
    result.synced = Some(file.hashes());
    if plan.changes.is_empty() {
        if let Some(why) = off.filter(|_| result.fields.iter().any(|(_, o)| matches!(o, FieldOutcome::Skipped(_) | FieldOutcome::Conflict { .. }))) {
            result.status = Status::Skipped(format!("{raw_name} is a RAW file and {why}"));
        }
        return result;
    }
    if let Some((p, _, false)) = &existing {
        return fail_writes(result, &plan.to_write, format!("its sidecar {} is read-only; the folder sidecar keeps the value", p.display()));
    }
    let bytes = match xmp_sidecar::merge(Some(raw), sidecar, &plan.changes, &write_options(options, now)) {
        Ok(SidecarWrite::Unchanged) => return written(result, &plan.to_write, Status::Unchanged),
        Ok(SidecarWrite::Write(bytes)) => bytes,
        Ok(SidecarWrite::ReplaceDamaged { bytes, .. }) => {
            if let Some((p, _, _)) = &existing {
                match dalica_fs::set_aside(p, &crate::tag("corrupt", now)) {
                    Ok(kept) => result.set_aside = Some(kept),
                    Err(e) => return fail_writes(result, &plan.to_write, format!("its damaged sidecar cannot be kept aside: {e}")),
                }
            }
            bytes
        }
        Err(e) => return fail_writes(result, &plan.to_write, format!("{e}; the folder sidecar keeps the value")),
    };
    let target = folder.join(&name.write);
    let expect = match (&existing, &result.set_aside) {
        (Some((_, r, _)), None) => Expect::Unchanged(r.identity),
        _ => Expect::Absent,
    };
    if let Err(e) = dalica_fs::replace(&target, &bytes, expect)
        && let Err((retry, why)) = landed(&target, &bytes, e, options)
    {
        return requeue_with(result, &plan.to_write, retry, why);
    }
    result.synced = Some(values_of(&xmp_sidecar::read(Some(raw), Some(&bytes), &options.limits).metadata.owned).hashes());
    written(result, &plan.to_write, Status::Written(target))
}

/// The file names of `folder` (files only; names that are not UTF-8 are left out: no sidecar
/// name `meta` gives can be one of them).
fn folder_names(folder: &Path) -> dalica_fs::Result<Vec<String>> {
    let limits = dalica_fs::Limits { max_depth: 1, ..dalica_fs::Limits::default() };
    let walk = dalica_fs::walk(folder, limits, |_| false)?;
    Ok(walk
        .entries
        .into_iter()
        .filter(|e| e.is_file && !e.is_link)
        .filter_map(|e| e.path.file_name().and_then(|n| n.to_str()).map(str::to_owned))
        .collect())
}

/// What one write is to do.
struct Plan {
    /// The result with every field's outcome so far (fields to write are filled in later).
    result: FileResult,
    changes: Changes,
    /// The fields in `changes`, in order, and whether each is a conflict.
    to_write: Vec<(Field, bool)>,
}

/// Decides every field (`xmp-interop.md` §5) and gathers the changes to write: the fields the
/// request asks for whose decision is a write or a conflict, each only with its option on.
/// `off`: why nothing may be written at all (a RAW without the sidecar option).
fn plan(request: &Request, file: &Values, owned: &OwnedFields, options: &Options, off: Option<&str>) -> Plan {
    let mut result = FileResult::new(request, Status::Unchanged);
    let mut changes = Changes::default();
    let mut to_write = Vec::new();
    let ours = &request.values;
    for (field, decision) in reconcile(file, ours, request.synced.as_ref(), request.first_scan) {
        let asked = request.fields.contains(&field) && !request.first_scan;
        let write = |result: &mut FileResult, changes: &mut Changes, conflict: bool| -> Option<(Field, bool)> {
            if let Some(why) = off {
                result.fields.push((field, if conflict { FieldOutcome::Conflict { written: false } } else { FieldOutcome::Skipped(why.to_owned()) }));
                return None;
            }
            if !options.allows(field) {
                let outcome =
                    if conflict { FieldOutcome::Conflict { written: false } } else { FieldOutcome::Skipped("its write option is off".to_owned()) };
                result.fields.push((field, outcome));
                return None;
            }
            match change(changes, field, ours, owned) {
                Ok(()) => Some((field, conflict)),
                Err(why) => {
                    result.fields.push((field, FieldOutcome::Failed(why)));
                    None
                }
            }
        };
        match decision {
            Decision::Keep if asked => result.fields.push((field, FieldOutcome::Unchanged)),
            Decision::Keep => {}
            Decision::Import => result.fields.push((field, FieldOutcome::Imported)),
            Decision::Write if asked => to_write.extend(write(&mut result, &mut changes, false)),
            Decision::Write => {}
            Decision::Conflict if asked => to_write.extend(write(&mut result, &mut changes, true)),
            Decision::Conflict => result.fields.push((field, FieldOutcome::Conflict { written: false })),
        }
    }
    Plan { result, changes, to_write }
}

/// Adds Dalica's value of `field` to `changes`.
fn change(changes: &mut Changes, field: Field, ours: &Values, owned: &OwnedFields) -> Result<(), String> {
    match field {
        Field::Caption => changes.caption = Some((!ours.caption.is_empty()).then(|| ours.caption.clone())),
        Field::Tags => changes.tags = Some(ours.tags.clone()),
        Field::Rating => changes.star = Some(ours.star),
        Field::Place => {
            changes.place = Some(match ours.place {
                None => None,
                Some((lat, lon)) => Some(Place::new(lat, lon).ok_or_else(|| "the place is not on the globe".to_owned())?),
            })
        }
        Field::Date => {
            // The file's offset from UTC is kept (the camera's time zone); the fraction of a second
            // goes, since Dalica's date is to the second.
            let Some(local) = ours.date else { return Err("there is no date to write".to_owned()) };
            let offset = owned.date_taken.as_ref().and_then(DateTaken::offset_minutes);
            changes.date_taken = Some(DateTaken::new(local, offset, None).unwrap_or_else(|| DateTaken::local(local)));
        }
    }
    Ok(())
}

/// A file's owned values in the library's terms (cleaned as the library stores them).
fn values_of(owned: &OwnedFields) -> Values {
    Values {
        caption: owned.caption.as_deref().map(clean_caption).unwrap_or_default(),
        tags: clean_tags(owned.tags.iter().map(String::as_str)),
        star: owned.starred(),
        place: owned.place.map(|p| (p.latitude(), p.longitude())),
        date: owned.date_taken.as_ref().map(DateTaken::date_time),
    }
}

/// `meta`'s write options for `options`, written at `now` (`xmp:MetadataDate`, in UTC).
fn write_options(options: &Options, now: Timestamp) -> WriteOptions {
    WriteOptions {
        captions_tags_places: options.captions_tags_places,
        stars_as_ratings: options.stars_as_ratings,
        dates: options.dates,
        metadata_date: DateTime::from_unix_secs(now.as_secs()).and_then(|d| DateTaken::new(d, Some(0), None)),
        limits: options.limits,
    }
}

/// After a replace that returned an error: whether the new bytes are in place anyway (an error
/// after the rename, such as the folder's `fsync` or the read-back): then the write landed and its
/// identity is returned. Otherwise why to try again. A simulated crash stops there, as a killed
/// process would.
fn landed(path: &Path, bytes: &[u8], error: FsError, options: &Options) -> Result<Identity, (Retry, String)> {
    match error {
        FsError::Changed(_) => Err((Retry::Changed, "it changed between read and write; it is read again".to_owned())),
        FsError::Crashed(point) => Err((Retry::Disk, format!("the write was cut short at {point}"))),
        e => match dalica_fs::read(path, options.max_file.max(u64::try_from(bytes.len()).unwrap_or(u64::MAX))) {
            Ok(Some(r)) if r.bytes == bytes => Ok(r.identity),
            _ => Err((Retry::Disk, e.to_string())),
        },
    }
}

/// The fields to write become `Written` (or a written conflict); `status` is the file's.
fn written(mut result: FileResult, to_write: &[(Field, bool)], status: Status) -> FileResult {
    for &(field, conflict) in to_write {
        result.fields.push((field, if conflict { FieldOutcome::Conflict { written: true } } else { FieldOutcome::Written }));
    }
    result.fields.sort_by_key(|(f, _)| *f);
    result.status = status;
    result
}

/// The fields to write failed with `why`, and so did the file.
fn fail_writes(mut result: FileResult, to_write: &[(Field, bool)], why: String) -> FileResult {
    for &(field, _) in to_write {
        result.fields.push((field, FieldOutcome::Failed(why.clone())));
    }
    result.fields.sort_by_key(|(f, _)| *f);
    result.status = Status::Failed(why);
    result
}

/// Not written yet: the file is read again on the next pass.
fn requeue_with(mut result: FileResult, to_write: &[(Field, bool)], retry: Retry, why: String) -> FileResult {
    for &(field, _) in to_write {
        result.fields.push((field, FieldOutcome::Skipped(format!("not written yet: {why}"))));
    }
    result.fields.sort_by_key(|(f, _)| *f);
    // Nothing is known for sure about the file now: no imports, no synced hashes.
    result.fields.retain(|(_, o)| !matches!(o, FieldOutcome::Imported | FieldOutcome::Conflict { written: false }));
    result.synced = None;
    result.file = None;
    result.status = Status::Requeued(why);
    result.retry = Some(retry);
    result
}

fn requeue(request: &Request, retry: Retry, why: String) -> FileResult {
    let mut r = FileResult::new(request, Status::Requeued(why));
    r.retry = Some(retry);
    r
}

/// The asked fields with `outcome`, and the file's `status`.
fn every_asked(request: &Request, status: Status, outcome: impl Fn() -> FieldOutcome) -> FileResult {
    let mut r = FileResult::new(request, status);
    if !request.first_scan {
        r.fields = request.fields.iter().map(|f| (*f, outcome())).collect();
    }
    r
}

fn failed(request: &Request, why: String) -> FileResult {
    every_asked(request, Status::Failed(why.clone()), || FieldOutcome::Failed(why.clone()))
}

fn skipped(request: &Request, why: String) -> FileResult {
    every_asked(request, Status::Skipped(why.clone()), || FieldOutcome::Skipped(why.clone()))
}

/// The file is not there: gone, if its folder is; waiting for its volume, if not.
fn absent(request: &Request) -> FileResult {
    let folder: PathBuf = request.path.parent().map(Path::to_path_buf).unwrap_or_default();
    match dalica_fs::identity(&folder) {
        Ok(Some(_)) => FileResult::new(request, Status::Gone),
        Ok(None) => {
            FileResult::new(request, Status::Waiting(format!("{} is not there (is its volume offline?); the write waits for it", folder.display())))
        }
        Err(e) => failed(request, e.to_string()),
    }
}
