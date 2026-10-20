//! The sound output (`viewing.md` §1.1 and §1.5, `editing.md` §11; `roadmap.md` M15 → reserved
//! decision 2), behind a service trait as [`crate::os::Os`] is: open at a rate, queue samples, its
//! clock (how much of what was queued has been heard), stop. The video player (M15.25) lets the
//! clock drive playback; the slideshow (M15.27) plays a video's sound and its music through it.
//!
//! The desktop app plugs in a pure-Rust output per OS (M15.27: a PulseAudio-protocol client on
//! Linux, WASAPI on Windows, CoreAudio on macOS) with [`Session::set_sound`]. [`Silent`] is the
//! fake that headless sessions, `dalica-cli`, MCP and tests use: it plays nothing, keeps time by a
//! [`Clock`] and records what it was given. An output says whether the system has one
//! ([`Sound::status`]: the app's backends answer [`NO_OUTPUT`] when no sound server or device is
//! reachable), and which stream is open ([`Sound::stream`]), so a worker that queued into a stream
//! another player has since replaced stops queueing.
//!
//! **Slideshow music** ([`MusicPlayer`], `viewing.md` §1.5): every music file of a folder
//! ([`dalica_image::formats::audio`]: MP3, Ogg Vorbis, FLAC, WAV) in name order, looping, listed
//! and decoded on a worker thread of its own and queued [`MUSIC_AHEAD_MS`] ahead of what has been
//! heard. A file that does not play is skipped with its reason ([`MusicPlayer::skipped`]); when no
//! file of the folder plays, the music ends with [`NO_MUSIC`]. [`MusicPlayer::pause`] gives the
//! output up (a clip's sound, the show paused) and remembers what was queued and not heard yet;
//! [`MusicPlayer::resume`] opens it again and plays on from there. [`MusicPlayer::fade_out`]
//! (the show ends) fades the music to silence over [`FADE_MS`] and closes the output; dropping the
//! player fades it out too.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dalica_core::{Clock, Timestamp};
use dalica_image::formats::audio::{self as audio, Music};

use crate::session::Session;

/// Why the Sound rows are greyed on a system with no sound output (`viewing.md` §1.5).
pub const NO_OUTPUT: &str = "No sound output on this system";

/// The reason Options ▸ Slideshow's sound rows carry in the options catalogue: none since Dalica
/// plays sound (M15.27). Whether this system has an output is [`Sound::status`]'s answer, which the
/// window shows on the rows.
pub const SOUND_UNAVAILABLE: Option<&str> = None;

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

    /// The stream open now: a number every [`Sound::open`] changes (0 before the first).
    fn stream(&self) -> u64;

    /// Whether this output plays: `Err` with the reason ([`NO_OUTPUT`]) when the system has none.
    fn status(&self) -> Result<(), String> {
        Ok(())
    }
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

    fn stream(&self) -> u64 {
        self.with(|s| s.opens.len() as u64)
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

// --- Slideshow music ----------------------------------------------------------------------------------

/// How far ahead of what has been heard music is queued, in milliseconds.
pub const MUSIC_AHEAD_MS: u64 = 250;
/// How long the music fades out over when the show ends, in milliseconds.
pub const FADE_MS: u64 = 1_000;
/// The most entries of the music folder looked at.
pub const MAX_MUSIC_FILES: usize = 10_000;
/// Why no music plays when the folder has no file that plays.
pub const NO_MUSIC: &str = "No music file in the folder can be played";
/// How long the music worker waits between rounds.
const MUSIC_ROUND: Duration = Duration::from_millis(5);
/// The most samples kept to queue again after a pause (beyond [`MUSIC_AHEAD_MS`] at any rate).
const MAX_RECENT: usize = 192_000 * MAX_CHANNELS as usize;

/// What the owner and the music worker share.
#[derive(Debug, Default)]
struct MusicShared {
    /// The owner gave the output up ([`MusicPlayer::pause`]).
    paused: bool,
    /// Fade out and end ([`MusicPlayer::fade_out`], or the player dropped).
    fade: bool,
    /// The player was dropped: a paused worker ends.
    dropped: bool,
    /// The stream the music is queued into ([`Sound::stream`] after its open); `None` when the
    /// output must be opened again.
    mine: Option<u64>,
    format: Option<Format>,
    /// Frames queued since the output was opened.
    queued: u64,
    /// The last samples queued, oldest first.
    recent: VecDeque<f32>,
    /// What was queued and not heard when the music paused: queued first when it plays again.
    replay: Vec<f32>,
    /// The files started, in order (at most [`MAX_MUSIC_FILES`]).
    played: Vec<PathBuf>,
    /// The files skipped and why.
    skipped: Vec<(PathBuf, String)>,
    /// Why the music ended before it was asked to.
    ended: Option<String>,
    /// The worker has ended.
    done: bool,
}

/// The slideshow's music (module docs).
pub struct MusicPlayer {
    shared: Arc<Mutex<MusicShared>>,
    sound: Arc<dyn Sound>,
}

impl std::fmt::Debug for MusicPlayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MusicPlayer").finish_non_exhaustive()
    }
}

