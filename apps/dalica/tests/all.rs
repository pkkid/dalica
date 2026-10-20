//! Every integration test of the app, in one binary (one link, tests run in parallel threads):
//! `cargo test -p dalica --test all <file>::` runs one file.

mod common;
mod control_headless;
mod control_security;
mod default_viewer;
mod instance;
mod os;
mod sound;
mod startup;
mod view;
