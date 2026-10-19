//! Rendering a collage (M14.10, `creating.md` §3): the page drawn at its pixel size from the
//! document and its placements, and encoded as a JPEG.
//!
//! [`draw`] makes the page, bottom to top:
//!
//! 1. **The background**: the solid colour, or the background picture covering the page (centred,
//!    cropped to the page's shape). Multiple Exposure has none.
//! 2. **Each placement in stacking order**: the picture is asked of the caller (rendered through
//!    the one output pipeline with its edits) at the size its place needs, one at a time, so memory
//!    stays within the page and one picture; the part [`Placement::shown`] names is stretched over
//!    the picture's box, turned by its angle with bilinear sampling and an anti-aliased edge. Before
//!    it go its **shadow** (Draw shadows; every style but Multiple Exposure): the closed form of
//!    `edit`'s Drop Shadow at its default distance, blur and fade, falling down and to the right;
//!    around it its **border** (Pile and Contact Sheet; white 3 % of the picture's short side all
//!    round, instant 5 % with a 20 % bottom); after it its **caption** (Show captions, the pile: in
//!    the instant border's bottom in dark text, otherwise across the bottom of the picture in the
//!    watermark's white) or its **file name** (Contact Sheet: a line under the picture, dark on a
//!    light page and light on a dark one), drawn with [`crate::watermark`]'s Inter text.
//!    Multiple Exposure blends every picture with equal weights instead.
//! 3. `cancelled` is asked before each picture and `progress(done, total)` told after each.
//!
//! [`encode`] writes the page as a JPEG at [`QUALITY`] (4:4:4, sRGB with its profile) with the date
//! of making as its date taken; [`render`] does both. Pure: no I/O, no clock; call it on a worker.

mod compose;
mod label;
mod paint;

use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata, Subsampling};
use dalica_meta::carry::{Carry, carry};
use dalica_meta::{Changes, DateTaken, WriteOptions};

use self::compose::place;
use self::label::{Ink, Label};
use self::paint::{Blend, Frame, Local, Sampler};
use super::doc::{Border, Collage, Style};
use super::layout::Placement;

/// The JPEG quality a collage is written at.
pub const QUALITY: u8 = 92;
/// The largest page drawn, in pixels (16 384 × 8 192; 512 MiB of RGBA).
pub const MAX_PAGE_PIXELS: u64 = 1 << 27;
/// Shadow: its distance and blur in thousandths of the picture's short side, its fade in percent
/// (`edit`'s Drop Shadow defaults) and the direction it falls, degrees clockwise from up.
pub const SHADOW: (f64, f64, f64, f64) = (15.0, 25.0, 30.0, 135.0);
/// A pile's caption on the picture: its line height as a fraction of the picture's short side.
pub const CAPTION_ON_PICTURE: f64 = 0.06;
/// A pile's caption in an instant border: its line height as a fraction of that border's bottom.
pub const CAPTION_IN_BORDER: f64 = 0.35;
/// A contact sheet's file name: its line height as a fraction of the picture's long side, drawn
/// that far again below the picture (the line under each picture the layout leaves is
/// [`NAME_LINE`] of the long side).
pub const NAME_SIZE: f64 = 0.08;
/// The room under a contact sheet's picture its file name takes, as a fraction of the picture's
/// long side.
pub const NAME_LINE: f64 = 2.0 * NAME_SIZE;
/// The colours of the borders.
pub const WHITE_BORDER: [u8; 3] = [255, 255, 255];
pub const INSTANT_BORDER: [u8; 3] = [250, 250, 246];
/// Dark text (on an instant border, or a contact sheet's light page).
pub const DARK_TEXT: [u8; 3] = [34, 34, 34];
/// Most threads the JPEG encoder uses.
const MAX_THREADS: usize = 8;

/// Why a collage was not rendered.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    #[error("cancelled")]
    Cancelled,
    /// The page's size is 0 or past [`MAX_PAGE_PIXELS`].
    #[error("{0}")]
    Size(String),
    #[error("not enough memory for the collage")]
    OutOfMemory,
    #[error("cannot write the collage: {0}")]
    Encode(String),
}

/// A picture the caller rendered for one placement: its pixels (8-bit sRGB, straight alpha, edits
/// applied), its caption and its file name.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub image: Rgba8,
    pub caption: String,
    pub name: String,
}

/// Answers the whole picture of one placement rendered at about `(width, height)` pixels (any size
/// works; that one shows it at its best without wasting memory), or `None` when it cannot be read:
/// its place is left empty. The background picture is asked with a placement covering the page.
pub type PictureFn<'a> = dyn FnMut(&Placement, (u32, u32)) -> Option<Picture> + 'a;

