//! Every integration test of `dalica-thumbs`, in one binary (one link instead of one per file).
//! Each former test file is a module: `cargo test -p dalica-thumbs --test all pack::` runs one.

mod common;

mod ladder;
mod lru;
mod pack;
mod pool;
mod recipe;
mod sketch;
mod upkeep;
