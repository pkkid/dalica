//! The window's sound output (`viewing.md` §1.1, §1.5, `editing.md` §11; reserved decision 2,
//! M15.27: a pure-Rust output per OS): the engine's [`Sound`] over one backend per OS, each on a
//! thread of its own, never the UI thread.
//!
//! - **Linux** ([`pulse`]): a PulseAudio-protocol client of our own over the `pulseaudio` crate's
//!   message types, on the server's Unix socket; PulseAudio and PipeWire's pulse server both
//!   serve it. It plays to the default sink, which the server follows when the user changes it.
//! - **Windows and macOS** ([`device`]): `cpal`'s WASAPI (the `windows` crate) and CoreAudio
//!   (`objc2-core-audio`) hosts, which keep those APIs' `unsafe` calls out of this workspace; the
//!   default output device at its own format, our samples resampled and mapped to it.
//!
//! [`Output`] is what the session holds: [`Sound::open`] starts a stream at a format, its samples
//! wait in a queue (at most [`MAX_QUEUED_MS`] of them) that the backend takes as the device asks,
//! and the backend reports how much of it has been heard, which is the clock
//! ([`Sound::clock_ms`]). When the output fails (the sound server went away, the device was
//! unplugged), the backend opens the default output again, at most [`RETRIES`] times, before the
//! stream answers the failure. Whether the system has an output is found once at start, on the
//! output's thread ([`Backend::probe`]); with none, [`Sound::status`] answers [`NO_OUTPUT`] and
//! [`Sound::open`] refuses with it, so the Sound rows are greyed with that reason and the video
//! player keeps time by the session's clock.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use dalica_engine::sound::{Format, NO_OUTPUT, Sound, SoundError};

#[cfg(any(target_os = "windows", target_os = "macos"))]
#[path = "sound/device.rs"]
pub mod device;
#[cfg(target_os = "linux")]
#[path = "sound/pulse.rs"]
pub mod pulse;

/// The most sound queued ahead of what has been heard, in milliseconds.
pub const MAX_QUEUED_MS: u64 = 10_000;
/// How many times a failed output is opened again before its stream answers the failure.
pub const RETRIES: u32 = 3;
/// How long the output's thread waits between tries.
const RETRY_WAIT: Duration = Duration::from_millis(300);

/// The window's sound output on this OS.
pub fn output() -> Arc<dyn Sound> {
    #[cfg(target_os = "linux")]
    let out = Output::start(pulse::Pulse::from_env());
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let out = Output::start(device::Device);
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let out = Output::none(NO_OUTPUT);
    Arc::new(out)
}

/// What one OS's output does on the output's thread.
pub trait Backend: Send + 'static {
    /// Whether the system has an output: `Err` with the reason ([`NO_OUTPUT`]) when it has none.
    fn probe(&mut self) -> Result<(), String>;

    /// Plays stream `stream` at `format` from `core` ([`Core::take`], [`Core::heard`]) until it
    /// closes or another one opens (`Ok`), or the output fails (`Err`).
    fn play(&mut self, core: &Arc<Core>, stream: u64, format: Format) -> Result<(), String>;
}

/// Whether the system has an output.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Status {
    /// Not known yet: taken as there being one.
    Probing,
    Ready,
    None(String),
}

#[derive(Debug)]
struct State {
    status: Status,
    /// Bumped by every open.
    stream: u64,
    format: Option<Format>,
    /// Samples queued and not taken by the backend yet.
    queue: VecDeque<f32>,
    /// Frames queued since the stream opened.
    queued: u64,
    /// Frames heard since the stream opened, as the backend last said.
    heard: u64,
    /// Why the stream failed, once the backend gave up on it.
    failed: Option<String>,
}

/// What the session's side and the output's thread share.
#[derive(Debug)]
pub struct Core {
    state: Mutex<State>,
    wake: Condvar,
}

impl Core {
    fn new(status: Status) -> Core {
        Core {
            state: Mutex::new(State { status, stream: 0, format: None, queue: VecDeque::new(), queued: 0, heard: 0, failed: None }),
            wake: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The stream open and its format, if one is.
    pub fn current(&self) -> Option<(u64, Format)> {
        let s = self.lock();
        s.format.map(|f| (s.stream, f))
    }

    /// Whether stream `stream` is still the one open at `format`.
    pub fn is_current(&self, stream: u64, format: Format) -> bool {
        self.current() == Some((stream, format))
    }

    /// Up to `max_frames` frames of stream `stream`'s samples (fewer, or none, when fewer wait);
    /// `None` when that stream is no longer open.
    pub fn take(&self, stream: u64, max_frames: usize) -> Option<Vec<f32>> {
        let mut s = self.lock();
        let format = s.format.filter(|_| s.stream == stream)?;
        let n = max_frames.saturating_mul(usize::from(format.channels)).min(s.queue.len());
        Some(s.queue.drain(..n).collect())
    }

    /// How many frames of stream `stream` have been heard since it opened (never back, never past
    /// what was queued).
    pub fn heard(&self, stream: u64, frames: u64) {
        let mut s = self.lock();
        if s.stream == stream && s.format.is_some() {
            s.heard = s.heard.max(frames.min(s.queued));
        }
    }

    /// Waits up to `timeout` for a stream to open (or another change).
    fn wait(&self, timeout: Duration) {
        let s = self.lock();
        if s.format.is_none() {
            let _ = self.wake.wait_timeout(s, timeout);
        }
    }

    fn fail(&self, stream: u64, why: String) {
        let mut s = self.lock();
        if s.stream == stream {
            s.failed = Some(why);
            s.queue.clear();
        }
    }
}

/// The session's sound output (module docs).
pub struct Output {
    core: Arc<Core>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output").finish_non_exhaustive()
    }
}

impl Output {
    /// An output over `backend`, probed and played on a thread of its own. A thread that cannot
    /// start leaves the output answering [`NO_OUTPUT`].
    pub fn start(backend: impl Backend) -> Output {
        let core = Arc::new(Core::new(Status::Probing));
        let thread_core = Arc::clone(&core);
        let spawned = std::thread::Builder::new().name("dalica-sound".into()).spawn(move || run(backend, &thread_core));
        if let Err(e) = spawned {
            log::warn!("the sound output could not start: {e}");
            core.lock().status = Status::None(NO_OUTPUT.into());
        }
        Output { core }
    }