/// The page's JPEG at `size` pixels: [`draw`], then [`encode`] with `made` as its date.
pub fn render(
    collage: &Collage,
    placements: &[Placement],
    picture: &mut PictureFn,
    size: (u32, u32),
    made: Option<&DateTaken>,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Vec<u8>, RenderError> {
    let page = draw(collage, placements, picture, size, cancelled, progress)?;
    if cancelled() {
        return Err(RenderError::Cancelled);
    }
    encode(&page, made)
}

/// The page at `size` pixels (see the module docs). `placements` are drawn in their stacking order
/// (`z`, bottom first); the background picture, when there is one, counts as one more picture.
pub fn draw(
    collage: &Collage,
    placements: &[Placement],
    picture: &mut PictureFn,
    size: (u32, u32),
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Rgba8, RenderError> {
    let (width, height) = size;
    let pixels = u64::from(width) * u64::from(height);
    if width == 0 || height == 0 || pixels > MAX_PAGE_PIXELS {
        return Err(RenderError::Size(format!("a collage is 1 to {MAX_PAGE_PIXELS} pixels, not {width} × {height}")));
    }
    let style = collage.style;
    let exposure = style == Style::MultipleExposure;
    let mut page = paint::page(width, height, (!exposure).then_some(collage.background.colour)).ok_or(RenderError::OutOfMemory)?;

    let mut order: Vec<&Placement> = placements.iter().collect();
    order.sort_by_key(|p| p.z);
    let background = if exposure { None } else { collage.background.picture.and_then(|id| collage.clip_index(id).map(|clip| (clip, id))) };
    let total = order.len() + usize::from(background.is_some());
    let mut done = 0;

    if let Some((clip, id)) = background {
        if cancelled() {
            return Err(RenderError::Cancelled);
        }
        let whole = Placement { clip, id, centre: [0.5, 0.5], size: [1.0, 1.0], angle: 0.0, z: 0, shown: dalica_core::Rect01::FULL };
        if let Some(pic) = picture(&whole, (width, height)).filter(|p| valid(&p.image)) {
            cover(&mut page, &pic.image);
        }
        done += 1;
        progress(done, total);
    }

    let ink = name_ink(collage.background.colour);
    for p in order {
        if cancelled() {
            return Err(RenderError::Cancelled);
        }
        if let Some(card) = Card::of(collage, p, width, height) {
            let asked = card.request(pixels);
            if let Some(pic) = picture(p, asked).filter(|p| valid(&p.image)) {
                card.draw(&mut page, &pic, collage, exposure, ink);
            }
        }
        done += 1;
        progress(done, total);
    }
    if exposure {
        paint::settle(&mut page);
    }
    Ok(page)
}

/// The page as a JPEG at [`QUALITY`], 4:4:4, with the sRGB profile and `made` as its date taken
/// (EXIF and XMP).
pub fn encode(page: &Rgba8, made: Option<&DateTaken>) -> Result<Vec<u8>, RenderError> {
    let profile = moxcms::ColorProfile::new_srgb().encode().map_err(|e| RenderError::Encode(e.to_string()))?;
    let metadata = Metadata { exif: None, xmp: None, icc: Some(&profile) };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_THREADS));
    let bytes = encode::jpeg_with_threads(&Image::rgba8(page), QUALITY, Subsampling::S444, &metadata, threads)
        .map_err(|e| RenderError::Encode(e.to_string()))?;
    let Some(made) = made else { return Ok(bytes) };
    let changes = Changes { date_taken: Some(made.clone()), ..Changes::default() };
    let options = WriteOptions { metadata_date: Some(made.clone()), ..WriteOptions::default() };
    let carried = carry(&bytes, &bytes, &Carry { width: page.width, height: page.height, thumbnail: None, remove_location: false, changes, options })
        .map_err(|e| RenderError::Encode(e.to_string()))?;
    Ok(carried.bytes)
}

/// Whether `img` holds the pixels its size says.
fn valid(img: &Rgba8) -> bool {
    img.width > 0 && img.height > 0 && u64::from(img.width) * u64::from(img.height) * 4 == img.data.len() as u64
}

/// The background picture over the whole page: scaled to cover it, centred, the rest cropped.
fn cover(page: &mut Rgba8, img: &Rgba8) {
    let (w, h) = (page.width, page.height);
    let Some((sw, sh)) = place(f64::from(img.width), f64::from(img.height), w, h, true) else { return };
    let (fx, fy) = (f64::from(w) / f64::from(sw), f64::from(h) / f64::from(sh));
    let shown = [(1.0 - fx) / 2.0, (1.0 - fy) / 2.0, (1.0 + fx) / 2.0, (1.0 + fy) / 2.0];
    let (hw, hh) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
    let rect = Local { u0: -hw, v0: -hh, u1: hw, v1: hh };
    let Some(sampler) = Sampler::new(img, shown, rect) else { return };
    paint::paint(page, Frame::new(hw, hh, 0.0), rect, false, Blend::Over, |u, v| sampler.at(u, v));
}

/// How a contact sheet's names show on a page of `colour`: dark on a light page, light on a dark
/// one.
fn name_ink([r, g, b]: [u8; 3]) -> Ink {
    let luma = 0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b);
    if luma > 140.0 { Ink::Dark(DARK_TEXT) } else { Ink::Light }
}

/// One placement on the page, in pixels.
struct Card {
    frame: Frame,
    /// The picture's box, about the frame's centre.
    picture: Local,
    /// The box with its border.
    card: Local,
    border: Option<[u8; 3]>,
    /// The bottom border's height (instant), 0 otherwise.
    bottom: f64,
    shown: [f64; 4],
}

