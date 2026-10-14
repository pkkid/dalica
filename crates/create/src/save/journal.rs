//! The journaled steps every Save in place shares (`data-model.md` §6): the record entry written
//! `pending` before anything moves, the original kept, the new file committed and verified, the
//! entry marked `done` last. The render (M8.12) and the lossless rotation (M8.14) both hand their
//! new file's bytes to [`commit`].

use std::io;

use dalica_fs::{Expect, Identity, originals as backup};
use dalica_image::Orientation;
use dalica_meta::fields::{clean_tags, clean_text};
use dalica_meta::write::PLACE_TOLERANCE;
use dalica_meta::{Changes, WriteOptions};
use dalica_sidecar::originals::{Entry, State};

use super::record::{Record, digest_matches};
use super::{Error, MAX_FILE, Options, Photo, Result, first_entry};

/// What a photo's Save starts from: its folder's record, and for a photo saved before its `done`
/// entry and the kept original's bytes.
pub(super) struct Base {
    record: Record,
    before: Option<Entry>,
    original: Option<Vec<u8>>,
}

impl Base {
    /// Reads `photo`'s folder record (a damaged one set aside) and, for a photo saved before, the
    /// kept original, checked against the record's size and digest. A `pending` entry is refused
    /// (an interrupted Save, repaired first); so is a backup no entry records.
    pub fn read(photo: &Photo, options: &Options) -> Result<Base> {
        let record = Record::read(&photo.folder, options.now, true)?;
        let backup_path = photo.folder.join(backup::DIR_NAME).join(&photo.name);
        let before = match record.entry(&photo.name)? {
            Some(e) if e.state == State::Pending => return Err(Error::Pending(photo.name.clone())),
            other => other,
        };
        let original = match &before {
            None => {
                // The first original is the one kept: a backup nobody recorded is never replaced.
                if dalica_fs::stat(&backup_path)?.is_some() {
                    return Err(dalica_fs::Error::Changed(backup_path).into());
                }
                None
            }
            Some(entry) => {
                let changed = || Error::from(dalica_fs::Error::Changed(backup_path.clone()));
                let read = dalica_fs::read(&backup_path, MAX_FILE)?.ok_or_else(changed)?;
                let same = digest_matches(&entry.digest, &mut read.bytes.as_slice()).map_err(|source| dalica_fs::Error::Io {
                    op: "read",
                    path: backup_path.clone(),
                    source,
                })?;
                if read.identity.size() != entry.size || !same {
                    return Err(changed());
                }
                Some(read.bytes)
            }
        };
        Ok(Base { record, before, original })
    }

    /// The recipe the saved file already holds (empty before the first Save).
    pub fn recipe(&self) -> &[String] {
        self.before.as_ref().map_or(&[], |e| e.recipe.as_slice())
    }

    /// The bytes to render from: the kept original, else the photo itself.
    pub fn source<'a>(&'a self, photo: &'a Photo) -> &'a [u8] {
        self.original.as_deref().unwrap_or(&photo.bytes)
    }
}

/// The new file a Save commits, and what it must read back as.
pub(crate) struct NewFile<'a> {
    pub bytes: &'a [u8],
    /// Its pixel size as displayed.
    pub width: u32,
    pub height: u32,
    /// Its EXIF orientation (1 after a render).
    pub orientation: Orientation,
    /// The owned fields merged into it, and the options they were written under.
    pub changes: &'a Changes,
    pub write: &'a WriteOptions,
}

/// Steps 4–8 for `photo` from `base`: the record entry with the whole recipe `recipe` written
/// `pending`, the original kept, `new` committed at the photo's name and verified, the entry
/// marked `done`. Returns the entry, the saved file's identity and whether the original moved.
pub(super) fn commit(photo: &Photo, base: Base, recipe: Vec<String>, new: &NewFile<'_>, options: &Options) -> Result<(Entry, Identity, bool)> {
    let Base { mut record, before, .. } = base;
    let first = before.is_none();
    let now = Some(options.now.as_millis());
    let mut entry = match &before {
        None => first_entry(photo, recipe, options.now),
        Some(old) => Entry { recipe, state: State::Pending, time: now, ..old.clone() },
    };
    let recorded_digest = entry.digest.clone();
    let check = |r: &mut dyn io::Read| digest_matches(&recorded_digest, r);
    let kept = backup::Recorded { size: entry.size, digest_matches: &check };
    let mut undo = Rollback { photo, record: &mut record, before: before.as_ref() };

    // 4. The journal: nothing has moved yet, so a failure here changes nothing.
    backup::ensure_dir(&photo.folder)?;
    undo.record.set(&photo.name, &entry)?;
    undo.record.store()?;

    // 5. Keep the original (first Save) or check the kept one (a later Save). A first Save's
    // step after the move can fail (its fsync, its check): the original is then in the backup and
    // goes back to its name before the entry does.
    if let Err(e) = backup::back_up(&photo.folder, &photo.name, photo.identity, (!first).then_some(&kept)) {
        let e = Error::from(e);
        if e.crashed() {
            return Err(e);
        }
        let backup_path = photo.folder.join(backup::DIR_NAME).join(&photo.name);
        return Err(match (first, dalica_fs::identity(&backup_path)) {
            (true, Ok(Some(id))) if id == photo.identity => undo.original_back(Expect::Absent, &kept, e),
            (true, Err(_)) => incomplete(photo, &e),
            _ => undo.entry_only(e),
        });
    }

    // 6. The new file at the photo's name: where nothing is (first Save), or over the saved file
    // only while it is the version read.
    let expect = if first { Expect::Absent } else { Expect::Unchanged(photo.identity) };
    let identity = match dalica_fs::replace(&photo.path, new.bytes, expect) {
        Ok(id) => id,
        Err(e) => {
            let e = Error::from(e);
            if e.crashed() {
                return Err(e);
            }
            // Refused before anything was written (the saved file changed, a link took its
            // place), or nothing at the name: Dalica's own state can be put back.
            let refused = matches!(e, Error::Fs(dalica_fs::Error::Changed(_) | dalica_fs::Error::Link(_)));
            return Err(match (first, dalica_fs::identity(&photo.path)) {
                (true, Ok(None)) => undo.original_back(Expect::Absent, &kept, e),
                (false, _) if refused => undo.entry_only(e),
                (false, Ok(Some(id))) if id == photo.identity => undo.entry_only(e),
                _ => incomplete(photo, &e),
            });
        }
    };

    // 7. Verify what is on disk.
    if let Err(why) = verify(photo, new, identity, &options.limits) {
        return Err(if first { undo.original_back(Expect::Unchanged(identity), &kept, Error::Verify(why)) } else { incomplete_why(photo, why) });
    }

    // 8. Done.
    entry.state = State::Done;
    let marked = record.set(&photo.name, &entry).and_then(|()| record.store());
    match marked {
        Ok(()) => Ok((entry, identity, first)),
        Err(e) if e.crashed() => Err(e),
        Err(e) => Err(incomplete(photo, &e)),
    }
}

