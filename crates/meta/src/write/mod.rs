//! Writing the owned fields into a file: the one entry point [`write_owned`], a writer per
//! container, and the verification every write passes (`xmp-interop.md` §1, §3–§4).
//!
//! PNG, WebP and TIFF have their writers (M6.18); JPEG answers [`Error::Unsupported`] until M6.17
//! fills it. Whatever a writer produces, [`write_owned`] returns it only after
//! [`verify`] has checked it: the owned fields re-read equal the request, and every byte outside
//! the rewritable metadata spans is unchanged. This crate never touches the disk; the caller
//! writes the returned bytes atomically through `dalica-fs`.

mod jpeg;
mod png;
mod tiff;
mod webp;

use crate::container::{self, Container, Scan, outside, payload, same_bytes};
use crate::fields::{DateTaken, Place, clean_tags, clean_text};
use crate::{BlockKind, Error, Limits, Metadata, Result, read_scan};

/// What to change. `None` leaves a field as the file has it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// `Some(None)` or an empty caption removes it.
    pub caption: Option<Option<String>>,
    /// `Some(vec![])` removes every tag.
    pub tags: Option<Vec<String>>,
    /// Star writes rating 5; unstar writes 0 only when the file's rating is 5 (`xmp-interop.md`
    /// §2), so another application's rating survives.
    pub star: Option<bool>,
    /// `Some(None)` removes the place.
    pub place: Option<Option<Place>>,
    pub date_taken: Option<DateTaken>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.caption.is_none() && self.tags.is_none() && self.star.is_none() && self.place.is_none() && self.date_taken.is_none()
    }

    /// The changes the options allow (`xmp-interop.md` §1 principle 5: with an option off the
    /// field is still read but never written).
    pub fn allowed(&self, options: &WriteOptions) -> Changes {
        Changes {
            caption: self.caption.clone().filter(|_| options.captions_tags_places),
            tags: self.tags.clone().filter(|_| options.captions_tags_places),
            star: self.star.filter(|_| options.stars_as_ratings),
            place: self.place.filter(|_| options.captions_tags_places),
            date_taken: self.date_taken.clone().filter(|_| options.dates),
        }
    }
}

/// The write options of Options → General → Files (`xmp-interop.md` §2), the time of the write and
/// the parse limits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteOptions {
    /// Write captions, tags and places into photo files (default on).
    pub captions_tags_places: bool,
    /// Write stars as ratings (default off).
    pub stars_as_ratings: bool,
    /// Write date changes to files (default on).
    pub dates: bool,
    /// The time of the write, for `xmp:MetadataDate`: the caller sets it from the Session's clock
    /// (this crate reads no clock). `None` (the default, for callers without a clock) leaves the
    /// `xmp:MetadataDate` of Dalica's description as it is, and a new description gets none.
    pub metadata_date: Option<DateTaken>,
    pub limits: Limits,
}

impl Default for WriteOptions {
    fn default() -> WriteOptions {
        WriteOptions { captions_tags_places: true, stars_as_ratings: false, dates: true, metadata_date: None, limits: Limits::default() }
    }
}

/// How close a written place must read back, in degrees on each axis (about 0.1 m; degree,
/// minute and second rationals round).
pub const PLACE_TOLERANCE: f64 = 1e-6;

