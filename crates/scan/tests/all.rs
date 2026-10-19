//! Every integration test of `dalica-scan`, in one binary (one link instead of one per file). Each
//! former test file is a module: `cargo test -p dalica-scan --test all watch::` runs one.

mod common;

mod albums;
mod devices;
mod facts;
mod import_source;
mod incremental;
mod legacy;
mod meta;
mod originals;
mod ptp;
mod rules;
mod scan;
mod skip;
mod volumes;
mod watch;
