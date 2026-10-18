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
//! ([`crate::Op::typed`]). An operation of §3.1 without a module yet (`trim`, M15) is known and
//! kept, has a label, and renders nothing.
//!
//! The 27 effects of `editing.md` §8 (M12.1) have one module per family, so each renderer task
//! owns one file: [`colour`] (Sepia, B&W, Warmify, Tint, Saturation, Filtered B&W), [`detail`]
//! (Sharpen, Film Grain, Glow), [`focal`] (Soft Focus, Focal B&W, Graduated Tint), [`remap`]
//! (Invert Colors, Infrared Film, Duo-Tone, Boost), [`looks`] (HDR Look, Dream Glow, Plastic Lens),
//! [`soften`], [`stylize`] (Vignette, Pixelate, Focal Zoom), [`canvas`] (Widescreen Film, Border)
//! and [`mats`] (Drop Shadow, Museum Matte). Each effect's parameters are read with [`Reader`]
//! and printed with [`num`] and [`hex`], so every effect writes numbers, colours, checkboxes and
//! choices the same way (`editing.md` §8.5).
//!
//! A module's `VERSION` is 0 while it renders nothing (a stub: its `prepare` returns `None`, and
//! [`crate::Op::typed`] leaves it untyped, so Save and every reader of "can Dalica render this"
//! still see it as not rendered); the first renderer sets 1, and any later change to an
//! operation's pixels bumps it, so the render key ([`crate::version`]) remakes cached pictures of
//! the recipes that use it and no others.

use std::fmt;

use crate::frame::Point;
use crate::render::Geometry;

pub mod auto;
pub mod canvas;
pub mod colour;
pub mod crop;
pub mod detail;
pub mod focal;
pub mod looks;
pub mod mats;
pub mod orient;
pub mod remap;
pub mod retouch;
pub mod soften;
pub mod straighten;
pub mod stylize;
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
    /// The size of the whole canvas at this operation's place, before the recipe's rotations and
    /// flips, in output pixels: the stored frame after crop and straighten, grown or cut by every
    /// frame standing before the operation ([`crate::render::Reframe`]: Border, Drop Shadow, …)
    /// and by none after it. A region render prepares for the whole canvas.
    pub width: u32,
    pub height: u32,
    /// Output pixels per stored-frame pixel of the source (the frames keep it: their bands are
    /// laid out at this scale).
    pub scale: f64,
    /// The picture at this operation's place in the pipeline at the fixed analysis size
    /// ([`crate::analysis::PROXY_EDGE`]), when the operation asks for it ([`Adjust::ANALYSES`]).
    pub proxy: Option<&'a dalica_image::Rgba8>,
    /// The geometric stage with the frames before this operation only, to map stored-frame
    /// points (retouch strokes, effect centres) onto this canvas where the photo lies on it.
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

/// `v` brought into `range` (`NaN` to its low end); `-0` becomes `0` so it prints as `0`.
pub fn clamp(v: f64, range: (f64, f64)) -> f64 {
    if v.is_nan() { range.0 } else { v.clamp(range.0, range.1) + 0.0 }
}

/// A number as an effect's entry writes it: the shortest text that reads back to it (`-0` as `0`).
pub fn num(v: f64) -> String {
    crop::number(v)
}

/// A stored-frame point as an entry writes it: `x,y`.
pub fn num_point(p: Point) -> String {
    format!("{},{}", num(p.x), num(p.y))
}

/// An sRGB colour as an entry writes it: `#rrggbb`, lowercase.
pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// A checkbox as an entry writes it: `true` or `false`.
pub fn flag(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

/// `params` of an operation that takes none (`sepia()`): anything else is refused.
pub fn no_params<T: Operation>(params: &str, value: T) -> Result<T, ParamError> {
    if params.is_empty() { Ok(value) } else { Err(ParamError::new(T::NAME, params, "takes no parameters")) }
}

/// Reads an effect's parameter text, the fields in the order its syntax gives them, separated by
/// `,` (`editing.md` §8.5):
///
/// - a number: plain decimal text; out of its range (±infinity too) it is brought to the nearest
///   end; `NaN` or text that is not a number is refused;
/// - a colour: `#rrggbb` in sRGB, either case;
/// - a checkbox: `true` or `false`;
/// - a choice: one of its words, exactly;
/// - a point: two numbers, `x` then `y`, normalised in the stored frame (`data-model.md` §3.3)
///   and brought into 0..1.
///
/// A missing or extra field is refused ([`Reader::end`]). A refusal keeps the entry in the recipe
/// and skips it, as for every module.
pub struct Reader<'a> {
    op: &'static str,
    params: &'a str,
    fields: std::str::Split<'a, char>,
}

