//! The sound output (`viewing.md` §1.1 and §1.5, `editing.md` §11; `roadmap.md` M15 → reserved
//! decision 2), behind a service trait as [`crate::os::Os`] is: open at a rate, queue samples, its
//! clock (how much of what was queued has been heard), stop. The video player (M15.25) lets the
//! clock drive playback; the slideshow (M15.27) plays a video's sound and its music through it.
//!
//! The desktop app plugs in a pure-Rust output per OS (M15.27: a PulseAudio-protocol client on
//! Linux, WASAPI on Windows, CoreAudio on macOS) with [`Session::set_sound`]. [`Silent`] is the
//! fake that headless sessions, `dalica-cli`, MCP and tests use: it plays nothing, keeps time by a
//! [`Clock`] and records what it was given.

use std::sync::{Arc, Mutex, PoisonError};

use dalica_core::{Clock, Timestamp};

use crate::session::Session;

/// The most channels an output is opened with.
pub const MAX_CHANNELS: u16 = 8;
/// The sample rates an output is opened at, in Hz.
pub const RATE_RANGE: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// The samples' shape: interleaved 32-bit floats in -1 ..= 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// Frames per second, in [`RATE_RANGE`].
    pub rate: u32,
    /// Samples per frame, 1 ..= [`MAX_CHANNELS`].
    pub channels: u16,
}

impl Format {
    /// `Err` with the reason unless the rate and channels are ones an output opens with.
    pub fn check(self) -> Result<Format, SoundError> {
        if !RATE_RANGE.contains(&self.rate) {
            return Err(SoundError::Refused(format!("a rate of {} Hz is not one sound plays at", self.rate)));
        }
        if self.channels == 0 || self.channels > MAX_CHANNELS {
            return Err(SoundError::Refused(format!("{} channels is not a number sound plays with", self.channels)));
        }
        Ok(self)
    }
}

/// Why sound did not play.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundError {
    /// There is no sound output (none on this system, or none in this build): the reason.
    Unavailable(String),
    /// The output refused the call (not open, a format it does not take, a partial frame).
    Refused(String),
    /// The output failed (the sound server went away): the reason.
    Failed(String),
}

impl std::fmt::Display for SoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoundError::Unavailable(why) | SoundError::Refused(why) | SoundError::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for SoundError {}

/// A sound output. One stream at a time: [`Sound::open`] again replaces it.
pub trait Sound: Send + Sync {
    /// Opens the output at `format`; the clock starts at zero.
    fn open(&self, format: Format) -> Result<(), SoundError>;

    /// Queues `samples` (interleaved, a whole number of frames) after those queued before.
    fn queue(&self, samples: &[f32]) -> Result<(), SoundError>;

    /// How much of what was queued has been heard since [`Sound::open`], in milliseconds (zero
    /// when closed).
    fn clock_ms(&self) -> u64;

    /// Stops and closes the output; what was queued and not heard is dropped.
    fn stop(&self);
}

#[derive(Debug, Default)]
struct Stream {
    format: Option<Format>,
    opened: Option<Timestamp>,
    /// Frames queued since the output was opened.
    frames: u64,
    /// What was queued, as given, while open.
    samples: Vec<f32>,
    opens: Vec<Format>,
}

/// The silent [`Sound`]: plays nothing; its clock is the time `clock` says has passed since
/// [`Sound::open`], never past what was queued; it records the formats it was opened at
/// ([`Silent::opens`]) and the samples queued since the last open ([`Silent::samples`]).
pub struct Silent {
    clock: Arc<dyn Clock>,
    stream: Mutex<Stream>,
}

impl std::fmt::Debug for Silent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Silent").finish_non_exhaustive()
    }
}

