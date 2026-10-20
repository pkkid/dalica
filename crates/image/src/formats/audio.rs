//! Slideshow music as samples (`viewing.md` §1.5; reserved decision 2, `docs/video-decoders.md`
//! §7.3, M15.27): MP3 through `nanomp3`, Ogg Vorbis through `lewton`, FLAC through `claxon` and WAV
//! through `hound`, the four pure-Rust decoders M15.46's record passes.
//!
//! [`Music::open`] knows a file by its first bytes, never its name ([`sniff`]); a file of another
//! kind, or one its decoder refuses, is answered with [`MusicError`], whose reason starts with
//! "This music file can't be played" and says why in brackets. Samples come out as interleaved
//! `f32` in −1 ..= 1 at the file's rate, a decoder's block at a time ([`Music::next_chunk`]), or up
//! to a number of frames at once ([`Music::read_frames`]).
//!
//! Every count is hostile (`security.md` §4.2): a file larger than [`MAX_MUSIC_BYTES`] is refused
//! before it is read; at most [`MAX_CHANNELS`] channels, a rate in [`RATES`], and at most
//! [`MAX_CHUNK_FRAMES`] frames in one block; the decoders read the file as a stream (never the
//! whole file into memory), and a panic in one is caught and answered as a damaged file. A
//! decoder that failed once is not asked again: the file ends there.

use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

/// What every refusal starts with (`viewing.md` §1.5).
pub const CANNOT_PLAY: &str = "This music file can't be played";

/// The largest music file read (a long DJ mix as a 24-bit WAV fits).
pub const MAX_MUSIC_BYTES: u64 = 2 << 30;

/// Most channels played (7.1).
pub const MAX_CHANNELS: u16 = 8;

/// The rates music plays at, in Hz.
pub const RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// Most frames (samples per channel) one block may hold (FLAC's largest block).
pub const MAX_CHUNK_FRAMES: usize = 65_535;

/// The frames a WAV block is read in.
const WAV_BLOCK: usize = 4_096;

/// The file extensions of music, lowercase (what the music folder plays).
pub const MUSIC_EXTENSIONS: [&str; 5] = ["mp3", "ogg", "oga", "flac", "wav"];

/// Whether a file named `name` is music by its extension (the music folder's list; the file is
/// still known by its bytes when it opens).
pub fn is_music_name(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    MUSIC_EXTENSIONS.contains(&ext.as_str())
}

/// The kinds of music Dalica plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MusicKind {
    Mp3,
    Vorbis,
    Flac,
    Wav,
}

impl MusicKind {
    /// The kind's name, as a reason says it.
    pub fn name(self) -> &'static str {
        match self {
            MusicKind::Mp3 => "MP3",
            MusicKind::Vorbis => "Ogg Vorbis",
            MusicKind::Flac => "FLAC",
            MusicKind::Wav => "WAV",
        }
    }
}

/// The kind of music `head` (a file's first bytes, at least 12 of them) starts, if any.
pub fn sniff(head: &[u8]) -> Option<MusicKind> {
    match head {
        [b'I', b'D', b'3', ..] => Some(MusicKind::Mp3),
        [b'O', b'g', b'g', b'S', ..] => Some(MusicKind::Vorbis),
        [b'f', b'L', b'a', b'C', ..] => Some(MusicKind::Flac),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'A', b'V', b'E', ..] => Some(MusicKind::Wav),
        // An MPEG audio frame's sync word, layer III.
        [0xff, b, ..] if b & 0xe0 == 0xe0 && b & 0x06 == 0x02 => Some(MusicKind::Mp3),
        _ => None,
    }
}

/// Why a music file does not play.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct MusicError {
    /// "This music file can't be played (…)".
    pub reason: String,
}

impl MusicError {
    fn because(detail: impl std::fmt::Display) -> MusicError {
        MusicError { reason: format!("{CANNOT_PLAY} ({detail})") }
    }

    fn damaged(kind: MusicKind) -> MusicError {
        MusicError::because(format_args!("the {} data is damaged", kind.name()))
    }
}

/// A stream the decoders read.
trait Input: Read + Seek + Send {}
impl<T: Read + Seek + Send> Input for T {}

type Source = Box<dyn Input>;

enum Decoder {
    Mp3(Box<nanomp3::Reader<Source, f32>>),
    Vorbis(Box<lewton::inside_ogg::OggStreamReader<Source>>),
    Flac { reader: Box<claxon::FlacReader<Source>>, bits: u32, buffer: Vec<i32> },
    Wav { reader: Box<hound::WavReader<Source>>, float: bool, bits: u16 },
}

/// One open music file.
pub struct Music {
    kind: MusicKind,
    decoder: Option<Decoder>,
    rate: u32,
    channels: u16,
    /// Its length in frames, when the file says.
    frames: Option<u64>,
    /// Why it ended early, if a decoder failed.
    failed: Option<MusicError>,
    /// Samples decoded and not handed out yet ([`Music::read_frames`] stopped inside a block).
    pending: Vec<f32>,
}