/// Taking a Save back after a failure, so nothing has changed.
struct Rollback<'a> {
    photo: &'a Photo,
    record: &'a mut Record,
    before: Option<&'a Entry>,
}

impl Rollback<'_> {
    /// Nothing moved: the entry goes back to what it was. Returns the error to report.
    fn entry_only(&mut self, e: Error) -> Error {
        if e.crashed() {
            return e;
        }
        match self.record.put_back(&self.photo.name, self.before).and_then(|()| self.record.store()) {
            Ok(()) => e,
            Err(_) => incomplete(self.photo, &e),
        }
    }

    /// The original moved back over whatever `saved` says is at the name, then the entry back.
    fn original_back(&mut self, saved: Expect, kept: &backup::Recorded<'_>, e: Error) -> Error {
        match backup::restore(&self.photo.folder, &self.photo.name, saved, kept) {
            Ok(_) => self.entry_only(e),
            Err(_) => incomplete(self.photo, &e),
        }
    }
}

fn incomplete(photo: &Photo, e: &Error) -> Error {
    incomplete_why(photo, e.reason())
}

fn incomplete_why(photo: &Photo, reason: String) -> Error {
    Error::Incomplete { name: photo.name.clone(), reason }
}

/// The file at the photo's name is the one just written (`identity`, the same bytes), with the
/// pixel size, orientation and owned fields `new` asks for.
fn verify(photo: &Photo, new: &NewFile<'_>, identity: Identity, limits: &dalica_image::Limits) -> Result<(), String> {
    let read = match dalica_fs::read(&photo.path, MAX_FILE) {
        Ok(Some(read)) => read,
        Ok(None) => return Err("the new file is gone".into()),
        Err(e) => return Err(e.to_string()),
    };
    if read.identity != identity {
        return Err("the new file changed after it was written".into());
    }
    if read.bytes != new.bytes {
        return Err("the new file does not hold the bytes written".into());
    }
    let header = dalica_image::read_header(&read.bytes, limits).map_err(|e| e.reason())?;
    if header.orientation != new.orientation {
        return Err(format!("its orientation reads {} instead of {}", header.orientation.get(), new.orientation.get()));
    }
    if header.display_size() != (new.width, new.height) {
        let (w, h) = header.display_size();
        return Err(format!("it reads {w} × {h} pixels instead of {} × {}", new.width, new.height));
    }
    verify_owned(&read.bytes, new.changes, new.write)
}

/// Each owned field the write-back options allow reads back as asked (`meta`'s own check of a
/// metadata write, repeated on the file on disk). The error names the field, never its value.
fn verify_owned(bytes: &[u8], changes: &Changes, options: &WriteOptions) -> Result<(), String> {
    let changes = changes.allowed(options);
    if changes.is_empty() {
        return Ok(());
    }
    let limits = &options.limits;
    let meta = dalica_meta::read(bytes, limits).map_err(|e| format!("its metadata does not read back: {e}"))?;
    let got = &meta.owned;
    let fail = |what: &str| Err(format!("its {what} does not read back as written"));
    if let Some(caption) = &changes.caption
        && got.caption != caption.as_deref().and_then(|c| clean_text(c, limits.max_string))
    {
        return fail("caption");
    }
    if let Some(tags) = &changes.tags
        && got.tags != clean_tags(tags.iter().map(String::as_str), limits.max_string, limits.max_tags)
    {
        return fail("tags");
    }
    match changes.star {
        Some(true) if got.rating != Some(5) => return fail("star"),
        Some(false) if got.rating == Some(5) => return fail("star"),
        _ => {}
    }
    match changes.place {
        Some(Some(want)) if !got.place.is_some_and(|p| p.close_to(&want, PLACE_TOLERANCE)) => return fail("place"),
        Some(None) if got.place.is_some() => return fail("place"),
        _ => {}
    }
    if let Some(date) = &changes.date_taken
        && got.date_taken.as_ref() != Some(date)
    {
        return fail("date taken");
    }
    Ok(())
}
