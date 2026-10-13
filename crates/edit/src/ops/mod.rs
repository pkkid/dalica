//! The edit operations, one module per operation family (`architecture.md` §6.3, `data-model.md`
//! §3.1). Each module owns its operations' typed parameters ([`Operation`]: parse, print, range,
//! default), their [`Operation::VERSION`] and their history label ("Crop", "Tuning", "Rotate"), and
//! how they render:
//!
//! - geometry ([`Warp`]): the output size of an input and the mapping of points through it, as an
//!   affine map of pixel coordinates; the renderer composes every warp with the scale into one
//!   resample;
//! - every other operation ([`Adjust`]): prepared once per render (with the analysis proxy when it
//!   asks for one), then run over row bands of the picture in working precision ([`Band`]).
//!
//! [`Typed`] is the closed set of operations that have a module; [`crate::Op`] dispatches to it
//! ([`crate::Op::typed`]). An operation of §3.1 without a module yet (the effects, M12; `trim`,
//! M15) is known and kept, has a label, and renders nothing.
//!
//! A module's `VERSION` is 0 while it renders nothing (a stub); the first renderer sets 1, and any
//! later change to an operation's pixels bumps it, so the render key ([`crate::version`]) remakes
//! cached pictures of the recipes that use it and no others.

use std::fmt;

use crate::frame::Point;
use crate::render::Geometry;

pub mod auto;
pub mod crop;
pub mod orient;
pub mod retouch;
pub mod straighten;
pub mod tone;

/// Why an operation's parameter text was refused by its module.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{op}({params}): {reason}")]
pub struct ParamError {
    pub op: &'static str,
    pub params: String,
    pub reason: String,
}

impl ParamError {
    pub fn new(op: &'static str, params: &str, reason: impl Into<String>) -> ParamError {
        ParamError { op, params: params.chars().take(64).collect(), reason: reason.into() }
    }
}

/// An operation's typed parameters. `Display` prints the text between the parentheses, which
/// [`Operation::parse`] reads back to an equal value.
pub trait Operation: Sized + Clone + fmt::Debug + PartialEq + Default + fmt::Display {
    /// The name in the recipe, one of [`crate::LISTED`] or `rotate`, `flip_h`, `flip_v`.
    const NAME: &'static str;
    /// Bumped whenever the operation's pixels change (0: renders nothing yet).
    const VERSION: u32;
    /// The history label: "Undo " + this, "Redo " + this (`editing.md` §5.1).
    const LABEL: &'static str;

    /// The parameters `params` (the text between the parentheses) stand for.
    fn parse(params: &str) -> Result<Self, ParamError>;

    /// The same parameters brought into their range (the default: unchanged).
    fn clamped(self) -> Self {
        self
    }
}

/// A 2 × 3 affine map of continuous pixel coordinates (a pixel's centre is at `x + 0.5`):
/// `(x, y) ↦ (a·x + b·y + c, d·x + e·y + f)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Default for Affine {
    fn default() -> Affine {
        Affine::IDENTITY
    }
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 0.0, e: 1.0, f: 0.0 };

    pub const fn translate(x: f64, y: f64) -> Affine {
        Affine { a: 1.0, b: 0.0, c: x, d: 0.0, e: 1.0, f: y }
    }

    pub const fn scale(sx: f64, sy: f64) -> Affine {
        Affine { a: sx, b: 0.0, c: 0.0, d: 0.0, e: sy, f: 0.0 }
    }

    /// A rotation by `radians` (clockwise on screen, `y` pointing down) about `(cx, cy)`.
    pub fn rotate_about(radians: f64, cx: f64, cy: f64) -> Affine {
        let (s, c) = radians.sin_cos();
        Affine::translate(cx, cy).then_after(Affine { a: c, b: -s, c: 0.0, d: s, e: c, f: 0.0 }).then_after(Affine::translate(-cx, -cy))
    }

    /// The image of `(x, y)`.
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (self.a * x + self.b * y + self.c, self.d * x + self.e * y + self.f)
    }

    /// `self ∘ inner`: first `inner`, then `self`.
    pub fn then_after(self, inner: Affine) -> Affine {
        Affine {
            a: self.a * inner.a + self.b * inner.d,
            b: self.a * inner.b + self.b * inner.e,
            c: self.a * inner.c + self.b * inner.f + self.c,
            d: self.d * inner.a + self.e * inner.d,
            e: self.d * inner.b + self.e * inner.e,
            f: self.d * inner.c + self.e * inner.f + self.f,
        }
    }

    /// The inverse map, when the map is invertible and finite.
    pub fn inverse(&self) -> Option<Affine> {
        let det = self.a * self.e - self.b * self.d;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let (a, b, d, e) = (self.e / det, -self.b / det, -self.d / det, self.a / det);
        let inv = Affine { a, b, c: -(a * self.c + b * self.f), d, e, f: -(d * self.c + e * self.f) };
        inv.is_finite().then_some(inv)
    }

    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f].iter().all(|v| v.is_finite())
    }
}

