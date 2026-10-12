//! `.xmp` sidecars for RAW files (`xmp-interop.md` §7): which file is a RAW's sidecar, a RAW's
//! metadata read with its sidecar, and the merged bytes of a whole sidecar file.
//!
//! - [`Sidecars`]: the sidecar each RAW of a folder reads and writes, from the folder's file names:
//!   `<stem>.xmp` (`IMG_0001.CR3` → `IMG_0001.xmp`), also found as `.XMP` and as `<file>.xmp`
//!   (`IMG_0001.CR3.xmp`). Only RAW files have one: in a RAW+JPEG pair the stem sidecar is the
//!   RAW's, and the JPEG keeps its metadata inside itself. When several RAWs share a stem, the stem
//!   sidecar belongs to one of them and the others use `<file>.xmp`, so two photos never write
//!   into one sidecar. [`Sidecars::owner`] answers the other way round.
//! - [`read`]: a RAW's own metadata (what [`crate::read`] finds in a TIFF-structured RAW such as a
//!   DNG: EXIF, IPTC and the embedded XMP) with the sidecar's XMP taking the place of the embedded
//!   packet. A sidecar that is not well-formed XMP is reported in [`RawMetadata::damaged`] and the
//!   embedded packet is read instead.
//! - [`merge`]: the whole new sidecar file. An existing sidecar is merged into exactly as a packet
//!   inside a photo ([`xmp::merge`]: raw converters' develop settings, history, every other
//!   namespace and the packet wrapper stay byte for byte). Without one, the new sidecar starts as a
//!   copy of the RAW's embedded packet (so the sidecar, once it exists, says everything the RAW's
//!   XMP said) or, for a RAW without one, as an empty `x:xmpmeta` document. A damaged sidecar is
//!   answered with [`SidecarWrite::ReplaceDamaged`]: the writer keeps it aside
//!   (`<name>.xmp.corrupt-<time>`) and writes the new bytes.
//!
//! Like the rest of the crate this module never touches the disk: the caller lists the folder,
//! reads the files and writes the result atomically through `dalica-fs` (a sidecar that cannot be
//! read at all, such as one without read permission, is left alone and the write fails there).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::write::{Changes, WriteOptions};
use crate::xmp::{self, xml};
use crate::{BlockKind, Error, Limits, Metadata, Problem, Result, container};

/// The extension Dalica gives the sidecars it creates.
pub const EXTENSION: &str = "xmp";

/// The RDF namespace.
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// A new sidecar file before Dalica's description goes in: the `x:xmpmeta` document most
/// applications write for a sidecar, without a packet wrapper or padding (a file needs neither).
const NEW_SIDECAR: &str =
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n </rdf:RDF>\n</x:xmpmeta>\n";

/// A RAW file's sidecar, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidecarName {
    /// The sidecar found in the folder, to read and to merge into.
    pub existing: Option<String>,
    /// The name to write: the existing sidecar's, else `<stem>.xmp` (or `<file>.xmp` when another
    /// RAW owns the stem).
    pub write: String,
}

/// The stem and extension of a file name (`a.b.CR3` → `a.b`, `CR3`). A name without a dot, or
/// with only a leading one, has no extension.
fn split(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem, Some(ext)),
        _ => (name, None),
    }
}

/// The name before an `.xmp` extension (any case), if `name` has one.
fn xmp_base(name: &str) -> Option<&str> {
    let (base, ext) = split(name);
    ext.filter(|e| e.eq_ignore_ascii_case(EXTENSION)).map(|_| base)
}

