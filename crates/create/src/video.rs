//! A video's outputs (`editing.md` §11, `creating.md` §4; reserved decision 2, `roadmap.md`
//! M15.25): **Export Clip**'s trimmed copy ([`cut`]) and **Save Frame as Picture**'s JPEG
//! ([`frame_picture`], [`frame_name`]). Pure but for reading the clip: bytes come back, and the
//! engine writes them through `fs` (never over a file, never into the clip).
//!
//! - **Export Clip copies, it never re-encodes.** The trimmed part is cut out of the container
//!   with its coded samples as they are, so it works on any clip whose container Dalica reads,
//!   whether or not Dalica decodes its codec. A decoder can only start at a key frame, so the cut
//!   starts at the video's last key frame at or before the start asked for and runs to the last
//!   frame shown before the end; the sound and the other tracks keep what plays in between. The
//!   real cut is returned ([`Cut::start_ms`], [`Cut::end_ms`]) for the reply to say.
//!   - **MP4, M4V and QuickTime** ([`VideoKind::Mp4`], …): the movie box is read (at most
//!     [`MAX_MOVIE_BYTES`]) and each track's sample tables expanded (ISO/IEC 14496-12 §8.6–§8.7:
//!     `stts`, `ctts`, `stss`, `stsc`, `stsz`/`stz2`, `stco`/`co64`, `edts`/`elst`); the new file
//!     is the source's `ftyp`, a movie box with each track's own boxes copied (`tkhd`, `mdhd`,
//!     `hdlr`, the media header, `dinf`, `stsd`, `tref`, `udta`, `meta`, the movie's `udta` and
//!     `meta` with the place and date), new sample tables, an edit list that starts the
//!     presentation at the cut, and the samples in one `mdat`. Tables Dalica does not rewrite
//!     (`sdtp`, `sgpd`, `sbgp`, `cslg`, …) are left out, never copied wrong. A fragmented movie
//!     (`mvex`, `moof`) is refused.
//!   - **Matroska** ([`VideoKind::Matroska`], RFC 9559): the EBML header and the segment's `Info`
//!     (with a new `Duration`), `Tracks` and `Tags` are copied, then the clusters holding the cut's
//!     blocks with new cluster timestamps and each block's relative time moved to match; the seek
//!     head, the cues and the chapters (whose times would be wrong) are left out.
//! - **Save Frame as Picture** goes through the one output pipeline ([`crate::output::render`]):
//!   the frame (upright, as the player shows it) losslessly into PNG, then rendered with the
//!   video's own rotations and flips and written as a JPEG at [`FRAME_QUALITY`] with an EXIF
//!   thumbnail and the frame's date as its date taken.
//!
//! Every count and size read is hostile (`security.md` §4.2): the movie box and each Matroska
//! element read are capped; boxes and elements are walked with a bounded step count; a track holds
//! at most [`MAX_SAMPLES`] samples and every table's entry count is checked against its box before
//! anything is expanded; every sample lies inside the file; the new clip is at most
//! [`MAX_CLIP_BYTES`].
//!
//! Provenance: written from the public descriptions of ISO/IEC 14496-12 (the box structure and the
//! sample tables), Apple's QuickTime File Format documentation and RFC 9559 (Matroska), as
//! summarised in open format notes; no code was consulted.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use dalica_core::names;
use dalica_edit::{Op, Recipe};
use dalica_image::encode::{self, Image, Metadata};
use dalica_image::formats::bmff::{BoxRef, Boxes};
use dalica_image::formats::video::recognise;
use dalica_image::limits::Work;
use dalica_image::sniff::VideoKind;
use dalica_image::{Limits, Rgba8};
use dalica_meta::{Changes, DateTaken};

use crate::output::{self, Choice, OutFormat, Quality, Spec};

/// The JPEG quality Save Frame as Picture writes.
pub const FRAME_QUALITY: u8 = 92;
/// The largest movie box (or Matroska `Info`, `Tracks`, `Tags`) read.
pub const MAX_MOVIE_BYTES: u64 = 64 << 20;
/// The largest clip Export Clip makes in one piece (it is built in memory and written whole).
pub const MAX_CLIP_BYTES: u64 = 1 << 30;
/// The most samples (or Matroska blocks) a clip's tracks hold together.
pub const MAX_SAMPLES: usize = 1 << 22;
/// The most tracks copied.
pub const MAX_TRACKS: usize = 32;
/// The largest Matroska cluster read.
pub const MAX_CLUSTER_BYTES: u64 = 256 << 20;
/// The most boxes or elements one cut walks.
const MAX_STEPS: usize = 1 << 20;
/// The largest single sample copied.
const MAX_SAMPLE_BYTES: u64 = 64 << 20;

/// Why Export Clip made nothing.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ClipError {
    /// Not a container Dalica cuts, or a layout it does not (a fragmented movie).
    #[error("{0}")]
    Unsupported(String),
    /// The file is damaged or cut short.
    #[error("the clip is damaged: {0}")]
    Damaged(String),
    /// Past a limit (the movie box, the samples, the new clip's size).
    #[error("{0}")]
    TooLarge(String),
    /// Nothing of the clip lies between the start and the end.
    #[error("nothing of the clip lies between {start_ms} ms and {end_ms} ms")]
    Empty { start_ms: u64, end_ms: u64 },
    /// The file could not be read.
    #[error("cannot read the clip: {0}")]
    Read(String),
}

impl From<dalica_image::Error> for ClipError {
    fn from(e: dalica_image::Error) -> ClipError {
        match e {
            dalica_image::Error::TooMany(what) => ClipError::TooLarge(format!("too many {what}")),
            other => ClipError::Damaged(other.reason()),
        }
    }
}

type Result<T, E = ClipError> = std::result::Result<T, E>;

fn damaged(what: impl Into<String>) -> ClipError {
    ClipError::Damaged(what.into())
}

/// The trimmed part of a clip, as a new file's bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    pub bytes: Vec<u8>,
    /// Where the copy really starts and ends in the source, in milliseconds (the start at a key
    /// frame at or before the one asked for).
    pub start_ms: u64,
    pub end_ms: u64,
    /// The container: the extension the new file takes (`mp4`, `m4v`, `mov`, `mkv`).
    pub extension: &'static str,
}

impl Cut {
    /// How long the copy lasts, in milliseconds.
    pub fn length_ms(&self) -> u64 {
        self.end_ms.saturating_sub(self.start_ms)
    }
}

/// The extension a clip of `kind` is written with.
pub fn extension(kind: VideoKind) -> &'static str {
    match kind {
        VideoKind::Mp4 => "mp4",
        VideoKind::M4v => "m4v",
        VideoKind::QuickTime => "mov",
        VideoKind::Matroska => "mkv",
    }
}

/// The part of the clip at `path` from `start_ms` to `end_ms`, copied without re-encoding (see the
/// module docs). Reads the file; writes nothing.
pub fn cut(path: &Path, start_ms: u64, end_ms: u64) -> Result<Cut> {
    if start_ms >= end_ms {
        return Err(ClipError::Empty { start_ms, end_ms });
    }
    let mut input = Input::open(path)?;
    let head = input.read_at(0, input.len.min(64))?;
    let kind = recognise(&head).ok_or_else(|| ClipError::Unsupported("Export Clip copies MP4, M4V, QuickTime and Matroska clips".into()))?;
    let mut work = Work::new(MAX_STEPS, "boxes or elements");
    let (bytes, start, end) = match kind {
        VideoKind::Matroska => mkv::cut(&mut input, start_ms, end_ms, &mut work)?,
        _ => mp4::cut(&mut input, start_ms, end_ms, &mut work)?,
    };
    Ok(Cut { bytes, start_ms: start, end_ms: end, extension: extension(kind) })
}

