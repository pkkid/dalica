//! Every integration test of `dalica-store`, in one binary (one link instead of one per file). Each
//! former test file is a module: `cargo test -p dalica-store --test all carry::` runs one.

mod common;

mod albums;
mod carry;
mod fields;
mod intents;
mod m6_fields;
mod m8_fields;
mod m9_fields;
mod metawrite;
mod offline;
mod sidecars;
mod sketch;
mod stale;