fn lock_music(shared: &Mutex<MusicShared>) -> std::sync::MutexGuard<'_, MusicShared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The music files of `folder`, in name order (case aside): every file whose extension is music's
/// ([`audio::is_music_name`]), at most [`MAX_MUSIC_FILES`] entries looked at.
pub fn music_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(folder).map_err(|e| format!("The music folder can't be read: {e}"))?;
    let mut files: Vec<(String, PathBuf)> = entries
        .take(MAX_MUSIC_FILES)
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            audio::is_music_name(&name).then(|| (name, e.path()))
        })
        .collect();
    files.sort_by(|(a, _), (b, _)| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
    Ok(files.into_iter().map(|(_, p)| p).collect())
}

impl MusicPlayer {
    /// Starts playing the music of `folder` through `sound` (listed and decoded on a worker). A
    /// worker that cannot start answers `None`, and no music plays.
    pub fn start(folder: PathBuf, sound: Arc<dyn Sound>) -> Option<MusicPlayer> {
        let shared = Arc::new(Mutex::new(MusicShared::default()));
        let worker = MusicWorker { folder, shared: Arc::clone(&shared), sound: Arc::clone(&sound) };
        match std::thread::Builder::new().name("dalica-music".into()).spawn(move || worker.run()) {
            Ok(_) => Some(MusicPlayer { shared, sound }),
            Err(e) => {
                log::warn!("the slideshow's music could not start: {e}");
                None
            }
        }
    }

    /// Gives the output up: what was queued and not heard yet is kept to play when the music
    /// resumes; the output is stopped when the music had it.
    pub fn pause(&self) {
        let mut m = lock_music(&self.shared);
        if m.paused {
            return;
        }
        m.paused = true;
        let Some(mine) = m.mine.take() else { return };
        if mine != self.sound.stream() {
            return;
        }
        if let Some(format) = m.format {
            let heard = self.sound.clock_ms().saturating_mul(u64::from(format.rate)) / 1000;
            let unheard = usize::try_from(m.queued.saturating_sub(heard)).unwrap_or(usize::MAX).saturating_mul(usize::from(format.channels));
            let keep = unheard.min(m.recent.len());
            let skip = m.recent.len() - keep;
            m.replay = m.recent.iter().skip(skip).copied().collect();
        }
        self.sound.stop();
    }

    /// Plays on after [`MusicPlayer::pause`] (the worker opens the output again).
    pub fn resume(&self) {
        lock_music(&self.shared).paused = false;
    }

    /// Whether the music is paused.
    pub fn is_paused(&self) -> bool {
        lock_music(&self.shared).paused
    }

    /// Fades the music out over [`FADE_MS`] and ends it.
    pub fn fade_out(&self) {
        let mut m = lock_music(&self.shared);
        m.fade = true;
        m.paused = false;
    }

    /// The files started so far, in order.
    pub fn played(&self) -> Vec<PathBuf> {
        lock_music(&self.shared).played.clone()
    }

    /// The files skipped, with why.
    pub fn skipped(&self) -> Vec<(PathBuf, String)> {
        lock_music(&self.shared).skipped.clone()
    }

    /// Why the music ended by itself (no file plays, no output), if it did.
    pub fn ended(&self) -> Option<String> {
        lock_music(&self.shared).ended.clone()
    }

    /// Whether the worker has ended (faded out, or ended by itself).
    pub fn is_done(&self) -> bool {
        lock_music(&self.shared).done
    }
}

impl Drop for MusicPlayer {
    fn drop(&mut self) {
        let mut m = lock_music(&self.shared);
        m.dropped = true;
        m.fade = true;
    }
}

/// The music player's worker.
struct MusicWorker {
    folder: PathBuf,
    shared: Arc<Mutex<MusicShared>>,
    sound: Arc<dyn Sound>,
}

/// A file playing: its decoder and a block decoded and not queued yet.
struct Playing {
    path: PathBuf,
    music: Music,
    carry: Option<Vec<f32>>,
}

impl MusicWorker {
    fn run(self) {
        let why = self.play();
        let mut m = lock_music(&self.shared);
        if let Some(why) = why {
            log::info!("the slideshow's music ended: {why}");
            m.ended = Some(why);
        }
        m.done = true;
    }

