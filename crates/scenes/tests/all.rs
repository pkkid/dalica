//! Every integration test of `dalica-scenes` in one binary (one link, one process): each former
//! test file is a module here and stays where it was (`cargo test -p dalica-scenes --test all
//! formats::` runs one of them).

mod common;
mod formats;
mod scenes;
