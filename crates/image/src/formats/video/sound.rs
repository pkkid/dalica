//! A clip's sound as samples (reserved decision 2, `docs/video-decoders.md` §2): AAC (LC, and
//! HE-AAC where its configuration says so) through `rusty_aac` (=1.1.1, no default features), for
//! the slideshow and the player (M15.25, M15.27) to play through the app's sound output.
//!
//! [`SoundSource::open`] reads the first sound track's sample table (MP4, M4V, QuickTime through
//! `frames/track.rs`, Matroska through `frames/mkv.rs`); a clip without one, or whose sound is not
//! AAC, opens as [`Muted`] with the reason, and plays without sound. Samples come out as
//! interleaved `f32` in −1..1 at the stream's output rate, packet by packet ([`SoundSource::next_chunk`])
//! or the whole track at once ([`SoundSource::read_all`], at most `max_frames`); the encoder's
//! priming before the start the edit list sets is cut.
//!
//! Every count is hostile (`security.md` §4.2): at most [`MAX_CHANNELS`] channels and
//! [`MAX_RATE`] samples a second; packets are read one at a time, each at most
//! `MAX_SAMPLE_BYTES`; a packet that decodes to more than [`MAX_PACKET_FRAMES`] frames, or to a
//! different layout than the track's, is refused; a panic in the decoder is caught. A damaged
//! packet is skipped (the decoder stays usable) and counted in [`SoundSource::damaged`].

use std::fs::File;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use rusty_aac::AacDecoder;

use super::frames::Input;
use super::frames::track::{self, Config, Track};
use super::{muted_reason, recognise, sound_codec_name};
use crate::limits::{Limits, Work};
use crate::sniff::VideoKind;
use crate::{Error, Result};

/// Most channels decoded (7.1).
pub const MAX_CHANNELS: u16 = 8;

/// Highest output rate, in samples a second.
pub const MAX_RATE: u32 = 192_000;

/// Most frames (samples per channel) one packet may decode to (HE-AAC doubles AAC's 1,024).
pub const MAX_PACKET_FRAMES: usize = 4096;

/// Why a clip plays without sound.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{reason}")]
pub struct Muted {
    /// What the player shows ("This clip has no sound", "This clip plays without sound (AC-3 …)").
    pub reason: String,
}

impl Muted {
    fn because(reason: impl Into<String>) -> Muted {
        Muted { reason: reason.into() }
    }

    fn damaged(e: &Error) -> Muted {
        Muted::because(format!("This clip plays without sound ({})", e.reason().to_lowercase()))
    }
}

/// Samples of one packet.
#[derive(Clone, Debug, PartialEq)]
pub struct SoundChunk {
    /// When the first of them plays, in milliseconds from the start.
    pub time_ms: u64,
    /// Interleaved, [`SoundSource::channels`] to a frame.
    pub samples: Vec<f32>,
}

/// The sound of one open clip.
pub struct SoundSource {
    input: Input,
    track: Track,
    decoder: AacDecoder,
    rate: u32,
    channels: u16,
    next: usize,
    damaged: usize,
    buf: Vec<u8>,
}

impl SoundSource {
    /// Opens the sound of the clip at `path`.
    pub fn open(path: &Path, limits: &Limits) -> std::result::Result<SoundSource, Muted> {
        let io = |e: std::io::Error| Muted::damaged(&Error::Io(e.to_string()));
        let file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        SoundSource::with_input(Input::File { file, len }, limits)
    }

