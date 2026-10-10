//! M1, the headless half of "Star a photo, end to end" (`docs/roadmap.md` → M1 → Done when; run by
//! `cargo xtask accept M1 --headless`): the steps in `m1_steps.rs`.

// `m1.rs` uses the outcome; here only the steps' own checks matter.
#[allow(dead_code)]
#[path = "m1_steps.rs"]
mod steps;

#[test]
fn star_a_photo_end_to_end_headless() {
    steps::run();
}