impl Silent {
    pub fn new(clock: Arc<dyn Clock>) -> Silent {
        Silent { clock, stream: Mutex::new(Stream::default()) }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Stream) -> R) -> R {
        f(&mut self.stream.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The format it is open at, if open.
    pub fn format(&self) -> Option<Format> {
        self.with(|s| s.format)
    }

    /// Every format it was opened at, oldest first.
    pub fn opens(&self) -> Vec<Format> {
        self.with(|s| s.opens.clone())
    }

    /// The samples queued since the last open.
    pub fn samples(&self) -> Vec<f32> {
        self.with(|s| s.samples.clone())
    }
}

/// The most samples [`Silent`] records per open (about a minute of 48 kHz stereo); beyond them
/// only the time is counted.
const MAX_RECORDED: usize = 48_000 * 2 * 64;

impl Sound for Silent {
    fn open(&self, format: Format) -> Result<(), SoundError> {
        let format = format.check()?;
        let now = self.clock.now();
        self.with(|s| {
            *s = Stream { format: Some(format), opened: Some(now), frames: 0, samples: Vec::new(), opens: std::mem::take(&mut s.opens) };
            s.opens.push(format);
        });
        Ok(())
    }

    fn queue(&self, samples: &[f32]) -> Result<(), SoundError> {
        self.with(|s| {
            let Some(format) = s.format else { return Err(SoundError::Refused("the sound output is not open".into())) };
            let channels = usize::from(format.channels);
            if !samples.len().is_multiple_of(channels) {
                return Err(SoundError::Refused("the samples are not a whole number of frames".into()));
            }
            s.frames = s.frames.saturating_add((samples.len() / channels) as u64);
            let room = MAX_RECORDED.saturating_sub(s.samples.len());
            s.samples.extend_from_slice(samples.get(..room.min(samples.len())).unwrap_or_default());
            Ok(())
        })
    }

    fn clock_ms(&self) -> u64 {
        let now = self.clock.now();
        self.with(|s| {
            let (Some(format), Some(opened)) = (s.format, s.opened) else { return 0 };
            let passed = u64::try_from(now.as_millis().saturating_sub(opened.as_millis())).unwrap_or(0);
            let queued = s.frames.saturating_mul(1000) / u64::from(format.rate.max(1));
            passed.min(queued)
        })
    }

    fn stop(&self) {
        self.with(|s| {
            s.format = None;
            s.opened = None;
            s.frames = 0;
        });
    }
}

impl Session {
    /// The sound output in use: a [`Silent`] one on the session's clock unless the app plugged in
    /// a real one.
    pub fn sound(&self) -> Arc<dyn Sound> {
        Arc::clone(&self.sound)
    }

    /// Plugs in the sound output (the desktop app's, or a test's [`Silent`]).
    pub fn set_sound(&mut self, sound: Arc<dyn Sound>) {
        self.sound = sound;
    }
}

#[cfg(test)]
mod tests {
    use dalica_core::VirtualClock;

    use super::*;

    /// One test of the silent output: its clock follows time, never past what was queued, from
    /// each open; what it was given is recorded; bad formats and partial frames are refused.
    #[test]
    fn the_silent_output_keeps_time_and_records_what_it_was_given() {
        let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(5_000)));
        let out = Silent::new(clock.clone());
        assert_eq!(out.queue(&[0.0]), Err(SoundError::Refused("the sound output is not open".into())));
        assert!(out.open(Format { rate: 4_000, channels: 2 }).is_err() && out.open(Format { rate: 48_000, channels: 0 }).is_err());
        let stereo = Format { rate: 8_000, channels: 2 };
        out.open(stereo).unwrap();
        assert!(out.queue(&[0.5; 3]).is_err(), "a partial frame");
        out.queue(&[0.25; 2 * 12_000]).unwrap();
        assert_eq!(out.clock_ms(), 0);
        clock.advance(std::time::Duration::from_millis(400));
        assert_eq!(out.clock_ms(), 400);
        clock.advance(std::time::Duration::from_secs(5));
        assert_eq!(out.clock_ms(), 1_500, "never past what was queued");
        assert_eq!((out.samples().len(), out.format()), (24_000, Some(stereo)));
        out.stop();
        assert_eq!((out.clock_ms(), out.format()), (0, None));
        let mono = Format { rate: 8_000, channels: 1 };
        out.open(mono).unwrap();
        assert!(out.samples().is_empty(), "recorded from the last open");
        assert_eq!(out.opens(), [stereo, mono]);
    }
}
