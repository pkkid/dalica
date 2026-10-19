//! dalica-scan: Watched-folder rules, walks, change detection, `notify` events, offline volumes and card/folder import; turns what it finds into `Op`s.
//!
//! Layer L3; see `docs/architecture.md` §2 and `data-model.md` §7.
//!
//! - [`scan_streaming`] walks the watched roots a folder at a time through `dalica-fs` (bounded,
//!   links followed safely), leaves out what the [`SkipRules`] say and the excluded subtrees,
//!   recognises every file by its content (the [`FileTypes`] toggles drop whole types), reads each
//!   header inside a per-file guard (`catch_unwind`, a time budget, failures remembered by path,
//!   size and modification time) and hands out each folder's photos as soon as the folder is read,
//!   with progress counts and a cancel flag. [`scan`] and [`scan_with`] gather the same into one
//!   [`Scan`]. A scan never fails as a whole: what it cannot read becomes a [`Warning`] or a
//!   [`Failure`].
//! - [`diff`] is the verification pass: the ops that make the model match what is on disk, in
//!   batches. A photo deleted and another added in the same folder with the same size and
//!   modification time is a rename: the photo keeps its id and star.
//! - [`volumes`](mod@volumes): the standard places, the mounted volumes, [`online`] and the
//!   bounded [`children`] listing for the Folder Manager.
//! - [`watch`](mod@watch): dirty folders from the system's change events (`notify`), or from a
//!   pure-Rust poller where those cannot be had; [`watch::rescan`] and [`diff_rescan`] verify only
//!   the dirty folders. Folder renames keep the folder's and its photos' ids.
//!
//! - [`meta`](mod@meta) (M6.32): with [`ReadMetadata`] on, the owned metadata of new and changed
//!   photos is read from the files (a RAW's `.xmp` sidecar, a DNG's embedded XMP) in the same pass;
//!   the diff imports it under `xmp-interop.md` §5 and ends with a sync op ([`meta::sync_parts`]).
//!
//! - [`rules`](mod@rules): the Folder Manager's modes per folder (Scan Always, Scan Once, Remove)
//!   and the faces flag, inherited by subfolders; the scan plans they give.
//!
//! Folders under a root that could not be walked (an offline volume) are never removed.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod devices;
mod diff;
pub mod facts;
pub mod import;
pub mod legacy;
pub mod meta;
pub mod originals;
pub mod ptp;
pub mod rules;
pub mod skip;
pub mod volumes;
mod walk;
pub mod watch;

pub use diff::{BATCH, diff, diff_found, diff_rescan};
pub use meta::{FileValues, Known, ReadMetadata};
pub use rules::{Effective, Mode, Plan, Rule, Rules, folder_path};
pub use skip::{FileType, FileTypes, Platform, Skip, SkipRules, skipped};
pub use volumes::{Child, Children, Place, PlaceKind, Places, Volume, VolumeKind, children, online, volumes};
pub use walk::{
    BUDGET, Event, Failure, FoundFolder, FoundPhoto, HeaderReader, Options, Progress, Scan, Summary, Warning, default_reader, scan, scan_streaming,
    scan_with,
};
pub use walk::{
    HeaderBytesReader, album_names, default_bytes_reader, folder_info, folder_people, folder_sort, photo_albums, photo_changed, photo_faces,
    photo_geo, photo_info,
};
