//! The engine's integration tests, one binary: every file here is a module (`mod layout;` is
//! `tests/layout.rs`), run one with `cargo test -p dalica-engine --test all <file>::`. Helpers they
//! share are in `common.rs`. `fileops_faults.rs` is its own binary (it re-runs itself per case).

mod common;

mod albums;
mod captions;
mod crop_commands;
mod dates;
mod dates_undo_written;
mod delete;
mod duplicates;
mod edited_thumbs;
mod execute_group;
mod export;
mod export_mixed;
mod facts;
mod filejobs;
mod fileops;
mod fileops_recovery;
mod fileops_store;
mod folder_info;
mod folder_move;
mod folders;
mod hide;
mod jobs;
mod keymap;
mod lessons_m9;
mod meta_import;
mod move_copy;
mod new_folder;
mod offline;
mod options_catalogue;
mod panic_hunt;
mod photo_history;
mod photo_metadata;
mod preview;
mod progressive_scan;
mod rename;
mod rotate;
mod save;
mod save_copy;
mod save_formats;
mod save_recovery;
mod search;
mod settings;
mod synthetic;
mod tags;
mod thumb_cache;
mod thumbs;
mod tuning_commands;
mod two_volumes;
mod undo_emptied_folder;
mod watching;
mod write_back;
