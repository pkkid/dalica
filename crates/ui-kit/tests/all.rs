//! Every integration test of the ui-kit, in one binary (one link, tests run in parallel threads):
//! `cargo test -p dalica-ui-kit --test all <file>::` runs one file.

mod badges;
mod common;
mod contrast;
mod datetime;
mod edit_widgets;
mod feedback;
mod fields;
mod form_widgets;
mod gallery;
mod icons;
mod icons_options;
mod input;
mod m9_widgets;
mod options_widgets;
mod widgets;
