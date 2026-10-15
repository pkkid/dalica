//! Every integration test of `dalica-fs`, in one binary (one link instead of one per file). Each
//! former test file is a module: `cargo test -p dalica-fs --test all trash::` runs one.

mod common;

mod atomic;
mod create;
mod dirfd;
mod identity;
mod lock;
mod originals;
mod pack;
mod stale;
mod sync;
mod times;
mod transfer;
mod trash;
mod walk;
