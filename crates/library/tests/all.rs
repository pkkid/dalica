//! Every integration test of `dalica-library`, in one binary (one link instead of one per file).
//! Each former test file is a module: `cargo test -p dalica-library --test all search::` runs one.

mod common;
mod seed;

mod albums;
mod legacy;
mod m6_fields;
mod m8_fields;
mod m9_fields;
mod ops;
mod people;
mod places;
mod search;
mod search_props;
mod sketch;
mod sync;
mod tags;
mod tree;
mod video;
