//! Every integration test of `dalica-create` in one binary (one link, one process): each former
//! test file is a module here and stays where it was (`cargo test -p dalica-create --test all
//! save_faults::` runs one of them). Shared helpers: `common/mod.rs`. The fault points
//! (`save_faults`, `save`) are armed per thread, so the tests run in parallel.

mod common;

mod export;
mod output;
mod save;
mod save_beside;
mod save_faults;
mod save_lossless;
mod watermark;