/// Names compare without case: on the case-insensitive disks of Windows and macOS `IMG.xmp` and
/// `img.XMP` are one file, so two RAWs must never be given names that differ only by case.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// The sidecars of one folder's RAW files, resolved once from the folder's file names
/// (`xmp-interop.md` §7). Building it is O(n log n) in the number of names; each lookup is a hash
/// lookup.
///
/// - A RAW reads and writes the first of `<stem>.xmp`, `<stem>.XMP` (or the extension in any other
///   case), `<file>.xmp` and `<file>.XMP` that exists, and writes `<stem>.xmp` when none does.
/// - The stem forms are only for the RAW that owns the stem: a RAW named exactly the stem (whose
///   own `<file>.xmp` that is), else the first by name of the RAWs sharing it. The others use
///   `<file>.xmp`, so two photos never write into one sidecar. Non-RAW photos have no sidecar: in
///   a RAW+JPEG pair the stem sidecar is the RAW's and the JPEG keeps its metadata inside itself.
/// - Names are matched without case (a name in the RAW's own spelling first), and no two RAWs get
///   sidecar names that differ only by case, nor one that is another RAW's name. A RAW whose name
///   differs from another's only by case (possible only on a case-sensitive disk) can be left
///   without one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sidecars {
    by_raw: HashMap<String, SidecarName>,
    /// Existing sidecar name → its RAW.
    owners: HashMap<String, String>,
}

impl Sidecars {
    /// `names`: the file names of one folder; `is_raw`: whether a name is a RAW photo (the scan
    /// knows from the file's content, never its name).
    pub fn new<S: AsRef<str>>(names: &[S], is_raw: impl Fn(&str) -> bool) -> Sidecars {
        let mut raws: Vec<&str> = names.iter().map(AsRef::as_ref).filter(|&n| is_raw(n)).collect();
        raws.sort_unstable();
        raws.dedup();
        // The `.xmp` files present, by the folded name before the extension.
        let mut found: HashMap<String, Vec<&str>> = HashMap::new();
        for n in names.iter().map(AsRef::as_ref).filter(|&n| !is_raw(n)) {
            if let Some(base) = xmp_base(n) {
                found.entry(fold(base)).or_default().push(n);
            }
        }
        // Who owns each stem: the RAW named the stem, else the first by name.
        let mut stem_owner: HashMap<String, &str> = HashMap::new();
        for &r in &raws {
            stem_owner.entry(fold(split(r).0)).or_insert(r);
        }
        let mut named = HashSet::new();
        for &r in &raws {
            let key = fold(r);
            if stem_owner.contains_key(&key) && named.insert(key.clone()) {
                stem_owner.insert(key, r);
            }
        }
        // Names already given out, RAW names included: no sidecar may be one of them.
        let mut taken: HashSet<String> = raws.iter().map(|r| fold(r)).collect();
        let mut out = Sidecars::default();
        for &raw in &raws {
            let stem = split(raw).0;
            let owns_stem = stem_owner.get(&fold(stem)) == Some(&raw);
            let by_stem = format!("{stem}.{EXTENSION}");
            let by_file = format!("{raw}.{EXTENSION}");
            let mut candidates: Vec<(u8, bool, &str)> = Vec::new();
            if owns_stem {
                candidates.extend(found.get(&fold(stem)).into_iter().flatten().map(|&n| (0, n != by_stem, n)));
            }
            candidates.extend(found.get(&fold(raw)).into_iter().flatten().map(|&n| (1, n != by_file, n)));
            candidates.sort_unstable();
            let existing = candidates.into_iter().map(|(_, _, n)| n).find(|n| !taken.contains(&fold(n)));
            let preferred = existing.map(str::to_owned).into_iter().chain(owns_stem.then_some(by_stem)).chain([by_file]);
            let Some(write) = preferred.into_iter().find(|n| !taken.contains(&fold(n))) else { continue };
            taken.insert(fold(&write));
            if let Some(e) = existing {
                out.owners.insert(e.to_owned(), raw.to_owned());
            }
            out.by_raw.insert(raw.to_owned(), SidecarName { existing: existing.map(str::to_owned), write });
        }
        out
    }

    /// The sidecar of the RAW named `raw`: `None` for a name that is not a RAW of the folder.
    pub fn get(&self, raw: &str) -> Option<&SidecarName> {
        self.by_raw.get(raw)
    }

