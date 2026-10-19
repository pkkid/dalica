//! Legacy edit operations mapped to Dalica's (`data-model.md` §9, `editing.md` §13).
//!
//! **Provenance.** The legacy organizer's edit list is implemented from public prose descriptions
//! of its per-folder sidecar and from black-box observation of the files it writes, as recorded in
//! the project's private research notes (the data-and-files and edit-view studies). No code, text
//! or data of the legacy program or of any GPL project was looked at or copied; the parameter
//! ranges below are the ones those descriptions give for the legacy controls.
//!
//! The legacy sidecar keeps one photo's edits as a list of tokens `name=1[,param]*` (the list is
//! the edit history, oldest first), a separate redo list in the same grammar, the photo's library
//! rotation in quarter turns, a mirror flag, the effective crop rectangle and a text overlay.
//! [`convert`] turns them into recipe entries:
//!
//! - each token whose operation and parameters the descriptions record with confidence becomes
//!   the equivalent Dalica operation, its parameters converted linearly from the legacy control's
//!   range (this module's constants) onto the Dalica operation's own (its module's constants),
//!   then clamped as the module clamps them;
//! - anything else (an unknown or differently cased name, a wrong number of parameters, a number
//!   that is not finite, an enabled flag other than `1`, the parameterless markers of red-eye and
//!   retouch fixes baked into the pixels, an operation whose parameters are not recorded with
//!   confidence) is kept verbatim as `legacy:<name>(<text after the first '='>)` (a token without
//!   `=` as `legacy:<token>`), in its place: Dalica skips it when rendering and never drops the
//!   entries after it;
//! - crop rectangles (`rect64`, four 16-bit fractions of the stored pixels, `0xffff` = 1) are mapped
//!   into Dalica's stored frame (`data-model.md` §3.3) through the photo's EXIF orientation, and
//!   only the last crop of the edit list is carried, in its place (only the last crop ever counts);
//! - the rotation and flip become `flip_h()` then `rotate(…)` at the start of the recipe (a
//!   mirror, then the clockwise turns: [`Dihedral`]'s own order); the text overlay and its on/off
//!   flag are kept as `legacy:text(…)` and `legacy:textactive(…)` at the end;
//! - the redo list is mapped token by token the same way (every crop kept: each is its own step).
//!
//! Nothing here panics or allocates beyond the input's size; more entries than a recipe may hold
//! is [`Error::TooManyOperations`].

use dalica_image::Orientation;

use crate::frame::Rect;
use crate::ops::{self, Typed, auto, canvas, colour, crop::Crop, detail, looks, remap, stylize, tone};
use crate::syntax::{LEGACY_PREFIX, Op, Turn};
use crate::{Dihedral, Error, MAX_OPERATIONS, Result};

/// The value of a `rect64` field that stands for 1 (the whole width or height).
pub const RECT64_ONE: f64 = 65535.0;

/// Fill Light (`fill`, and the first field of `finetune2`).
pub const FILL: (f64, f64) = (0.0, 1.0);
/// Tuning's Highlights.
pub const HIGHLIGHTS: (f64, f64) = (0.0, 0.48);
/// Tuning's Shadows.
pub const SHADOWS: (f64, f64) = (0.0, 0.48);
/// Tuning's Color Temperature.
pub const TEMP: (f64, f64) = (-1.0, 1.0);
/// Sharpen's Amount (`unsharp2`).
pub const SHARPEN: (f64, f64) = (0.0, 3.0);
/// Saturation's Amount (`sat`).
pub const SATURATION: (f64, f64) = (-1.0, 1.0);
/// Glow's Intensity (`glow2`).
pub const GLOW_INTENSITY: (f64, f64) = (0.0, 1.0);
/// Glow's Radius (`glow2`).
pub const GLOW_RADIUS: (f64, f64) = (0.3, 30.0);
/// Boost's Strength.
pub const BOOST: (f64, f64) = (0.0, 100.0);
/// The HDR look's Radius.
pub const HDR_RADIUS: (f64, f64) = (1.3, 80.0);
/// The HDR look's Strength.
pub const HDR_STRENGTH: (f64, f64) = (1.0, 7.0);
/// The plastic-lens look's Blur Edges (`Holga`).
pub const BLUR_EDGES: (f64, f64) = (0.0, 100.0);
/// The plastic-lens look's Grain (`Holga`).
pub const LENS_GRAIN: (f64, f64) = (0.0, 100.0);
/// Duo-Tone's Brightness (`TwoTone`).
pub const DUO_BRIGHTNESS: (f64, f64) = (-95.0, 95.0);
/// Duo-Tone's Contrast (`TwoTone`).
pub const DUO_CONTRAST: (f64, f64) = (0.0, 100.0);
/// Vignette's Size.
pub const VIGNETTE_SIZE: (f64, f64) = (0.0, 50.0);
/// Vignette's Strength.
pub const VIGNETTE_STRENGTH: (f64, f64) = (1.0, 2.0);
/// Border's outer and inner thickness.
pub const BORDER_THICKNESS: (f64, f64) = (0.0, 100.0);
/// Fade, on the effects that have it.
pub const FADE: (f64, f64) = (0.0, 100.0);

