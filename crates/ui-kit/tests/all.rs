//! Every integration test of the ui-kit, in one binary (one link, tests run in parallel threads):
//! `cargo test -p dalica-ui-kit --test all <file>::` runs one file.

mod badges;
mod collage_widgets;
mod common;
mod contrast;
mod datetime;
mod edit_widgets;
mod effect_widgets;
mod face_widgets;
mod feedback;
mod fields;
mod fonts;
mod form_widgets;
mod gallery;
mod icons;
mod icons_collage;
mod icons_effects;
mod icons_options;
mod icons_people;
mod icons_video;
mod icons_viewing;
mod input;
mod m9_widgets;
mod map_widget;
mod options_widgets;
mod video_widgets;
mod viewing_widgets;
mod widgets;