// --- Reading -------------------------------------------------------------------------------------

/// The clip's file, read range by range.
struct Input {
    file: File,
    len: u64,
}

impl Input {
    fn open(path: &Path) -> Result<Input> {
        let file = File::open(path).map_err(|e| ClipError::Read(e.to_string()))?;
        let len = file.metadata().map_err(|e| ClipError::Read(e.to_string()))?.len();
        Ok(Input { file, len })
    }

    /// `len` bytes at `offset`, all inside the file.
    fn read_at(&mut self, offset: u64, len: u64) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.read_into(offset, len, &mut buf)?;
        Ok(buf)
    }

    /// Appends `len` bytes at `offset` to `out`.
    fn read_into(&mut self, offset: u64, len: u64, out: &mut Vec<u8>) -> Result<()> {
        let end = offset.checked_add(len).filter(|&e| e <= self.len).ok_or_else(|| damaged("a part of it lies past the end of the file"))?;
        let n = usize::try_from(end - offset).map_err(|_| ClipError::TooLarge("a part too large to read".into()))?;
        out.try_reserve(n).map_err(|_| ClipError::TooLarge("not enough memory for the clip".into()))?;
        self.file.seek(SeekFrom::Start(offset)).map_err(|e| ClipError::Read(e.to_string()))?;
        let before = out.len();
        (&mut self.file).take(len).read_to_end(out).map_err(|e| ClipError::Read(e.to_string()))?;
        if out.len() - before != n {
            return Err(ClipError::Read("the file changed while it was read".into()));
        }
        Ok(())
    }
}

fn be16(b: &[u8], at: usize) -> Result<u16> {
    b.get(at..at + 2).and_then(|s| s.try_into().ok()).map(u16::from_be_bytes).ok_or_else(|| damaged("a table is cut short"))
}

fn be32(b: &[u8], at: usize) -> Result<u32> {
    b.get(at..at + 4).and_then(|s| s.try_into().ok()).map(u32::from_be_bytes).ok_or_else(|| damaged("a table is cut short"))
}

fn be64(b: &[u8], at: usize) -> Result<u64> {
    b.get(at..at + 8).and_then(|s| s.try_into().ok()).map(u64::from_be_bytes).ok_or_else(|| damaged("a table is cut short"))
}

/// Milliseconds of `ticks` at `scale` ticks a second (rounded down; 0 for a zero scale).
fn ms_of(ticks: i128, scale: u64) -> i128 {
    if scale == 0 { 0 } else { ticks * 1000 / i128::from(scale) }
}

/// Ticks at `scale` of `ms` milliseconds.
fn ticks_of(ms: i128, scale: u64) -> i128 {
    ms * i128::from(scale) / 1000
}

fn clamp_u64(v: i128) -> u64 {
    u64::try_from(v.max(0)).unwrap_or(u64::MAX)
}

/// Adds `n` bytes to a running total, refusing past [`MAX_CLIP_BYTES`].
fn add_bytes(total: &mut u64, n: u64) -> Result<()> {
    *total = total.saturating_add(n);
    if *total > MAX_CLIP_BYTES {
        return Err(ClipError::TooLarge(format!("the trimmed part is larger than {} MB: trim it shorter to export it", MAX_CLIP_BYTES >> 20)));
    }
    Ok(())
}

// --- MP4, M4V and QuickTime ----------------------------------------------------------------------

mod mp4 {
    use super::*;

    /// One sample, in decode order.
    #[derive(Clone, Copy, Debug)]
    struct Sample {
        offset: u64,
        size: u32,
        dts: u64,
        duration: u32,
        /// Composition offset (the `ctts` entry).
        cts: i64,
        sync: bool,
        desc: u32,
    }

    /// One track as read.
    struct Track {
        /// The track's boxes copied as they are (`tref`, `udta`, `meta`, …), in order.
        extra: Vec<Vec<u8>>,
        tkhd: Vec<u8>,
        /// `mdia`'s boxes other than `minf` (`mdhd` patched with the new duration, `hdlr`, …).
        mdhd: Vec<u8>,
        mdia_extra: Vec<Vec<u8>>,
        /// `minf`'s boxes other than `stbl` (the media header, `dinf`, …).
        minf_extra: Vec<Vec<u8>>,
        stsd: Vec<u8>,
        timescale: u64,
        handler: [u8; 4],
        samples: Vec<Sample>,
        has_ctts: bool,
        has_stss: bool,
        /// The edit list's shift: presentation = dts + cts − `shift` (ticks of the track).
        shift: i64,
    }

    impl Track {
        fn pts(&self, s: &Sample) -> i128 {
            i128::from(s.dts) + i128::from(s.cts) - i128::from(self.shift)
        }
    }

    /// The box at `range` of `moov`, whole.
    fn whole(moov: &[u8], b: &BoxRef) -> Result<Vec<u8>> {
        Ok(moov.get(b.range.clone()).ok_or_else(|| damaged("a box lies outside the movie"))?.to_vec())
    }