    /// As [`SoundSource::open`], for a clip in memory.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>, limits: &Limits) -> std::result::Result<SoundSource, Muted> {
        SoundSource::with_input(Input::Bytes(bytes.into()), limits)
    }

    fn with_input(mut input: Input, limits: &Limits) -> std::result::Result<SoundSource, Muted> {
        let track = find_track(&mut input, limits).map_err(|e| Muted::damaged(&e))?;
        let Some(track) = track else { return Err(Muted::because("This clip has no sound")) };
        let config = match &track.config {
            Config::Aac { config } => config.clone(),
            Config::Other(code) => return Err(Muted::because(muted_reason(&sound_codec_name(code)))),
            _ => return Err(Muted::because(muted_reason(&sound_codec_name(&track.code)))),
        };
        if track.samples.is_empty() {
            return Err(Muted::because("This clip has no sound"));
        }
        let decoder = guard(|| AacDecoder::with_config_bytes(&config))
            .and_then(|r| r.map_err(|e| Error::Malformed(format!("an AAC configuration: {e}"))))
            .map_err(|e| match e {
                Error::Malformed(m) if m.contains("unsupported") => Muted::because(muted_reason("this AAC")),
                e => Muted::damaged(&e),
            })?;
        let rate = decoder.output_sample_rate().unwrap_or(0);
        let channels = guard(|| rusty_aac::parse_audio_specific_config(&config)).ok().and_then(|r| r.ok()).map_or(0, |c| c.channels);
        // Channel configuration 0 (a program config element) is resolved by the first packet.
        if rate == 0 || rate > MAX_RATE || channels > MAX_CHANNELS {
            return Err(Muted::because(format!("This clip plays without sound ({channels} channels at {rate} Hz can't be played)")));
        }
        Ok(SoundSource { input, track, decoder, rate, channels, next: 0, damaged: 0, buf: Vec::new() })
    }

    /// Samples a second, per channel.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// Channels to a frame (known after the first packet when the configuration leaves it out).
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The track's length in milliseconds (the last packet's start).
    pub fn duration_ms(&self) -> u64 {
        self.track.samples.iter().map(|s| self.track.millis(s.pts)).max().unwrap_or(0).max(0) as u64
    }

    /// Packets skipped as damaged so far.
    pub fn damaged(&self) -> usize {
        self.damaged
    }

    /// Starts again at the packet playing at `ms`.
    pub fn seek(&mut self, ms: u64) {
        let at = self.track.samples.partition_point(|s| self.track.millis(s.pts) <= ms as i64);
        self.next = at.saturating_sub(1);
    }

    /// The next packet's samples, or `None` at the end.
    pub fn next_chunk(&mut self) -> Result<Option<SoundChunk>> {
        loop {
            let Some(&sample) = self.track.samples.get(self.next) else { return Ok(None) };
            self.next += 1;
            self.input.read_at(sample.offset, sample.size, &mut self.buf)?;
            let buf = &self.buf;
            let decoder = &mut self.decoder;
            let decoded = match guard(|| decoder.decode(buf, None))? {
                Ok(d) => d,
                Err(rusty_aac::Error::Again) => continue,
                Err(_) => {
                    self.damaged += 1;
                    continue;
                }
            };
            let frames = decoded.frames();
            if decoded.channels == 0 || decoded.channels > MAX_CHANNELS || frames > MAX_PACKET_FRAMES || decoded.sample_rate != self.rate {
                return Err(Error::Malformed(format!(
                    "an AAC packet of {frames} frames, {} channels at {} Hz",
                    decoded.channels, decoded.sample_rate
                )));
            }
            if self.channels == 0 {
                self.channels = decoded.channels;
            } else if decoded.channels != self.channels {
                return Err(Error::Malformed("an AAC stream changing its channels".into()));
            }
            let mut samples = decoded.samples;
            let ms = self.track.millis(sample.pts);
            // Priming before the start: cut.
            let mut time_ms = ms;
            if ms < 0 {
                let skip = (i128::from(-ms) * i128::from(self.rate) / 1000) as usize;
                let skip = skip.min(frames) * usize::from(self.channels);
                samples.drain(..skip.min(samples.len()));
                time_ms = 0;
                if samples.is_empty() {
                    continue;
                }
            }
            return Ok(Some(SoundChunk { time_ms: time_ms as u64, samples }));
        }
    }

    /// The whole track's samples from where it stands, at most `max_frames` frames (more is
    /// [`Error::TooMany`]).
    pub fn read_all(&mut self, max_frames: usize) -> Result<Vec<f32>> {
        let mut out: Vec<f32> = Vec::new();
        while let Some(chunk) = self.next_chunk()? {
            let channels = usize::from(self.channels.max(1));
            if (out.len() + chunk.samples.len()) / channels > max_frames {
                return Err(Error::TooMany("sound samples in a clip"));
            }
            out.try_reserve(chunk.samples.len()).map_err(|_| Error::TooMany("sound samples in a clip"))?;
            out.extend_from_slice(&chunk.samples);
        }
        Ok(out)
    }
}

/// The first sound track of the clip in `input`.
fn find_track(input: &mut Input, limits: &Limits) -> Result<Option<Track>> {
    let mut head = Vec::new();
    let first = input.len().min(crate::sniff::SNIFF_LEN as u64 * 2) as u32;
    input.read_at(0, first, &mut head)?;
    let kind = recognise(&head).ok_or(Error::Unknown)?;
    let mut work = Work::new(super::MAX_STEPS, "boxes or elements in a video");
    match kind {
        VideoKind::Matroska => Ok(super::frames::mkv::open(input, b"soun", limits, &mut work)?.1),
        _ => {
            let movie = input.movie(limits)?;
            track::find(&movie, b"soun", &mut work)
        }
    }
}

fn guard<T>(f: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| Error::Malformed("the AAC decoder failed on damaged data".into()))
}