/// A geometric operation in the stored frame (`data-model.md` §3.3): crop and straighten. (The
/// rotations and flips implement it too, through [`crate::Dihedral`], but the renderer applies
/// them last and pixel-exactly.)
pub trait Warp: Operation {
    /// The size of the result for a `width × height` input, in the input's pixels.
    fn output_size(&self, width: f64, height: f64) -> (f64, f64);

    /// For a `width × height` input: the map from the result's pixel coordinates to the input's.
    fn to_input(&self, width: f64, height: f64) -> Affine;

    /// Where the normalised input point `p` lands in the result, normalised (`None` when the
    /// operation's map is degenerate).
    fn map_point(&self, p: Point, width: f64, height: f64) -> Option<Point> {
        let (ow, oh) = self.output_size(width, height);
        let (x, y) = self.to_input(width, height).inverse()?.apply(p.x * width, p.y * height);
        let q = Point::new(x / ow, y / oh);
        (q.x.is_finite() && q.y.is_finite()).then_some(q)
    }

    /// The normalised input point under the normalised result point `p`.
    fn unmap_point(&self, p: Point, width: f64, height: f64) -> Option<Point> {
        let (ow, oh) = self.output_size(width, height);
        let (x, y) = self.to_input(width, height).apply(p.x * ow, p.y * oh);
        let q = Point::new(x / width, y / height);
        (q.x.is_finite() && q.y.is_finite()).then_some(q)
    }
}

/// How far around a pixel a [`Pass`] reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// Only the pixel itself (a curve, a colour matrix): such passes are fused, band by band, in
    /// working precision.
    Pixel,
    /// Pixels up to this many output pixels away (a blur, a sharpen).
    Pixels(u32),
    /// Anywhere in the picture (a retouch patch copies from a distant source).
    Whole,
}

/// What an [`Adjust`] operation is prepared with, once per render.
#[derive(Clone, Copy, Debug)]
pub struct Prepare<'a> {
    /// The whole output's size before the recipe's rotations and flips (the stored frame, after
    /// crop and straighten), in output pixels; a region render prepares for the whole output.
    pub width: u32,
    pub height: u32,
    /// Output pixels per stored-frame pixel of the source.
    pub scale: f64,
    /// The picture at this operation's place in the pipeline at the fixed analysis size
    /// ([`crate::analysis::PROXY_EDGE`]), when the operation asks for it ([`Adjust::ANALYSES`]).
    pub proxy: Option<&'a dalica_image::Rgba8>,
    /// The geometric stage, to map stored-frame points (retouch strokes, effect centres) into the
    /// output.
    pub geometry: &'a Geometry,
}

/// An operation that changes the picture's pixels but not its geometry.
pub trait Adjust: Operation {
    /// Whether [`Adjust::prepare`] needs [`Prepare::proxy`].
    const ANALYSES: bool = false;

    /// What to run over the picture, or `None` when the parameters change nothing (the default,
    /// and every stub).
    fn prepare(&self, ctx: &Prepare<'_>) -> Option<Box<dyn Pass>>;
}

/// One prepared operation, run over row bands of the picture (in parallel).
pub trait Pass: Send + Sync {
    fn reach(&self) -> Reach {
        Reach::Pixel
    }