    fn body<'a>(moov: &'a [u8], b: &BoxRef) -> Result<&'a [u8]> {
        moov.get(b.body.clone()).ok_or_else(|| damaged("a box lies outside the movie"))
    }

    /// A full box's entry table: `count` entries of `size` bytes after `skip` bytes (version and
    /// flags, then the count); the count checked against the box's length.
    fn table(b: &[u8], skip: usize, size: usize) -> Result<(usize, &[u8])> {
        let count = be32(b, skip)? as usize;
        let rest = b.get(skip + 4..).ok_or_else(|| damaged("a table is cut short"))?;
        if count.checked_mul(size).is_none_or(|n| n > rest.len()) {
            return Err(damaged("a table counts more entries than it holds"));
        }
        if count > MAX_SAMPLES {
            return Err(ClipError::TooLarge(format!("a table of more than {MAX_SAMPLES} entries")));
        }
        Ok((count, rest))
    }

    /// The `ftyp` box (whole) and the movie box's bytes of the file, with no fragments.
    fn top(input: &mut Input, work: &mut Work) -> Result<(Option<Vec<u8>>, Vec<u8>)> {
        let (mut at, mut ftyp, mut moov) = (0u64, None, None);
        while at < input.len {
            work.step()?;
            let head = input.read_at(at, (input.len - at).min(16))?;
            if head.len() < 8 {
                break;
            }
            let size32 = be32(&head, 0)?;
            let kind: [u8; 4] = head.get(4..8).and_then(|k| k.try_into().ok()).ok_or_else(|| damaged("a box is cut short"))?;
            let size = match size32 {
                0 => input.len - at,
                1 => be64(&head, 8)?,
                n => u64::from(n),
            };
            if size < 8 || at.checked_add(size).is_none_or(|end| end > input.len) {
                return Err(damaged(format!("a {} box runs past the end of the file", String::from_utf8_lossy(&kind))));
            }
            match &kind {
                b"ftyp" if ftyp.is_none() && size <= 4096 => ftyp = Some(input.read_at(at, size)?),
                b"moov" if moov.is_none() => {
                    if size > MAX_MOVIE_BYTES {
                        return Err(ClipError::TooLarge(format!("a movie box larger than {} MB", MAX_MOVIE_BYTES >> 20)));
                    }
                    moov = Some(input.read_at(at, size)?);
                }
                b"moof" | b"mfra" => return Err(ClipError::Unsupported("Export Clip does not cut fragmented movies".into())),
                _ => {}
            }
            at += size;
        }
        let moov = moov.ok_or_else(|| damaged("it has no movie box"))?;
        Ok((ftyp, moov))
    }

    /// The `mvhd` (patchable) and the movie's other boxes copied, and its tracks.
    struct Movie {
        mvhd: Vec<u8>,
        timescale: u64,
        extra: Vec<Vec<u8>>,
        tracks: Vec<Track>,
    }

    fn movie(moov: &[u8], work: &mut Work) -> Result<Movie> {
        let root = Boxes::top(moov).next(work)?.ok_or_else(|| damaged("an empty movie box"))?;
        let mvhd = Boxes::children(moov, &root, 0)?.find(b"mvhd", None, work)?.ok_or_else(|| damaged("the movie has no header"))?;
        let timescale = times_scale(body(moov, &mvhd)?)?;
        let mvhd = whole(moov, &mvhd)?;
        let (mut extra, mut tracks) = (Vec::new(), Vec::new());
        let mut kids = Boxes::children(moov, &root, 0)?;
        let mut total = 0usize;
        while let Some(b) = kids.next(work)? {
            match &b.kind {
                b"mvhd" => {}
                b"mvex" => return Err(ClipError::Unsupported("Export Clip does not cut fragmented movies".into())),
                b"trak" => {
                    if tracks.len() >= MAX_TRACKS {
                        return Err(ClipError::TooLarge(format!("more than {MAX_TRACKS} tracks")));
                    }
                    let t = track(moov, &b, timescale, work)?;
                    total = total.saturating_add(t.samples.len());
                    if total > MAX_SAMPLES {
                        return Err(ClipError::TooLarge(format!("more than {MAX_SAMPLES} samples")));
                    }
                    tracks.push(t);
                }
                b"free" | b"skip" | b"wide" => {}
                _ => extra.push(whole(moov, &b)?),
            }
        }
        Ok(Movie { mvhd, timescale, extra, tracks })
    }

    /// The timescale of an `mvhd` or `mdhd` body.
    fn times_scale(b: &[u8]) -> Result<u64> {
        let at = if b.first() == Some(&1) { 20 } else { 12 };
        Ok(u64::from(be32(b, at)?))
    }

    /// Writes `duration` into the `mvhd`, `mdhd` (`at` 0: after the timescale) or `tkhd` box `whole`.
    fn patch_duration(whole: &mut [u8], tkhd: bool, duration: u64) {
        let v1 = whole.get(8) == Some(&1);
        let at = match (tkhd, v1) {
            (false, false) => 8 + 16,
            (false, true) => 8 + 24,
            (true, false) => 8 + 20,
            (true, true) => 8 + 28,
        };
        if v1 {
            if let Some(s) = whole.get_mut(at..at + 8) {
                s.copy_from_slice(&duration.to_be_bytes());
            }
        } else if let Some(s) = whole.get_mut(at..at + 4) {
            s.copy_from_slice(&u32::try_from(duration).unwrap_or(u32::MAX).to_be_bytes());
        }
    }

    fn track(moov: &[u8], trak: &BoxRef, movie_scale: u64, work: &mut Work) -> Result<Track> {
        let mut t = Track {
            extra: Vec::new(),
            tkhd: Vec::new(),
            mdhd: Vec::new(),
            mdia_extra: Vec::new(),
            minf_extra: Vec::new(),
            stsd: Vec::new(),
            timescale: 0,
            handler: [0; 4],
            samples: Vec::new(),
            has_ctts: false,
            has_stss: false,
            shift: 0,
        };
        let (mut elst, mut stbl) = (None, None);
        let mut kids = Boxes::children(moov, trak, 0)?;
        while let Some(b) = kids.next(work)? {
            match &b.kind {
                b"tkhd" => t.tkhd = whole(moov, &b)?,
                b"edts" => elst = Boxes::children(moov, &b, 0)?.find(b"elst", None, work)?,
                b"mdia" => {
                    let mut m = Boxes::children(moov, &b, 0)?;
                    while let Some(c) = m.next(work)? {
                        match &c.kind {
                            b"mdhd" => {
                                t.mdhd = whole(moov, &c)?;
                                t.timescale = times_scale(body(moov, &c)?)?;
                            }
                            b"hdlr" => {
                                let h = body(moov, &c)?;
                                t.handler = h.get(8..12).and_then(|k| k.try_into().ok()).ok_or_else(|| damaged("a handler is cut short"))?;
                                t.mdia_extra.push(whole(moov, &c)?);
                            }
                            b"minf" => {
                                let mut i = Boxes::children(moov, &c, 0)?;
                                while let Some(d) = i.next(work)? {
                                    match &d.kind {
                                        b"stbl" => stbl = Some(d),
                                        _ => t.minf_extra.push(whole(moov, &d)?),
                                    }
                                }
                            }
                            _ => t.mdia_extra.push(whole(moov, &c)?),
                        }
                    }
                }
                _ => t.extra.push(whole(moov, &b)?),
            }
        }
        if t.tkhd.is_empty() || t.mdhd.is_empty() {
            return Err(damaged("a track without its header"));
        }
        let stbl = stbl.ok_or_else(|| damaged("a track without its sample tables"))?;
        samples(moov, &stbl, &mut t, work)?;
        if let Some(elst) = elst {
            t.shift = edit_shift(body(moov, &elst)?, t.timescale, movie_scale)?;
        }
        Ok(t)
    }

    /// The ticks the edit list moves the track's times back by (negative: forward): the first
    /// media time, less the empty edits before it (a delay, in the movie's timescale). Only what
    /// moves the start is read; later edits are not (phones write one).
    fn edit_shift(b: &[u8], timescale: u64, movie_scale: u64) -> Result<i64> {
        let v1 = b.first() == Some(&1);
        let size = if v1 { 20 } else { 12 };
        let (count, t) = table(b, 4, size)?;
        let mut delay: i64 = 0;
        for e in 0..count.min(16) {
            let at = e * size;
            let (duration, media_time) =
                if v1 { (be64(t, at)?, be64(t, at + 8)? as i64) } else { (u64::from(be32(t, at)?), i64::from(be32(t, at + 4)? as i32)) };
            if media_time == -1 {
                if movie_scale > 0 {
                    let ticks = i128::from(duration) * i128::from(timescale) / i128::from(movie_scale);
                    delay = delay.saturating_add(i64::try_from(ticks).unwrap_or(i64::MAX));
                }
                continue;
            }
            return Ok(media_time.saturating_sub(delay));
        }
        Ok(delay.saturating_neg())
    }

    /// Expands the sample tables of `stbl` into `t.samples`.
    fn samples(moov: &[u8], stbl: &BoxRef, t: &mut Track, work: &mut Work) -> Result<()> {
        let (mut stts, mut ctts, mut stss, mut stsc, mut stsz, mut stz2, mut stco, mut co64) = (None, None, None, None, None, None, None, None);
        let mut kids = Boxes::children(moov, stbl, 0)?;
        while let Some(b) = kids.next(work)? {
            match &b.kind {
                b"stsd" => t.stsd = whole(moov, &b)?,
                b"stts" => stts = Some(body(moov, &b)?),
                b"ctts" => ctts = Some(body(moov, &b)?),
                b"stss" => stss = Some(body(moov, &b)?),
                b"stsc" => stsc = Some(body(moov, &b)?),
                b"stsz" => stsz = Some(body(moov, &b)?),
                b"stz2" => stz2 = Some(body(moov, &b)?),
                b"stco" => stco = Some(body(moov, &b)?),
                b"co64" => co64 = Some(body(moov, &b)?),
                _ => {}
            }
        }
        if t.stsd.is_empty() {
            return Err(damaged("a track without its sample description"));
        }
        // Sizes.
        let sizes: Vec<u32> = match (stsz, stz2) {
            (Some(b), _) => {
                let fixed = be32(b, 4)?;
                let count = be32(b, 8)? as usize;
                if count > MAX_SAMPLES {
                    return Err(ClipError::TooLarge(format!("a track of more than {MAX_SAMPLES} samples")));
                }
                if fixed != 0 {
                    vec![fixed; count]
                } else {
                    let (n, rest) = table(b, 8, 4)?;
                    (0..n).map(|i| be32(rest, i * 4)).collect::<Result<_>>()?
                }
            }
            (None, Some(b)) => {
                let field = b.get(7).copied().unwrap_or(0);
                let count = be32(b, 8)? as usize;
                let bits = usize::from(field);
                if !matches!(bits, 4 | 8 | 16) || count > MAX_SAMPLES {
                    return Err(damaged("a compact size table Dalica does not read"));
                }
                let rest = b.get(12..).ok_or_else(|| damaged("a table is cut short"))?;
                if count.saturating_mul(bits).div_ceil(8) > rest.len() {
                    return Err(damaged("a table counts more entries than it holds"));
                }
                (0..count)
                    .map(|i| match bits {
                        4 => rest
                            .get(i / 2)
                            .map(|v| u32::from(if i % 2 == 0 { v >> 4 } else { v & 15 }))
                            .ok_or_else(|| damaged("a table is cut short")),
                        8 => rest.get(i).map(|v| u32::from(*v)).ok_or_else(|| damaged("a table is cut short")),
                        _ => be16(rest, i * 2).map(u32::from),
                    })
                    .collect::<Result<_>>()?
            }
            (None, None) => return Err(damaged("a track without its sample sizes")),
        };
        let n = sizes.len();
        // Chunk offsets.
        let chunks: Vec<u64> = match (stco, co64) {
            (Some(b), _) => {
                let (c, rest) = table(b, 4, 4)?;
                (0..c).map(|i| be32(rest, i * 4).map(u64::from)).collect::<Result<_>>()?
            }
            (None, Some(b)) => {
                let (c, rest) = table(b, 4, 8)?;
                (0..c).map(|i| be64(rest, i * 8)).collect::<Result<_>>()?
            }
            (None, None) if n == 0 => Vec::new(),
            (None, None) => return Err(damaged("a track without its chunk offsets")),
        };
        // Samples to chunks: each sample's offset and description.
        let mut placed: Vec<(u64, u32)> = Vec::with_capacity(n);
        if n > 0 {
            let b = stsc.ok_or_else(|| damaged("a track without its sample-to-chunk table"))?;
            let (c, rest) = table(b, 4, 12)?;
            let runs: Vec<(u32, u32, u32)> =
                (0..c).map(|i| Ok((be32(rest, i * 12)?, be32(rest, i * 12 + 4)?, be32(rest, i * 12 + 8)?))).collect::<Result<_>>()?;
            let mut sample = 0usize;
            for (r, &(first, per, desc)) in runs.iter().enumerate() {
                let last = runs.get(r + 1).map_or(chunks.len() as u64 + 1, |next| u64::from(next.0));
                if first == 0 || u64::from(first) > last {
                    return Err(damaged("a sample-to-chunk table out of order"));
                }
                for chunk in u64::from(first)..last {
                    work.step()?;
                    let Some(&base) = chunks.get(usize::try_from(chunk - 1).unwrap_or(usize::MAX)) else { break };
                    let mut at = base;
                    for _ in 0..per {
                        let Some(&size) = sizes.get(sample) else { break };
                        placed.push((at, desc));
                        at = at.saturating_add(u64::from(size));
                        sample += 1;
                    }
                    if sample >= n {
                        break;
                    }
                }
                if sample >= n {
                    break;
                }
            }
            if placed.len() < n {
                return Err(damaged("the chunks hold fewer samples than the track counts"));
            }
        }
        // Decode times.
        let mut durations = Vec::with_capacity(n);
        if let Some(b) = stts {
            let (c, rest) = table(b, 4, 8)?;
            for i in 0..c {
                let (count, delta) = (be32(rest, i * 8)?, be32(rest, i * 8 + 4)?);
                for _ in 0..count {
                    if durations.len() >= n {
                        break;
                    }
                    durations.push(delta);
                }
            }
        }
        durations.resize(n, durations.last().copied().unwrap_or(0));
        // Composition offsets.
        let mut offsets = vec![0i64; n];
        if let Some(b) = ctts {
            t.has_ctts = true;
            let v1 = b.first() == Some(&1);
            let (c, rest) = table(b, 4, 8)?;
            let mut k = 0usize;
            for i in 0..c {
                let count = be32(rest, i * 8)?;
                let raw = be32(rest, i * 8 + 4)?;
                let off = if v1 { i64::from(raw as i32) } else { i64::from(raw) };
                for _ in 0..count {
                    let Some(slot) = offsets.get_mut(k) else { break };
                    *slot = off;
                    k += 1;
                }
            }
        }
        // Sync samples.
        let mut sync = vec![stss.is_none(); n];
        if let Some(b) = stss {
            t.has_stss = true;
            let (c, rest) = table(b, 4, 4)?;
            for i in 0..c {
                let k = be32(rest, i * 4)? as usize;
                if let Some(s) = k.checked_sub(1).and_then(|k| sync.get_mut(k)) {
                    *s = true;
                }
            }
        }
        let mut dts = 0u64;
        t.samples.reserve(n);
        for (i, &size) in sizes.iter().enumerate() {
            let (offset, desc) = placed.get(i).copied().unwrap_or((0, 1));
            let duration = durations.get(i).copied().unwrap_or(0);
            t.samples.push(Sample {
                offset,
                size,
                dts,
                duration,
                cts: offsets.get(i).copied().unwrap_or(0),
                sync: sync.get(i).copied().unwrap_or(false),
                desc,
            });
            dts = dts.saturating_add(u64::from(duration));
        }
        Ok(())
    }

    /// Which samples of each track the cut keeps (a range in decode order, possibly empty), and
    /// the real cut in milliseconds.
    fn choose(movie: &Movie, start_ms: u64, end_ms: u64) -> Result<(Vec<std::ops::Range<usize>>, u64, u64)> {
        let ms = |t: &Track, s: &Sample| ms_of(t.pts(s), t.timescale);
        let video = movie.tracks.iter().position(|t| &t.handler == b"vide" && !t.samples.is_empty());
        let (real_start, real_end, mut ranges) = match video {
            Some(v) => {
                let t = movie.tracks.get(v).ok_or_else(|| damaged("a track went missing"))?;
                let (start, end) = (i128::from(start_ms), i128::from(end_ms));
                let key = t.samples.iter().rposition(|s| s.sync && ms(t, s) <= start).or_else(|| t.samples.iter().position(|s| s.sync)).unwrap_or(0);
                let last = t.samples.iter().rposition(|s| ms(t, s) < end).filter(|&m| m >= key);
                let Some(last) = last else { return Err(ClipError::Empty { start_ms, end_ms }) };
                let range = key..last + 1;
                let picked = t.samples.get(range.clone()).unwrap_or_default();
                let first_pts = picked.iter().map(|s| t.pts(s)).min().unwrap_or(0);
                let last_end = picked.iter().map(|s| t.pts(s) + i128::from(s.duration)).max().unwrap_or(0);
                let mut ranges = vec![0..0; movie.tracks.len()];
                if let Some(r) = ranges.get_mut(v) {
                    *r = range;
                }
                (ms_of(first_pts, t.timescale).max(0), ms_of(last_end, t.timescale).max(0), ranges)
            }
            None => (i128::from(start_ms), i128::from(end_ms), vec![0..0; movie.tracks.len()]),
        };
        for (i, t) in movie.tracks.iter().enumerate() {
            if Some(i) == video {
                continue;
            }
            let (from, to) = (ticks_of(real_start, t.timescale), ticks_of(real_end, t.timescale));
            let keep = |s: &Sample| t.pts(s) + i128::from(s.duration.max(1)) > from && t.pts(s) < to;
            let first = t.samples.iter().position(keep);
            let last = t.samples.iter().rposition(keep);
            if let (Some(a), Some(b), Some(r)) = (first, last, ranges.get_mut(i)) {
                *r = a..b + 1;
            }
        }
        if ranges.iter().all(|r| r.is_empty()) || real_end <= real_start || real_end <= i128::from(start_ms) {
            return Err(ClipError::Empty { start_ms, end_ms });
        }
        Ok((ranges, clamp_u64(real_start), clamp_u64(real_end)))
    }

    pub(super) fn cut(input: &mut Input, start_ms: u64, end_ms: u64, work: &mut Work) -> Result<(Vec<u8>, u64, u64)> {
        let (ftyp, moov) = top(input, work)?;
        let movie = movie(&moov, work)?;
        let (ranges, real_start, real_end) = choose(&movie, start_ms, end_ms)?;
        let mut total = 0u64;
        for (t, r) in movie.tracks.iter().zip(&ranges) {
            for s in t.samples.get(r.clone()).unwrap_or_default() {
                if u64::from(s.size) > MAX_SAMPLE_BYTES {
                    return Err(ClipError::TooLarge("a sample larger than 64 MB".into()));
                }
                if s.offset.checked_add(u64::from(s.size)).is_none_or(|e| e > input.len) {
                    return Err(damaged("a sample lies past the end of the file"));
                }
                add_bytes(&mut total, u64::from(s.size))?;
            }
        }
        let ftyp = ftyp.unwrap_or_default();
        // The movie box's size does not depend on where the data starts: build it once to measure.
        let probe = moov_box(&movie, &ranges, real_start, real_end, 0)?;
        let data_at = (ftyp.len() + probe.len() + 8) as u64;
        let moov = moov_box(&movie, &ranges, real_start, real_end, data_at)?;
        if moov.len() != probe.len() {
            return Err(damaged("the movie box changed size"));
        }
        let mut out = Vec::new();
        out.try_reserve(usize::try_from(data_at.saturating_add(total)).unwrap_or(usize::MAX))
            .map_err(|_| ClipError::TooLarge("not enough memory for the clip".into()))?;
        out.extend_from_slice(&ftyp);
        out.extend_from_slice(&moov);
        let mdat_size = u32::try_from(total + 8).map_err(|_| ClipError::TooLarge("the trimmed part is too large".into()))?;
        out.extend_from_slice(&mdat_size.to_be_bytes());
        out.extend_from_slice(b"mdat");
        for (t, r) in movie.tracks.iter().zip(&ranges) {
            // Runs of samples lying end to end in the source are read at once.
            let picked = t.samples.get(r.clone()).unwrap_or_default();
            let mut i = 0;
            while let Some(first) = picked.get(i) {
                let (from, mut to) = (first.offset, first.offset + u64::from(first.size));
                let mut j = i + 1;
                while let Some(next) = picked.get(j) {
                    if next.offset != to || to - from > MAX_SAMPLE_BYTES {
                        break;
                    }
                    to += u64::from(next.size);
                    j += 1;
                }
                work.step()?;
                input.read_into(from, to - from, &mut out)?;
                i = j;
            }
        }
        Ok((out, real_start, real_end))
    }

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Result<Vec<u8>> {
        let size = u32::try_from(payload.len() + 8).map_err(|_| ClipError::TooLarge("a box too large to write".into()))?;
        let mut v = Vec::with_capacity(payload.len() + 8);
        v.extend_from_slice(&size.to_be_bytes());
        v.extend_from_slice(kind);
        v.extend_from_slice(payload);
        Ok(v)
    }

    /// A full box of version `version`, flags 0.
    fn full(kind: &[u8; 4], version: u8, payload: &[u8]) -> Result<Vec<u8>> {
        let mut p = vec![version, 0, 0, 0];
        p.extend_from_slice(payload);
        bx(kind, &p)
    }

    /// Run-length entries `(count, value)` of `values`.
    fn runs<T: PartialEq + Copy>(values: impl Iterator<Item = T>) -> Vec<(u32, T)> {
        let mut out: Vec<(u32, T)> = Vec::new();
        for v in values {
            match out.last_mut() {
                Some((n, last)) if *last == v && *n < u32::MAX => *n += 1,
                _ => out.push((1, v)),
            }
        }
        out
    }

    /// The new movie box: each track's chosen samples laid end to end from `data_at`, track after
    /// track.
    fn moov_box(movie: &Movie, ranges: &[std::ops::Range<usize>], real_start: u64, real_end: u64, data_at: u64) -> Result<Vec<u8>> {
        let mscale = movie.timescale.max(1);
        let movie_ticks = |ms: u64| clamp_u64(ticks_of(i128::from(ms), mscale));
        let duration = movie_ticks(real_end.saturating_sub(real_start));
        let mut mvhd = movie.mvhd.clone();
        patch_duration(&mut mvhd, false, duration);
        let mut parts = vec![mvhd];
        let mut at = data_at;
        for (t, r) in movie.tracks.iter().zip(ranges) {
            let picked = t.samples.get(r.clone()).unwrap_or_default();
            parts.push(trak(t, picked, real_start, real_end, mscale, &mut at)?);
        }
        parts.extend(movie.extra.iter().cloned());
        bx(b"moov", &parts.concat())
    }

    fn trak(t: &Track, picked: &[Sample], real_start: u64, real_end: u64, mscale: u64, at: &mut u64) -> Result<Vec<u8>> {
        let scale = t.timescale.max(1);
        let media: u64 = picked.iter().map(|s| u64::from(s.duration)).sum();
        // Where the presentation starts in the new media: presentation = dts + cts − shift, and the
        // new dts start at the first sample's, so the new shift is shift + start − dts₀ (an empty
        // edit when the track starts after the cut).
        let d0 = picked.first().map_or(0, |s| s.dts);
        let real = ticks_of(i128::from(real_start), scale);
        let media_time = i128::from(t.shift) + real - i128::from(d0);
        let (empty, media_time) = if media_time < 0 { (clamp_u64(-media_time), 0u64) } else { (0, clamp_u64(media_time)) };
        let shown = clamp_u64(ticks_of(i128::from(real_end.saturating_sub(real_start)), mscale));
        let empty_movie = clamp_u64(i128::from(empty) * i128::from(mscale) / i128::from(scale));
        let mut entries: Vec<u8> = Vec::new();
        let mut count = 0u32;
        if empty_movie > 0 {
            entries.extend_from_slice(&u32::try_from(empty_movie).unwrap_or(u32::MAX).to_be_bytes());
            entries.extend_from_slice(&(-1i32).to_be_bytes());
            entries.extend_from_slice(&0x0001_0000u32.to_be_bytes());
            count += 1;
        }
        if !picked.is_empty() {
            entries.extend_from_slice(&u32::try_from(shown.saturating_sub(empty_movie)).unwrap_or(u32::MAX).to_be_bytes());
            entries.extend_from_slice(&i32::try_from(media_time).unwrap_or(i32::MAX).to_be_bytes());
            entries.extend_from_slice(&0x0001_0000u32.to_be_bytes());
            count += 1;
        }
        let mut elst = count.to_be_bytes().to_vec();
        elst.extend(entries);
        let edts = bx(b"edts", &full(b"elst", 0, &elst)?)?;

        let mut tkhd = t.tkhd.clone();
        patch_duration(&mut tkhd, true, shown);
        let mut mdhd = t.mdhd.clone();
        patch_duration(&mut mdhd, false, media);

        // Sample tables.
        let stts_runs = runs(picked.iter().map(|s| s.duration));
        let mut stts = (stts_runs.len() as u32).to_be_bytes().to_vec();
        for (n, d) in &stts_runs {
            stts.extend_from_slice(&n.to_be_bytes());
            stts.extend_from_slice(&d.to_be_bytes());
        }
        let mut stbl = vec![t.stsd.clone(), full(b"stts", 0, &stts)?];
        if t.has_ctts {
            let ctts_runs = runs(picked.iter().map(|s| s.cts));
            let signed = ctts_runs.iter().any(|(_, c)| *c < 0);
            let mut ctts = (ctts_runs.len() as u32).to_be_bytes().to_vec();
            for (n, c) in &ctts_runs {
                ctts.extend_from_slice(&n.to_be_bytes());
                let v = if signed { (i32::try_from(*c).unwrap_or(0)) as u32 } else { u32::try_from(*c).unwrap_or(0) };
                ctts.extend_from_slice(&v.to_be_bytes());
            }
            stbl.push(full(b"ctts", u8::from(signed), &ctts)?);
        }
        if t.has_stss {
            let syncs: Vec<u32> = picked.iter().enumerate().filter(|(_, s)| s.sync).map(|(i, _)| i as u32 + 1).collect();
            let mut stss = (syncs.len() as u32).to_be_bytes().to_vec();
            for k in syncs {
                stss.extend_from_slice(&k.to_be_bytes());
            }
            stbl.push(full(b"stss", 0, &stss)?);
        }
        // One chunk per run of samples sharing a description, laid end to end.
        let desc_runs = runs(picked.iter().map(|s| s.desc));
        let mut stsc = (desc_runs.len() as u32).to_be_bytes().to_vec();
        let mut stco = (desc_runs.len() as u32).to_be_bytes().to_vec();
        let mut first = 0usize;
        for (k, (n, desc)) in desc_runs.iter().enumerate() {
            stsc.extend_from_slice(&(k as u32 + 1).to_be_bytes());
            stsc.extend_from_slice(&n.to_be_bytes());
            stsc.extend_from_slice(&desc.to_be_bytes());
            stco.extend_from_slice(&u32::try_from(*at).map_err(|_| ClipError::TooLarge("the trimmed part is too large".into()))?.to_be_bytes());
            let bytes: u64 = picked.get(first..first + *n as usize).unwrap_or_default().iter().map(|s| u64::from(s.size)).sum();
            *at += bytes;
            first += *n as usize;
        }
        let mut stsz = 0u32.to_be_bytes().to_vec();
        stsz.extend_from_slice(&(picked.len() as u32).to_be_bytes());
        for s in picked {
            stsz.extend_from_slice(&s.size.to_be_bytes());
        }
        stbl.push(full(b"stsc", 0, &stsc)?);
        stbl.push(full(b"stsz", 0, &stsz)?);
        stbl.push(full(b"stco", 0, &stco)?);
        let mut minf = t.minf_extra.clone();
        minf.push(bx(b"stbl", &stbl.concat())?);
        let mut mdia = vec![mdhd];
        mdia.extend(t.mdia_extra.iter().cloned());
        mdia.push(bx(b"minf", &minf.concat())?);
        let mut trak = vec![tkhd, edts, bx(b"mdia", &mdia.concat())?];
        trak.extend(t.extra.iter().cloned());
        bx(b"trak", &trak.concat())
    }
}