/// What the legacy sidecar holds about one photo's edits, as the legacy reader gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Legacy {
    /// The edit list's tokens, oldest first, each `name=1[,param]*` without its `;` ([`tokens`]
    /// splits a raw value).
    pub filters: Vec<String>,
    /// The redo list's tokens, in the order stored.
    pub redo: Vec<String>,
    /// The library rotation, clockwise quarter turns (taken modulo 4).
    pub quarters: u8,
    /// Whether the photo is mirrored left–right.
    pub flipped: bool,
    /// The photo's effective crop, on the stored pixels, used when the edit list carries none.
    pub crop: Option<Rect>,
    /// The text overlay's value, kept whole.
    pub text: Option<String>,
    /// The text overlay's on/off value, kept whole.
    pub text_active: Option<String>,
    /// The file's EXIF orientation: legacy rectangles are on the stored pixels before it.
    pub orientation: Orientation,
}

/// The recipe entries a [`Legacy`] gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Converted {
    /// The photo's recipe, in order.
    pub edits: Vec<Op>,
    /// The redo list, in the order stored.
    pub redo: Vec<Op>,
}

impl Converted {
    /// The recipe as the sidecar's `"edits"` strings.
    pub fn edits_text(&self) -> Vec<String> {
        self.edits.iter().map(Op::to_string).collect()
    }

    /// The redo list as the sidecar's `"redo"` strings.
    pub fn redo_text(&self) -> Vec<String> {
        self.redo.iter().map(Op::to_string).collect()
    }
}

/// A raw edit-list value split into its tokens: on `;`, empty pieces left out.
pub fn tokens(list: &str) -> Vec<&str> {
    list.split(';').filter(|t| !t.is_empty()).collect()
}

/// The recipe and redo list `legacy` gives; `Err(TooManyOperations)` when either would hold more
/// than [`MAX_OPERATIONS`] entries.
pub fn convert(legacy: &Legacy) -> Result<Converted> {
    let exif = legacy.orientation;
    if legacy.filters.len() > MAX_OPERATIONS || legacy.redo.len() > MAX_OPERATIONS {
        return Err(Error::TooManyOperations);
    }
    let mapped: Vec<Op> = legacy.filters.iter().map(|t| map(t, exif)).collect();
    let last_crop = mapped.iter().rposition(is_crop);
    let mut edits = Vec::with_capacity(mapped.len().saturating_add(5));
    if legacy.flipped {
        edits.push(Op::FlipH);
    }
    if let Some(turn) = Turn::from_quarters(legacy.quarters) {
        edits.push(Op::Rotate(turn));
    }
    if last_crop.is_none()
        && let Some(rect) = legacy.crop
    {
        edits.push(crop(rect, exif).map_or_else(|| kept_rect(rect), Op::from));
    }
    for (i, op) in mapped.into_iter().enumerate() {
        if is_crop(&op) && Some(i) != last_crop {
            continue;
        }
        edits.push(op);
    }
    if let Some(text) = &legacy.text {
        edits.push(Op::Legacy(format!("{LEGACY_PREFIX}text({text})")));
    }
    if let Some(active) = &legacy.text_active {
        edits.push(Op::Legacy(format!("{LEGACY_PREFIX}textactive({active})")));
    }
    let redo: Vec<Op> = legacy.redo.iter().map(|t| map(t, exif)).collect();
    if edits.len() > MAX_OPERATIONS || redo.len() > MAX_OPERATIONS {
        return Err(Error::TooManyOperations);
    }
    Ok(Converted { edits, redo })
}

/// One legacy token as a recipe entry: its Dalica operation, or the token kept verbatim as a
/// `legacy:` entry. `exif` is the file's orientation, for crop rectangles.
pub fn map(token: &str, exif: Orientation) -> Op {
    typed(token, exif).map_or_else(|| kept(token), Op::from)
}