    /// An output that never plays: `reason` is why (an OS without a backend; the tests).
    #[allow(dead_code)]
    pub fn none(reason: &str) -> Output {
        Output { core: Arc::new(Core::new(Status::None(reason.into()))) }
    }

    /// Waits up to `timeout` for the probe's answer; answers whether it came (`tests/sound.rs`).
    #[allow(dead_code)]
    pub fn wait_probed(&self, timeout: Duration) -> bool {
        let until = std::time::Instant::now() + timeout;
        while self.core.lock().status == Status::Probing {
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        true
    }
}

/// The output's thread: the probe, then each stream as it opens.
fn run(mut backend: impl Backend, core: &Arc<Core>) {
    let status = match backend.probe() {
        Ok(()) => Status::Ready,
        Err(why) => {
            log::info!("no sound output: {why}");
            Status::None(why)
        }
    };
    let none = matches!(status, Status::None(_));
    core.lock().status = status;
    if none {
        return;
    }
    let mut tries = 0u32;
    let mut last: Option<u64> = None;
    loop {
        let Some((stream, format)) = core.current() else {
            core.wait(Duration::from_millis(200));
            continue;
        };
        if core.lock().failed.is_some() {
            core.wait(Duration::from_millis(50));
            continue;
        }
        if last != Some(stream) {
            tries = 0;
            last = Some(stream);
        }
        match backend.play(core, stream, format) {
            Ok(()) => {}
            Err(why) => {
                tries += 1;
                log::warn!("the sound output failed ({why}); opening the default output again");
                if tries > RETRIES {
                    core.fail(stream, why);
                } else {
                    std::thread::sleep(RETRY_WAIT);
                }
            }
        }
    }
}

impl Sound for Output {
    fn open(&self, format: Format) -> Result<(), SoundError> {
        let format = format.check()?;
        let mut s = self.core.lock();
        if let Status::None(why) = &s.status {
            return Err(SoundError::Unavailable(why.clone()));
        }
        s.stream = s.stream.wrapping_add(1);
        s.format = Some(format);
        s.queue.clear();
        s.queued = 0;
        s.heard = 0;
        s.failed = None;
        drop(s);
        self.core.wake.notify_all();
        Ok(())
    }

    fn queue(&self, samples: &[f32]) -> Result<(), SoundError> {
        let mut s = self.core.lock();
        let Some(format) = s.format else { return Err(SoundError::Refused("the sound output is not open".into())) };
        if let Some(why) = &s.failed {
            return Err(SoundError::Failed(why.clone()));
        }
        let channels = usize::from(format.channels);
        if !samples.len().is_multiple_of(channels) {
            return Err(SoundError::Refused("the samples are not a whole number of frames".into()));
        }
        let frames = (samples.len() / channels) as u64;
        let ahead = s.queued.saturating_add(frames).saturating_sub(s.heard);
        if ahead.saturating_mul(1000) / u64::from(format.rate.max(1)) > MAX_QUEUED_MS {
            return Err(SoundError::Refused("the sound is queued too far ahead".into()));
        }
        s.queue.extend(samples.iter().map(|v| if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 }));
        s.queued = s.queued.saturating_add(frames);
        Ok(())
    }

    fn clock_ms(&self) -> u64 {
        let s = self.core.lock();
        match s.format {
            Some(f) => s.heard.min(s.queued).saturating_mul(1000) / u64::from(f.rate.max(1)),
            None => 0,
        }
    }

    fn stop(&self) {
        let mut s = self.core.lock();
        s.format = None;
        s.queue.clear();
        s.queued = 0;
        s.heard = 0;
    }

    fn stream(&self) -> u64 {
        self.core.lock().stream
    }

    fn status(&self) -> Result<(), String> {
        match &self.core.lock().status {
            Status::None(why) => Err(why.clone()),
            Status::Probing | Status::Ready => Ok(()),
        }
    }
}

/// Interleaved `samples` of `channels` channels as `out` channels (1 or 2): mono and stereo as
/// they are (mono doubled for stereo), more channels mixed down evenly.
pub fn downmix(samples: &[f32], channels: u16, out: u16) -> Vec<f32> {
    let channels = usize::from(channels.max(1));
    let out = usize::from(out.clamp(1, 2));
    if channels == out {
        return samples.to_vec();
    }
    let mut v = Vec::with_capacity(samples.len() / channels * out);
    for frame in samples.chunks_exact(channels) {
        match (channels, out) {
            (1, _) => v.extend(std::iter::repeat_n(frame.first().copied().unwrap_or(0.0), out)),
            (2, 1) => v.push((frame.iter().sum::<f32>() / 2.0).clamp(-1.0, 1.0)),
            _ => {
                let mixed = (frame.iter().sum::<f32>() / channels as f32).clamp(-1.0, 1.0);
                v.extend(std::iter::repeat_n(mixed, out));
            }
        }
    }
    v
}