// --- Matroska --------------------------------------------------------------------------------------

mod mkv {
    use super::*;

    const EBML: u32 = 0x1A45_DFA3;
    const SEGMENT: u32 = 0x1853_8067;
    const INFO: u32 = 0x1549_A966;
    const TRACKS: u32 = 0x1654_AE6B;
    const TAGS: u32 = 0x1254_C367;
    const CLUSTER: u32 = 0x1F43_B675;
    const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
    const DURATION: u32 = 0x4489;
    const TRACK_ENTRY: u32 = 0xAE;
    const TRACK_NUMBER: u32 = 0xD7;
    const TRACK_TYPE: u32 = 0x83;
    const TIMESTAMP: u32 = 0xE7;
    const SIMPLE_BLOCK: u32 = 0xA3;
    const BLOCK_GROUP: u32 = 0xA0;
    const BLOCK: u32 = 0xA1;
    const REFERENCE_BLOCK: u32 = 0xFB;
    /// The segment's own children: an element of unknown size ends where one of these starts.
    const LEVEL1: [u32; 8] = [0x114D_9B74, INFO, TRACKS, CLUSTER, 0x1C53_BB6B, TAGS, 0x1043_A770, 0x1941_A469];

    /// One element's header: its id, where its data starts, and its size (`None`: unknown).
    #[derive(Clone, Copy, Debug)]
    struct Head {
        id: u32,
        data: u64,
        size: Option<u64>,
    }