    fn run(&self, band: &mut Band<'_>);
}

/// A run of rows of the output in working precision: sRGB-encoded values in 0..1 as `f32` (the
/// encoded values of the 8-bit pixels; an operation that wants linear light converts per pixel).
/// Alpha is not part of the band and never changes.
pub struct Band<'a> {
    /// `width × rows` pixels, row-major.
    pub pixels: &'a mut [[f32; 3]],
    pub width: u32,
    pub rows: u32,
    /// The band's top-left pixel in the whole output (whose size [`Prepare`] gives).
    pub x0: u32,
    pub y0: u32,
    /// The picture as it was before this pass, for passes that read beyond the pixel
    /// ([`Reach::Pixels`], [`Reach::Whole`]); `None` for [`Reach::Pixel`] passes.
    pub before: Option<&'a View<'a>>,
}

impl Band<'_> {
    /// The pixel at `(x, y)` of the band.
    pub fn get_mut(&mut self, x: u32, y: u32) -> Option<&mut [f32; 3]> {
        let i = (y as usize).checked_mul(self.width as usize)?.checked_add(x as usize)?;
        self.pixels.get_mut(i)
    }
}

/// Read access to the picture as it was before a pass, in whole-output coordinates. Reads outside
/// the picture take the nearest edge pixel.
pub struct View<'a> {
    pub(crate) data: &'a [u8],
    /// The rendered window's top-left pixel in the whole output, and its size.
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The whole output's size.
    pub(crate) full_width: u32,
    pub(crate) full_height: u32,
}

impl View<'_> {
    /// The pixel at `(x, y)` of the whole output, clamped into the picture, as 0..1 values.
    pub fn get(&self, x: i64, y: i64) -> [f32; 3] {
        let cx = x.clamp(0, i64::from(self.full_width.max(1)) - 1);
        let cy = y.clamp(0, i64::from(self.full_height.max(1)) - 1);
        // A region render holds enough margin; clamping into the window only guards a pass that
        // reads further than its reach says.
        let lx = (cx - i64::from(self.x0)).clamp(0, i64::from(self.width.max(1)) - 1);
        let ly = (cy - i64::from(self.y0)).clamp(0, i64::from(self.height.max(1)) - 1);
        let at = usize::try_from((ly * i64::from(self.width) + lx) * 4).ok();
        match at.and_then(|i| self.data.get(i..i.checked_add(3)?)) {
            Some(&[r, g, b]) => [f32::from(r) / 255.0, f32::from(g) / 255.0, f32::from(b) / 255.0],
            _ => [0.0; 3],
        }
    }

    pub fn full_size(&self) -> (u32, u32) {
        (self.full_width, self.full_height)
    }
}

/// An operation with a module, with its typed parameters.
#[derive(Clone, Debug, PartialEq)]
pub enum Typed {
    Rotate(orient::Rotate),
    FlipH(orient::FlipH),
    FlipV(orient::FlipV),
    Crop(crop::Crop),
    Straighten(straighten::Straighten),
    Retouch(retouch::Retouch),
    AutoFix(auto::AutoFix),
    AutoContrast(auto::AutoContrast),
    AutoColor(auto::AutoColor),
    FillLight(tone::FillLight),
    Tuning(tone::Tuning),
}

/// Runs `$body` with `$p` bound to the typed parameters of whichever variant `$t` holds.
macro_rules! each_typed {
    ($t:expr, $p:ident => $body:expr) => {
        match $t {
            Typed::Rotate($p) => $body,
            Typed::FlipH($p) => $body,
            Typed::FlipV($p) => $body,
            Typed::Crop($p) => $body,
            Typed::Straighten($p) => $body,
            Typed::Retouch($p) => $body,
            Typed::AutoFix($p) => $body,
            Typed::AutoContrast($p) => $body,
            Typed::AutoColor($p) => $body,
            Typed::FillLight($p) => $body,
            Typed::Tuning($p) => $body,
        }
    };
}

fn version_of<T: Operation>(_: &T) -> u32 {
    T::VERSION
}