impl std::fmt::Debug for Music {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Music").field("kind", &self.kind).field("rate", &self.rate).field("channels", &self.channels).finish_non_exhaustive()
    }
}

/// Runs a decoder call, a panic answered as `None`.
fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

/// Checks the layout a header declares.
fn layout(rate: u32, channels: u16) -> Result<(u32, u16), MusicError> {
    if channels == 0 || channels > MAX_CHANNELS || !RATES.contains(&rate) {
        return Err(MusicError::because(format_args!("{channels} channels at {rate} Hz")));
    }
    Ok((rate, channels))
}

impl Music {
    /// Opens the music file at `path` (its size checked first, its decoder by its bytes).
    pub fn open(path: &Path) -> Result<Music, MusicError> {
        let io = |e: std::io::Error| MusicError::because(format_args!("it could not be read: {e}"));
        let mut file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        if len > MAX_MUSIC_BYTES {
            return Err(MusicError::because("the file is too large"));
        }
        let mut head = [0u8; 12];
        let n = read_head(&mut file, &mut head).map_err(io)?;
        file.rewind().map_err(io)?;
        let kind = sniff(head.get(..n).unwrap_or_default()).ok_or_else(|| MusicError::because("not MP3, Ogg Vorbis, FLAC or WAV"))?;
        Music::with_source(kind, Box::new(BufReader::new(file)))
    }

    /// As [`Music::open`], for a file in memory.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Music, MusicError> {
        if bytes.len() as u64 > MAX_MUSIC_BYTES {
            return Err(MusicError::because("the file is too large"));
        }
        let kind = sniff(&bytes).ok_or_else(|| MusicError::because("not MP3, Ogg Vorbis, FLAC or WAV"))?;
        Music::with_source(kind, Box::new(Cursor::new(bytes)))
    }

    fn with_source(kind: MusicKind, source: Source) -> Result<Music, MusicError> {
        let damaged = || MusicError::damaged(kind);
        let (decoder, rate, channels, frames) = match kind {
            MusicKind::Mp3 => {
                let r = guard(|| nanomp3::Reader::<Source, f32>::new(source)).ok_or_else(damaged)?.map_err(|_| damaged())?;
                let channels = r.channels().map(|c| u16::from(c.num())).ok_or_else(|| MusicError::because("the MP3 file holds no sound"))?;
                let (rate, channels) = layout(r.sample_rate(), channels)?;
                let frames = r.total_samples();
                (Decoder::Mp3(Box::new(r)), rate, channels, frames)
            }
            MusicKind::Vorbis => {
                let r = guard(|| lewton::inside_ogg::OggStreamReader::new(source))
                    .ok_or_else(damaged)?
                    .map_err(|_| MusicError::because("not an Ogg Vorbis stream, or a damaged one"))?;
                let (rate, channels) = layout(r.ident_hdr.audio_sample_rate, u16::from(r.ident_hdr.audio_channels))?;
                (Decoder::Vorbis(Box::new(r)), rate, channels, None)
            }
            MusicKind::Flac => {
                let r = guard(|| claxon::FlacReader::new(source)).ok_or_else(damaged)?.map_err(|_| damaged())?;
                let info = r.streaminfo();
                let channels = u16::try_from(info.channels).unwrap_or(u16::MAX);
                let (rate, channels) = layout(info.sample_rate, channels)?;
                if !(4..=32).contains(&info.bits_per_sample) {
                    return Err(MusicError::because(format_args!("{}-bit FLAC", info.bits_per_sample)));
                }
                let bits = info.bits_per_sample;
                (Decoder::Flac { reader: Box::new(r), bits, buffer: Vec::new() }, rate, channels, info.samples)
            }
            MusicKind::Wav => {
                let r = guard(|| hound::WavReader::new(source)).ok_or_else(damaged)?.map_err(|_| damaged())?;
                let spec = r.spec();
                let (rate, channels) = layout(spec.sample_rate, spec.channels)?;
                let float = spec.sample_format == hound::SampleFormat::Float;
                let bits = spec.bits_per_sample;
                let ok = if float { bits == 32 } else { (1..=32).contains(&bits) };
                if !ok {
                    return Err(MusicError::because(format_args!("{bits}-bit WAV")));
                }
                let frames = Some(u64::from(r.duration()));
                (Decoder::Wav { reader: Box::new(r), float, bits }, rate, channels, frames)
            }
        };
        Ok(Music { kind, decoder: Some(decoder), rate, channels, frames, failed: None, pending: Vec::new() })
    }

    pub fn kind(&self) -> MusicKind {
        self.kind
    }

    /// Frames a second.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Samples a frame.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The file's length in frames, when its header says (MP3, FLAC, WAV).
    pub fn frames(&self) -> Option<u64> {
        self.frames
    }

    /// Why the file ended early, when its decoder failed partway (the samples before stand).
    pub fn failed(&self) -> Option<&MusicError> {
        self.failed.as_ref()
    }

    /// The next block of interleaved samples (a whole number of frames, never empty), or `None`
    /// at the end of the file. A decoder that fails partway answers its reason once; the file then
    /// ends.
    pub fn next_chunk(&mut self) -> Result<Option<Vec<f32>>, MusicError> {
        let channels = usize::from(self.channels);
        if !self.pending.is_empty() {
            return Ok(Some(std::mem::take(&mut self.pending)));
        }
        loop {
            let Some(decoder) = self.decoder.as_mut() else { return Ok(None) };
            let kind = self.kind;
            let got = guard(|| decode(decoder, channels)).unwrap_or_else(|| Err(MusicError::damaged(kind)));
            match got {
                Ok(Some(samples)) if samples.is_empty() => continue,
                Ok(Some(mut samples)) => {
                    if samples.len() > MAX_CHUNK_FRAMES.saturating_mul(channels) {
                        return self.fail(MusicError::damaged(kind));
                    }
                    samples.truncate(samples.len() / channels * channels);
                    if samples.is_empty() {
                        continue;
                    }
                    return Ok(Some(samples));
                }
                Ok(None) => {
                    self.decoder = None;
                    return Ok(None);
                }
                Err(e) => return self.fail(e),
            }
        }
    }

    fn fail(&mut self, e: MusicError) -> Result<Option<Vec<f32>>, MusicError> {
        self.decoder = None;
        self.failed = Some(e.clone());
        Err(e)
    }

    /// Up to `max_frames` frames of interleaved samples from where the file is, fewer at its end.
    pub fn read_frames(&mut self, max_frames: usize) -> Result<Vec<f32>, MusicError> {
        let channels = usize::from(self.channels);
        let want = max_frames.saturating_mul(channels);
        let mut out = Vec::new();
        while out.len() < want {
            match self.next_chunk()? {
                Some(mut chunk) => {
                    let room = want - out.len();
                    if chunk.len() > room {
                        self.pending = chunk.split_off(room);
                    }
                    out.extend(chunk);
                }
                None => break,
            }
        }
        Ok(out)
    }
}