/// `token` kept verbatim: `legacy:<name>(<text after the first '='>)`, or `legacy:<token>` when it
/// has no `=`.
pub fn kept(token: &str) -> Op {
    Op::Legacy(match token.split_once('=') {
        Some((name, rest)) => format!("{LEGACY_PREFIX}{name}({rest})"),
        None => format!("{LEGACY_PREFIX}{token}"),
    })
}

/// A `rect64` value (bare hex, or wrapped in `rect64(…)`): up to 16 hex digits, left-padded, read as
/// left, top, right and bottom over [`RECT64_ONE`]. `None` for anything else.
pub fn rect64(text: &str) -> Option<Rect> {
    let hex = text.strip_prefix("rect64(").and_then(|t| t.strip_suffix(')')).unwrap_or(text);
    if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u64::from_str_radix(hex, 16).ok()?;
    let field = |shift: u32| f64::from(((v >> shift) & 0xffff) as u16) / RECT64_ONE;
    Some(Rect::new(field(48), field(32), field(16), field(0)))
}

/// `rect` on the stored pixels, in Dalica's stored frame (after the EXIF orientation).
pub fn to_stored_frame(rect: Rect, exif: Orientation) -> Rect {
    rect.transform(Dihedral::from_exif(exif))
}

/// The crop of a legacy rectangle, or `None` when it is empty or reversed.
fn crop(rect: Rect, exif: Orientation) -> Option<Typed> {
    let ordered = rect.x0 < rect.x1 && rect.y0 < rect.y1;
    ordered.then(|| Crop::new(to_stored_frame(rect, exif))).flatten().map(Typed::Crop)
}

/// A crop rectangle Dalica cannot use, kept as its numbers.
fn kept_rect(r: Rect) -> Op {
    let n = ops::num;
    Op::Legacy(format!("{LEGACY_PREFIX}crop({},{},{},{})", n(r.x0), n(r.y0), n(r.x1), n(r.y1)))
}

fn is_crop(op: &Op) -> bool {
    matches!(op, Op::Listed { name: "crop", .. })
}

/// `v` on the legacy range `from` placed on the Dalica range `to` (the ends onto the ends), then
/// clamped into `to`.
fn scale(v: f64, from: (f64, f64), to: (f64, f64)) -> f64 {
    let span = from.1 - from.0;
    let t = if span == 0.0 { 0.0 } else { (v - from.0) / span };
    ops::clamp(to.0 + t * (to.1 - to.0), to)
}

/// A finite decimal number.
fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// A legacy colour: hex `AARRGGBB`, leading zeros optional, only the first eight digits read; its
/// red, green and blue.
fn colour(text: &str) -> Option<[u8; 3]> {
    let hex = text.get(..text.len().min(8))?;
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    let [_, r, g, b] = v.to_be_bytes();
    Some([r, g, b])
}

