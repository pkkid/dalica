//! A video's frames (reserved decision 2, `docs/video-decoders.md`): one [`FrameSource`] per open
//! clip gives its poster frame, the frame at a time, and the next and previous frames, each
//! upright (the container's rotation applied), scaled to the size asked for and converted to 8-bit
//! sRGB (10-bit and HDR sources tone-mapped, `yuv.rs`).
//!
//! - **Codecs:** H.264 through `altc264` (`h264.rs`), HEVC through `rusty_h265` (`hevc.rs`, only
//!   with the `hevc` feature). Which clip plays is [`super::playability`]'s answer; a clip it
//!   refuses opens as [`FrameError::Unplayable`] with the codec's name.
//! - **Containers:** MP4, M4V and QuickTime through their sample tables (`track.rs`) on
//!   `bmff.rs`'s bounded walk; Matroska through its clusters (`mkv.rs`), each read on its own.
//! - **Seeking:** frames are numbered in display order. The frame at a time is the last one shown
//!   at or before it; reaching it decodes from the sync sample at or before it (once more from the
//!   one before that when the stream's leading pictures need it). Stepping forward continues the
//!   decode; stepping back decodes again from the sync sample, the last [`LOOK_BEHIND`] frames
//!   kept so a step back after a step forward costs nothing.
//! - **Memory** is bounded by the frame size and a fixed look-ahead: the coded frame size of every
//!   sequence parameter set is checked against [`MAX_FRAME_SIDE`], [`MAX_FRAME_PIXELS`] and the
//!   caller's [`Limits`] before the decoder sees it, and the pictures a decoder may hold (its
//!   largest reference buffer, [`HELD_PICTURES`], plus [`PICTURES_PER_THREAD`]) with the converted frames
//!   kept must fit [`Limits::max_alloc`]; samples are read one at a time, each at most
//!   [`MAX_SAMPLE_BYTES`]; a track holds at most [`MAX_SAMPLES`].
//! - **Hostile input** (`security.md` §4.2): every length comes from the file and is checked
//!   where it is read; a third-party decoder's panic is caught ([`Error::Panic`]) and the decoder
//!   made again for the next request; a damaged stream decodes what it can (the decoders conceal
//!   damaged slices) and a frame it cannot produce is an error with its reason.

mod h264;
#[cfg(feature = "hevc")]
mod hevc;
pub(super) mod mkv;
pub(super) mod track;
pub mod yuv;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use track::{Config, Track};
pub use track::{MAX_SAMPLE_BYTES, MAX_SAMPLES};

use super::{Unplayable, VideoCodec, VideoInfo, Why, mp4, playability, probe_as, recognise};
use crate::limits::{Budget, Limits, Work};
use crate::orient::{self, Orientation};
use crate::sniff::{Format, VideoKind};
use crate::{Error, Result, Rgba8, resize};

/// Longest side of a coded frame Dalica decodes.
pub const MAX_FRAME_SIDE: u32 = 8192;

/// Most pixels in a coded frame Dalica decodes (8K UHD, coded in whole blocks).
pub const MAX_FRAME_PIXELS: u64 = 8192 * 4352;

/// Pictures a decoder may hold besides the threads' own: the largest reference buffer the
/// standards allow (16), the picture being decoded, and the frames waiting to be shown.
pub const HELD_PICTURES: u64 = 16 + 4;

/// Pictures each decoding thread may add (frame threading keeps several in flight; measured on a
/// 4K clip with 8 threads, `docs/video-decoders.md` §6).
pub const PICTURES_PER_THREAD: u64 = 8;

/// Frames kept after they were returned, for a step back.
pub const LOOK_BEHIND: usize = 4;

/// Most decoding threads a caller may ask for.
pub const MAX_THREADS: usize = 8;

/// The poster is the first frame whose mean luma (0 black to 1 white) clears this...
pub const POSTER_LUMA: f32 = 0.06;

