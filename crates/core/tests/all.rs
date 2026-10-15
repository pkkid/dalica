//! Every integration test of `dalica-core`, in one binary (one link instead of one per file). Each
//! former test file is a module: `cargo test -p dalica-core --test all poskey::` runs one.

mod common;

mod app_id;
mod build;
mod poskey;
mod props;
mod sanitize;
