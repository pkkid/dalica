//! Videos: MP4, M4V, QuickTime and Matroska files recognised by their content and probed for
//! their facts (`formats.md` §1), with no frame decoded: until a decoder lands (M15.15), every
//! image path answers [`Error::Unsupported`] with [`REASON`].
//!
//! - [`recognise`]: the kind from the first bytes. ISO BMFF files by their `ftyp` brands: a major
//!   or compatible brand naming MP4, M4V or QuickTime, and no still-image brand (`heic`, `mif1`,
//!   `avif`, `crx `, …) nor an audio-only major brand (`M4A `); older QuickTime movies with no
//!   `ftyp` by their first box (`moov`, `mdat`, `wide`, `pnot`). Matroska by its EBML header's
//!   DocType `matroska`.
//! - [`probe`]: duration, frame size, rotation, creation date, codecs, frame rate, sound and
//!   location, read through `bmff.rs`'s bounded box walk (`mp4.rs`) or our own bounded EBML reader
//!   (`ebml.rs`, `mkv.rs`). [`probe_file`] reads only the boxes it needs from a file on disk.
//!
//! Every size and count is hostile (`security.md` §4.2): boxes nest at most 12 deep with at most
//! 4,096 per level; an EBML element lies inside its parent, and one of unknown size runs to its
//! parent's end and no further; sample tables are never read whole (only their headers); every
//! box or element read is counted by one [`Work`] of [`MAX_STEPS`]. Bytes that stop before the
//! facts are complete are [`Error::Truncated`], so a caller reading a prefix can read on.

mod ebml;
mod mkv;
mod mp4;

use std::io::{Read as _, Seek as _, SeekFrom};
use std::ops::Range;
use std::path::Path;

use dalica_core::DateTime;
use dalica_core::geo::LatLon;

use super::bmff;
use crate::decode::Decoded;
use crate::limits::{Budget, Limits, Work};
use crate::sniff::{Format, VideoKind};
use crate::{Error, Header, Result};

/// No build decodes video frames yet (M15.15).
pub const DECODES: bool = false;

/// The tile text of a video until its frames are decoded (`formats.md` §1).
pub const REASON: &str = "This video can't be played";

/// Most boxes or elements one probe reads.
pub const MAX_STEPS: usize = 1 << 16;

/// Longest string a probe keeps (codec ids, tag values, location text).
pub const MAX_TEXT: usize = 256;

/// What a video file says about itself, without decoding a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    pub kind: VideoKind,
    /// The length, to the millisecond (`None` when the container does not say).
    pub duration_ms: Option<u64>,
    /// The stored frame size, before [`VideoInfo::rotation`] (`None` without a video track).
    pub size: Option<(u32, u32)>,
    /// Quarter turns clockwise that show the frames upright (0–3).
    pub rotation: u8,
    /// When it was recorded, as the container records it (UTC by the formats' definitions).
    pub created: Option<DateTime>,
    /// The video track's codec: a sample entry code (`avc1`, `hvc1`, `jpeg`, …) or a Matroska
    /// `CodecID` (`V_MPEG4/ISO/AVC`, …).
    pub video_codec: Option<String>,
    /// The first sound track's codec (`mp4a`, `A_AAC`, …).
    pub sound_codec: Option<String>,
    /// Frames per second, to three decimals (an average over the track where the container
    /// gives no fixed rate).
    pub frame_rate: Option<f64>,
    /// Whether it has a sound track.
    pub has_sound: bool,
    /// Where it was recorded.
    pub location: Option<LatLon>,
}

impl VideoInfo {
    fn new(kind: VideoKind) -> VideoInfo {
        VideoInfo {
            kind,
            duration_ms: None,
            size: None,
            rotation: 0,
            created: None,
            video_codec: None,
            sound_codec: None,
            frame_rate: None,
            has_sound: false,
            location: None,
        }
    }

    /// The frame size as shown, after the rotation.
    pub fn display_size(&self) -> Option<(u32, u32)> {
        self.size.map(|(w, h)| if self.rotation % 2 == 1 { (h, w) } else { (w, h) })
    }
}

