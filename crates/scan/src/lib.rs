//! dalica-scan: Watched-folder rules, walks, change detection, `notify` events, offline volumes and card/folder import; turns what it finds into `Op`s.
//!
//! Layer L3; see `docs/architecture.md` §2 and `data-model.md` §7. M1 has the one-shot scan:
//!
//! - [`scan`] walks the watched roots through `dalica-fs` (bounded, links followed safely), skips
//!   dot files and folders (which covers `.dalica.json` and `.dalica-originals`) and OS system
//!   folders, recognises JPEG and PNG by their content, reads their headers (size as displayed,
//!   capture date) and each folder's sidecar (stars). It never fails as a whole: what it cannot
//!   read becomes a [`Warning`].
//! - [`diff`] is the verification pass: the ops that make the model match what is on disk, in
//!   batches. A photo deleted and another added in the same folder with the same size and
//!   modification time is a rename: the photo keeps its id and star.
//!
//! Folders under a root that could not be walked (an offline volume) are never removed.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod diff;
mod walk;

pub use diff::{BATCH, diff};
pub use walk::{FoundFolder, FoundPhoto, Scan, Warning, scan, skipped};