    /// The RAW whose sidecar the existing file `sidecar` is: `None` for a file no RAW of the folder
    /// reads (not an `.xmp`, another application's file, an `.XMP` shadowed by an `.xmp` of the
    /// same name, or one beside a non-RAW photo only).
    pub fn owner(&self, sidecar: &str) -> Option<&str> {
        self.owners.get(sidecar).map(String::as_str)
    }
}

/// Where a RAW's XMP values came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XmpFrom {
    /// Its `.xmp` sidecar.
    Sidecar,
    /// The packet embedded in the RAW (a DNG's), because there is no readable sidecar.
    Embedded,
    /// Neither: no sidecar (or a damaged one) and no embedded packet.
    Nothing,
}

/// A RAW file's metadata, read with its sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMetadata {
    /// The RAW's own metadata with the sidecar's XMP in place of the embedded packet, the owned
    /// fields resolved by the precedence rules (`xmp-interop.md` §2): the RAW's EXIF date and
    /// place still come first.
    pub metadata: Metadata,
    pub xmp_from: XmpFrom,
    /// The sidecar is not well-formed XMP (a writer keeps it aside before writing a new one).
    pub damaged: Option<Error>,
}

/// Reads a RAW file's metadata with its sidecar (`xmp-interop.md` §7). `raw` is the RAW's bytes
/// when the caller has them (only TIFF-structured RAWs such as DNG hold anything this crate reads;
/// for the others `None` is the same as their bytes); `sidecar` is the sidecar's bytes, if one was
/// found ([`Sidecars`]). A sidecar of nothing but white space counts as none.
pub fn read(raw: Option<&[u8]>, sidecar: Option<&[u8]>, limits: &Limits) -> RawMetadata {
    let (mut metadata, embedded) = match raw.map(|b| container::scan(b, limits)) {
        None | Some(Err(Error::Unknown)) => (Metadata::default(), false),
        Some(Err(error)) => (Metadata { problems: vec![Problem { block: BlockKind::Structure, error }], ..Metadata::default() }, false),
        Some(Ok(scan)) => {
            let has = scan.xmp.as_deref().is_some_and(|p| !xml::is_blank(p));
            (crate::read_scan(&scan, limits), has)
        }
    };
    let mut xmp_from = if embedded { XmpFrom::Embedded } else { XmpFrom::Nothing };
    let mut damaged = None;
    if let Some(bytes) = sidecar.filter(|b| !xml::is_blank(b)) {
        match well_formed(bytes, limits).and_then(|_| xmp::read(bytes, None, limits)) {
            Ok(x) => {
                metadata.sources.xmp = x;
                // The embedded packet no longer counts, nor does a problem with it.
                metadata.problems.retain(|p| !matches!(p.block, BlockKind::Xmp | BlockKind::ExtendedXmp));
                xmp_from = XmpFrom::Sidecar;
            }
            Err(e) => damaged = Some(e),
        }
    }
    metadata.owned = metadata.sources.resolve();
    RawMetadata { metadata, xmp_from, damaged }
}

/// What to do with a RAW's sidecar file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidecarWrite {
    /// Nothing to write: the sidecar (or, without one, the RAW's embedded packet) already holds what
    /// was asked. A damaged sidecar is left as it is too.
    Unchanged,
    /// The whole new sidecar file, replacing the one read or created under [`SidecarName::write`].
    Write(Vec<u8>),
    /// The sidecar read is not well-formed XMP: keep it aside (`<name>.corrupt-<time>`), then write
    /// `bytes`, a new sidecar.
    ReplaceDamaged { bytes: Vec<u8>, damaged: Error },
}