/// Writes `changes` (those `options` allow) into a file held in memory and returns the new bytes,
/// verified. Nothing to change returns the bytes as they are. The file is read first; a file
/// whose EXIF, XMP or IPTC cannot be read is not rewritten, since its data could not be kept.
pub fn write_owned(bytes: &[u8], changes: &Changes, options: &WriteOptions) -> Result<Vec<u8>> {
    let limits = &options.limits;
    let changes = changes.allowed(options);
    let scan = container::scan(bytes, limits)?;
    let before = read_scan(&scan, limits);
    if changes.is_empty() {
        return Ok(bytes.to_vec());
    }
    if let Some(p) = before.problems.iter().find(|p| rewritten(p.block)) {
        return Err(Error::Malformed(format!("its {:?} block is unreadable ({}); not rewriting it", p.block, p.error)));
    }
    let after = match scan.container {
        Container::Jpeg => jpeg::write(bytes, &scan, &before, &changes, options)?,
        Container::Png => png::write(bytes, &scan, &before, &changes, options)?,
        Container::WebP => webp::write(bytes, &scan, &before, &changes, options)?,
        // RAW files are never written (`xmp-interop.md` §4), even when they are TIFF-structured.
        Container::Tiff if !matches!(dalica_image::sniff(bytes), Some(dalica_image::Format::Tiff)) => {
            return Err(Error::Unsupported(Container::Tiff));
        }
        Container::Tiff => tiff::write(bytes, &scan, &before, &changes, options)?,
    };
    verify(bytes, &after, &changes, limits)?;
    Ok(after)
}

/// The blocks a write may rewrite, which must therefore read cleanly first.
fn rewritten(kind: BlockKind) -> bool {
    matches!(kind, BlockKind::Exif | BlockKind::Xmp | BlockKind::Photoshop | BlockKind::Iptc)
}

/// The check every write passes before its bytes may replace a file: the result is the same
/// container and reads back cleanly; every byte outside the rewritable metadata spans is unchanged
/// (for TIFF, written append-only, every such byte of the old file is still at its offset); and
/// each changed owned field re-reads equal to the request. The error names the field, never its
/// value (`security.md` §4.4).
pub fn verify(before: &[u8], after: &[u8], changes: &Changes, limits: &Limits) -> Result<()> {
    let fail = |what: &str| Error::Verify(what.to_owned());
    let a = container::scan(before, limits)?;
    let b = container::scan(after, limits).map_err(|e| Error::Verify(format!("the written file does not read back: {e}")))?;
    if a.container != b.container {
        return Err(fail("the written file is a different format"));
    }
    if !untouched(before, &a, after, &b) {
        return Err(fail("bytes outside the metadata changed"));
    }
    let old = read_scan(&a, limits);
    let new = read_scan(&b, limits);
    if let Some(p) = new.problems.iter().find(|p| rewritten(p.block) && !old.problems.iter().any(|o| o.block == p.block)) {
        return Err(Error::Verify(format!("the written {:?} block does not read back: {}", p.block, p.error)));
    }
    check_fields(&old, &new, changes, limits)
}

/// Whether every byte outside the rewritable spans survived.
fn untouched(before: &[u8], a: &Scan<'_>, after: &[u8], b: &Scan<'_>) -> bool {
    if a.container == Container::Tiff {
        // Append-only: what was outside the metadata is still where it was.
        return after.len() >= before.len() && outside(before.len(), &a.spans).into_iter().all(|r| before.get(r.clone()) == after.get(r));
    }
    same_bytes(&payload(before, &a.spans), &payload(after, &b.spans))
}

fn check_fields(old: &Metadata, new: &Metadata, changes: &Changes, limits: &Limits) -> Result<()> {
    let fail = |what: &str| Err(Error::Verify(format!("the {what} does not read back as written")));
    let got = &new.owned;
    if let Some(caption) = &changes.caption {
        let want = caption.as_deref().and_then(|c| clean_text(c, limits.max_string));
        if got.caption != want {
            return fail("caption");
        }
    }
    if let Some(tags) = &changes.tags
        && got.tags != clean_tags(tags.iter().map(String::as_str), limits.max_string, limits.max_tags)
    {
        return fail("tags");
    }
    match changes.star {
        Some(true) if got.rating != Some(5) => return fail("star"),
        Some(false) if old.owned.starred() && got.rating != Some(0) => return fail("star"),
        Some(false) if !old.owned.starred() && got.rating != old.owned.rating => return fail("rating"),
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