/// ...within this many milliseconds of the start; else the first frame.
pub const POSTER_WINDOW_MS: u64 = 10_000;

/// How a [`FrameSource`] decodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameOptions {
    /// The longest side of the frames returned, upright (`None`: the clip's own size). Frames are
    /// never made larger than the clip.
    pub max_side: Option<u32>,
    /// Decoding threads besides the caller's (0: decode on the calling thread). H.264's decoder
    /// and the colour conversion use them; at most [`MAX_THREADS`].
    pub threads: usize,
}

/// One frame, upright, 8-bit sRGB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub image: Rgba8,
    /// Its number in display order, from 0.
    pub index: usize,
    /// When it is shown, in milliseconds from the start.
    pub time_ms: u64,
}

/// Why a clip gives no frame.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// Dalica cannot play this clip's video (its codec, or a profile its decoder refuses).
    #[error("{}", .0.reason())]
    Unplayable(Unplayable),
    /// The file is damaged, cut, too large or unreadable.
    #[error(transparent)]
    Image(#[from] Error),
}

impl FrameError {
    /// The text the clip's tile and the edit view show.
    pub fn reason(&self) -> String {
        match self {
            FrameError::Unplayable(u) => u.reason(),
            FrameError::Image(e) => e.reason(),
        }
    }
}

type FrameResult<T> = std::result::Result<T, FrameError>;

/// Where a clip's bytes come from: a file read sample by sample, or bytes in memory.
pub(super) enum Input {
    File { file: File, len: u64 },
    Bytes(Arc<[u8]>),
}

impl Input {
    pub(super) fn len(&self) -> u64 {
        match self {
            Input::File { len, .. } => *len,
            Input::Bytes(b) => b.len() as u64,
        }
    }

    /// `size` bytes at `offset` into `buf`; past the end of the file is [`Error::Truncated`].
    pub(super) fn read_at(&mut self, offset: u64, size: u32, buf: &mut Vec<u8>) -> Result<()> {
        if size > MAX_SAMPLE_BYTES {
            return Err(Error::TooMany("bytes in one video sample"));
        }
        let end = offset.checked_add(u64::from(size)).ok_or(Error::Truncated)?;
        if end > self.len() {
            return Err(Error::Truncated);
        }
        buf.clear();
        match self {
            Input::Bytes(b) => {
                let range = usize::try_from(offset).ok().zip(usize::try_from(end).ok()).ok_or(Error::Truncated)?;
                buf.extend_from_slice(b.get(range.0..range.1).ok_or(Error::Truncated)?);
            }
            Input::File { file, .. } => {
                let io = |e: std::io::Error| Error::Io(e.to_string());
                file.seek(SeekFrom::Start(offset)).map_err(io)?;
                buf.try_reserve_exact(size as usize).map_err(|_| Error::TooMany("bytes in one video sample"))?;
                file.by_ref().take(u64::from(size)).read_to_end(buf).map_err(io)?;
                if buf.len() != size as usize {
                    return Err(Error::Truncated);
                }
            }
        }
        Ok(())
    }

    /// The `ftyp` and `moov` boxes (an ISO BMFF file).
    pub(super) fn movie(&mut self, limits: &Limits) -> Result<Vec<u8>> {
        match self {
            Input::File { file, len } => mp4::read_movie(file, *len, limits),
            Input::Bytes(b) => mp4::read_movie(&mut std::io::Cursor::new(&b[..]), b.len() as u64, limits),
        }
    }
}

/// The frame limits every sequence parameter set is checked against before a decoder sees it.
pub(super) struct Gate {
    limits: Limits,
    /// Bytes per pixel of a decoder picture's samples (HEVC's decoder keeps 16 bits for every depth).
    wide_samples: bool,
    /// Bytes the converted frames take besides the decoder's pictures, per pixel.
    pub(super) largest: (u32, u32),
}