impl<'a> Reader<'a> {
    pub fn new(op: &'static str, params: &'a str) -> Reader<'a> {
        Reader { op, params, fields: params.split(',') }
    }

    /// A refusal of these parameters.
    pub fn refuse(&self, why: impl Into<String>) -> ParamError {
        ParamError::new(self.op, self.params, why)
    }

    fn next(&mut self, what: &str) -> Result<&'a str, ParamError> {
        match self.fields.next() {
            Some(field) => Ok(field),
            None => Err(self.refuse(format!("{what} is missing"))),
        }
    }

    /// A number brought into `range`.
    pub fn number(&mut self, what: &str, range: (f64, f64)) -> Result<f64, ParamError> {
        let field = self.next(what)?;
        match field.parse::<f64>() {
            Ok(v) if !v.is_nan() => Ok(clamp(v, range)),
            _ => Err(self.refuse(format!("{what} is not a number"))),
        }
    }

    /// An sRGB colour, `#rrggbb`.
    pub fn colour(&mut self, what: &str) -> Result<[u8; 3], ParamError> {
        let field = self.next(what)?;
        parse_hex(field).ok_or_else(|| self.refuse(format!("{what} is not #rrggbb")))
    }

    /// A checkbox, `true` or `false`.
    pub fn flag(&mut self, what: &str) -> Result<bool, ParamError> {
        match self.next(what)? {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(self.refuse(format!("{what} is not true or false"))),
        }
    }

    /// One of `choices`, by its word.
    pub fn choice<T: Copy>(&mut self, what: &str, choices: &[(&str, T)]) -> Result<T, ParamError> {
        let field = self.next(what)?;
        match choices.iter().find(|(word, _)| *word == field) {
            Some((_, v)) => Ok(*v),
            None => Err(self.refuse(format!("{what} is not one of its choices"))),
        }
    }

    /// A point of the stored frame, `x,y`, brought into 0..1.
    pub fn point(&mut self, what: &str) -> Result<Point, ParamError> {
        let x = self.number(what, UNIT)?;
        let y = self.number(what, UNIT)?;
        Ok(Point::new(x, y))
    }

    /// Refuses a field past the last one the syntax names.
    pub fn end(mut self) -> Result<(), ParamError> {
        match self.fields.next() {
            None => Ok(()),
            Some(_) => Err(self.refuse("holds more fields than its syntax")),
        }
    }
}

/// The range of a normalised coordinate and of the 0–1 sliders.
pub const UNIT: (f64, f64) = (0.0, 1.0);
/// The range of Fade (`editing.md` §8.1): 0 is the effect, 100 the input.
pub const FADE: (f64, f64) = (0.0, 100.0);
/// The range of an angle in degrees in the stored frame, clockwise from pointing up (90 points
/// right), as [`crate::fx::Gradient`] takes it.
pub const DEGREES: (f64, f64) = (0.0, 360.0);

/// `#rrggbb` (either case) as its three bytes.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#').filter(|h| h.len() == 6 && h.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let byte = |i: usize| hex.get(i..i + 2).and_then(|s| u8::from_str_radix(s, 16).ok());
    Some([byte(0)?, byte(2)?, byte(4)?])
}

/// Clamps a stored-frame point into 0..1 (`NaN` to 0).
pub fn clamp_point(p: Point) -> Point {
    Point::new(clamp(p.x, UNIT), clamp(p.y, UNIT))
}

