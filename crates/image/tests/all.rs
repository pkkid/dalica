//! Every integration test of `dalica-image` in one binary (one link, one process): each former
//! test file is a module here and stays where it was (`cargo test -p dalica-image --test all
//! tiff::` runs one of them). Shared helpers: `common.rs`. Separate targets: `heif` (its HEIF
//! half needs the `heif` feature: `cargo test -p dalica-image --features heif --test heif`) and
//! `corpus` (feature `corpus`, `cargo xtask test-corpus`).
//!
//! `jxl.rs` links `allocation-counter`, whose global allocator now counts every test here; it
//! counts each thread apart, so a measurement sees only its own thread's allocations.

mod common;

mod color;
mod decode;
mod encode;
mod jpeg_quality;
mod jpeg_transform;
mod jxl;
mod psd;
mod raw_containers;
mod raw_tiff;
mod tiff;
mod webp_gif_bmp;