/// The video kind of a file starting with `head`, or `None`.
pub fn recognise(head: &[u8]) -> Option<VideoKind> {
    if head.get(0..4) == Some(&ebml::EBML_MAGIC[..]) {
        return mkv::recognise(head).then_some(VideoKind::Matroska);
    }
    if let Some(major) = bmff::major_brand(head) {
        return brand_kind(&major, bmff::compatible_brands(head));
    }
    // An older QuickTime movie without `ftyp`: its first box is the movie or its media data.
    let size = super::u32_be(head, 0).ok()?;
    let first = head.get(4..8)?;
    let sane = size == 0 || size == 1 || size >= 8;
    (sane && matches!(first, b"moov" | b"mdat" | b"wide" | b"pnot")).then_some(VideoKind::QuickTime)
}

/// Brands of still images (HEIF, AVIF, CR3, JPEG in HEIF): a file naming one is never a video.
const STILL_BRANDS: [&[u8; 4]; 17] = [
    b"heic", b"heix", b"heim", b"heis", b"hevc", b"hevx", b"mif1", b"mif2", b"mif3", b"msf1", b"miaf", b"avif", b"avis", b"avio", b"crx ", b"jpeg",
    b"jpgs",
];

/// Major brands of sound-only files, which name MP4 among their compatible brands.
const AUDIO_BRANDS: [&[u8; 4]; 5] = [b"M4A ", b"M4B ", b"M4P ", b"F4A ", b"F4B "];

/// The video kind a brand names.
fn kind_of(brand: &[u8; 4]) -> Option<VideoKind> {
    match brand {
        b"qt  " => Some(VideoKind::QuickTime),
        b"M4V " | b"M4VH" | b"M4VP" => Some(VideoKind::M4v),
        b"isom" | b"iso2" | b"iso3" | b"iso4" | b"iso5" | b"iso6" | b"iso7" | b"iso8" | b"iso9" | b"mp41" | b"mp42" | b"mp71" | b"avc1" | b"dash"
        | b"MSNV" | b"XAVC" | b"3gp4" | b"3gp5" | b"3gp6" | b"3gp7" | b"3g2a" | b"mmp4" | b"f4v " => Some(VideoKind::Mp4),
        _ => None,
    }
}

/// The kind an `ftyp` box names: its major brand's, else its first compatible brand's.
fn brand_kind(major: &[u8; 4], compatible: &[[u8; 4]]) -> Option<VideoKind> {
    let still = |b: &[u8; 4]| STILL_BRANDS.contains(&b);
    if still(major) || AUDIO_BRANDS.contains(&major) || compatible.iter().any(still) {
        return None;
    }
    kind_of(major).or_else(|| compatible.iter().find_map(kind_of))
}

/// The facts of the video in `bytes` (the whole file, or as much of it as holds them).
pub fn probe(bytes: &[u8], limits: &Limits) -> Result<VideoInfo> {
    let kind = recognise(bytes).ok_or_else(|| unrecognised(bytes))?;
    probe_as(kind, bytes, limits)
}

/// Why `bytes` are no video: too few to tell (an EBML header they cut included), or not one.
fn unrecognised(bytes: &[u8]) -> Error {
    let ebml = bytes.get(0..4) == Some(&ebml::EBML_MAGIC[..]);
    if bytes.len() < crate::sniff::SNIFF_LEN || (ebml && mkv::header_is_cut(bytes)) { Error::Truncated } else { Error::Unknown }
}

/// The facts of `bytes` read as a video of `kind` (no sniffing).
pub fn probe_as(kind: VideoKind, bytes: &[u8], limits: &Limits) -> Result<VideoInfo> {
    let mut work = Work::new(MAX_STEPS, "boxes or elements in a video");
    let mut info = VideoInfo::new(kind);
    match kind {
        VideoKind::Matroska => mkv::probe(bytes, &mut info, limits, &mut work)?,
        _ => mp4::probe(bytes, &mut info, limits, &mut work)?,
    }
    Ok(info)
}

