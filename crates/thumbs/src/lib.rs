//! dalica-thumbs: The prioritised, de-duplicating job pool, the in-memory LRU and the on-disk thumbnail cache.
//!
//! Layer L2; see `docs/architecture.md` §2, §5 and §6. The [`Pool`] of worker threads, the
//! byte-budgeted [`Lru`], the thumbnail job ([`thumbnail`]: decode at size, upright), the colour
//! [`sketch`], the packed disk cache ([`pack`]) and the size [`ladder`] with its scan and fetch jobs,
//! which draw edited photos with `edit`'s renderer ([`ladder::Edits`]).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod ladder;
pub mod lru;
pub mod pack;
pub mod pool;
pub mod sketch;
pub mod upkeep;

use std::io::Read as _;
use std::path::Path;

pub use dalica_image::Rgba8;
pub use lru::Lru;
pub use pool::{Pool, Priority, Submitted, default_workers};

/// The largest photo file the thumbnail job reads.
pub const MAX_FILE: u64 = 1 << 30;

/// Why a thumbnail could not be made.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("cannot read the file: {0}")]
    Io(String),
    #[error(transparent)]
    Image(#[from] dalica_image::Error),
}

/// The thumbnail job: the photo at `path`, decoded to fit `long_side` pixels, upright, colour
/// managed. Runs on a pool worker; never on the UI thread.
pub fn thumbnail(path: &Path, long_side: u32, limits: &dalica_image::Limits) -> Result<Rgba8, Error> {
    thumbnail_with(path, long_side, &dalica_image::DecodeOptions::default(), limits)
}

/// [`thumbnail`] with the decode `options` (View → Use Color Management).
pub fn thumbnail_with(path: &Path, long_side: u32, options: &dalica_image::DecodeOptions, limits: &dalica_image::Limits) -> Result<Rgba8, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Io(e.to_string()))?;
    let size = file.metadata().map_err(|e| Error::Io(e.to_string()))?.len();
    if size > MAX_FILE {
        return Err(Error::Io(format!("{size} bytes is more than a photo file may be")));
    }
    let mut bytes = Vec::with_capacity(usize::try_from(size).unwrap_or(0));
    file.take(MAX_FILE).read_to_end(&mut bytes).map_err(|e| Error::Io(e.to_string()))?;
    Ok(dalica_image::decode_thumbnail_with(&bytes, long_side, options, limits)?)
}
