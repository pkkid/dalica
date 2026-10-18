//! The third kind of operation beside [`crate::ops::Warp`] and [`crate::ops::Adjust`]: an effect
//! that changes the picture's size where it stands among the effects (`editing.md` §8.3–§8.4,
//! `data-model.md` §3.3): Widescreen Film and Border (M12.13), Drop Shadow and Museum Matte (M12.14).
//!
//! - **Layout** ([`Reframe::layout`]): for an input picture of `width × height` output pixels, the
//!   size of the new canvas and where the input's top-left pixel lands on it ([`Layout`]), in whole
//!   pixels, so every render size, a region and the point maps agree on where the photo is. The
//!   offset may be negative (Widescreen Film crops); canvas pixels the input does not cover start
//!   opaque black.
//! - **Paint** ([`Reframe::prepare`]): a [`Pass`] run over the new canvas (bands in whole-canvas
//!   coordinates, as every pass), which paints the pixels outside the photo and may recolour the
//!   rest; it is prepared with the canvas's size and the [`crate::Geometry`] that includes this
//!   frame, so [`crate::fx::Space`] places stored-frame points and noise on the photo where it now
//!   lies.
//! - **The renderer** (`render.rs`) splits the effects at each frame: the effects before it work on
//!   the input picture, the frame places the input on its canvas, the effects after it work on the
//!   canvas. A region render maps its window back through each frame (only the part of the input
//!   that lands in the window, with the reach of the effects before it). `Geometry` knows every
//!   frame, so `output_size`, `display_size` and the point maps include them; the geometry (crop,
//!   straighten) stays first and the rotations and flips last, so a later crop crops the photo and
//!   a rotation turns the framed picture.
//! - **Limits:** a frame whose canvas would pass [`super::MAX_EDGE`] or [`super::MAX_PIXELS`] at the
//!   source's resolution, or whose layout overflows, is skipped and kept in the recipe (decided
//!   once per recipe and source, so every render size skips the same frames).
//!
//! A new frame effect implements [`Reframe`] in its `ops` module and is named in
//! `render::reframe_of` (and removed from the `Adjust` dispatch there).

use crate::ops::{Pass, Prepare};

/// Where a frame puts its input: the canvas's size and the input's top-left pixel on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The canvas, in output pixels (each side at least 1).
    pub size: (u32, u32),
    /// Where the input's pixel `(0, 0)` lands on the canvas (negative when the frame crops).
    pub at: (i64, i64),
}

impl Layout {
    /// The canvas pixel `(x, y)` in the input's pixels, when the input covers it.
    pub fn to_input(&self, input: (u32, u32), x: u32, y: u32) -> Option<(u32, u32)> {
        let ix = i64::from(x).checked_sub(self.at.0)?;
        let iy = i64::from(y).checked_sub(self.at.1)?;
        let inside = ix >= 0 && iy >= 0 && ix < i64::from(input.0) && iy < i64::from(input.1);
        inside.then_some((ix as u32, iy as u32))
    }
}

/// What a frame is prepared with: the canvas's [`Prepare`] (its size, the scale and the geometry
/// including this frame), the input's size and the layout.
#[derive(Clone, Copy, Debug)]
pub struct Placed<'a> {
    pub ctx: &'a Prepare<'a>,
    /// The input picture's size, in output pixels.
    pub input: (u32, u32),
    pub layout: Layout,
}

/// An effect that changes the picture's size (see the module).
pub trait Reframe {
    /// The canvas for an input of `width × height` output pixels and where the input lands on it;
    /// `None` when it cannot be made (an overflow), which skips the frame.
    fn layout(&self, width: u32, height: u32) -> Option<Layout>;

    /// What to paint over the canvas once the input is placed on it (`None`: nothing, the pixels
    /// outside the input stay opaque black).
    fn prepare(&self, placed: &Placed<'_>) -> Option<Box<dyn Pass>>;
}

/// `v` rounded to whole pixels, as a `u32` (`None` when it is not finite, negative or too large).
pub fn pixels(v: f64) -> Option<u32> {
    let r = v.round();
    (r.is_finite() && r >= 0.0 && r <= f64::from(u32::MAX)).then_some(r as u32)
}
