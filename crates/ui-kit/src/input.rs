//! Canvas input helpers for the edit view (`editing.md` §3, §6.4): what the wheel, the trackpad
//! and a pinch do to a photo ([`wheel`]), and the glide after a pan is let go ([`flick`]). They read
//! egui's input and return what to do to the view; the caller owns the view (zoom, pan, the snap
//! to 100 %).

pub mod flick;
pub mod wheel;

pub use flick::Flick;
pub use wheel::Wheel;