    /// An EBML variable-length integer at `b[at..]`: its value (the length marker removed unless
    /// `id`), its length, and whether every value bit is set (an unknown size).
    fn vint(b: &[u8], at: usize, id: bool) -> Result<(u64, usize, bool)> {
        let first = *b.get(at).ok_or_else(|| damaged("an element is cut short"))?;
        let len = first.leading_zeros() as usize + 1;
        if len > 8 || (id && len > 4) {
            return Err(damaged("an element's header is not valid"));
        }
        let mut v = if id { u64::from(first) } else { u64::from(first) & (0xFF >> len) };
        for k in 1..len {
            v = (v << 8) | u64::from(*b.get(at + k).ok_or_else(|| damaged("an element is cut short"))?);
        }
        let all = (1u64 << (7 * len)) - 1;
        Ok((v, len, !id && v == all))
    }

    fn head_at(input: &mut Input, at: u64) -> Result<Head> {
        let b = input.read_at(at, (input.len - at).min(12))?;
        let (id, n, _) = vint(&b, 0, true)?;
        let (size, m, unknown) = vint(&b, n, false)?;
        Ok(Head { id: id as u32, data: at + (n + m) as u64, size: (!unknown).then_some(size) })
    }

    /// The children of the element bytes `b` (its data): `(id, data range)` each.
    fn children(b: &[u8], work: &mut Work) -> Result<Vec<(u32, std::ops::Range<usize>)>> {
        let mut out = Vec::new();
        let mut at = 0usize;
        while at < b.len() {
            work.step()?;
            let (id, n, _) = vint(b, at, true)?;
            let (size, m, unknown) = vint(b, at + n, false)?;
            let data = at + n + m;
            let end = if unknown { b.len() } else { usize::try_from(size).ok().and_then(|s| data.checked_add(s)).unwrap_or(usize::MAX) };
            if end > b.len() {
                return Err(damaged("an element runs past its parent"));
            }
            out.push((id as u32, data..end));
            at = end;
        }
        Ok(out)
    }

