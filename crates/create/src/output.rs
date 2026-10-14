//! The one output pipeline (`architecture.md` §6, `data-model.md` §6): every new file Dalica
//! writes from a photo's pixels — Save, Save a Copy, Save As and Export — is made by [`render`].
//!
//! Steps, in order:
//!
//! 1. **Read** the source at full size under the decode limits (`formats.md` §5): the header and
//!    its declared size first, then the stored pixels inside the decoder panic guard.
//! 2. **Choose the format** ([`target`]): in place, beside, copy or export, from the source's
//!    format and whether it has transparency; or the format the caller names (Save As).
//! 3. **Render** the recipe at full size with the EXIF orientation applied (`edit::render`, the
//!    CPU oracle), in 8-bit sRGB through the colour stage; then **resize** to the long side asked
//!    for, never enlarging (`image::resize::fit`); then the **watermark** hook
//!    ([`crate::watermark`]). A 16-bit grey or RGB source without a profile written as PNG, whose
//!    recipe only rotates and flips and which is not resized or watermarked, skips the 8-bit
//!    stage: its samples are moved pixel for pixel at their own precision.
//! 4. **Encode** (`image::encode`): JPEG at the source's estimated quality and chroma subsampling
//!    ([`Quality::Match`], 90 and 4:4:4 when there is no estimate) or a fixed quality, transparent
//!    pixels composited over white; PNG keeping transparency and 16 bits. Grey sources stay grey while
//!    the edits leave every pixel neutral. Output is sRGB; an sRGB profile is embedded only when
//!    the source had a profile.
//! 5. **Metadata** (`meta::carry`): the source's EXIF, XMP and IPTC with orientation 1, the new
//!    pixel size and a fresh EXIF thumbnail ([`Spec::thumbnail`]) or none; GPS removed when
//!    asked; then the owned fields the caller passes, under its write options. A source format
//!    whose metadata Dalica does not read (GIF, BMP, HEIF, JPEG XL, PSD and the non-TIFF raws)
//!    gives a file holding only those facts and the owned fields ([`Report::source_metadata`]).
//!
//! Pure: bytes in, bytes out, and a [`Report`] of what was written. Nothing touches the disk.

mod pixels;
mod target;

pub use target::{OutFormat, Purpose, target};

use dalica_edit::{Op, Recipe, Source, Target};
use dalica_image::encode::{self, Channels, EncodeError, Image, Metadata, Pixels, Subsampling};
use dalica_image::{DecodeOptions, Format, Limits, Rgba8, color, jpeg_quality, resize};
use dalica_meta::carry::{Carry, carry};
use dalica_meta::{Changes, Container, WriteOptions};

use self::pixels::Wide;

/// The JPEG quality used when the source's cannot be estimated (`data-model.md` §6).
pub const DEFAULT_QUALITY: u8 = 90;
/// The longer side of the EXIF thumbnail made for a saved file.
pub const THUMBNAIL_SIDE: u32 = 160;
/// The JPEG quality of that thumbnail.
pub const THUMBNAIL_QUALITY: u8 = 75;
/// Most threads the JPEG encoder uses by default.
const MAX_DEFAULT_THREADS: usize = 8;