/// The new sidecar file for `changes` (those `options` allow; `xmp-interop.md` §1 principle 5).
/// `sidecar` is the existing sidecar's bytes (blank counts as none), `raw` the RAW's bytes when
/// the caller has them (a new sidecar starts from the packet embedded in a DNG). The time of the
/// write (`xmp:MetadataDate`) is [`WriteOptions::metadata_date`], as for photo files. Errors are the merge writer's: the result never
/// replaces a well-formed sidecar unless its every changed property reads back as written.
pub fn merge(raw: Option<&[u8]>, sidecar: Option<&[u8]>, changes: &Changes, options: &WriteOptions) -> Result<SidecarWrite> {
    let limits = &options.limits;
    let metadata_date = options.metadata_date.as_ref();
    let changes = changes.allowed(options);
    let damaged = match sidecar.filter(|b| !xml::is_blank(b)) {
        Some(bytes) => match well_formed(bytes, limits) {
            Ok(()) => {
                return match xmp::merge::merge_at(Some(bytes), &changes, metadata_date, limits)? {
                    Some(m) => Ok(SidecarWrite::Write(checked(m.into_bytes(), limits)?)),
                    None => Ok(SidecarWrite::Unchanged),
                };
            }
            Err(e) => Some(e),
        },
        None => None,
    };
    let base = raw.and_then(|r| embedded(r, limits)).unwrap_or(Cow::Borrowed(NEW_SIDECAR.as_bytes()));
    let Some(m) = xmp::merge::merge_at(Some(&base), &changes, metadata_date, limits)? else { return Ok(SidecarWrite::Unchanged) };
    // A file needs no padding: the copy of an embedded packet loses it (and nothing else).
    let bytes = m.fit(m.min_len()).unwrap_or_else(|| m.into_bytes());
    let bytes = checked(bytes, limits)?;
    Ok(match damaged {
        Some(damaged) => SidecarWrite::ReplaceDamaged { bytes, damaged },
        None => SidecarWrite::Write(bytes),
    })
}

/// The well-formed XMP packet embedded in a RAW (a DNG's tag 700), if there is one.
fn embedded<'r>(raw: &'r [u8], limits: &Limits) -> Option<Cow<'r, [u8]>> {
    let packet = container::scan(raw, limits).ok()?.xmp?;
    (!xml::is_blank(&packet) && well_formed(&packet, limits).is_ok()).then_some(packet)
}

/// A written sidecar must itself read as well-formed XMP.
fn checked(bytes: Vec<u8>, limits: &Limits) -> Result<Vec<u8>> {
    match well_formed(&bytes, limits) {
        Ok(()) => Ok(bytes),
        Err(e) => Err(Error::Verify(format!("the written sidecar is not well-formed XMP: {e}"))),
    }
}

/// Whether a sidecar is well-formed XMP that Dalica can merge into: UTF-8 within
/// [`Limits::max_xmp`], XML within the tokenizer's limits (`security.md` §4.2), with an
/// `rdf:RDF` element that has content (an empty `<rdf:RDF/>` has nowhere to write).
pub fn well_formed(bytes: &[u8], limits: &Limits) -> Result<()> {
    if bytes.len() > limits.max_xmp {
        return Err(Error::TooMany("XMP bytes"));
    }
    let src = std::str::from_utf8(bytes).map_err(|_| Error::Malformed("XMP: not UTF-8".to_owned()))?;
    let doc = xml::parse(src)?;
    match doc.elements.iter().find(|e| doc.is(&e.name, RDF, "RDF")) {
        Some(e) if !e.empty => Ok(()),
        Some(_) => Err(Error::Malformed("XMP: an empty rdf:RDF element".to_owned())),
        None => Err(Error::Malformed("XMP: no rdf:RDF element".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_names() {
        assert_eq!(split("IMG_0001.CR3"), ("IMG_0001", Some("CR3")));
        assert_eq!(split("a.b.NEF"), ("a.b", Some("NEF")));
        assert_eq!(split("IMG"), ("IMG", None));
        assert_eq!(split(".hidden"), (".hidden", None));
        assert_eq!(split("x."), ("x", Some("")));
        assert_eq!(xmp_base("IMG.XmP"), Some("IMG"));
        assert_eq!(xmp_base("IMG.CR3.xmp"), Some("IMG.CR3"));
        assert_eq!(xmp_base("IMG.xmpx"), None);
        assert_eq!(xmp_base(".xmp"), None);
    }

    #[test]
    fn the_new_sidecar_is_well_formed() {
        assert_eq!(well_formed(NEW_SIDECAR.as_bytes(), &Limits::default()), Ok(()));
    }
}