    fn uint(b: &[u8]) -> u64 {
        b.iter().take(8).fold(0u64, |v, &x| (v << 8) | u64::from(x))
    }

    /// An element: its id's bytes, an 8-byte size, its data.
    fn element(id: u32, data: &[u8]) -> Vec<u8> {
        let mut v: Vec<u8> = id.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
        v.push(0x01);
        v.extend_from_slice(&(data.len() as u64).to_be_bytes()[1..]);
        v.extend_from_slice(data);
        v
    }

    /// One block of a cluster.
    #[derive(Clone, Copy, Debug)]
    struct Block {
        cluster: usize,
        /// The element (header included) in the cluster's data.
        range: (usize, usize),
        /// Where its 2-byte relative time is, from the element's start.
        rel_at: usize,
        track: u64,
        /// Its time, in the segment's timestamp units.
        time: i64,
        key: bool,
    }

    /// The block header of a `Block` or `SimpleBlock`'s data: its track, relative time and flags.
    fn block_head(d: &[u8]) -> Result<(u64, usize, i16, u8)> {
        let (track, n, _) = vint(d, 0, false)?;
        let rel = i16::from_be_bytes([
            *d.get(n).ok_or_else(|| damaged("a block is cut short"))?,
            *d.get(n + 1).ok_or_else(|| damaged("a block is cut short"))?,
        ]);
        let flags = *d.get(n + 2).ok_or_else(|| damaged("a block is cut short"))?;
        Ok((track, n, rel, flags))
    }

