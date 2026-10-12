//! dalica-meta: EXIF / IPTC / XMP reading; write-back of owned fields by segment surgery (merge, never replace); MWG face regions; `.xmp` sidecars for RAW.
//!
//! Layer L1; see `docs/architecture.md` §2 and the contract in `docs/xmp-interop.md`. Every input
//! is hostile (`security.md` §4): walkers take [`Limits`], check every range before reading it and
//! return an [`Error`] instead of panicking or looping.
//!
//! - [`read`]: the owned fields of a photo file ([`OwnedFields`], resolved by the precedence rules
//!   of `xmp-interop.md` §2 from each source's values in [`Sources`]) and the EXIF summary the
//!   Properties pane shows ([`exif::Summary`]).
//! - [`container`]: one walker per container (JPEG, PNG, WebP, TIFF) that finds the metadata blocks
//!   and reports the byte span of every one ([`Scan`]).
//! - [`exif`], [`iptc`], [`xmp`]: one module per metadata kind (XMP is a stub until M6.9).
//! - [`listing`]: every field as label/value rows for Show All Metadata, bounded.
//! - [`write`]: the one write entry point [`write_owned`] and the verification every write passes.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod container;
pub mod exif;
pub mod fields;
pub mod iptc;
pub mod listing;
pub mod write;
pub mod xmp;
pub mod xmp_sidecar;

pub use container::{BlockKind, Container, Scan, Span, scan};
pub use fields::{DateSource, DateTaken, OwnedFields, Place, Sources};
pub use listing::{Listing, Row, listing};
pub use write::{Changes, WriteOptions, verify, write_owned};

/// Why a file's metadata could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Not a container this crate reads (JPEG, PNG, WebP or TIFF).
    #[error("not a file format whose metadata Dalica reads")]
    Unknown,
    #[error("the file ends too early")]
    Truncated,
    #[error("malformed: {0}")]
    Malformed(String),
    #[error("an offset points outside the data")]
    OutOfRange,
    #[error("an IFD chain loops")]
    Loop,
    /// A count or size went past one of the [`Limits`].
    #[error("too many {0}")]
    TooMany(&'static str),
    /// Writing into this container is not supported (yet).
    #[error("writing metadata into {0:?} files is not supported")]
    Unsupported(Container),
    /// A write produced a file that failed its verification; nothing may be written.
    #[error("the written file failed its check: {0}")]
    Verify(String),
}

impl From<dalica_image::Error> for Error {
    fn from(e: dalica_image::Error) -> Error {
        use dalica_image::Error as E;
        match e {
            E::Truncated => Error::Truncated,
            E::OutOfRange => Error::OutOfRange,
            E::Loop => Error::Loop,
            E::TooMany(what) => Error::TooMany(what),
            other => Error::Malformed(other.to_string()),
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Parse limits (`security.md` §4.2). Every walker takes them explicitly; a change to a value
/// needs a test at the new boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Most IFDs followed in one EXIF/TIFF structure.
    pub max_ifds: usize,
    /// Deepest chain of IFDs (IFD0 → EXIF IFD → Interoperability IFD).
    pub max_ifd_depth: usize,
    /// Most entries in one IFD.
    pub max_ifd_entries: usize,
    /// Longest string value kept (EXIF strings, captions, tags, text chunks), in bytes.
    pub max_string: usize,
    /// Most JPEG segments, PNG chunks or WebP chunks looked at in one file.
    pub max_segments: usize,
    /// Largest XMP packet, standard and extended parts together.
    pub max_xmp: usize,
    /// Largest output of one compressed PNG text chunk.
    pub max_inflate: usize,
    /// Most IPTC datasets in one block.
    pub max_iptc_datasets: usize,
    /// Largest IPTC dataset.
    pub max_dataset: usize,
    /// Most Photoshop image resources in one block.
    pub max_resources: usize,
    /// Most tags kept from one source.
    pub max_tags: usize,
    /// Most rows in a [`Listing`].
    pub max_rows: usize,
    /// Longest value text in a listing row, in characters.
    pub max_row_value: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            max_ifds: 1024,
            max_ifd_depth: 10,
            max_ifd_entries: 8192,
            max_string: 64 * 1024,
            max_segments: 4096,
            max_xmp: 16 * 1024 * 1024,
            max_inflate: 16 * 1024 * 1024,
            max_iptc_datasets: 10_000,
            max_dataset: 64 * 1024,
            max_resources: 4096,
            max_tags: 10_000,
            max_rows: 10_000,
            max_row_value: 1024,
        }
    }
}

impl Limits {
    /// The IFD walker's limits, passed explicitly (the rest of `dalica_image::Limits` is about
    /// pixels and does not apply here).
    pub fn ifd(&self) -> dalica_image::Limits {
        dalica_image::Limits {
            max_ifds: self.max_ifds,
            max_ifd_depth: self.max_ifd_depth,
            max_ifd_entries: self.max_ifd_entries,
            max_string: self.max_string,
            ..dalica_image::Limits::default()
        }
    }
}

/// A metadata block that was found but could not be read. The other blocks still count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub block: BlockKind,
    pub error: Error,
}

/// What a file says about itself: the owned fields, each source's values and the EXIF summary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub container: Option<Container>,
    /// The fields Dalica owns, resolved by the precedence rules (`xmp-interop.md` §2).
    pub owned: OwnedFields,
    /// Each source's values before precedence.
    pub sources: Sources,
    /// Camera, lens, exposure and the other read-only facts of the Properties pane.
    pub summary: exif::Summary,
    /// Blocks that were present but unreadable (a looping EXIF IFD, a refused extended XMP …).
    pub problems: Vec<Problem>,
}

/// Reads the metadata of a JPEG, PNG, WebP or TIFF file held in memory.
///
/// The container walk is strict: a damaged segment or chunk structure is an `Err`. A metadata
/// block that is itself damaged is skipped and listed in [`Metadata::problems`], so a broken EXIF
/// block does not hide the IPTC caption next to it.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<Metadata> {
    let scan = container::scan(bytes, limits)?;
    Ok(read_scan(&scan, limits))
}

/// [`read`] for a file already walked.
pub fn read_scan(scan: &Scan<'_>, limits: &Limits) -> Metadata {
    let mut out = Metadata { container: Some(scan.container), ..Metadata::default() };
    if let Some(block) = scan.exif.as_deref() {
        match exif::read::read(block, limits) {
            Ok(e) => {
                out.sources.exif = e.owned;
                out.summary = e.summary;
            }
            Err(error) => out.problems.push(Problem { block: BlockKind::Exif, error }),
        }
    }
    match scan.iptc_stream(limits) {
        Ok(Some(stream)) => match iptc::read::read(stream, limits) {
            Ok(i) => out.sources.iptc = i,
            Err(error) => out.problems.push(Problem { block: BlockKind::Iptc, error }),
        },
        Ok(None) => {}
        Err(error) => out.problems.push(Problem { block: BlockKind::Photoshop, error }),
    }
    out.problems.extend(scan.problems.iter().cloned());
    if let Some(packet) = scan.xmp.as_deref() {
        match xmp::read(packet, scan.extended_xmp.as_deref(), limits) {
            Ok(x) => out.sources.xmp = x,
            Err(error) => out.problems.push(Problem { block: BlockKind::Xmp, error }),
        }
    }
    out.owned = out.sources.resolve();
    out
}
