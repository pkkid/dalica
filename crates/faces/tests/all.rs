//! Every integration test of `dalica-faces`, in one binary (one link instead of one per file). Each
//! test file is a module: `cargo test -p dalica-faces --test all grouping::` runs one.

mod grouping;
mod inference;
