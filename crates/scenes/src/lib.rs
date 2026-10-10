//! dalica-scenes: Procedural test and demo photos (gradients, horizons, soft blobs, text-free patterns) at any size, depth and orientation, with a minimal EXIF block. No media files.
//!
//! Layer L1; see `docs/architecture.md` §2. Every test photo in the workspace comes from here
//! (`docs/development.md` §6): the same [`Scene`] always gives the same pixels and the same file
//! bytes, on every platform, because rendering uses only `+ − × ÷ √` and a seeded generator.
//!
//! - [`Scene::render`] draws the upright picture; [`orient::store`] turns it into the pixels a
//!   camera would store for an EXIF orientation, so that applying the orientation shows it upright.
//! - [`jpeg`] and [`png`] encode a scene; [`exif::Exif`] describes the EXIF block (orientation,
//!   DateTimeOriginal, make/model, an optional embedded thumbnail, either byte order).
//! - [`demo_library`] plans a library of folders and photos as (relative path, bytes) pairs. This
//!   crate never writes to disk (layer rule 4); callers write the files through `dalica-fs`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod demo;
pub mod encode;
pub mod exif;
pub mod fingerprint;
pub mod orient;
pub mod render;

pub use demo::{DemoFile, demo_library};
pub use encode::{jpeg, png};
pub use exif::{ByteOrder, Exif};
pub use fingerprint::Fingerprint;
pub use render::{Kind, Rgb8, Scene};

/// The largest width or height a scene may have (the JPEG limit).
pub const MAX_SIDE: u32 = 65_535;
/// The most pixels a scene may have (a 64 MP cap, as for decoding in `dalica-image`).
pub const MAX_PIXELS: u64 = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("a scene must be 1 to {MAX_SIDE} pixels on each side and at most {MAX_PIXELS} pixels, not {0} × {1}")]
    Size(u32, u32),
    #[error("EXIF orientation must be 1 to 8, not {0}")]
    Orientation(u8),
    #[error("the EXIF block is {0} bytes; a JPEG APP1 segment holds at most 65 527")]
    ExifTooLarge(usize),
    #[error("JPEG encoding failed: {0}")]
    Jpeg(String),
    #[error("PNG encoding failed: {0}")]
    Png(String),
    #[error("a demo library holds at most {max} photos, not {0}", max = demo::MAX_PHOTOS)]
    TooManyPhotos(usize),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