/// The facts of the video at `path`, reading only what they need: an ISO BMFF file's `ftyp` and
/// `moov` boxes (the media data is skipped, wherever the movie box lies), a Matroska file's
/// first bytes (from 1 MiB, doubling up to [`Limits::max_header_bytes`], until its segment
/// information and tracks are in).
pub fn probe_file(path: &Path, limits: &Limits) -> Result<VideoInfo> {
    let io = |e: std::io::Error| Error::Io(e.to_string());
    let mut file = std::fs::File::open(path).map_err(io)?;
    let size = file.metadata().map_err(io)?.len();
    let mut head = Vec::new();
    (&mut file).take(crate::sniff::SNIFF_LEN as u64 * 2).read_to_end(&mut head).map_err(io)?;
    let kind = recognise(&head).ok_or_else(|| unrecognised(&head))?;
    if kind == VideoKind::Matroska {
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        let mut file = file.take(limits.max_header_bytes);
        let mut bytes = Vec::new();
        let mut want: u64 = 1 << 20;
        loop {
            let have = bytes.len() as u64;
            let target = want.min(limits.max_header_bytes);
            if target > have {
                (&mut file).take(target - have).read_to_end(&mut bytes).map_err(io)?;
            }
            let complete = bytes.len() as u64 >= size.min(limits.max_header_bytes);
            match probe_as(kind, &bytes, limits) {
                Err(Error::Truncated) if !complete => want = want.saturating_mul(2),
                result => return result,
            }
        }
    }
    let boxes = mp4::read_movie(&mut file, size, limits)?;
    probe_as(kind, &boxes, limits)
}

/// A video's header: unsupported until its frames are decoded (the facts come from [`probe`]).
pub fn header(kind: VideoKind, _bytes: &[u8], _limits: &Limits) -> Result<Header> {
    Err(Error::Unsupported(Format::Video(kind)))
}

/// A video's frames: unsupported until M15.15.
pub fn decode(_bytes: &[u8], header: &Header, _budget: &mut Budget, _limits: &Limits) -> Result<Decoded> {
    Err(Error::Unsupported(header.format))
}

/// A video carries no embedded picture Dalica reads yet.
pub fn embedded_thumbnail(_bytes: &[u8], _limits: &Limits) -> Result<Option<Range<usize>>> {
    Ok(None)
}

/// The point an ISO 6709 string names (`+37.7749-122.4194+010.000/`; degrees, degrees and
/// minutes, or degrees, minutes and seconds, each with an optional fraction), or `None`.
///
/// Provenance: written from the public description of ISO 6709 Annex H (the text form) as
/// QuickTime and Matroska use it; no code was consulted.
pub fn iso6709(text: &str) -> Option<LatLon> {
    let text = text.trim().trim_end_matches('\0').trim_end_matches('/');
    if text.len() > MAX_TEXT || !text.is_ascii() {
        return None;
    }
    // A coordinate reference system, if named, ends the numbers.
    let text = text.get(..text.find("CRS").unwrap_or(text.len()))?;
    // Split at each sign: latitude, longitude, then an optional altitude we ignore.
    let starts: Vec<usize> = text.char_indices().filter(|&(_, c)| c == '+' || c == '-').map(|(i, _)| i).take(3).collect();
    let (&a, &b) = (starts.first()?, starts.get(1)?);
    if a != 0 {
        return None;
    }
    let end = starts.get(2).copied().unwrap_or(text.len());
    let lat = angle(text.get(a..b)?, 2)?;
    let lon = angle(text.get(b..end)?, 3)?;
    LatLon::new(lat, lon).ok()
}

/// One signed ISO 6709 angle whose degrees take `deg` digits: `±D[.d]`, `±DM[.m]` or `±DMS[.s]`.
fn angle(part: &str, deg: usize) -> Option<f64> {
    let (sign, digits) = match part.as_bytes().first()? {
        b'+' => (1.0, part.get(1..)?),
        b'-' => (-1.0, part.get(1..)?),
        _ => return None,
    };
    let int_len = digits.find('.').unwrap_or(digits.len());
    if int_len == 0 || !digits.bytes().enumerate().all(|(i, c)| c.is_ascii_digit() || (c == b'.' && i == int_len)) {
        return None;
    }
    let value: f64 = digits.parse().ok()?;
    let degrees = match int_len.checked_sub(deg)? {
        0 => value,
        2 => {
            let d = (value / 100.0).trunc();
            let m = value - d * 100.0;
            if m >= 60.0 {
                return None;
            }
            d + m / 60.0
        }
        4 => {
            let d = (value / 10_000.0).trunc();
            let rest = value - d * 10_000.0;
            let m = (rest / 100.0).trunc();
            let sec = rest - m * 100.0;
            if m >= 60.0 || sec >= 60.0 {
                return None;
            }
            d + m / 60.0 + sec / 3600.0
        }
        _ => return None,
    };
    degrees.is_finite().then_some(sign * degrees)
}

