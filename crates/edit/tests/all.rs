//! Every integration test of `dalica-edit` in one binary (one link, one process): each former test
//! file is a module here and stays where it was (`cargo test -p dalica-edit --test all tone::` runs
//! one of them). Shared helpers: `common/mod.rs`.

mod common;

mod auto;
mod catalog;
mod colour_effects;
mod crop;
mod frame;
mod fx;
mod geometry;
mod recipe;
mod render;
mod retouch;
mod straighten;
mod suggest;
mod tone;