    /// One cluster as read: where it is, its timestamp, its data.
    struct Cluster {
        data: (u64, u64),
        time: i64,
    }

    fn read_cluster(input: &mut Input, c: &Cluster) -> Result<Vec<u8>> {
        input.read_at(c.data.0, c.data.1 - c.data.0)
    }

    pub(super) fn cut(input: &mut Input, start_ms: u64, end_ms: u64, work: &mut Work) -> Result<(Vec<u8>, u64, u64)> {
        let header = head_at(input, 0)?;
        if header.id != EBML {
            return Err(damaged("no EBML header"));
        }
        let header_end =
            header.data.checked_add(header.size.ok_or_else(|| damaged("an EBML header of unknown size"))?).ok_or_else(|| damaged("too large"))?;
        let ebml = input.read_at(0, header_end.min(input.len))?;
        let segment = head_at(input, header_end)?;
        if segment.id != SEGMENT {
            return Err(damaged("no segment after the EBML header"));
        }
        let seg_end = segment.size.map_or(input.len, |s| segment.data.saturating_add(s).min(input.len));
        let (mut info, mut tracks, mut tags) = (None, None, None);
        let mut clusters: Vec<Cluster> = Vec::new();
        let mut at = segment.data;
        while at < seg_end {
            work.step()?;
            let h = head_at(input, at)?;
            let end = match h.size {
                Some(s) => h.data.checked_add(s).filter(|&e| e <= seg_end).ok_or_else(|| damaged("an element runs past the segment"))?,
                None => unknown_end(input, h.data, seg_end, work)?,
            };
            let small = |what: &str| {
                if end - h.data > MAX_MOVIE_BYTES {
                    Err(ClipError::TooLarge(format!("{what} larger than {} MB", MAX_MOVIE_BYTES >> 20)))
                } else {
                    Ok(())
                }
            };
            match h.id {
                INFO => {
                    small("the segment's information")?;
                    info = Some(input.read_at(h.data, end - h.data)?);
                }
                TRACKS => {
                    small("the track list")?;
                    tracks = Some(input.read_at(h.data, end - h.data)?);
                }
                TAGS => {
                    small("the tags")?;
                    tags = Some(input.read_at(h.data, end - h.data)?);
                }
                CLUSTER => {
                    if end - h.data > MAX_CLUSTER_BYTES {
                        return Err(ClipError::TooLarge(format!("a cluster larger than {} MB", MAX_CLUSTER_BYTES >> 20)));
                    }
                    clusters.push(Cluster { data: (h.data, end), time: 0 });
                }
                _ => {}
            }
            at = end;
        }
        let info = info.ok_or_else(|| damaged("no segment information"))?;
        let tracks = tracks.ok_or_else(|| damaged("no track list"))?;
        let mut scale = 1_000_000u64;
        let mut info_kept = Vec::new();
        for (id, r) in children(&info, work)? {
            let d = info.get(r).unwrap_or_default();
            match id {
                TIMESTAMP_SCALE => scale = uint(d).max(1),
                DURATION => {}
                _ => info_kept.push((id, d.to_vec())),
            }
        }
        let mut video_track = None;
        for (id, r) in children(&tracks, work)? {
            if id != TRACK_ENTRY {
                continue;
            }
            let entry = tracks.get(r).unwrap_or_default();
            let (mut number, mut kind) = (None, None);
            for (cid, cr) in children(entry, work)? {
                let d = entry.get(cr).unwrap_or_default();
                match cid {
                    TRACK_NUMBER => number = Some(uint(d)),
                    TRACK_TYPE => kind = Some(uint(d)),
                    _ => {}
                }
            }
            if kind == Some(1) && video_track.is_none() {
                video_track = number;
            }
        }
        // Pass 1: every block's time and key flag.
        let mut blocks: Vec<Block> = Vec::new();
        for (ci, c) in clusters.iter_mut().enumerate() {
            let data = read_cluster(input, c)?;
            let kids = children(&data, work)?;
            c.time = kids
                .iter()
                .find(|(id, _)| *id == TIMESTAMP)
                .map_or(0, |(_, r)| i64::try_from(uint(data.get(r.clone()).unwrap_or_default())).unwrap_or(i64::MAX));
            let mut el_start = 0usize;
            for (id, r) in kids {
                let start = el_start;
                el_start = r.end;
                let (body, key_override) = match id {
                    SIMPLE_BLOCK => (r.clone(), None),
                    BLOCK_GROUP => {
                        let group = data.get(r.clone()).unwrap_or_default();
                        let inner = children(group, work)?;
                        let has_ref = inner.iter().any(|(i, _)| *i == REFERENCE_BLOCK);
                        let Some((_, br)) = inner.into_iter().find(|(i, _)| *i == BLOCK) else { continue };
                        (r.start + br.start..r.start + br.end, Some(!has_ref))
                    }
                    _ => continue,
                };
                let d = data.get(body.clone()).ok_or_else(|| damaged("a block lies outside its cluster"))?;
                let (track, n, rel, flags) = block_head(d)?;
                if blocks.len() >= MAX_SAMPLES {
                    return Err(ClipError::TooLarge(format!("more than {MAX_SAMPLES} blocks")));
                }
                blocks.push(Block {
                    cluster: ci,
                    range: (start, r.end),
                    rel_at: body.start - start + n,
                    track,
                    time: c.time.saturating_add(i64::from(rel)),
                    key: key_override.unwrap_or(flags & 0x80 != 0),
                });
            }
        }
        let ms = |t: i64| ms_of(i128::from(t) * i128::from(scale), 1_000_000_000);
        let units = |ms: i128| ms * 1_000_000 / i128::from(scale);
        let video: Vec<usize> = blocks.iter().enumerate().filter(|(_, b)| Some(b.track) == video_track).map(|(i, _)| i).collect();
        let (real_start, real_end, keep_video) = if video.is_empty() {
            (units(i128::from(start_ms)), units(i128::from(end_ms)), None)
        } else {
            let at = |k: usize| blocks.get(k).copied();
            let key = video
                .iter()
                .rposition(|&k| at(k).is_some_and(|b| b.key && ms(b.time) <= i128::from(start_ms)))
                .or_else(|| video.iter().position(|&k| at(k).is_some_and(|b| b.key)))
                .unwrap_or(0);
            let last = video.iter().rposition(|&k| at(k).is_some_and(|b| ms(b.time) < i128::from(end_ms))).filter(|&m| m >= key);
            let Some(last) = last else { return Err(ClipError::Empty { start_ms, end_ms }) };
            let picked: Vec<usize> = video.get(key..=last).unwrap_or_default().to_vec();
            let first = picked.iter().filter_map(|&k| at(k)).map(|b| b.time).min().unwrap_or(0);
            let latest = picked.iter().filter_map(|&k| at(k)).map(|b| b.time).max().unwrap_or(0);
            // The last frame lasts until the next one (or as long as the one before it).
            let next = video.get(last + 1).and_then(|&k| at(k)).map(|b| b.time);
            let before = last.checked_sub(1).and_then(|p| video.get(p)).and_then(|&k| at(k)).map(|b| b.time);
            let end = next.filter(|&n| n > latest).unwrap_or_else(|| latest + before.map_or(1, |b| (latest - b).max(1)));
            (i128::from(first), i128::from(end), Some(picked))
        };
        let keep: Vec<bool> = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| match &keep_video {
                Some(picked) if Some(b.track) == video_track => picked.binary_search(&i).is_ok(),
                _ => i128::from(b.time) >= real_start && i128::from(b.time) < real_end,
            })
            .collect();
        if !keep.iter().any(|k| *k) || real_end <= real_start || ms(i64::try_from(real_end).unwrap_or(i64::MAX)) <= i128::from(start_ms) {
            return Err(ClipError::Empty { start_ms, end_ms });
        }
        let cut_start = i64::try_from(real_start).unwrap_or(0);
        // Pass 2: the clusters holding kept blocks, rewritten.
        let mut body = Vec::new();
        let mut info_new = Vec::new();
        for (id, d) in &info_kept {
            info_new.extend(element(*id, d));
        }
        let length = (real_end - real_start) as f64;
        info_new.extend(element(DURATION, &length.to_be_bytes()));
        body.extend(element(INFO, &info_new));
        body.extend(element(TRACKS, &tracks));
        if let Some(t) = &tags {
            body.extend(element(TAGS, t));
        }
        let mut total = (ebml.len() + body.len()) as u64;
        for (ci, c) in clusters.iter().enumerate() {
            let kept: Vec<&Block> = blocks.iter().zip(&keep).filter(|(b, k)| **k && b.cluster == ci).map(|(b, _)| b).collect();
            if kept.is_empty() {
                continue;
            }
            let data = read_cluster(input, c)?;
            let new_time = c.time.saturating_sub(cut_start).max(0);
            let mut cluster = element(TIMESTAMP, &(new_time as u64).to_be_bytes());
            for b in kept {
                let mut el = data.get(b.range.0..b.range.1).ok_or_else(|| damaged("a block lies outside its cluster"))?.to_vec();
                let rel = b.time.saturating_sub(cut_start).saturating_sub(new_time);
                let rel = i16::try_from(rel).map_err(|_| damaged("a block's time is out of range"))?;
                if let Some(s) = el.get_mut(b.rel_at..b.rel_at + 2) {
                    s.copy_from_slice(&rel.to_be_bytes());
                }
                cluster.extend(el);
            }
            let cluster = element(CLUSTER, &cluster);
            add_bytes(&mut total, cluster.len() as u64)?;
            body.extend(cluster);
        }
        let mut out = ebml;
        out.extend(element(SEGMENT, &body));
        let to_ms = |t: i128| clamp_u64(ms_of(t * i128::from(scale), 1_000_000_000));
        Ok((out, to_ms(real_start), to_ms(real_end)))
    }

    /// Where an element of unknown size that starts its data at `data` ends: at the next element
    /// of the segment's own level, or at the segment's end.
    fn unknown_end(input: &mut Input, data: u64, seg_end: u64, work: &mut Work) -> Result<u64> {
        let mut at = data;
        while at < seg_end {
            work.step()?;
            let h = head_at(input, at)?;
            if LEVEL1.contains(&h.id) {
                return Ok(at);
            }
            let size = h.size.ok_or_else(|| damaged("an element of unknown size inside one"))?;
            at = h.data.checked_add(size).ok_or_else(|| damaged("an element's size is out of range"))?;
        }
        Ok(seg_end.min(at))
    }
}

