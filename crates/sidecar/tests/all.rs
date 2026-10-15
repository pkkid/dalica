//! Every integration test of `dalica-sidecar`, in one binary (one link instead of one per file).
//! Each former test file is a module: `cargo test -p dalica-sidecar --test all albums::` runs one.

mod common;

mod albums;
mod originals;