/// An operation with a module, with its typed parameters: the geometry, the fixes and Tuning (M6,
/// M7), then the 27 effects in the order of their tabs' grids (`editing.md` §8).
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
    Sharpen(detail::Sharpen),
    Sepia(colour::Sepia),
    Bw(colour::Bw),
    Warmify(colour::Warmify),
    Grain(detail::Grain),
    Tint(colour::Tint),
    Saturation(colour::Saturation),
    SoftFocus(focal::SoftFocus),
    Glow(detail::Glow),
    FilteredBw(colour::FilteredBw),
    FocalBw(focal::FocalBw),
    GraduatedTint(focal::GraduatedTint),
    Infrared(remap::Infrared),
    PlasticLens(looks::PlasticLens),
    Hdr(looks::Hdr),
    Widescreen(canvas::Widescreen),
    DreamGlow(looks::DreamGlow),
    Invert(remap::Invert),
    Duotone(remap::Duotone),
    Boost(remap::Boost),
    Soften(soften::Soften),
    Vignette(stylize::Vignette),
    Pixelate(stylize::Pixelate),
    FocalZoom(stylize::FocalZoom),
    Border(canvas::Border),
    DropShadow(mats::DropShadow),
    MuseumMatte(mats::MuseumMatte),
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
            Typed::Sharpen($p) => $body,
            Typed::Sepia($p) => $body,
            Typed::Bw($p) => $body,
            Typed::Warmify($p) => $body,
            Typed::Grain($p) => $body,
            Typed::Tint($p) => $body,
            Typed::Saturation($p) => $body,
            Typed::SoftFocus($p) => $body,
            Typed::Glow($p) => $body,
            Typed::FilteredBw($p) => $body,
            Typed::FocalBw($p) => $body,
            Typed::GraduatedTint($p) => $body,
            Typed::Infrared($p) => $body,
            Typed::PlasticLens($p) => $body,
            Typed::Hdr($p) => $body,
            Typed::Widescreen($p) => $body,
            Typed::DreamGlow($p) => $body,
            Typed::Invert($p) => $body,
            Typed::Duotone($p) => $body,
            Typed::Boost($p) => $body,
            Typed::Soften($p) => $body,
            Typed::Vignette($p) => $body,
            Typed::Pixelate($p) => $body,
            Typed::FocalZoom($p) => $body,
            Typed::Border($p) => $body,
            Typed::DropShadow($p) => $body,
            Typed::MuseumMatte($p) => $body,
        }
    };
}

/// Runs `$m!(Type)` for every module type and gives the first `Some` (`None` when no type's
/// `NAME` is `$name`). Used to look an operation up by name.
macro_rules! by_name {
    ($name:expr, $T:ident => $body:expr) => {{
        let name: &str = $name;
        let mut found = None;
        if found.is_none() && name == orient::Rotate::NAME {
            type $T = orient::Rotate;
            found = Some($body);
        }
        if found.is_none() && name == orient::FlipH::NAME {
            type $T = orient::FlipH;
            found = Some($body);
        }
        if found.is_none() && name == orient::FlipV::NAME {
            type $T = orient::FlipV;
            found = Some($body);
        }
        if found.is_none() && name == crop::Crop::NAME {
            type $T = crop::Crop;
            found = Some($body);
        }
        if found.is_none() && name == straighten::Straighten::NAME {
            type $T = straighten::Straighten;
            found = Some($body);
        }
        if found.is_none() && name == retouch::Retouch::NAME {
            type $T = retouch::Retouch;
            found = Some($body);
        }
        if found.is_none() && name == auto::AutoFix::NAME {
            type $T = auto::AutoFix;
            found = Some($body);
        }
        if found.is_none() && name == auto::AutoContrast::NAME {
            type $T = auto::AutoContrast;
            found = Some($body);
        }
        if found.is_none() && name == auto::AutoColor::NAME {
            type $T = auto::AutoColor;
            found = Some($body);
        }
        if found.is_none() && name == tone::FillLight::NAME {
            type $T = tone::FillLight;
            found = Some($body);
        }
        if found.is_none() && name == tone::Tuning::NAME {
            type $T = tone::Tuning;
            found = Some($body);
        }
        if found.is_none() && name == detail::Sharpen::NAME {
            type $T = detail::Sharpen;
            found = Some($body);
        }
        if found.is_none() && name == colour::Sepia::NAME {
            type $T = colour::Sepia;
            found = Some($body);
        }
        if found.is_none() && name == colour::Bw::NAME {
            type $T = colour::Bw;
            found = Some($body);
        }
        if found.is_none() && name == colour::Warmify::NAME {
            type $T = colour::Warmify;
            found = Some($body);
        }
        if found.is_none() && name == detail::Grain::NAME {
            type $T = detail::Grain;
            found = Some($body);
        }
        if found.is_none() && name == colour::Tint::NAME {
            type $T = colour::Tint;
            found = Some($body);
        }
        if found.is_none() && name == colour::Saturation::NAME {
            type $T = colour::Saturation;
            found = Some($body);
        }
        if found.is_none() && name == focal::SoftFocus::NAME {
            type $T = focal::SoftFocus;
            found = Some($body);
        }
        if found.is_none() && name == detail::Glow::NAME {
            type $T = detail::Glow;
            found = Some($body);
        }
        if found.is_none() && name == colour::FilteredBw::NAME {
            type $T = colour::FilteredBw;
            found = Some($body);
        }
        if found.is_none() && name == focal::FocalBw::NAME {
            type $T = focal::FocalBw;
            found = Some($body);
        }
        if found.is_none() && name == focal::GraduatedTint::NAME {
            type $T = focal::GraduatedTint;
            found = Some($body);
        }
        if found.is_none() && name == remap::Infrared::NAME {
            type $T = remap::Infrared;
            found = Some($body);
        }
        if found.is_none() && name == looks::PlasticLens::NAME {
            type $T = looks::PlasticLens;
            found = Some($body);
        }
        if found.is_none() && name == looks::Hdr::NAME {
            type $T = looks::Hdr;
            found = Some($body);
        }
        if found.is_none() && name == canvas::Widescreen::NAME {
            type $T = canvas::Widescreen;
            found = Some($body);
        }
        if found.is_none() && name == looks::DreamGlow::NAME {
            type $T = looks::DreamGlow;
            found = Some($body);
        }
        if found.is_none() && name == remap::Invert::NAME {
            type $T = remap::Invert;
            found = Some($body);
        }
        if found.is_none() && name == remap::Duotone::NAME {
            type $T = remap::Duotone;
            found = Some($body);
        }
        if found.is_none() && name == remap::Boost::NAME {
            type $T = remap::Boost;
            found = Some($body);
        }
        if found.is_none() && name == soften::Soften::NAME {
            type $T = soften::Soften;
            found = Some($body);
        }
        if found.is_none() && name == stylize::Vignette::NAME {
            type $T = stylize::Vignette;
            found = Some($body);
        }
        if found.is_none() && name == stylize::Pixelate::NAME {
            type $T = stylize::Pixelate;
            found = Some($body);
        }
        if found.is_none() && name == stylize::FocalZoom::NAME {
            type $T = stylize::FocalZoom;
            found = Some($body);
        }
        if found.is_none() && name == canvas::Border::NAME {
            type $T = canvas::Border;
            found = Some($body);
        }
        if found.is_none() && name == mats::DropShadow::NAME {
            type $T = mats::DropShadow;
            found = Some($body);
        }
        if found.is_none() && name == mats::MuseumMatte::NAME {
            type $T = mats::MuseumMatte;
            found = Some($body);
        }
        found
    }};
}