impl Gate {
    fn new(limits: Limits, wide_samples: bool) -> Gate {
        Gate { limits, wide_samples, largest: (0, 0) }
    }

    /// Checks a coded frame of `width × height` at `depth` bits, its chroma planes adding
    /// `chroma_halves` halves of a luma plane, decoded with `threads` threads.
    pub(super) fn admit(&mut self, width: u32, height: u32, depth: u32, chroma_halves: u64, threads: usize) -> Result<()> {
        if width == 0 || height == 0 {
            return Err(Error::Malformed(format!("a video frame of {width} × {height}")));
        }
        if !(8..=16).contains(&depth) {
            return Err(Error::Malformed(format!("video samples of {depth} bits")));
        }
        let too_large = Error::TooLarge { width, height };
        let pixels = u64::from(width) * u64::from(height);
        if width > MAX_FRAME_SIDE.min(self.limits.max_side)
            || height > MAX_FRAME_SIDE.min(self.limits.max_side)
            || pixels > MAX_FRAME_PIXELS.min(self.limits.max_pixels)
        {
            return Err(too_large);
        }
        let bytes = if self.wide_samples || depth > 8 { 2 } else { 1 };
        let picture = pixels * bytes * (2 + chroma_halves) / 2;
        // HEVC pictures carry motion fields and per-block state besides their samples.
        let picture = if self.wide_samples { picture + picture / 4 } else { picture };
        let held = HELD_PICTURES + PICTURES_PER_THREAD * threads as u64;
        let frames = pixels * 4 * (LOOK_BEHIND as u64 + 2);
        let total = picture.checked_mul(held).and_then(|p| p.checked_add(frames));
        if total.is_none_or(|t| t > self.limits.max_alloc) {
            return Err(too_large);
        }
        if pixels > u64::from(self.largest.0) * u64::from(self.largest.1) {
            self.largest = (width, height);
        }
        Ok(())
    }
}

/// The decoder of a clip's codec.
enum Codec {
    H264(Box<h264::H264>),
    #[cfg(feature = "hevc")]
    Hevc(Box<hevc::Hevc>),
}

/// A decoded frame, before conversion.
enum Raw {
    H264(altc264::decoder::Frame),
    #[cfg(feature = "hevc")]
    Hevc(rusty_h265::Frame, yuv::Colour),
}

impl Raw {
    fn pts(&self) -> Option<i64> {
        match self {
            Raw::H264(f) => f.pts,
            #[cfg(feature = "hevc")]
            Raw::Hevc(f, _) => f.pts,
        }
    }

    fn picture(&self) -> Option<yuv::Picture<'_>> {
        match self {
            Raw::H264(f) => h264::picture(f),
            #[cfg(feature = "hevc")]
            Raw::Hevc(f, colour) => hevc::picture(f, *colour),
        }
    }
}

/// The frames of one open clip.
pub struct FrameSource {
    info: VideoInfo,
    codec_kind: VideoCodec,
    input: Input,
    track: Track,
    /// Decode index of each frame, in display order.
    display: Vec<u32>,
    /// Display number of each sample, in decode order (`u32::MAX`: never shown).
    rank: Vec<u32>,
    /// Decode indices of the sync samples, ascending.
    syncs: Vec<u32>,
    decoder: Option<Codec>,
    gate: Gate,
    options: FrameOptions,
    limits: Limits,
    /// The next sample to feed, in decode order.
    fed: usize,
    flushed: bool,
    /// The last frame the decoder gave in its current run (frames come in display order).
    decoded: Option<usize>,
    /// The last frame returned.
    last: Option<usize>,
    behind: VecDeque<Frame>,
    buf: Vec<u8>,
    converter: yuv::Converter,
}

const HIDDEN: u32 = u32::MAX;

