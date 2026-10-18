//! The workflow acceptance tests (`mcp.md` §9, `process.md` §5): scripted end-to-end user tasks
//! run over MCP against the real engine, one module per workflow (`wNN_<slug>`), checked only
//! through public state and the files on disk. `cargo xtask stats` counts them: a workflow passes
//! when all its tests pass and is ignored, with its reason, while its feature is not built yet.
//!
//! A test binary of its own (not a module of `tests/all.rs`): `cargo xtask stats` runs it alone
//! (`cargo test -p dalica-mcp --test workflows`) and reads its results.

#[path = "../common/mod.rs"]
#[allow(dead_code)]
mod common;
mod harness;

mod w01_organize_export;
mod w02_tray_across_folders;
mod w03_save_undo_revert;
mod w04_rebuild_loses_nothing;
mod w05_move_carries_data;
mod w06_search_save_results;
mod w07_batch_edit_one_step;
mod w08_foreign_metadata;
mod w09_hidden_photos;
mod w10_import_duplicates;
mod w11_effects_stack;
mod w12_destructive_asks;
mod w13_geotags;
mod w14_people;