/// Why a new file could not be made. Every failure is an `Err`; nothing here panics.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The request itself is wrong (a long side of 0, a quality outside 1–100).
    #[error("{0}")]
    Spec(String),
    /// The source's format cannot be written in place; its Save goes beside it.
    #[error("{} files cannot be saved in place", .0.name())]
    NotInPlace(Format),
    #[error("cannot read the photo: {0}")]
    Decode(#[from] dalica_image::Error),
    #[error("cannot render the edits: {0}")]
    Render(#[from] dalica_edit::Error),
    #[error("cannot write the new file: {0}")]
    Encode(#[from] EncodeError),
    #[error("cannot carry the metadata: {0}")]
    Metadata(#[from] dalica_meta::Error),
    #[error("not enough memory for the new file")]
    OutOfMemory,
    /// The sRGB profile could not be written (never expected; reported rather than left out).
    #[error("cannot write the sRGB profile: {0}")]
    Profile(String),
}

impl Error {
    /// The text shown to people.
    pub fn reason(&self) -> String {
        match self {
            Error::Decode(e) => e.reason(),
            other => other.to_string(),
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// The JPEG quality to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// The source's estimated quality and chroma subsampling; [`DEFAULT_QUALITY`] and 4:4:4 when
    /// the source is not a JPEG or its tables give no estimate.
    Match,
    /// A fixed quality, 1–100, with the source's chroma subsampling when it is a JPEG (4:4:4
    /// otherwise).
    Fixed(u8),
}

/// Which format the new file takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The format [`target`] gives for this purpose.
    For(Purpose),
    /// A format the person chose (Save As).
    Exactly(OutFormat),
}

/// What to make.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub format: Choice,
    pub quality: Quality,
    /// Fit the longer side into this many pixels; never enlarges. `None` keeps the rendered size.
    pub long_side: Option<u32>,
    /// The export watermark's text; `None` or blank draws none.
    pub watermark: Option<String>,
    /// Put a fresh EXIF thumbnail of the new pixels into the file (Save); otherwise none (Export).
    pub thumbnail: bool,
    /// Remove GPS coordinates (the EXIF GPS IFD and `exif:GPS*` in XMP); place names stay.
    pub remove_location: bool,
    /// The library's owned fields to merge into the new file.
    pub changes: Changes,
    /// The options those fields are written under (all on for copies and exports; the
    /// write-back options for Save in place) and the metadata limits.
    pub write: WriteOptions,
    /// The decode limits the source is read under.
    pub limits: Limits,
    /// Threads the JPEG encoder may use (the bytes are the same for any count).
    pub threads: usize,
}

impl Spec {
    /// A full-size file for `purpose` at the matched quality, without a thumbnail, watermark or
    /// owned fields.
    pub fn new(purpose: Purpose) -> Spec {
        Spec {
            format: Choice::For(purpose),
            quality: Quality::Match,
            long_side: None,
            watermark: None,
            thumbnail: false,
            remove_location: false,
            changes: Changes::default(),
            write: WriteOptions::default(),
            limits: Limits::default(),
            threads: std::thread::available_parallelism().map_or(1, |n| n.get().min(MAX_DEFAULT_THREADS)),
        }
    }

    fn check(&self) -> Result<()> {
        if self.long_side == Some(0) {
            return Err(Error::Spec("the long side must be at least 1 pixel".into()));
        }
        if let Quality::Fixed(q) = self.quality
            && !(1..=100).contains(&q)
        {
            return Err(Error::Spec(format!("JPEG quality {q} is outside 1–100")));
        }
        Ok(())
    }
}

/// What was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub format: OutFormat,
    pub width: u32,
    pub height: u32,
    /// Bits per sample: 8, or 16 for a PNG from a 16-bit or floating-point source.
    pub bits: u8,
    pub channels: Channels,
    /// The JPEG quality written (`None` for PNG).
    pub quality: Option<u8>,
    /// The JPEG chroma subsampling written (`None` for PNG and grey JPEGs).
    pub subsampling: Option<Subsampling>,
    /// The quality estimated from the source's tables, when it is a JPEG that gives one.
    pub source_quality: Option<u8>,
    /// Whether an sRGB profile was embedded (the source had a profile).
    pub icc: bool,
    /// Whether the EXIF thumbnail went in.
    pub thumbnail: bool,
    /// Whether the source's own metadata was read and carried (see the module docs).
    pub source_metadata: bool,
    /// What the target had no place or room for ("extended XMP").
    pub dropped: Vec<&'static str>,
}

/// A new file's bytes and its report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub bytes: Vec<u8>,
    pub report: Report,
}

/// The pixels on their way to the encoder.
enum Rendered {
    Narrow(Rgba8),
    Wide(Wide),
}

/// Renders `recipe` on the photo whose file is `source` and encodes the new file `spec` asks
/// for; see the module docs for the steps.
pub fn render(source: &[u8], recipe: &Recipe, spec: &Spec) -> Result<Output> {
    spec.check()?;
    let pixels::Read { header, decoded, mut budget } = pixels::read(source, &spec.limits)?;
    let transparent = pixels::transparency(&decoded);
    let format = match spec.format {
        Choice::For(purpose) => target(header.format, transparent, purpose).ok_or(Error::NotInPlace(header.format))?,
        Choice::Exactly(format) => format,
    };
    let tagged = decoded.icc.is_some();
    let grey_source = decoded.model == dalica_image::ColorModel::Gray;
    // Alpha is kept only when it says something: the PNG decoder adds an opaque one to every file.
    let keep_alpha = format == OutFormat::Png && transparent;
    let deep = format == OutFormat::Png && !matches!(decoded.samples, dalica_image::Samples::U8(_));

    let transform = recipe.display(header.orientation);
    let (shown_w, shown_h) = transform.display_size(decoded.width, decoded.height);
    let resizes = spec.long_side.is_some_and(|side| side < shown_w.max(shown_h));
    let watermark = spec.watermark.as_deref().filter(|t| !t.trim().is_empty());
    let moves_only = recipe.pipeline().all(|op| matches!(op, Op::Rotate(_) | Op::FlipH | Op::FlipV));

    let exact = if deep && moves_only && !resizes && watermark.is_none() && !tagged {
        pixels::wide(&decoded, transform, keep_alpha, &mut budget)?
    } else {
        None
    };
    let rendered = match exact {
        Some(wide) => Rendered::Wide(wide),
        None => {
            let srgb = color::to_srgb8(decoded, &DecodeOptions::default(), &mut budget, &spec.limits)?.image;
            let full = dalica_edit::render(Source::new(&srgb, header.orientation), recipe, Target::Full)?;
            drop(srgb);
            // Never enlarged, also when a crop made the picture smaller than the long side (and no
            // copy of a picture that already fits).
            let mut img = match spec.long_side {
                Some(side) if full.width.max(full.height) > side => resize::fit(&full, side)?,
                _ => full,
            };
            if let Some(text) = watermark {
                crate::watermark::draw(&mut img, text);
            }
            Rendered::Narrow(img)
        }
    };

    let thumbnail = match (spec.thumbnail, &rendered) {
        (false, _) => None,
        (true, Rendered::Narrow(img)) => Some(thumbnail(img)?),
        (true, Rendered::Wide(wide)) => Some(thumbnail(&wide.rgba8()?)?),
    };
    let profile = if tagged { Some(srgb_profile()?) } else { None };
    let metadata = Metadata { exif: None, xmp: None, icc: profile.as_deref() };
    let settings = jpeg_quality::read(source);
    let source_quality = settings.map(|s| s.quality);

    let (encoded, width, height, bits, channels, quality, subsampling) = match rendered {
        Rendered::Wide(wide) => {
            let image = Image { width: wide.width, height: wide.height, channels: wide.channels, pixels: Pixels::U16(&wide.samples) };
            (encode::png(&image, &metadata)?, wide.width, wide.height, 16, wide.channels, None, None)
        }
        Rendered::Narrow(mut img) => {
            let grey = grey_source && pixels::is_grey(&img);
            match format {
                OutFormat::Jpeg => {
                    pixels::flatten(&mut img);
                    let quality = match spec.quality {
                        Quality::Match => source_quality.unwrap_or(DEFAULT_QUALITY),
                        Quality::Fixed(q) => q,
                    };
                    let sub = settings.map_or(Subsampling::S444, |s| s.sampling.for_jpeg());
                    let channels = pixels::channels(grey, false);
                    let bytes = if grey {
                        let samples = pixels::pack8(&img, channels)?;
                        let image = Image { width: img.width, height: img.height, channels, pixels: Pixels::U8(&samples) };
                        encode::jpeg_with_threads(&image, quality, sub, &metadata, spec.threads)?
                    } else {
                        encode::jpeg_with_threads(&Image::rgba8(&img), quality, sub, &metadata, spec.threads)?
                    };
                    (bytes, img.width, img.height, 8, channels, Some(quality), (!grey).then_some(sub))
                }
                OutFormat::Png => {
                    let channels = pixels::channels(grey, keep_alpha);
                    let bytes = if deep {
                        let samples = pixels::pack16(&img, channels)?;
                        encode::png(&Image { width: img.width, height: img.height, channels, pixels: Pixels::U16(&samples) }, &metadata)?
                    } else if channels == Channels::Rgba {
                        encode::png(&Image::rgba8(&img), &metadata)?
                    } else {
                        let samples = pixels::pack8(&img, channels)?;
                        encode::png(&Image { width: img.width, height: img.height, channels, pixels: Pixels::U8(&samples) }, &metadata)?
                    };
                    (bytes, img.width, img.height, if deep { 16 } else { 8 }, channels, None, None)
                }
            }
        }
    };

    // A source whose container Dalica's metadata reader does not walk carries nothing of its own:
    // the new file is its own source, so it still gets orientation 1, its size and the owned fields.
    let source_metadata = Container::detect(source).is_some();
    let from = if source_metadata { source } else { &encoded };
    let carried = carry(
        from,
        &encoded,
        &Carry {
            width,
            height,
            thumbnail: thumbnail.as_deref(),
            remove_location: spec.remove_location,
            changes: spec.changes.clone(),
            options: spec.write.clone(),
        },
    )?;
    let report = Report {
        format,
        width,
        height,
        bits,
        channels,
        quality,
        subsampling,
        source_quality,
        icc: tagged,
        thumbnail: carried.thumbnail,
        source_metadata,
        dropped: carried.dropped,
    };
    Ok(Output { bytes: carried.bytes, report })
}

/// The EXIF thumbnail of a new file: [`THUMBNAIL_SIDE`] on its longer side, over white, as a
/// JPEG without metadata.
fn thumbnail(img: &Rgba8) -> Result<Vec<u8>> {
    let mut small = resize::fit(img, THUMBNAIL_SIDE)?;
    pixels::flatten(&mut small);
    Ok(encode::jpeg(&Image::rgba8(&small), THUMBNAIL_QUALITY, Subsampling::S420, &Metadata::default())?)
}

/// The sRGB profile embedded in an output whose source had a profile.
fn srgb_profile() -> Result<Vec<u8>> {
    moxcms::ColorProfile::new_srgb().encode().map_err(|e| Error::Profile(e.to_string()))
}