/// Milliseconds of `duration` ticks at `timescale` ticks per second, rounded to the nearest.
fn millis(duration: u64, timescale: u64) -> Option<u64> {
    if timescale == 0 {
        return None;
    }
    let ms = (u128::from(duration) * 1000 + u128::from(timescale) / 2) / u128::from(timescale);
    u64::try_from(ms).ok()
}

/// `value` rounded to three decimals, if finite and positive.
fn three_decimals(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then(|| (value * 1000.0).round() / 1000.0)
}

/// Text of at most [`MAX_TEXT`] bytes, up to its first NUL.
fn text(bytes: &[u8]) -> String {
    let bytes = bytes.get(..bytes.len().min(MAX_TEXT)).unwrap_or_default();
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(bytes.get(..end).unwrap_or_default()).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso6709_forms() {
        let at = |s: &str| iso6709(s).map(|p| ((p.lat() * 1e4).round() / 1e4, (p.lon() * 1e4).round() / 1e4));
        assert_eq!(at("+37.7749-122.4194+010.000/"), Some((37.7749, -122.4194)));
        assert_eq!(at("+38.7139-009.1394/"), Some((38.7139, -9.1394)));
        assert_eq!(at("-33.8688+151.2093"), Some((-33.8688, 151.2093)));
        // Degrees and minutes; degrees, minutes and seconds.
        assert_eq!(at("+3830.00-00908.40/"), Some((38.5, -9.14)));
        assert_eq!(at("+383000-0090824/"), Some((38.5, -9.14)));
        assert_eq!(at("+40.6894-074.0447+000.000CRSWGS_84/"), Some((40.6894, -74.0447)));
        assert_eq!(at("+40.6894-074.0447CRSWGS_84/"), Some((40.6894, -74.0447)));
        for bad in ["", "37.7-122.4", "+37.7", "+95.0+010.0/", "+37.7+190.0/", "+3790.0-00908.0/", "+3.7-12.4/", "+1e2+010.0/", "+37..1-122.4/"] {
            assert_eq!(iso6709(bad), None, "{bad}");
        }
    }

    #[test]
    fn brands_name_the_kind() {
        assert_eq!(brand_kind(b"mp42", &[*b"mp42", *b"isom"]), Some(VideoKind::Mp4));
        assert_eq!(brand_kind(b"qt  ", &[*b"qt  "]), Some(VideoKind::QuickTime));
        assert_eq!(brand_kind(b"M4V ", &[*b"M4V ", *b"mp42", *b"isom"]), Some(VideoKind::M4v));
        // An unknown major brand: the compatible ones decide.
        assert_eq!(brand_kind(b"XYZW", &[*b"XYZW", *b"isom"]), Some(VideoKind::Mp4));
        assert_eq!(brand_kind(b"XYZW", &[*b"XYZW"]), None);
        // Still images and sound-only files are never videos, whatever else they name.
        assert_eq!(brand_kind(b"heic", &[*b"mif1", *b"heic"]), None);
        assert_eq!(brand_kind(b"isom", &[*b"isom", *b"mif1"]), None);
        assert_eq!(brand_kind(b"M4A ", &[*b"M4A ", *b"mp42", *b"isom"]), None);
    }

    #[test]
    fn durations_round_to_the_millisecond() {
        assert_eq!(millis(90_000, 600), Some(150_000));
        assert_eq!(millis(1, 3), Some(333));
        assert_eq!(millis(2, 3), Some(667));
        assert_eq!(millis(5, 0), None);
        assert_eq!(millis(u64::MAX, 1), None);
        assert_eq!(three_decimals(30_000.0 / 1001.0), Some(29.97));
        assert_eq!(three_decimals(f64::NAN), None);
    }
}
