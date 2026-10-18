//! Every integration test of `dalica-meta` in one binary (one link, one process): each former test
//! file is a module here and stays where it was (`cargo test -p dalica-meta --test all xmp::` runs
//! one of them). Shared: `build/mod.rs` (the file builders) and `common.rs`. The corpus test
//! (`corpus.rs`, feature `corpus`) stays its own target.

mod build;
mod common;

mod carry;
mod exif_edit;
mod iptc_write;
mod read;
mod regions;
mod write_jpeg;
mod write_png_webp_tiff;
mod xmp;
mod xmp_merge;
mod xmp_sidecar;
