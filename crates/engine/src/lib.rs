//! dalica-engine: `Session`: library + store + jobs + settings + undo stacks; the command registry (`cmd/<area>.rs`); keyboard contexts and the keymap; control-protocol types; demo library setup.
//!
//! Layer L4; see `docs/architecture.md` §2–§5. [`Session::execute`] is the one seam every surface
//! (UI, keyboard, CLI, control channel, MCP) goes through; commands are [`CommandSpec`]s
//! registered per area in `cmd/`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod albums_file;
pub mod automation;
pub mod cmd;
pub mod demo;
pub mod duplicates;
pub mod effect_tiles;
pub mod error;
pub mod face_models;
pub mod face_tiles;
pub mod faces_index;
pub mod faces_scan;
pub mod filejobs;
pub mod fileops;
pub mod fileops_recovery;
pub mod folders;
pub mod geocode;
pub mod history;
pub mod import;
pub mod import_run;
pub mod jobs;
pub mod keymap;
pub mod map_tiles;
pub mod metadata;
pub mod metadata_cache;
pub mod offline;
pub mod options;
mod people_file;
pub mod places;
pub mod preview;
pub mod registry;
pub mod save_recovery;
pub mod scanning;
pub mod search;
pub mod session;
pub mod settings;
pub mod suggestions;
pub mod synthetic;
pub mod thumbs;
pub mod undo;
pub mod variant;
pub mod viewer;
pub mod watching;

pub use effect_tiles::{EffectTile, EffectTileStats};
pub use error::{CmdError, OpenError};
pub use filejobs::{Applied, Done, FileChange, FileItem, FileJob, FileJobEvent, ItemFailure, Notice, Tally};
pub use fileops::{Direction, Driver, FileOp, FileOpEvent, FileOpKind, Ran};
pub use history::{PhotoHistory, RecipeChanges};
pub use jobs::{JobEntry, JobId, JobKind, JobProgress, JobState};
pub use keymap::{Binding, Key, Keymap, Override};
pub use metadata::{Field, Fields, MetadataChange, MetadataEvent, Origin};
pub use preview::{Histogram, PreviewError, PreviewImage, PreviewStats, PreviewUpdate, Quality, Refined, UpdateKind};
pub use registry::{CommandInfo, CommandSpec, Context, Disabled, Effect, History};
pub use scanning::ScanProgress;
pub use session::{Options, Session};
pub use thumbs::{CacheDir, Refreshed, Thumb, ThumbOptions, ThumbStats};