fn name_of<T: Operation>(_: &T) -> &'static str {
    T::NAME
}

impl Typed {
    /// The parameters of operation `name` read from `params`; `None` when `name` has no module.
    pub fn parse(name: &str, params: &str) -> Option<Result<Typed, ParamError>> {
        fn read<T: Operation>(params: &str, wrap: fn(T) -> Typed) -> Option<Result<Typed, ParamError>> {
            Some(T::parse(params).map(wrap))
        }
        match name {
            orient::Rotate::NAME => read(params, Typed::Rotate),
            orient::FlipH::NAME => read(params, Typed::FlipH),
            orient::FlipV::NAME => read(params, Typed::FlipV),
            crop::Crop::NAME => read(params, Typed::Crop),
            straighten::Straighten::NAME => read(params, Typed::Straighten),
            retouch::Retouch::NAME => read(params, Typed::Retouch),
            auto::AutoFix::NAME => read(params, Typed::AutoFix),
            auto::AutoContrast::NAME => read(params, Typed::AutoContrast),
            auto::AutoColor::NAME => read(params, Typed::AutoColor),
            tone::FillLight::NAME => read(params, Typed::FillLight),
            tone::Tuning::NAME => read(params, Typed::Tuning),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        each_typed!(self, p => name_of(p))
    }

    pub fn version(&self) -> u32 {
        each_typed!(self, p => version_of(p))
    }

    /// The text between the parentheses.
    pub fn params(&self) -> String {
        each_typed!(self, p => p.to_string())
    }
}

/// The history label of every operation of `data-model.md` §3.1 (`editing.md` §5.1, §8).
pub fn label(name: &str) -> Option<&'static str> {
    Some(match name {
        orient::Rotate::NAME => orient::Rotate::LABEL,
        orient::FlipH::NAME => orient::FlipH::LABEL,
        orient::FlipV::NAME => orient::FlipV::LABEL,
        crop::Crop::NAME => crop::Crop::LABEL,
        straighten::Straighten::NAME => straighten::Straighten::LABEL,
        retouch::Retouch::NAME => retouch::Retouch::LABEL,
        auto::AutoFix::NAME => auto::AutoFix::LABEL,
        auto::AutoContrast::NAME => auto::AutoContrast::LABEL,
        auto::AutoColor::NAME => auto::AutoColor::LABEL,
        tone::FillLight::NAME => tone::FillLight::LABEL,
        tone::Tuning::NAME => tone::Tuning::LABEL,
        // Operations without a module yet: the effects (M12) and video trimming (M15).
        "trim" => "Trim",
        "sharpen" => "Sharpen",
        "sepia" => "Sepia",
        "bw" => "B&W",
        "warmify" => "Warmify",
        "grain" => "Film Grain",
        "tint" => "Tint",
        "saturation" => "Saturation",
        "soft_focus" => "Soft Focus",
        "glow" => "Glow",
        "filtered_bw" => "Filtered B&W",
        "focal_bw" => "Focal B&W",
        "graduated_tint" => "Graduated Tint",
        _ => return None,
    })
}

/// The version of operation `name`: its module's, or 0 for a known operation without a module
/// (it renders nothing). `None` for names Dalica does not know.
pub fn version(name: &str) -> Option<u32> {
    Some(match name {
        orient::Rotate::NAME => orient::Rotate::VERSION,
        orient::FlipH::NAME => orient::FlipH::VERSION,
        orient::FlipV::NAME => orient::FlipV::VERSION,
        crop::Crop::NAME => crop::Crop::VERSION,
        straighten::Straighten::NAME => straighten::Straighten::VERSION,
        retouch::Retouch::NAME => retouch::Retouch::VERSION,
        auto::AutoFix::NAME => auto::AutoFix::VERSION,
        auto::AutoContrast::NAME => auto::AutoContrast::VERSION,
        auto::AutoColor::NAME => auto::AutoColor::VERSION,
        tone::FillLight::NAME => tone::FillLight::VERSION,
        tone::Tuning::NAME => tone::Tuning::VERSION,
        other => {
            label(other)?;
            0
        }
    })
}