// --- Save Frame as Picture -------------------------------------------------------------------------

/// The new file's name for a frame of the clip `stem` at `ms`: `<stem> <hh-mm-ss>.jpg`, made safe
/// by `core::names::sanitize` (`IMG_0042 00-01-05.jpg`).
pub fn frame_name(stem: &str, ms: u64) -> String {
    let secs = ms / 1000;
    let base = format!("{stem} {:02}-{:02}-{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    format!("{}.jpg", names::sanitize(&base, "Video Frame"))
}

/// The JPEG of `frame` (upright, as the player shows it) with the video's rotations and flips of
/// `edits` applied, through the one output pipeline: at [`FRAME_QUALITY`], with an EXIF thumbnail
/// and `taken` as its date taken.
pub fn frame_picture(frame: &Rgba8, edits: &[String], taken: Option<DateTaken>) -> output::Result<Vec<u8>> {
    let png = encode::png(&Image::rgba8(frame), &Metadata { exif: None, xmp: None, icc: None })?;
    let moves: Vec<String> = edits.iter().filter(|e| matches!(Op::parse(e), Op::Rotate(_) | Op::FlipH | Op::FlipV)).cloned().collect();
    let recipe = Recipe::parse(&moves)?;
    let mut spec = Spec::new(output::Purpose::Copy);
    spec.format = Choice::Exactly(OutFormat::Jpeg);
    spec.quality = Quality::Fixed(FRAME_QUALITY);
    spec.thumbnail = true;
    spec.limits = Limits::default();
    if let Some(taken) = taken {
        spec.changes = Changes { date_taken: Some(taken.clone()), ..Changes::default() };
        spec.write.metadata_date = Some(taken);
    }
    Ok(output::render(&png, &recipe, &spec)?.bytes)
}

#[cfg(test)]
mod tests {
    use super::frame_name;

    #[test]
    fn a_frame_is_named_after_its_clip_and_time() {
        assert_eq!(frame_name("IMG_0042", 0), "IMG_0042 00-00-00.jpg");
        assert_eq!(frame_name("IMG_0042", 65_999), "IMG_0042 00-01-05.jpg");
        assert_eq!(frame_name("Trip", 3_725_000), "Trip 01-02-05.jpg");
        assert_eq!(frame_name("a/b:c", 1_500), "abc 00-00-01.jpg");
    }
}