    /// Plays until faded out or ended; answers why it ended by itself.
    fn play(&self) -> Option<String> {
        let files = match music_files(&self.folder) {
            Ok(f) if f.is_empty() => return Some("The music folder holds no music".into()),
            Ok(f) => f,
            Err(e) => return Some(e),
        };
        let mut next = 0usize;
        let mut failures = 0usize;
        let mut playing: Option<Playing> = None;
        // The fade's progress in frames, once it began.
        let mut faded: Option<u64> = None;
        loop {
            std::thread::sleep(MUSIC_ROUND);
            let (paused, fade, dropped) = {
                let m = lock_music(&self.shared);
                (m.paused, m.fade, m.dropped)
            };
            if paused {
                if dropped {
                    return None;
                }
                continue;
            }
            if fade && faded.is_none() {
                faded = Some(0);
            }
            // The file playing, or the next one that opens.
            if playing.is_none() {
                if failures >= files.len() {
                    return Some(NO_MUSIC.into());
                }
                let Some(path) = files.get(next % files.len()) else { return Some(NO_MUSIC.into()) };
                next = next.wrapping_add(1);
                match Music::open(path) {
                    Ok(music) => {
                        let mut m = lock_music(&self.shared);
                        if m.played.len() < MAX_MUSIC_FILES {
                            m.played.push(path.clone());
                        }
                        drop(m);
                        playing = Some(Playing { path: path.clone(), music, carry: None });
                    }
                    Err(e) => {
                        self.skip(path, &e.reason);
                        failures += 1;
                        continue;
                    }
                }
            }
            let Some(p) = playing.as_mut() else { continue };
            let format = Format { rate: p.music.rate(), channels: p.music.channels() };
            // The output: opened at the file's format (after what was queued at another format
            // has been heard).
            let (mine, have) = {
                let m = lock_music(&self.shared);
                (m.mine, m.format)
            };
            let current = mine.is_some_and(|mine| mine == self.sound.stream());
            if mine.is_some() && !current {
                // Another player took the output without pausing the music.
                return None;
            }
            if !current || have != Some(format) {
                if current && let Some(have) = have {
                    let heard = self.sound.clock_ms().saturating_mul(u64::from(have.rate)) / 1000;
                    if heard < lock_music(&self.shared).queued {
                        continue;
                    }
                }
                if let Err(e) = self.sound.open(format) {
                    return Some(e.to_string());
                }
                let mut m = lock_music(&self.shared);
                if m.paused {
                    continue;
                }
                m.mine = Some(self.sound.stream());
                // What was not heard before a pause comes first, at the format it was queued at.
                let replay = if m.format == Some(format) { std::mem::take(&mut m.replay) } else { Vec::new() };
                m.replay.clear();
                m.format = Some(format);
                m.queued = 0;
                m.recent.clear();
                if !replay.is_empty() && self.sound.queue(&replay).is_ok() {
                    m.queued = (replay.len() / usize::from(format.channels.max(1))) as u64;
                    m.recent.extend(replay);
                }
            }
            // Queue ahead of what has been heard.
            let rate = u64::from(format.rate.max(1));
            let channels = usize::from(format.channels.max(1));
            let mut rounds = 0;
            loop {
                rounds += 1;
                let queued = lock_music(&self.shared).queued;
                let heard_ms = self.sound.clock_ms();
                if queued.saturating_mul(1000) / rate >= heard_ms.saturating_add(MUSIC_AHEAD_MS) || rounds > 64 {
                    break;
                }
                if faded.is_some_and(|f| f >= FADE_MS * rate / 1000) {
                    // Faded: once heard, the output closes.
                    if heard_ms.saturating_mul(rate) / 1000 >= queued {
                        let m = lock_music(&self.shared);
                        if m.mine == Some(self.sound.stream()) {
                            self.sound.stop();
                        }
                        return None;
                    }
                    break;
                }
                let block = match p.carry.take() {
                    Some(b) => Some(b),
                    None => match p.music.next_chunk() {
                        Ok(b) => b,
                        Err(e) => {
                            self.skip(&p.path, &e.reason);
                            None
                        }
                    },
                };
                let Some(mut block) = block else {
                    // The file ended: the next one.
                    playing = None;
                    break;
                };
                failures = 0;
                if let Some(done) = faded.as_mut() {
                    let total = (FADE_MS * rate / 1000).max(1);
                    for frame in block.chunks_mut(channels) {
                        let gain = 1.0 - (*done as f32 / total as f32).min(1.0);
                        frame.iter_mut().for_each(|v| *v *= gain);
                        *done += 1;
                    }
                }
                let mut m = lock_music(&self.shared);
                if m.paused || m.mine != Some(self.sound.stream()) {
                    drop(m);
                    p.carry = Some(block);
                    break;
                }
                if let Err(e) = self.sound.queue(&block) {
                    return Some(e.to_string());
                }
                m.queued = m.queued.saturating_add((block.len() / channels) as u64);
                m.recent.extend(block);
                let over = m.recent.len().saturating_sub(MAX_RECENT.min(usize::try_from(rate).unwrap_or(0).saturating_mul(channels)));
                m.recent.drain(..over);
            }
        }
    }

    fn skip(&self, path: &Path, why: &str) {
        log::info!("{}: skipped: {why}", path.display());
        let mut m = lock_music(&self.shared);
        if m.skipped.len() < MAX_MUSIC_FILES {
            m.skipped.push((path.to_path_buf(), why.to_string()));
        }
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