/// Reads up to `head.len()` bytes; how many there were.
fn read_head(file: &mut File, head: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < head.len() {
        match file.read(head.get_mut(n..).unwrap_or_default())? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

/// A signed integer sample of `bits` bits as a float in −1 ..= 1.
fn scale(v: i32, bits: u32) -> f32 {
    let full = f64::from(1u32 << bits.clamp(1, 32).saturating_sub(1).min(31));
    (f64::from(v) / full).clamp(-1.0, 1.0) as f32
}

/// The next block from `decoder` (`Some(empty)` for a block with nothing in it).
fn decode(decoder: &mut Decoder, channels: usize) -> Result<Option<Vec<f32>>, MusicError> {
    match decoder {
        Decoder::Mp3(r) => match r.read_frame() {
            Ok(Some(frame)) => Ok(Some(frame.to_vec())),
            Ok(None) => Ok(None),
            Err(_) => Err(MusicError::damaged(MusicKind::Mp3)),
        },
        Decoder::Vorbis(r) => match r.read_dec_packet_generic::<lewton::samples::InterleavedSamples<f32>>() {
            Ok(Some(p)) if p.channel_count == channels => Ok(Some(p.samples)),
            Ok(Some(_)) => Ok(Some(Vec::new())),
            Ok(None) => Ok(None),
            Err(_) => Err(MusicError::damaged(MusicKind::Vorbis)),
        },
        Decoder::Flac { reader, bits, buffer } => {
            let block = reader.blocks().read_next_or_eof(std::mem::take(buffer)).map_err(|_| MusicError::damaged(MusicKind::Flac))?;
            let Some(block) = block else { return Ok(None) };
            if block.channels() as usize != channels || block.duration() as usize > MAX_CHUNK_FRAMES {
                return Err(MusicError::damaged(MusicKind::Flac));
            }
            let frames = block.duration() as usize;
            let mut out = Vec::with_capacity(frames.saturating_mul(channels));
            let lanes: Vec<&[i32]> = (0..channels as u32).map(|c| block.channel(c)).collect();
            for i in 0..frames {
                for lane in &lanes {
                    out.push(scale(lane.get(i).copied().unwrap_or(0), *bits));
                }
            }
            *buffer = block.into_buffer();
            Ok(Some(out))
        }
        Decoder::Wav { reader, float, bits } => {
            let want = WAV_BLOCK.saturating_mul(channels);
            let mut out = Vec::with_capacity(want);
            if *float {
                for s in reader.samples::<f32>().take(want) {
                    let v = s.map_err(|_| MusicError::damaged(MusicKind::Wav))?;
                    out.push(if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 });
                }
            } else {
                for s in reader.samples::<i32>().take(want) {
                    out.push(scale(s.map_err(|_| MusicError::damaged(MusicKind::Wav))?, u32::from(*bits)));
                }
            }
            if out.is_empty() { Ok(None) } else { Ok(Some(out)) }
        }
    }
}