fn version_of<T: Operation>(_: &T) -> u32 {
    T::VERSION
}

fn name_of<T: Operation>(_: &T) -> &'static str {
    T::NAME
}

impl Typed {
    /// The parameters of operation `name` read from `params`; `None` when `name` has no module.
    /// (A stub module, `VERSION` 0, reads its parameters too; [`crate::Op::typed`] is what leaves
    /// it untyped until it renders.)
    pub fn parse(name: &str, params: &str) -> Option<Result<Typed, ParamError>> {
        by_name!(name, T => T::parse(params).map(Typed::from))
    }

    /// Operation `name` at its defaults (an effect as its tile previews it, `editing.md` §8.1);
    /// `None` when `name` has no module.
    pub fn default_of(name: &str) -> Option<Typed> {
        by_name!(name, T => Typed::from(T::default()))
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

    /// The same parameters brought into their ranges.
    pub fn clamped(self) -> Typed {
        each_typed!(self, p => Typed::from(p.clamped()))
    }
}

impl From<orient::Rotate> for Typed {
    fn from(p: orient::Rotate) -> Typed {
        Typed::Rotate(p)
    }
}

impl From<orient::FlipH> for Typed {
    fn from(p: orient::FlipH) -> Typed {
        Typed::FlipH(p)
    }
}

impl From<orient::FlipV> for Typed {
    fn from(p: orient::FlipV) -> Typed {
        Typed::FlipV(p)
    }
}

impl From<crop::Crop> for Typed {
    fn from(p: crop::Crop) -> Typed {
        Typed::Crop(p)
    }
}

impl From<straighten::Straighten> for Typed {
    fn from(p: straighten::Straighten) -> Typed {
        Typed::Straighten(p)
    }
}

impl From<retouch::Retouch> for Typed {
    fn from(p: retouch::Retouch) -> Typed {
        Typed::Retouch(p)
    }
}

impl From<auto::AutoFix> for Typed {
    fn from(p: auto::AutoFix) -> Typed {
        Typed::AutoFix(p)
    }
}

impl From<auto::AutoContrast> for Typed {
    fn from(p: auto::AutoContrast) -> Typed {
        Typed::AutoContrast(p)
    }
}

impl From<auto::AutoColor> for Typed {
    fn from(p: auto::AutoColor) -> Typed {
        Typed::AutoColor(p)
    }
}

impl From<tone::FillLight> for Typed {
    fn from(p: tone::FillLight) -> Typed {
        Typed::FillLight(p)
    }
}

impl From<tone::Tuning> for Typed {
    fn from(p: tone::Tuning) -> Typed {
        Typed::Tuning(p)
    }
}

impl From<detail::Sharpen> for Typed {
    fn from(p: detail::Sharpen) -> Typed {
        Typed::Sharpen(p)
    }
}

impl From<colour::Sepia> for Typed {
    fn from(p: colour::Sepia) -> Typed {
        Typed::Sepia(p)
    }
}

impl From<colour::Bw> for Typed {
    fn from(p: colour::Bw) -> Typed {
        Typed::Bw(p)
    }
}

impl From<colour::Warmify> for Typed {
    fn from(p: colour::Warmify) -> Typed {
        Typed::Warmify(p)
    }
}

impl From<detail::Grain> for Typed {
    fn from(p: detail::Grain) -> Typed {
        Typed::Grain(p)
    }
}

impl From<colour::Tint> for Typed {
    fn from(p: colour::Tint) -> Typed {
        Typed::Tint(p)
    }
}

impl From<colour::Saturation> for Typed {
    fn from(p: colour::Saturation) -> Typed {
        Typed::Saturation(p)
    }
}

impl From<focal::SoftFocus> for Typed {
    fn from(p: focal::SoftFocus) -> Typed {
        Typed::SoftFocus(p)
    }
}

impl From<detail::Glow> for Typed {
    fn from(p: detail::Glow) -> Typed {
        Typed::Glow(p)
    }
}

impl From<colour::FilteredBw> for Typed {
    fn from(p: colour::FilteredBw) -> Typed {
        Typed::FilteredBw(p)
    }
}

impl From<focal::FocalBw> for Typed {
    fn from(p: focal::FocalBw) -> Typed {
        Typed::FocalBw(p)
    }
}

impl From<focal::GraduatedTint> for Typed {
    fn from(p: focal::GraduatedTint) -> Typed {
        Typed::GraduatedTint(p)
    }
}

impl From<remap::Infrared> for Typed {
    fn from(p: remap::Infrared) -> Typed {
        Typed::Infrared(p)
    }
}

impl From<looks::PlasticLens> for Typed {
    fn from(p: looks::PlasticLens) -> Typed {
        Typed::PlasticLens(p)
    }
}

impl From<looks::Hdr> for Typed {
    fn from(p: looks::Hdr) -> Typed {
        Typed::Hdr(p)
    }
}

impl From<canvas::Widescreen> for Typed {
    fn from(p: canvas::Widescreen) -> Typed {
        Typed::Widescreen(p)
    }
}

impl From<looks::DreamGlow> for Typed {
    fn from(p: looks::DreamGlow) -> Typed {
        Typed::DreamGlow(p)
    }
}

impl From<remap::Invert> for Typed {
    fn from(p: remap::Invert) -> Typed {
        Typed::Invert(p)
    }
}

impl From<remap::Duotone> for Typed {
    fn from(p: remap::Duotone) -> Typed {
        Typed::Duotone(p)
    }
}

impl From<remap::Boost> for Typed {
    fn from(p: remap::Boost) -> Typed {
        Typed::Boost(p)
    }
}

impl From<soften::Soften> for Typed {
    fn from(p: soften::Soften) -> Typed {
        Typed::Soften(p)
    }
}

impl From<stylize::Vignette> for Typed {
    fn from(p: stylize::Vignette) -> Typed {
        Typed::Vignette(p)
    }
}

impl From<stylize::Pixelate> for Typed {
    fn from(p: stylize::Pixelate) -> Typed {
        Typed::Pixelate(p)
    }
}

impl From<stylize::FocalZoom> for Typed {
    fn from(p: stylize::FocalZoom) -> Typed {
        Typed::FocalZoom(p)
    }
}

impl From<canvas::Border> for Typed {
    fn from(p: canvas::Border) -> Typed {
        Typed::Border(p)
    }
}

impl From<mats::DropShadow> for Typed {
    fn from(p: mats::DropShadow) -> Typed {
        Typed::DropShadow(p)
    }
}

impl From<mats::MuseumMatte> for Typed {
    fn from(p: mats::MuseumMatte) -> Typed {
        Typed::MuseumMatte(p)
    }
}

/// The history label of every operation of `data-model.md` §3.1 (`editing.md` §5.1, §8).
pub fn label(name: &str) -> Option<&'static str> {
    match by_name!(name, T => T::LABEL) {
        Some(label) => Some(label),
        // Video trimming has no module yet (M15).
        None if name == "trim" => Some("Trim"),
        None => None,
    }
}

/// The version of operation `name`: its module's, or 0 for a known operation without a module
/// (it renders nothing). `None` for names Dalica does not know.
pub fn version(name: &str) -> Option<u32> {
    match by_name!(name, T => T::VERSION) {
        Some(v) => Some(v),
        None => {
            label(name)?;
            Some(0)
        }
    }
}
