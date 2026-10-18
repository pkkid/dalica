//! Every integration test of `dalica-edit` in one binary (one link, one process): each former test
//! file is a module here and stays where it was (`cargo test -p dalica-edit --test all tone::` runs
//! one of them). Shared helpers: `common/mod.rs`.

mod common;

mod auto;
mod canvas;
mod catalog;
mod colour_effects;
mod crop;
mod detail_effects;
mod focal_effects;
mod frame;
mod fx;
mod geometry;
mod mats;
mod recipe;
mod remap_effects;
mod render;
mod retouch;
mod soften;
mod straighten;
mod stylize_effects;
mod suggest;
mod tone;