impl Card {
    /// `None` for a placement with no size on the page (or numbers that are not finite).
    fn of(collage: &Collage, p: &Placement, width: u32, height: u32) -> Option<Card> {
        let (w, h) = (f64::from(width), f64::from(height));
        let (bw, bh) = (p.size[0] * w, p.size[1] * h);
        let (cx, cy) = (p.centre[0] * w, p.centre[1] * h);
        if !(bw.is_finite() && bh.is_finite() && cx.is_finite() && cy.is_finite()) || bw < 0.5 || bh < 0.5 {
            return None;
        }
        let picture = Local { u0: -bw / 2.0, v0: -bh / 2.0, u1: bw / 2.0, v1: bh / 2.0 };
        let border = if collage.style.has_borders() { collage.settings.border } else { Border::None };
        let short = bw.min(bh);
        let (side, bottom) = (border.sides() * short, border.bottom() * short);
        let card = Local { u0: picture.u0 - side, v0: picture.v0 - side, u1: picture.u1 + side, v1: picture.v1 + bottom };
        let colour = match border {
            Border::None => None,
            Border::White => Some(WHITE_BORDER),
            Border::Instant => Some(INSTANT_BORDER),
        };
        let s = &p.shown;
        Some(Card {
            frame: Frame::new(cx, cy, p.angle),
            picture,
            card,
            border: colour,
            bottom: if border == Border::Instant { bottom } else { 0.0 },
            shown: [s.x0(), s.y0(), s.x1(), s.y1()],
        })
    }

    /// The whole picture's size that shows its part at the box's size, at most `limit` pixels and
    /// the JPEG side limit.
    fn request(&self, limit: u64) -> (u32, u32) {
        let [l, t, r, b] = self.shown;
        let (w, h) = (self.picture.width() / (r - l).max(1e-6), self.picture.height() / (b - t).max(1e-6));
        let cap = (limit as f64 / (w * h).max(1.0)).sqrt().min(1.0);
        let side = f64::from(encode::JPEG_MAX_SIDE);
        let (w, h) = ((w * cap).ceil().clamp(1.0, side), (h * cap).ceil().clamp(1.0, side));
        (w as u32, h as u32)
    }

    fn draw(&self, page: &mut Rgba8, pic: &Picture, collage: &Collage, exposure: bool, ink: Ink) {
        let Some(sampler) = Sampler::new(&pic.image, self.shown, self.picture) else { return };
        if exposure {
            paint::paint(page, self.frame, self.picture, true, Blend::Mean, |u, v| sampler.at(u, v));
            return;
        }
        if collage.settings.shadows {
            let (distance, blur, fade, angle) = SHADOW;
            let short = self.card.width().min(self.card.height()) / 1000.0;
            let (s, c) = angle.to_radians().sin_cos();
            let offset = (s * distance * short, -c * distance * short);
            paint::shadow(page, self.frame, self.card, offset, blur * short, (1.0 - fade / 100.0) as f32);
        }
        let pr = self.picture;
        let border = self.border.map(|[r, g, b]| [f32::from(r), f32::from(g), f32::from(b), 255.0]);
        paint::paint(page, self.frame, self.card, true, Blend::Over, |u, v| {
            let Some(border) = border else { return sampler.at(u, v) };
            // The picture's own edge, anti-aliased against the border.
            let cu = ((u - pr.u0).min(pr.u1 - u) + 0.5).clamp(0.0, 1.0);
            let cv = ((v - pr.v0).min(pr.v1 - v) + 0.5).clamp(0.0, 1.0);
            let k = (cu * cv) as f32;
            if k <= 0.0 {
                return border;
            }
            let s = sampler.at(u, v);
            let rest = 1.0 - s[3] / 255.0 * k;
            [s[0] * k + border[0] * rest, s[1] * k + border[1] * rest, s[2] * k + border[2] * rest, s[3] * k + border[3] * rest]
        });
        if let Some(label) = self.label(collage, pic, ink) {
            label.paint(page, self.frame);
        }
    }

    /// The pile's caption or the contact sheet's file name, when it shows.
    fn label(&self, collage: &Collage, pic: &Picture, ink: Ink) -> Option<Label> {
        let pr = self.picture;
        match collage.style {
            Style::Pile if collage.settings.captions => {
                if self.bottom > 0.0 {
                    let centre = (0.0, pr.v1 + self.bottom / 2.0);
                    Label::new(&pic.caption, self.bottom * CAPTION_IN_BORDER, pr.width() * 0.9, centre, Ink::Dark(DARK_TEXT))
                } else {
                    let px = pr.width().min(pr.height()) * CAPTION_ON_PICTURE;
                    Label::new(&pic.caption, px, pr.width() * 0.9, (0.0, pr.v1 - px * 1.25), Ink::Light)
                }
            }
            Style::ContactSheet => {
                let px = pr.width().max(pr.height()) * NAME_SIZE;
                Label::new(&pic.name, px, self.card.width(), (0.0, self.card.v1 + px), ink)
            }
            _ => None,
        }
    }
}
