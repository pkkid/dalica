//! dalica-core: Ids (`PhotoId`, `FolderId`, `AlbumId`, `PersonId`), errors, normalised geometry, dates, path and name utilities (collision names `-001`), position keys and the `Clock` trait.
//!
//! Layer L0; see `docs/architecture.md` §2. Pure: no file system, no threads of its own.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod app_id;
pub mod build;
pub mod clock;
pub mod color;
pub mod dates;
pub mod error;
pub mod geo;
pub mod geom;
pub mod ids;
pub mod names;
pub mod poskey;

pub use clock::{Clock, SystemClock, Timestamp, VirtualClock};
pub use dates::{Date, DateTime};
pub use error::Error;
pub use geom::{PixelRect, Rect01};
pub use ids::{AlbumId, FolderId, PersonId, PhotoId};
pub use poskey::PosKey;
