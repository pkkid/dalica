//! TIFF write-back: a stub answering [`Error::Unsupported`] until M6.18 fills it
//! (`roadmap.md`): XMP (700) and IPTC (33723) by the append-only rule, place and date into the XMP mirrors. Whatever it returns passes [`super::verify`] before
//! [`super::write_owned`] hands it out.

use super::{Changes, WriteOptions};
use crate::container::{Container, Scan};
use crate::{Error, Metadata, Result};

/// The new bytes of `bytes` (walked as `scan`, read as `before`) with `changes` written.
pub(super) fn write(bytes: &[u8], scan: &Scan<'_>, before: &Metadata, changes: &Changes, options: &WriteOptions) -> Result<Vec<u8>> {
    let _ = (bytes, scan, before, changes, options);
    Err(Error::Unsupported(Container::Tiff))
}