/// The Dalica operation of a token, when there is a confident equivalent and the token is well
/// formed.
fn typed(token: &str, exif: Orientation) -> Option<Typed> {
    let (name, rest) = token.split_once('=')?;
    let mut fields: Vec<&str> = rest.split(',').collect();
    // A trailing empty field is tolerated, as the legacy reader does.
    if fields.len() > 1 && fields.last() == Some(&"") {
        fields.pop();
    }
    let (&flag, p) = fields.split_first()?;
    if flag != "1" {
        return None;
    }
    let num = |i: usize, from: (f64, f64), to: (f64, f64)| p.get(i).and_then(|t| number(t)).map(|v| scale(v, from, to));
    let col = |i: usize| p.get(i).and_then(|t| colour(t));
    let count = |n: usize| p.len() == n;
    let t = match name {
        "crop64" if count(1) => return crop(rect64(p.first()?)?, exif),
        "enhance" if count(0) => Typed::AutoFix(auto::AutoFix),
        "autolight" if count(0) => Typed::AutoContrast(auto::AutoContrast),
        "autocolor" if count(0) => Typed::AutoColor(auto::AutoColor),
        "fill" if count(1) => Typed::Tuning(tone::Tuning { fill: num(0, FILL, tone::FILL)?, ..tone::Tuning::default() }),
        "finetune2" if count(5) => {
            let neutral = col(3)?;
            Typed::Tuning(tone::Tuning {
                fill: num(0, FILL, tone::FILL)?,
                highlights: num(1, HIGHLIGHTS, tone::HIGHLIGHTS)?,
                shadows: num(2, SHADOWS, tone::SHADOWS)?,
                temp: num(4, TEMP, tone::TEMP)?,
                // Black, the picker's empty swatch, picks nothing.
                neutral: (neutral != [0, 0, 0]).then_some(neutral),
            })
        }
        "unsharp2" if count(1) => Typed::Sharpen(detail::Sharpen { amount: num(0, SHARPEN, detail::SHARPEN)? }),
        "sepia" if count(0) => Typed::Sepia(colour::Sepia),
        "bw" if count(0) => Typed::Bw(colour::Bw),
        "warm" if count(0) => Typed::Warmify(colour::Warmify),
        // The legacy Film Grain takes no parameters: it is the effect at its defaults.
        "grain2" if count(0) => Typed::Grain(detail::Grain::default()),
        "sat" if count(1) => Typed::Saturation(colour::Saturation { amount: num(0, SATURATION, colour::SATURATION)? }),
        "glow2" if count(2) => Typed::Glow(detail::Glow {
            intensity: num(0, GLOW_INTENSITY, detail::GLOW_INTENSITY)?,
            radius: num(1, GLOW_RADIUS, detail::GLOW_RADIUS)?,
        }),
        "ansel" if count(1) => Typed::FilteredBw(colour::FilteredBw { color: col(0)? }),
        "Boost" if count(1) => Typed::Boost(remap::Boost { strength: num(0, BOOST, remap::STRENGTH)? }),
        "HDR" if count(3) => Typed::Hdr(looks::Hdr {
            radius: num(0, HDR_RADIUS, looks::HDR_RADIUS)?,
            strength: num(1, HDR_STRENGTH, looks::HDR_STRENGTH)?,
            fade: num(2, FADE, ops::FADE)?,
        }),
        "Holga" if count(3) => Typed::PlasticLens(looks::PlasticLens {
            blur: num(0, BLUR_EDGES, looks::BLUR_EDGES)?,
            grain: num(1, LENS_GRAIN, looks::GRAIN)?,
            fade: num(2, FADE, ops::FADE)?,
        }),
        // Stored as brightness, contrast, fade, then the two colours.
        "TwoTone" if count(5) => Typed::Duotone(remap::Duotone {
            brightness: num(0, DUO_BRIGHTNESS, remap::BRIGHTNESS)?,
            contrast: num(1, DUO_CONTRAST, remap::CONTRAST)?,
            fade: num(2, FADE, ops::FADE)?,
            first: col(3)?,
            second: col(4)?,
        }),
        // Stored as size, strength, fade, then the colour.
        "Vignette" if count(4) => Typed::Vignette(stylize::Vignette {
            size: num(0, VIGNETTE_SIZE, stylize::VIGNETTE_SIZE)?,
            strength: num(1, VIGNETTE_STRENGTH, stylize::VIGNETTE_STRENGTH)?,
            fade: num(2, FADE, ops::FADE)?,
            color: col(3)?,
        }),
        // Stored as outer and inner thickness, corner radius, outer and inner colour, caption
        // height. The radius and the caption are in units the descriptions do not record, so only
        // a border with neither is mapped.
        "Border" if count(6) => {
            let (radius, caption) = (number(p.get(2)?)?, number(p.get(5)?)?);
            if radius != 0.0 || caption != 0.0 {
                return None;
            }
            Typed::Border(canvas::Border {
                outer: num(0, BORDER_THICKNESS, canvas::THICKNESS)?,
                inner: num(1, BORDER_THICKNESS, canvas::THICKNESS)?,
                outer_color: col(3)?,
                inner_color: col(4)?,
                radius: 0.0,
                caption: 0.0,
            })
        }
        _ => return None,
    };
    Some(t.clamped())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_the_first_eight_digits_with_leading_zeros_optional() {
        assert_eq!(colour("00ffff00"), Some([0xff, 0xff, 0x00]));
        assert_eq!(colour("ffff"), Some([0x00, 0xff, 0xff]));
        assert_eq!(colour("ff5bfff3"), Some([0x5b, 0xff, 0xf3]));
        assert_eq!(colour("ff5bfff3ab"), Some([0x5b, 0xff, 0xf3]));
        for bad in ["", "#ffffff", "xyz", "é"] {
            assert_eq!(colour(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn scale_puts_the_ends_on_the_ends() {
        assert_eq!(scale(0.48, HIGHLIGHTS, tone::HIGHLIGHTS), 0.5);
        assert_eq!(scale(0.0, HIGHLIGHTS, tone::HIGHLIGHTS), 0.0);
        assert_eq!(scale(5.0, HIGHLIGHTS, tone::HIGHLIGHTS), 0.5);
        assert_eq!(scale(1.0, (1.0, 1.0), (0.0, 2.0)), 0.0);
    }
}
