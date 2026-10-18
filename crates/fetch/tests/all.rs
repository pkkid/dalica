//! Every integration test of `dalica-fetch`, in one binary.
//! Each test file is a module: `cargo test -p dalica-fetch --test all download::` runs one.

mod download;
mod http;
