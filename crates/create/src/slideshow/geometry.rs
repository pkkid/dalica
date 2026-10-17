//! The slideshow's plain geometry (`viewing.md` §1.2): sizes, rectangles and points in the
//! screen's units, the fit of a photo on the screen and the caption plate's place. No egui: the UI
//! turns these into its own types and only paints.
//!
//! Every input is sanitised before use: a size or coordinate that is not finite, or is negative,
//! counts as zero, and a side is capped at [`MAX_SIDE`], so the arithmetic below never overflows
//! `f32` and never panics.

/// The largest screen side, in the screen's units, any geometry here works with.
pub const MAX_SIDE: f32 = 1_000_000.0;

/// A photo is never shown above twice its own pixel size (`viewing.md` §1.2).
pub const MAX_UPSCALE: f64 = 2.0;

/// A side or coordinate made safe: not finite or negative → 0, capped at [`MAX_SIDE`].
pub(crate) fn side(v: f32) -> f32 {
    if v.is_finite() && v > 0.0 { v.min(MAX_SIDE) } else { 0.0 }
}

/// A fraction made safe: not finite → 0, else clamped to 0..=1.
pub(crate) fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

/// A width and height, in the screen's units (points, as the UI paints).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Size {
        Size { width, height }
    }

    /// The size with each side made safe (see the module documentation).
    pub fn sane(self) -> Size {
        Size { width: side(self.width), height: side(self.height) }
    }
}

/// A point, in the screen's units, or normalised (0..1) inside a photo where a field says so.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    /// The middle of a photo, in normalised coordinates.
    pub const CENTRE: Point = Point { x: 0.5, y: 0.5 };
}

/// An axis-aligned rectangle: its top-left corner and its size, in the screen's units, with the
/// screen's top-left at (0, 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect { x, y, width, height }
    }

    /// The whole screen.
    pub fn screen(screen: Size) -> Rect {
        let s = screen.sane();
        Rect::new(0.0, 0.0, s.width, s.height)
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn centre(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// True when the rectangle covers no area.
    pub fn is_empty(&self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    /// The overlap of two rectangles (empty, with a zero size, when they do not overlap).
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }

    /// True when `inner` lies inside this rectangle, allowing `eps` of rounding at each edge.
    pub fn contains_rect(&self, inner: &Rect, eps: f32) -> bool {
        inner.x >= self.x - eps && inner.y >= self.y - eps && inner.right() <= self.right() + eps && inner.bottom() <= self.bottom() + eps
    }

    /// The rectangle moved by `(dx, dy)`.
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }
}

/// Where a photo of `photo` pixels (width, height) sits on a black screen of `screen`: as large as
/// fits, centred, never above [`MAX_UPSCALE`] times its pixel size. `pixels_per_point` is the
/// screen's scale (2 on a high-density screen), so the cap is in real pixels; a value that is not
/// finite or not positive counts as 1. A photo with a zero side answers an empty rectangle at the
/// screen's centre.
pub fn fit(photo: (u32, u32), screen: Size, pixels_per_point: f32) -> Rect {
    let s = screen.sane();
    let (sw, sh) = (f64::from(s.width), f64::from(s.height));
    let (pw, ph) = (f64::from(photo.0), f64::from(photo.1));
    if pw <= 0.0 || ph <= 0.0 || sw <= 0.0 || sh <= 0.0 {
        return Rect::new(s.width / 2.0, s.height / 2.0, 0.0, 0.0);
    }
    let ppp = if pixels_per_point.is_finite() && pixels_per_point > 0.0 { f64::from(pixels_per_point) } else { 1.0 };
    let scale = (sw / pw).min(sh / ph).min(MAX_UPSCALE / ppp);
    let (w, h) = ((pw * scale).min(sw), (ph * scale).min(sh));
    Rect::new(((sw - w) / 2.0) as f32, ((sh - h) / 2.0) as f32, w as f32, h as f32)
}

/// The caption's text size, in points (Inter, `viewing.md` §1.2).
pub const CAPTION_FONT_SIZE: f32 = 22.0;
/// The caption plate's opacity over the photo: `black α160`.
pub const CAPTION_PLATE_ALPHA: u8 = 160;
/// The plate's widest, as a fraction of the screen's width; the text wraps inside it.
pub const CAPTION_MAX_WIDTH: f32 = 0.7;
/// The caption wraps to at most this many lines.
pub const CAPTION_MAX_LINES: usize = 2;
/// The plate's padding around the text, horizontally and vertically, in points.
pub const CAPTION_PAD: (f32, f32) = (14.0, 8.0);
/// The plate's corner radius, in points.
pub const CAPTION_RADIUS: f32 = 8.0;
/// The gap under the plate, as a fraction of the screen's height ("near the bottom").
pub const CAPTION_BOTTOM: f32 = 0.08;

/// The widest the caption's text may be laid out on `screen`: the plate's widest less its padding.
pub fn caption_wrap_width(screen: Size) -> f32 {
    let s = screen.sane();
    (s.width * CAPTION_MAX_WIDTH - 2.0 * CAPTION_PAD.0).max(0.0)
}

/// The caption plate's rectangle for laid-out text of `text` size on `screen`: the text plus its
/// padding, no wider than [`CAPTION_MAX_WIDTH`] of the screen, centred, its bottom
/// [`CAPTION_BOTTOM`] of the screen's height above the bottom edge, and always inside the screen.
pub fn caption_plate(screen: Size, text: Size) -> Rect {
    let s = screen.sane();
    let t = text.sane();
    let w = (t.width + 2.0 * CAPTION_PAD.0).min(s.width * CAPTION_MAX_WIDTH);
    let h = (t.height + 2.0 * CAPTION_PAD.1).min(s.height);
    let bottom = s.height * (1.0 - CAPTION_BOTTOM);
    let y = (bottom - h).max(0.0);
    Rect::new((s.width - w) / 2.0, y, w, h)
}