impl FrameSource {
    /// Opens the clip at `path`: its facts and its video track's sample table are read, its codec
    /// checked, and its decoder made (the configuration's parameter sets checked by the gate).
    pub fn open(path: &Path, limits: &Limits, options: FrameOptions) -> FrameResult<FrameSource> {
        let io = |e: std::io::Error| Error::Io(e.to_string());
        let file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        FrameSource::with_input(Input::File { file, len }, limits, options)
    }

    /// As [`FrameSource::open`], for a clip in memory.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>, limits: &Limits, options: FrameOptions) -> FrameResult<FrameSource> {
        FrameSource::with_input(Input::Bytes(bytes.into()), limits, options)
    }

    fn with_input(mut input: Input, limits: &Limits, options: FrameOptions) -> FrameResult<FrameSource> {
        let mut head = Vec::new();
        let first = input.len().min(crate::sniff::SNIFF_LEN as u64 * 2) as u32;
        input.read_at(0, first, &mut head)?;
        let kind = recognise(&head).ok_or(if head.len() < crate::sniff::SNIFF_LEN { Error::Truncated } else { Error::Unknown })?;
        let mut work = Work::new(super::MAX_STEPS, "boxes or elements in a video");
        let (info, track) = match kind {
            VideoKind::Matroska => {
                let (info, track) = mkv::open(&mut input, b"vide", limits, &mut work)?;
                (info, track.ok_or(FrameError::Unplayable(Unplayable { codec: String::new(), why: Why::NoVideo }))?)
            }
            _ => {
                let movie = input.movie(limits)?;
                let info = probe_as(kind, &movie, limits)?;
                let track = track::find(&movie, b"vide", &mut work)?;
                (info, track.ok_or(FrameError::Unplayable(Unplayable { codec: String::new(), why: Why::NoVideo }))?)
            }
        };
        let codec_kind = playability(&info).video.map_err(FrameError::Unplayable)?;
        let options = FrameOptions { threads: options.threads.min(MAX_THREADS), ..options };
        let mut source = FrameSource {
            gate: Gate::new(*limits, codec_kind == VideoCodec::Hevc),
            codec_kind,
            info,
            input,
            display: Vec::new(),
            rank: Vec::new(),
            syncs: Vec::new(),
            decoder: None,
            options,
            limits: *limits,
            fed: 0,
            flushed: false,
            decoded: None,
            last: None,
            behind: VecDeque::new(),
            buf: Vec::new(),
            converter: yuv::Converter::default(),
            track,
        };
        source.index()?;
        source.decoder = Some(source.make_decoder()?);
        Ok(source)
    }

    /// Display order, and the sync samples.
    fn index(&mut self) -> Result<()> {
        let samples = &self.track.samples;
        if samples.is_empty() {
            return Err(Error::Malformed("a video track without frames".into()));
        }
        let mut order: Vec<u32> = (0..samples.len() as u32).collect();
        order.sort_by_key(|&i| (samples.get(i as usize).map_or(0, |s| s.pts), i));
        // Frames before the start the edit list sets are decoded, never shown (unless all are).
        let shown = |i: &u32| samples.get(*i as usize).is_some_and(|s| s.pts >= 0);
        if order.iter().any(shown) {
            order.retain(shown);
        }
        let mut rank = vec![HIDDEN; samples.len()];
        for (d, &i) in order.iter().enumerate() {
            if let Some(r) = rank.get_mut(i as usize) {
                *r = d as u32;
            }
        }
        self.syncs = samples.iter().enumerate().filter(|(_, s)| s.sync).map(|(i, _)| i as u32).collect();
        if self.syncs.first() != Some(&0) {
            // A decoder can always try the first sample.
            self.syncs.insert(0, 0);
        }
        self.display = order;
        self.rank = rank;
        Ok(())
    }

    fn make_decoder(&mut self) -> FrameResult<Codec> {
        match (&self.track.config, self.codec_kind) {
            (Config::Avc { length_size, params }, VideoCodec::H264) => {
                Ok(Codec::H264(Box::new(h264::H264::new(*length_size, params, self.options.threads, self.info.kind, &mut self.gate)?)))
            }
            #[cfg(feature = "hevc")]
            (Config::Hevc { length_size, params }, VideoCodec::Hevc) => {
                hevc::Hevc::new(*length_size, params, self.info.kind, &mut self.gate).map(|h| Codec::Hevc(Box::new(h))).map_err(|r| self.refusal(r))
            }
            _ => Err(FrameError::Image(Error::Malformed(format!("a {} track without its decoder configuration", self.track.code)))),
        }
    }

    #[cfg(feature = "hevc")]
    fn refusal(&self, r: hevc::Refusal) -> FrameError {
        match r {
            hevc::Refusal::Unsupported(what) => FrameError::Unplayable(Unplayable { codec: "HEVC".into(), why: Why::Unsupported(what) }),
            hevc::Refusal::Error(e) => FrameError::Image(e),
        }
    }

    /// The clip's facts.
    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// Frames in display order.
    pub fn frame_count(&self) -> usize {
        self.display.len()
    }

    /// The display time of frame `index`, in milliseconds.
    pub fn time_of(&self, index: usize) -> Option<u64> {
        let s = self.track.samples.get(*self.display.get(index)? as usize)?;
        Some(self.track.millis(s.pts).max(0) as u64)
    }

    /// The frame shown at `ms` (the last one at or before it; the first before the first).
    pub fn index_at(&self, ms: u64) -> usize {
        let after = self.display.partition_point(|&i| self.track.samples.get(i as usize).is_some_and(|s| self.track.millis(s.pts) <= ms as i64));
        after.saturating_sub(1)
    }

    /// The first frame at or after `index` that decoding can start at (a sync sample's), in
    /// display order: asking [`FrameSource::frame`] for it decodes from there, not from the sync
    /// sample before. `None` when no sync sample's frame lies at or after `index`.
    pub fn key_after(&self, index: usize) -> Option<usize> {
        self.syncs
            .iter()
            .filter_map(|&s| self.rank.get(s as usize).copied())
            .filter(|&r| r != HIDDEN && r as usize >= index)
            .min()
            .map(|r| r as usize)
    }

    /// The poster frame: the first whose mean luma clears [`POSTER_LUMA`] within the first
    /// [`POSTER_WINDOW_MS`], else the first frame.
    pub fn poster(&mut self) -> FrameResult<Frame> {
        let (d, raw) = self.seek_raw(0)?;
        let first = (d, raw);
        if self.lit(&first.1) {
            return self.finish(first.0, &first.1);
        }
        while let Some((d, raw)) = self.next_raw()? {
            if self.time_of(d).is_none_or(|t| t > POSTER_WINDOW_MS) {
                break;
            }
            if self.lit(&raw) {
                return self.finish(d, &raw);
            }
        }
        self.finish(first.0, &first.1)
    }

    fn lit(&self, raw: &Raw) -> bool {
        raw.picture().is_some_and(|p| p.mean_luma() >= POSTER_LUMA)
    }

    /// The frame shown at `ms`.
    pub fn frame_at(&mut self, ms: u64) -> FrameResult<Frame> {
        let target = self.index_at(ms);
        self.frame(target)
    }

    /// Frame `index` in display order.
    pub fn frame(&mut self, index: usize) -> FrameResult<Frame> {
        if index >= self.display.len() {
            return Err(Error::Malformed(format!("frame {index} of a clip of {}", self.display.len())).into());
        }
        if let Some(f) = self.behind.iter().find(|f| f.index == index) {
            let f = f.clone();
            self.last = Some(index);
            return Ok(f);
        }
        // Going on from where the decoder stands is cheaper than a seek when no sync sample lies
        // between the two.
        if let Some(cur) = self.decoded
            && cur < index
            && self.sync_before(index) <= self.decode_index(cur)
        {
            while let Some((d, raw)) = self.next_raw()? {
                if d >= index {
                    return self.finish(d, &raw);
                }
            }
            return Err(self.missing(index));
        }
        let (d, raw) = self.seek_raw(index)?;
        self.finish(d, &raw)
    }

    /// The frame after the one returned last (the first frame if none was), or `None` at the end.
    pub fn next_frame(&mut self) -> FrameResult<Option<Frame>> {
        let index = self.last.map_or(0, |c| c + 1);
        if index >= self.display.len() {
            return Ok(None);
        }
        self.frame(index).map(Some)
    }

    /// The frame before the one returned last, or `None` at the start.
    pub fn previous_frame(&mut self) -> FrameResult<Option<Frame>> {
        let Some(index) = self.current_index().and_then(|c| c.checked_sub(1)) else { return Ok(None) };
        self.frame(index).map(Some)
    }

    /// The frame returned last.
    pub fn current_index(&self) -> Option<usize> {
        self.last
    }

    fn decode_index(&self, display: usize) -> usize {
        self.display.get(display).map_or(0, |&i| i as usize)
    }

    /// The sync sample to start at for frame `display`.
    fn sync_before(&self, display: usize) -> usize {
        let k = self.decode_index(display) as u32;
        let at = self.syncs.partition_point(|&s| s <= k);
        self.syncs.get(at.saturating_sub(1)).map_or(0, |&s| s as usize)
    }

    fn missing(&self, index: usize) -> FrameError {
        let detail = match &self.decoder {
            Some(Codec::H264(h)) => h.last_error(),
            _ => None,
        };
        match detail {
            Some(e) if e.starts_with("unsupported") => FrameError::Unplayable(Unplayable { codec: "H.264".into(), why: Why::Unsupported(e) }),
            _ => Error::Malformed(format!("frame {index} of the clip could not be decoded")).into(),
        }
    }

    /// Decodes from the sync sample before frame `index` to it (or the first frame after it, when
    /// the stream lost it).
    fn seek_raw(&mut self, index: usize) -> FrameResult<(usize, Raw)> {
        let mut start = self.sync_before(index);
        for attempt in 0..2 {
            self.restart(start)?;
            let mut after = None;
            while let Some((d, raw)) = self.next_raw()? {
                if d == index {
                    return Ok((d, raw));
                }
                if d > index {
                    after = Some((d, raw));
                    break;
                }
            }
            // Leading pictures of an open GOP need the sync sample before.
            let earlier = self.syncs.partition_point(|&s| (s as usize) < start).checked_sub(1).and_then(|i| self.syncs.get(i));
            match (after, earlier) {
                (Some(found), _) if attempt == 1 => return Ok(found),
                (Some(found), None) => return Ok(found),
                (_, Some(&s)) if attempt == 0 => start = s as usize,
                (Some(found), _) => return Ok(found),
                (None, _) => return Err(self.missing(index)),
            }
        }
        Err(self.missing(index))
    }

    /// Starts decoding again at decode index `start`.
    fn restart(&mut self, start: usize) -> FrameResult<()> {
        self.decoded = None;
        match self.decoder.as_mut() {
            Some(Codec::H264(h)) => h.reset()?,
            #[cfg(feature = "hevc")]
            Some(Codec::Hevc(h)) => {
                let r = h.reset(&mut self.gate);
                if let Err(r) = r {
                    return Err(self.refusal(r));
                }
            }
            None => self.decoder = Some(self.make_decoder()?),
        }
        self.fed = start;
        self.flushed = false;
        Ok(())
    }

    /// The next frame out of the decoder, in display order, feeding it as needed; `None` once the
    /// stream is drained. A panic in the decoder drops it (made again at the next seek).
    fn next_raw(&mut self) -> FrameResult<Option<(usize, Raw)>> {
        let result = self.next_raw_inner();
        if matches!(result, Err(FrameError::Image(Error::Panic(_)))) {
            self.decoder = None;
            self.decoded = None;
        }
        result
    }

    fn next_raw_inner(&mut self) -> FrameResult<Option<(usize, Raw)>> {
        let total = self.track.samples.len();
        // Every pass either takes a frame or feeds a sample, and both are finite.
        loop {
            let Some(decoder) = self.decoder.as_mut() else { return Err(Error::Malformed("the video decoder failed".into()).into()) };
            let raw = match decoder {
                Codec::H264(h) => h.pull()?.map(Raw::H264),
                #[cfg(feature = "hevc")]
                Codec::Hevc(h) => {
                    let colour = h.colour();
                    h.pull()?.map(|f| Raw::Hevc(f, colour))
                }
            };
            if let Some(raw) = raw {
                let d = raw.pts().and_then(|p| usize::try_from(p).ok()).and_then(|i| self.rank.get(i)).copied().unwrap_or(HIDDEN);
                if d != HIDDEN {
                    self.decoded = Some(d as usize);
                    return Ok(Some((d as usize, raw)));
                }
                continue;
            }
            if self.fed < total {
                let i = self.fed;
                self.fed += 1;
                let Some(&sample) = self.track.samples.get(i) else { continue };
                self.input.read_at(sample.offset, sample.size, &mut self.buf)?;
                let pts = i as i64;
                match decoder {
                    Codec::H264(h) => h.push(&self.buf, pts, &mut self.gate)?,
                    #[cfg(feature = "hevc")]
                    Codec::Hevc(h) => {
                        if let Err(r) = h.push(&self.buf, pts, &mut self.gate) {
                            return Err(self.refusal(r));
                        }
                    }
                }
                continue;
            }
            if !self.flushed {
                self.flushed = true;
                match decoder {
                    Codec::H264(h) => h.flush()?,
                    #[cfg(feature = "hevc")]
                    Codec::Hevc(h) => h.flush()?,
                }
                continue;
            }
            return Ok(None);
        }
    }

    /// Converts frame `d`, upright and at the size asked for, and remembers it.
    fn finish(&mut self, d: usize, raw: &Raw) -> FrameResult<Frame> {
        let pic = raw.picture().ok_or_else(|| Error::Malformed("a decoded frame without its planes".into()))?;
        let (w, h) = (pic.width(), pic.height());
        let turns = self.info.rotation % 4;
        let (ow, oh) = match self.options.max_side {
            Some(side) if side > 0 && w.max(h) > side => {
                let scale = f64::from(side) / f64::from(w.max(h));
                (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1))
            }
            _ => (w, h),
        };
        let mut budget = Budget::new(&self.limits, ow, oh);
        let mut image = self.converter.convert_on(&pic, ow, oh, &mut budget, self.options.threads)?;
        // An odd target the averaging could not reach exactly is fitted afterwards.
        if (image.width, image.height) != (ow, oh) {
            image = resize::resize(&image, ow, oh)?;
        }
        let orientation = Orientation::new(match turns {
            1 => 6,
            2 => 3,
            3 => 8,
            _ => 1,
        });
        if let Some(o) = orientation.filter(|o| o.get() != 1) {
            image = orient::apply(image, o)?;
        }
        let frame = Frame { image, index: d, time_ms: self.time_of(d).unwrap_or(0) };
        self.last = Some(d);
        if self.behind.len() >= LOOK_BEHIND {
            self.behind.pop_front();
        }
        self.behind.retain(|f| f.index != d);
        self.behind.push_back(frame.clone());
        Ok(frame)
    }

    /// The video's format.
    pub fn format(&self) -> Format {
        Format::Video(self.info.kind)
    }

    /// The largest coded frame size the gate admitted so far.
    pub fn coded_size(&self) -> (u32, u32) {
        self.gate.largest
    }
}
