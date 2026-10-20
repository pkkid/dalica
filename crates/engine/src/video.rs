//! Videos in the session (`editing.md` §11, `viewing.md` §4; `roadmap.md` M15 → *Video*, reserved
//! decision 2): what the session says about a video and the rules every still-only path asks, and
//! the edit view's player.
//!
//! - **What a photo is** ([`is_video`]): a video when the library says so ([`Library::kind`]: its
//!   container's facts, else the scan's word) or its name has a video's extension (so a path
//!   never decodes a video the scan has not described yet). [`kind_fields`] gives `photo.inspect`,
//!   `library.query` and `viewer.list` its `kind` (`"still"` | `"video"`) and `durationMs`.
//! - **`video.inspect {id}`** ([`inspect`], `cmd/video.rs`): the container's facts, whether Dalica
//!   plays it ([`playback`]) and its trim ([`trim_of`]).
//! - **Whether Dalica plays it** ([`playback`]): from the facts the scan read, never the file: the
//!   video by its codec (H.264 always, HEVC with the `hevc` build feature; `docs/video-decoders.md`),
//!   else "This video can't be played (ProRes)"; the sound when it is AAC, else the clip plays
//!   muted with the reason. The edit view's player (M15.26) asks [`playback`]; [`playable`] is
//!   no longer asked.
//! - **The still-only paths** ask [`edit_refusal`] and [`CROP_REFUSED`]: a video takes only its
//!   own edits (a rotation and the trim, `editing.md` §11), never an effect, a tuning, a crop or a
//!   pasted recipe; Save refuses it by its extension (`cmd/save.rs`), Export and the slideshow
//!   leave it out and count it, a collage leaves it out with [`collage_notice`], the face scan
//!   skips it and its thumbnail is the video tile ([`crate::thumbs`]: never decoded, never the
//!   error badge).
//!
//! **The player** ([`Video`], the commands `video.open`, `.play`, `.pause`, `.seek`, `.step`,
//! `.setStart`, `.setEnd`, `.volume`, `.state` of `cmd/video.rs`): at most one video open. Its
//! frames are decoded on a worker thread of its own ([`FrameSource`], at most [`PLAY_SIDE`] on the
//! long side, upright), never on the owner thread, which only reads what the worker left:
//!
//! - **The clock is the sound's.** Play opens the session's [`Sound`] at the clip's format (the
//!   worker found it on open) and the worker queues the clip's sound [`SOUND_AHEAD_MS`] ahead of
//!   what has been heard, at the volume; a clip without sound, or whose sound Dalica cannot decode,
//!   queues silence, so its clock runs the same way. The position is where play started plus the
//!   sound's clock ([`Session::video_position`]); with no sound output at all it is the session's
//!   clock instead.
//! - **Frames are decoded ahead**, at most [`FRAMES_AHEAD`], and the one shown is the last one due
//!   ([`Session::video_frame`]); every frame decoded is handed over, however late, so a decoder
//!   slower than the clip shows frames at its own rate (one superseded before the window asks is
//!   counted in `dropped`). A decoder that falls more than [`BEHIND_MS`] behind skips ahead to a
//!   key frame (`catch_up`: one already due; for a slow decoder one due by the time it is
//!   decoded, or the next one once it is [`FAR_BEHIND_MS`] behind), the frames skipped counted in
//!   `dropped`: the picture catches up with the sound at each key frame and the sound is never
//!   held back. The worker stops decoding to queue more sound after a quarter of what is queued.
//! - **The trim is respected:** Play starts at the trim's start when the position is outside it,
//!   and stops at its end (or the clip's), paused there.
//! - **Seek and step** are requests the worker answers: the reply comes at once with `pending`,
//!   the frame and the position follow (`video.state`).
//! - **Set Start / Set End** write `trim(start,end)` into the video's recipe as one step of its own
//!   history (replacing the trim it had): the container's question, so they work on a clip Dalica
//!   cannot play too (with `{ms}`).
//! - **A clip Dalica cannot play** opens with the reason ("This video can't be played (ProRes)");
//!   play, the steps, seek and Save Frame refuse with it. A muted clip plays without sound, and
//!   `video.volume` refuses with the reason.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{Clock, PhotoId, Timestamp};
use dalica_edit::Op as EditOp;
use dalica_edit::ops::video::{self as trim, Trim};
use dalica_image::Limits;
use dalica_image::formats::video::frames::{Frame, FrameError, FrameOptions, FrameSource};
use dalica_image::formats::video::sound::SoundSource;
use dalica_image::formats::video::{self as clip, SoundPlay, VideoInfo};
use dalica_image::sniff::VideoKind;
use dalica_library::{Library, PhotoKind, VideoFacts};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::history::Change;
use crate::session::Session;
use crate::sound::{Format, Sound};

/// What the edit view and the viewer say of a video Dalica cannot play (`viewing.md` §4).
pub const CANNOT_PLAY: &str = "This video can't be played";
/// Why Batch Edit, an effect, a tuning or Paste All Edits refuses a video.
pub const EDITS_REFUSED: &str = "A video takes no photo edits.";
/// Why Crop and Straighten refuse a video.
pub const CROP_REFUSED: &str = "A video can't be cropped.";
/// Why a collage of only videos does not open (until M15.28 takes their first frames).
pub const COLLAGE_REFUSED: &str = "Videos can't go in a collage yet.";
/// Why a player command is refused with no video open.
pub const NOT_OPEN: &str = "No video is open.";
/// Why play, seek and step wait while the worker opens the clip.
pub const OPENING: &str = "The video is still opening.";

/// The extensions of the videos the scan lists (`formats.md` §1), lowercase.
pub const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "m4v", "mkv"];

/// The longest side of the frames decoded for playback (a 4K clip plays at 1080p's size).
pub const PLAY_SIDE: u32 = 1920;
/// Frames decoded ahead of the one shown.
pub const FRAMES_AHEAD: usize = 8;
/// How far ahead of what has been heard the sound is queued, in milliseconds.
pub const SOUND_AHEAD_MS: u64 = 500;
/// A decoder whose last frame is this far behind the position skips ahead to a key frame due.
pub const BEHIND_MS: u64 = 100;
/// A decoder this far behind skips to the next key frame however far it is (the picture holds
/// until it is due): the most the picture lags the sound, but in a clip whose key frames are
/// further apart than that.
pub const FAR_BEHIND_MS: u64 = 1_000;
/// The most frames one step moves.
pub const MAX_STEP: i64 = 100_000;
/// The format silence is queued in, for a clip without sound it plays.
pub const SILENCE: Format = Format { rate: 48_000, channels: 1 };
/// Silence is queued this many milliseconds at a time.
const SILENCE_CHUNK_MS: u64 = 100;
/// How long the worker waits for a request while it plays, and while it does not.
const PUMP_WAIT: Duration = Duration::from_millis(4);
const IDLE_WAIT: Duration = Duration::from_millis(250);
/// The most decoding threads the player uses besides its own.
const MAX_THREADS: usize = 4;

/// Whether a file named `name` is a video by its extension.
pub fn is_video_name(name: &str) -> bool {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    VIDEO_EXTENSIONS.contains(&ext.as_str())
}

/// Whether photo `id` is a video: the library says so ([`Library::is_video`]) or its name has a
/// video's extension. An unknown id is not.
pub fn is_video(library: &Library, id: PhotoId) -> bool {
    library.photo(id).is_some_and(|p| library.is_video(id) || is_video_name(&p.name))
}

/// Photo `id`'s kind as the commands name it: `"video"` or `"still"`.
pub fn kind_name(library: &Library, id: PhotoId) -> &'static str {
    if is_video(library, id) { "video" } else { "still" }
}

/// `{kind, durationMs}` of photo `id`: `durationMs` the video's length in milliseconds, `null`
/// for a still or a video whose container did not say.
pub fn kind_fields(library: &Library, id: PhotoId) -> Value {
    let duration = library.video_facts(id).map(|f| f.duration_ms).filter(|ms| *ms > 0);
    json!({ "kind": kind_name(library, id), "durationMs": duration })
}

/// Puts [`kind_fields`] into the object `out` (a photo's reply), leaving anything else alone.
pub fn add_kind(out: &mut Value, library: &Library, id: PhotoId) {
    if let (Some(out), Value::Object(fields)) = (out.as_object_mut(), kind_fields(library, id)) {
        out.extend(fields);
    }
}

/// Puts the kind and duration of a file the viewer lists outside the library into the object
/// `out`: `facts` is what its scan read (`None`: a still).
pub fn add_listed_kind(out: &mut Value, facts: Option<&VideoFacts>) {
    let fields = match facts {
        Some(f) => json!({ "kind": PhotoKind::Video, "durationMs": (f.duration_ms > 0).then_some(f.duration_ms) }),
        None => json!({ "kind": PhotoKind::Still, "durationMs": null }),
    };
    if let (Some(out), Value::Object(fields)) = (out.as_object_mut(), fields) {
        out.extend(fields);
    }
}

/// The edit view's question before its player (M15.26), which now asks [`playback`]: every video
/// answers [`CANNOT_PLAY`] here. Nothing calls it any more.
pub fn playable(_facts: &VideoFacts) -> Result<(), &'static str> {
    Err(CANNOT_PLAY)
}

/// What Dalica plays of a video: its pictures, or why not, and its sound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playback {
    /// `Err("This video can't be played (ProRes)")` when its codec is not one Dalica decodes.
    pub video: Result<(), String>,
    pub sound: SoundPlay,
}

impl Playback {
    /// Why the clip plays without sound, when it has sound Dalica cannot decode.
    pub fn muted(&self) -> Option<&str> {
        match &self.sound {
            SoundPlay::Muted(why) => Some(why),
            _ => None,
        }
    }

    fn sound_word(&self) -> &'static str {
        match self.sound {
            SoundPlay::Plays => "plays",
            SoundPlay::Muted(_) => "muted",
            SoundPlay::Silent => "none",
        }
    }
}

/// The container facts of `facts` as the image crate reads them.
fn info_of(facts: &VideoFacts) -> VideoInfo {
    VideoInfo {
        kind: VideoKind::Mp4,
        duration_ms: (facts.duration_ms > 0).then_some(facts.duration_ms),
        size: (facts.width > 0 && facts.height > 0).then_some((facts.width, facts.height)),
        rotation: facts.rotation,
        created: None,
        video_codec: (!facts.video_codec.is_empty()).then(|| facts.video_codec.clone()),
        sound_codec: (!facts.sound_codec.is_empty()).then(|| facts.sound_codec.clone()),
        frame_rate: None,
        has_sound: facts.sound,
        location: None,
    }
}

/// What Dalica plays of a video of `facts`, from the facts alone (the codecs this build decodes,
/// `docs/video-decoders.md`). A stream a decoder refuses after all is found when it opens.
pub fn playback(facts: &VideoFacts) -> Playback {
    let p = clip::playability(&info_of(facts));
    Playback { video: p.video.map(|_| ()).map_err(|u| u.reason()), sound: p.sound }
}

/// The trim of a recipe as `(start, end)` milliseconds: its last `trim(start,end)` entry, when
/// that reads (`edit::ops::video`).
pub fn trim_of(edits: &[String]) -> Option<(u64, u64)> {
    trim::of(edits).map(|t| (t.start_ms, t.end_ms))
}

/// Why `op` may not be added to photo `id`'s recipe, if `id` is a video and `op` is not one of a
/// video's own edits (a rotation, the trim): [`EDITS_REFUSED`]. A still: `None`.
pub fn edit_refusal(library: &Library, id: PhotoId, op: &EditOp) -> Option<&'static str> {
    let own = matches!(op, EditOp::Rotate(_) | EditOp::Listed { name: "trim", .. });
    (!own && is_video(library, id)).then_some(EDITS_REFUSED)
}

/// What a collage says of the videos it left out (`None` when it left none): "1 video left out:
/// videos can't go in a collage yet."
pub fn collage_notice(videos: usize) -> Option<String> {
    match videos {
        0 => None,
        1 => Some("1 video left out: videos can't go in a collage yet.".into()),
        n => Some(format!("{n} videos left out: videos can't go in a collage yet.")),
    }
}

/// Puts `videos` (how many videos a collage left out) and its `notice` ([`collage_notice`]) into
/// the object `out` (`create.collage`'s and `collage.clips.add`'s reply), when it left any out.
pub fn note_collage_videos(out: &mut Value, videos: usize) {
    if videos == 0 {
        return;
    }
    if let Some(out) = out.as_object_mut() {
        out.insert("videos".into(), json!(videos));
        out.insert("notice".into(), json!(collage_notice(videos)));
    }
}

/// `video.inspect`'s reply for video `id` (the caller checked it is one): its path, kind and
/// duration, the container's facts (the stored size, the turn, the size shown, the codecs, the
/// sound, the frame rate in frames per 1 000 s), `playable` with the `reason` when it is not, the
/// sound (`"plays"`, `"muted"` with `muted` the reason, `"none"`) and its `trim`
/// (`{startMs, endMs}` or `null`).
pub fn inspect(library: &Library, id: PhotoId) -> Option<Value> {
    let photo = library.photo(id)?;
    let folder = library.folder(photo.folder)?;
    let empty = VideoFacts::default();
    let facts = library.video_facts(id).unwrap_or(&empty);
    let (dw, dh) = facts.display_size();
    let play = playback(facts);
    let trim = trim_of(&library.info(id).edits).map(|(start, end)| json!({ "startMs": start, "endMs": end }));
    let mut out = json!({
        "id": id, "name": photo.name, "path": folder.path.join(&photo.name),
        "width": facts.width, "height": facts.height, "rotation": facts.rotation,
        "displayed": { "width": dw, "height": dh },
        "videoCodec": facts.video_codec, "soundCodec": facts.sound_codec, "sound": facts.sound,
        "frameRateMilli": facts.frame_rate_milli,
        "playable": play.video.is_ok(), "reason": play.video.as_ref().err(),
        "soundPlays": play.sound_word(), "muted": play.muted(),
        "trim": trim,
    });
    add_kind(&mut out, library, id);
    Some(out)
}

// --- The player ------------------------------------------------------------------------------------

/// What drives the position while playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Timing {
    /// The sound's clock (opened when play started).
    Sound,
    /// The session's clock, from this time (no sound output).
    Wall(Timestamp),
}

/// What the owner and the worker share.
#[derive(Debug)]
struct Shared {
    /// Bumped by every request that changes what is shown: the worker's answers to an older one
    /// are dropped.
    generation: u64,
    /// The worker opened the clip.
    ready: bool,
    /// Why the worker could not open or decode it.
    failed: Option<String>,
    /// The format the worker queues sound in.
    format: Format,
    /// Why the clip plays without sound, found when its sound would not open.
    muted: Option<String>,
    frame_count: usize,
    /// The clip's length from its frames, in milliseconds.
    frames_ms: u64,
    shown: Option<Arc<Frame>>,
    ahead: VecDeque<(u64, Arc<Frame>)>,
    /// A seek or step's answer: the position it lands on.
    landed: Option<(u64, u64)>,
    /// A seek or step not answered yet.
    busy: bool,
    dropped: u64,
    playing: bool,
    /// The position when play started (or where it is paused).
    anchor_ms: u64,
    timing: Timing,
    /// Play stops here.
    end_ms: u64,
    /// The volume, 0 … 1.
    volume: f32,
}

/// What the worker is asked.
#[derive(Debug)]
enum Request {
    /// Show the frame at a time.
    Show { generation: u64, ms: u64 },
    /// Show the frame `frames` from the one shown.
    Step { generation: u64, frames: i64 },
    /// Play from a time.
    Play { generation: u64, from_ms: u64 },
}

/// What the player's worker decodes frames with: a [`FrameSource`], or a test's stand-in
/// ([`Session::set_video_frames`]).
pub trait PlayFrames: Send {
    /// Frames in display order.
    fn frame_count(&self) -> usize;
    /// The display time of frame `index`, in milliseconds.
    fn time_of(&self, index: usize) -> Option<u64>;
    /// The frame shown at `ms`.
    fn index_at(&self, ms: u64) -> usize;
    /// The first frame at or after `index` that decoding can start at ([`FrameSource::key_after`]).
    fn key_after(&self, index: usize) -> Option<usize>;
    /// Frame `index` in display order.
    fn frame(&mut self, index: usize) -> Result<Frame, FrameError>;
    /// The frame after the one returned last, or `None` at the end.
    fn next_frame(&mut self) -> Result<Option<Frame>, FrameError>;
    /// The frame returned last.
    fn current_index(&self) -> Option<usize>;
    /// What decoding the frame returned last took, in milliseconds, when the source says (a test's
    /// decoder on a virtual clock); `None`: the worker measures it.
    fn decode_ms(&self) -> Option<u64> {
        None
    }
    /// The frame shown at `ms`, decoded.
    fn frame_at(&mut self, ms: u64) -> Result<Frame, FrameError> {
        let index = self.index_at(ms);
        self.frame(index)
    }
}

impl PlayFrames for FrameSource {
    fn frame_count(&self) -> usize {
        FrameSource::frame_count(self)
    }
    fn time_of(&self, index: usize) -> Option<u64> {
        FrameSource::time_of(self, index)
    }
    fn index_at(&self, ms: u64) -> usize {
        FrameSource::index_at(self, ms)
    }
    fn key_after(&self, index: usize) -> Option<usize> {
        FrameSource::key_after(self, index)
    }
    fn frame(&mut self, index: usize) -> Result<Frame, FrameError> {
        FrameSource::frame(self, index)
    }
    fn next_frame(&mut self) -> Result<Option<Frame>, FrameError> {
        FrameSource::next_frame(self)
    }
    fn current_index(&self) -> Option<usize> {
        FrameSource::current_index(self)
    }
    fn frame_at(&mut self, ms: u64) -> Result<Frame, FrameError> {
        FrameSource::frame_at(self, ms)
    }
}

/// Opens a clip's frames for the player's worker, on the worker (a test's stand-in for
/// [`FrameSource::open`]); `Err` is the reason the player shows.
pub type FrameOpener = Arc<dyn Fn(&Path) -> Result<Box<dyn PlayFrames>, String> + Send + Sync>;

/// The video open in the edit view.
struct Open {
    id: PhotoId,
    path: PathBuf,
    /// The clip's length from its facts (0: unknown).
    duration_ms: u64,
    playback: Playback,
    shared: Arc<Mutex<Shared>>,
    /// To the worker (none for a clip Dalica cannot play); dropping it ends the worker.
    worker: Option<Sender<Request>>,
}

impl std::fmt::Debug for Open {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Open").field("id", &self.id).finish_non_exhaustive()
    }
}

/// The edit view's video player.
#[derive(Default)]
pub struct Video {
    open: Option<Open>,
    /// Opens the frames instead of [`FrameSource::open`] (tests).
    opener: Option<FrameOpener>,
}

impl std::fmt::Debug for Video {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Video").field("open", &self.open).field("opener", &self.opener.is_some()).finish()
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The position: where play started plus what the clock says has passed, up to the end.
fn position(s: &Shared, sound: &dyn Sound, clock: &dyn Clock) -> u64 {
    if !s.playing {
        return s.anchor_ms;
    }
    let passed = match s.timing {
        Timing::Sound => sound.clock_ms(),
        Timing::Wall(from) => u64::try_from(clock.now().as_millis().saturating_sub(from.as_millis())).unwrap_or(0),
    };
    s.anchor_ms.saturating_add(passed).min(s.end_ms)
}

impl Video {
    /// The video open in the edit view, if any.
    pub fn open(&self) -> Option<PhotoId> {
        self.open.as_ref().map(|o| o.id)
    }

    /// Whether it is playing.
    pub fn is_playing(&self) -> bool {
        self.open.as_ref().is_some_and(|o| lock(&o.shared).playing)
    }
}

impl Session {
    /// The edit view's video player.
    pub fn video(&self) -> &Video {
        &self.video
    }

    /// Makes the player's worker open the frames of the videos opened from now on with `opener`
    /// instead of [`FrameSource::open`] (`None`: the clip's own decoder again). For tests: a
    /// decoder of a known speed on the session's clock.
    pub fn set_video_frames(&mut self, opener: Option<FrameOpener>) {
        self.video.opener = opener;
    }

    /// Whether photo `id` is a video ([`is_video`]).
    pub fn is_video(&self, id: PhotoId) -> bool {
        is_video(self.library(), id)
    }

    /// The open video's position in milliseconds (paused at its end once play reaches it).
    pub fn video_position(&mut self) -> Option<u64> {
        self.video_sync();
        let open = self.video.open.as_ref()?;
        Some(position(&lock(&open.shared), self.sound.as_ref(), self.clock.as_ref()))
    }

    /// The frame to show now: the last one due ([`Video`]); `None` before the worker has one.
    pub fn video_frame(&mut self) -> Option<Arc<Frame>> {
        self.video_sync();
        self.video.open.as_ref().and_then(|o| lock(&o.shared).shown.clone())
    }

    /// Brings the shown frame and the position up to date: a seek or step's answer taken, the
    /// frames due taken from those decoded ahead (all but the last of them dropped), play paused
    /// at its end.
    fn video_sync(&mut self) {
        let Some(open) = self.video.open.as_ref() else { return };
        let mut s = lock(&open.shared);
        if let Some((generation, ms)) = s.landed.take()
            && generation == s.generation
        {
            s.anchor_ms = ms;
        }
        let pos = position(&s, self.sound.as_ref(), self.clock.as_ref());
        let mut taken = 0u64;
        while let Some((generation, frame)) = s.ahead.front().cloned() {
            if generation != s.generation {
                s.ahead.pop_front();
                continue;
            }
            if frame.time_ms > pos {
                break;
            }
            s.ahead.pop_front();
            s.shown = Some(frame);
            taken += 1;
        }
        s.dropped = s.dropped.saturating_add(taken.saturating_sub(1));
        if s.playing && pos >= s.end_ms {
            s.playing = false;
            s.anchor_ms = s.end_ms;
            s.generation += 1;
            s.ahead.clear();
            drop(s);
            self.sound.stop();
        }
    }

    fn open_video(&self) -> Result<&Open, CmdError> {
        self.video.open.as_ref().ok_or_else(|| CmdError::Disabled(NOT_OPEN.into()))
    }

    /// The open video's trim, as the recipe has it now, else the whole clip.
    fn video_bounds(&self, open: &Open) -> (u64, u64) {
        let length = if open.duration_ms > 0 { open.duration_ms } else { lock(&open.shared).frames_ms };
        let whole = if length > 0 { length } else { trim::MAX_MS };
        match trim_of(&self.library().info(open.id).edits) {
            Some((start, end)) => (start.min(whole), end.min(whole)),
            None => (0, whole),
        }
    }

    /// `video.open {id}`: opens video `id` in the player (the one open before is closed). A clip
    /// Dalica cannot play opens with the reason; otherwise its worker starts opening it.
    pub(crate) fn video_open(&mut self, id: PhotoId) -> Result<Value, CmdError> {
        let lib = self.library();
        let photo = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        if !is_video(lib, id) {
            return Err(CmdError::Disabled(format!("{} is not a video.", photo.name)));
        }
        if self.video.open.as_ref().is_some_and(|o| o.id == id) {
            return self.video_state();
        }
        let folder = lib.folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder for photo {id}")))?;
        let path = folder.path.join(&photo.name);
        let empty = VideoFacts::default();
        let facts = lib.video_facts(id).unwrap_or(&empty).clone();
        self.video_close();
        let playback = playback(&facts);
        let volume = f32::from(self.settings().video().volume) / 100.0;
        let shared = Arc::new(Mutex::new(Shared {
            generation: 0,
            ready: false,
            failed: None,
            format: SILENCE,
            muted: None,
            frame_count: 0,
            frames_ms: 0,
            shown: None,
            ahead: VecDeque::new(),
            landed: None,
            busy: false,
            dropped: 0,
            playing: false,
            anchor_ms: 0,
            timing: Timing::Sound,
            end_ms: facts.duration_ms,
            volume,
        }));
        let mut open = Open { id, path: path.clone(), duration_ms: facts.duration_ms, playback, shared: Arc::clone(&shared), worker: None };
        let start = trim_of(&self.library().info(id).edits).map_or(0, |(s, _)| s);
        lock(&shared).anchor_ms = start;
        if open.playback.video.is_ok() {
            let (tx, rx) = mpsc::channel();
            let sound_plays = matches!(open.playback.sound, SoundPlay::Plays);
            let opener = self.video.opener.clone();
            let worker = Worker { path, shared: Arc::clone(&shared), sound: self.sound(), clock: Arc::clone(&self.clock), rx, opener };
            let spawned = std::thread::Builder::new().name("dalica-video".into()).spawn(move || worker.run(sound_plays, start));
            match spawned {
                Ok(_) => open.worker = Some(tx),
                Err(e) => lock(&shared).failed = Some(format!("the video could not be opened: {e}")),
            }
        }
        self.video.open = Some(open);
        self.video_state()
    }

    /// Closes the open video, stopping its sound.
    pub(crate) fn video_close(&mut self) {
        if let Some(open) = self.video.open.take() {
            if lock(&open.shared).playing {
                self.sound.stop();
            }
            drop(open);
        }
    }

    /// Refuses a player command that needs the pictures: none open, a clip Dalica cannot play, one
    /// still opening or one that failed.
    fn video_pictures(&self) -> Result<&Open, CmdError> {
        let open = self.open_video()?;
        if let Err(why) = &open.playback.video {
            return Err(CmdError::Disabled(why.clone()));
        }
        let s = lock(&open.shared);
        if let Some(why) = &s.failed {
            return Err(CmdError::Disabled(why.clone()));
        }
        if !s.ready {
            return Err(CmdError::Disabled(OPENING.into()));
        }
        Ok(open)
    }

    /// `video.state`: the open video's player, as `{open, id, playable, reason, sound, muted,
    /// ready, playing, positionMs, durationMs, trim, frame, pending, dropped, volume}`.
    pub(crate) fn video_state(&mut self) -> Result<Value, CmdError> {
        self.video_sync();
        let volume = self.settings().video().volume;
        let Some(open) = self.video.open.as_ref() else {
            return Ok(json!({ "open": false, "id": null, "volume": volume }));
        };
        let (start, end) = self.video_bounds(open);
        let trim = trim_of(&self.library().info(open.id).edits).map(|_| json!({ "startMs": start, "endMs": end }));
        let s = lock(&open.shared);
        let pos = position(&s, self.sound.as_ref(), self.clock.as_ref());
        let frame = s.shown.as_ref().map(|f| json!({ "index": f.index, "timeMs": f.time_ms, "width": f.image.width, "height": f.image.height }));
        let duration = if open.duration_ms > 0 { open.duration_ms } else { s.frames_ms };
        let reason = open.playback.video.as_ref().err().cloned().or_else(|| s.failed.clone());
        let muted = s.muted.clone().or_else(|| open.playback.muted().map(str::to_string));
        Ok(json!({
            "open": true, "id": open.id,
            "playable": reason.is_none(), "reason": reason,
            "sound": if muted.is_some() { "muted" } else { open.playback.sound_word() }, "muted": muted,
            "ready": s.ready, "playing": s.playing, "positionMs": pos, "durationMs": duration,
            "frameCount": s.frame_count, "trim": trim, "frame": frame,
            "pending": s.busy || (open.worker.is_some() && !s.ready && s.failed.is_none()),
            "dropped": s.dropped, "volume": volume,
        }))
    }

    /// `video.play`: plays from the position (from the trim's start when it is outside the trim)
    /// to the trim's end.
    pub(crate) fn video_play(&mut self) -> Result<Value, CmdError> {
        self.video_sync();
        let open = self.video_pictures()?;
        let (start, end) = self.video_bounds(open);
        let (generation, from, format) = {
            let mut s = lock(&open.shared);
            if s.playing {
                drop(s);
                return self.video_state();
            }
            let at = s.anchor_ms;
            let from = if at < start || at >= end { start } else { at };
            s.generation += 1;
            s.ahead.clear();
            s.anchor_ms = from;
            s.end_ms = end;
            s.playing = true;
            s.busy = false;
            s.landed = None;
            (s.generation, from, s.format)
        };
        let timing = match self.sound.open(format) {
            Ok(()) => Timing::Sound,
            Err(e) => {
                log::info!("the video plays without sound: {e}");
                Timing::Wall(self.clock.now())
            }
        };
        let open = self.open_video()?;
        lock(&open.shared).timing = timing;
        if let Some(tx) = &open.worker {
            let _ = tx.send(Request::Play { generation, from_ms: from });
        }
        self.video_state()
    }

    /// Stops playing at the position; the shared state's generation moves on.
    fn video_halt(&mut self) -> Result<(), CmdError> {
        self.video_sync();
        let open = self.open_video()?;
        let pos = {
            let s = lock(&open.shared);
            if !s.playing {
                return Ok(());
            }
            position(&s, self.sound.as_ref(), self.clock.as_ref())
        };
        let mut s = lock(&open.shared);
        s.playing = false;
        s.anchor_ms = pos;
        s.generation += 1;
        s.ahead.clear();
        drop(s);
        self.sound.stop();
        Ok(())
    }

    /// `video.pause`.
    pub(crate) fn video_pause(&mut self) -> Result<Value, CmdError> {
        self.video_pictures()?;
        self.video_halt()?;
        self.video_state()
    }

    /// `video.seek {ms}`: shows the frame at `ms` (playing on from there when it was playing).
    pub(crate) fn video_seek(&mut self, ms: u64) -> Result<Value, CmdError> {
        let open = self.video_pictures()?;
        let playing = lock(&open.shared).playing;
        let whole = if open.duration_ms > 0 { open.duration_ms } else { lock(&open.shared).frames_ms.max(1) };
        let ms = ms.min(whole);
        self.video_halt()?;
        let open = self.open_video()?;
        {
            let mut s = lock(&open.shared);
            s.generation += 1;
            s.anchor_ms = ms;
            s.busy = true;
            let generation = s.generation;
            drop(s);
            if let Some(tx) = &open.worker {
                let _ = tx.send(Request::Show { generation, ms });
            }
        }
        if playing {
            let open = self.open_video()?;
            let (start, end) = self.video_bounds(open);
            if ms >= start && ms < end {
                return self.video_play();
            }
        }
        self.video_state()
    }

    /// `video.step {frames}`: pauses and shows the frame `frames` from the one shown.
    pub(crate) fn video_step(&mut self, frames: i64) -> Result<Value, CmdError> {
        self.video_pictures()?;
        self.video_halt()?;
        let open = self.open_video()?;
        let mut s = lock(&open.shared);
        s.generation += 1;
        s.busy = true;
        let generation = s.generation;
        drop(s);
        if let Some(tx) = &open.worker {
            let _ = tx.send(Request::Step { generation, frames });
        }
        self.video_state()
    }

    /// `video.setStart {ms?}` / `video.setEnd {ms?}`: the trim's start (or end) at `ms`, else at the
    /// position, the other end kept (the clip's start or end when it had no trim); one step of the
    /// video's history, replacing the trim it had.
    pub(crate) fn video_set_trim(&mut self, end: bool, ms: Option<u64>) -> Result<Value, CmdError> {
        self.video_sync();
        let open = self.open_video()?;
        let id = open.id;
        let at = match ms {
            Some(ms) => ms,
            None => position(&lock(&open.shared), self.sound.as_ref(), self.clock.as_ref()),
        };
        let length = if open.duration_ms > 0 { open.duration_ms } else { lock(&open.shared).frames_ms };
        let at = if length > 0 { at.min(length) } else { at };
        let edits = self.library().info(id).edits.clone();
        let old = trim::of(&edits).unwrap_or_default();
        let whole_end = if length > 0 { length } else { trim::MAX_MS };
        let (start, stop) = if end { (old.start_ms, at) } else { (at, old.end_ms.min(whole_end)) };
        let t = Trim::new(start, stop)
            .map_err(|e| CmdError::Disabled(format!("{}: {}", if end { "Set End" } else { "Set Start" }, trim_refusal(&e.reason))))?;
        let op = EditOp::parse(&t.entry());
        let change = match trim::find(&edits) {
            Some((i, _)) => Change::ReplaceAt(i, op),
            None => Change::Append(op),
        };
        self.photo_step(id, change, None)?;
        self.video_state()
    }

    /// `video.volume {volume}`: the volume (0–100), kept in the settings; refused with the reason
    /// for a clip that plays muted.
    pub(crate) fn video_volume(&mut self, volume: u8) -> Result<Value, CmdError> {
        if let Some(open) = self.video.open.as_ref() {
            let muted = lock(&open.shared).muted.clone().or_else(|| open.playback.muted().map(str::to_string));
            if let Some(why) = muted {
                return Err(CmdError::Disabled(why));
            }
            lock(&open.shared).volume = f32::from(volume) / 100.0;
        }
        self.change_settings(|s| {
            let mut v = s.video();
            v.volume = volume;
            s.set_video(&v);
        });
        self.video_state()
    }

    /// The open video's path, id, trim (or the whole clip) and the time of the frame shown, for
    /// Save Frame and Export Clip (`video_output.rs`).
    pub(crate) fn video_target(&mut self) -> Result<(PhotoId, PathBuf, (u64, u64), u64), CmdError> {
        self.video_sync();
        let open = self.open_video()?;
        let bounds = self.video_bounds(open);
        let s = lock(&open.shared);
        let at = match &s.shown {
            Some(f) if !s.playing && !s.busy => f.time_ms,
            _ => position(&s, self.sound.as_ref(), self.clock.as_ref()),
        };
        Ok((open.id, open.path.clone(), bounds, at))
    }

    /// Refuses Save Frame as the player's pictures refuse it ([`Session::video_pictures`]).
    pub(crate) fn video_frames_allowed(&self) -> Result<(), CmdError> {
        self.video_pictures().map(|_| ())
    }
}

/// A trim's refusal in the words of the trim ruler.
fn trim_refusal(reason: &str) -> &str {
    if reason.contains("before the end") { "the start must come before the end" } else { reason }
}

// --- The worker --------------------------------------------------------------------------------------

/// The player's worker: opens the clip, answers seeks and steps, decodes ahead and queues the sound
/// while playing.
struct Worker {
    path: PathBuf,
    shared: Arc<Mutex<Shared>>,
    sound: Arc<dyn Sound>,
    clock: Arc<dyn Clock>,
    rx: Receiver<Request>,
    opener: Option<FrameOpener>,
}

/// One play's progress.
struct Play {
    generation: u64,
    from_ms: u64,
    /// Sound frames queued since play started.
    queued: u64,
    /// The clip's sound has run out (silence follows).
    sound_done: bool,
    frames_done: bool,
    /// The last frame handed to the owner.
    handed: Option<usize>,
    /// What the last three frames decoded in turn took, in milliseconds ([`PlayFrames::decode_ms`],
    /// else measured; their median is the decoder's speed: one held up by something else does not
    /// count).
    costs: [u64; 3],
}

impl Worker {
    fn run(self, sound_plays: bool, start_ms: u64) {
        let limits = Limits::default();
        let threads = std::thread::available_parallelism().map_or(0, |n| (n.get() / 2).min(MAX_THREADS));
        let options = FrameOptions { max_side: Some(PLAY_SIDE), threads };
        let opened = match &self.opener {
            Some(open) => open(&self.path),
            None => FrameSource::open(&self.path, &limits, options).map(|f| Box::new(f) as Box<dyn PlayFrames>).map_err(|e| e.reason()),
        };
        let mut frames = match opened {
            Ok(f) => f,
            Err(why) => {
                lock(&self.shared).failed = Some(why);
                return;
            }
        };
        let frames = frames.as_mut();
        let mut sound = None;
        let mut muted = None;
        if sound_plays {
            match SoundSource::open(&self.path, &limits) {
                Ok(s) => sound = Some(s),
                Err(m) => muted = Some(m.reason),
            }
        }
        let format = sound.as_ref().map_or(SILENCE, |s| Format { rate: s.rate(), channels: s.channels() });
        let format = format.check().unwrap_or(SILENCE);
        let count = frames.frame_count();
        let last = count.checked_sub(1).and_then(|i| frames.time_of(i)).unwrap_or(0);
        let step = if count > 1 { last / (count as u64 - 1).max(1) } else { 0 };
        let first = frames.frame_at(start_ms);
        {
            let mut s = lock(&self.shared);
            s.format = format;
            s.muted = muted;
            s.frame_count = count;
            s.frames_ms = last.saturating_add(step);
            if s.end_ms == 0 {
                s.end_ms = s.frames_ms;
            }
            match first {
                Ok(f) => {
                    if s.shown.is_none() {
                        s.shown = Some(Arc::new(f));
                    }
                    s.ready = true;
                }
                Err(e) => s.failed = Some(e.reason()),
            }
        }
        let mut play: Option<Play> = None;
        loop {
            let wait = if play.is_some() { PUMP_WAIT } else { IDLE_WAIT };
            match self.rx.recv_timeout(wait) {
                Ok(r) => self.handle(r, frames, sound.as_mut(), &mut play),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            while let Ok(r) = self.rx.try_recv() {
                self.handle(r, frames, sound.as_mut(), &mut play);
            }
            if let Some(p) = play.as_mut() {
                let current = {
                    let s = lock(&self.shared);
                    s.playing && s.generation == p.generation
                };
                if !current {
                    play = None;
                    continue;
                }
                self.pump(p, frames, sound.as_mut(), format);
            }
        }
    }

    /// Whether `generation` is still the one asked for.
    fn current(&self, generation: u64) -> bool {
        lock(&self.shared).generation == generation
    }

    fn handle(&self, r: Request, frames: &mut dyn PlayFrames, sound: Option<&mut SoundSource>, play: &mut Option<Play>) {
        match r {
            Request::Show { generation, ms } => {
                *play = None;
                if !self.current(generation) {
                    return;
                }
                let got = frames.frame_at(ms);
                self.land(generation, got.map(|f| (f, ms)));
            }
            Request::Step { generation, frames: n } => {
                *play = None;
                if !self.current(generation) {
                    return;
                }
                let shown = lock(&self.shared).shown.as_ref().map(|f| f.index);
                let base = shown.or(frames.current_index()).unwrap_or(0);
                let last = frames.frame_count().saturating_sub(1);
                let target = i64::try_from(base).unwrap_or(0).saturating_add(n).clamp(0, i64::try_from(last).unwrap_or(0));
                let got = frames.frame(usize::try_from(target).unwrap_or(0));
                self.land(
                    generation,
                    got.map(|f| {
                        let t = f.time_ms;
                        (f, t)
                    }),
                );
            }
            Request::Play { generation, from_ms } => {
                if !self.current(generation) {
                    return;
                }
                if let Some(s) = sound {
                    s.seek(from_ms);
                }
                match frames.frame_at(from_ms) {
                    Ok(f) => {
                        let mut s = lock(&self.shared);
                        if s.generation == generation {
                            s.shown = Some(Arc::new(f));
                        }
                    }
                    Err(e) => {
                        lock(&self.shared).failed = Some(e.reason());
                        return;
                    }
                }
                *play = Some(Play {
                    generation,
                    from_ms,
                    queued: 0,
                    sound_done: false,
                    frames_done: false,
                    handed: frames.current_index(),
                    costs: [0; 3],
                });
            }
        }
    }

    /// A seek or step's frame, or why it failed.
    fn land(&self, generation: u64, got: Result<(Frame, u64), FrameError>) {
        let mut s = lock(&self.shared);
        if s.generation != generation {
            return;
        }
        s.busy = false;
        match got {
            Ok((f, ms)) => {
                s.shown = Some(Arc::new(f));
                s.landed = Some((generation, ms));
            }
            Err(e) => log::warn!("{}: a frame could not be decoded: {}", self.path.display(), e.reason()),
        }
    }

    /// Queues the sound ahead of what has been heard and decodes the frames ahead of the one shown.
    fn pump(&self, p: &mut Play, frames: &mut dyn PlayFrames, mut sound: Option<&mut SoundSource>, format: Format) {
        let (end, volume, timing) = {
            let s = lock(&self.shared);
            (s.end_ms, s.volume, s.timing)
        };
        let rate = u64::from(format.rate.max(1));
        let channels = usize::from(format.channels.max(1));
        let ms_queued = |p: &Play| p.queued.saturating_mul(1000) / rate;
        if timing == Timing::Sound {
            let room = end.saturating_sub(p.from_ms);
            let heard = self.sound.clock_ms();
            let mut rounds = 0;
            while ms_queued(p) < heard.saturating_add(SOUND_AHEAD_MS) && ms_queued(p) < room && rounds < 64 {
                rounds += 1;
                let left = room.saturating_sub(ms_queued(p)).saturating_mul(rate) / 1000;
                let mut samples = match (p.sound_done, sound.as_deref_mut()) {
                    (false, Some(s)) => match s.next_chunk() {
                        Ok(Some(chunk)) => {
                            // A packet that starts before play started is cut to it.
                            let skip = p.from_ms.saturating_sub(chunk.time_ms).saturating_mul(rate) / 1000;
                            let skip = usize::try_from(skip).unwrap_or(usize::MAX).saturating_mul(channels);
                            chunk.samples.get(skip.min(chunk.samples.len())..).unwrap_or_default().to_vec()
                        }
                        Ok(None) | Err(_) => {
                            p.sound_done = true;
                            continue;
                        }
                    },
                    _ => vec![0.0; usize::try_from(SILENCE_CHUNK_MS * rate / 1000).unwrap_or(0) * channels],
                };
                let frames_left = usize::try_from(left).unwrap_or(usize::MAX);
                samples.truncate(frames_left.saturating_mul(channels));
                samples.truncate(samples.len() / channels * channels);
                if samples.is_empty() {
                    break;
                }
                for v in &mut samples {
                    *v *= volume;
                }
                if let Err(e) = self.sound.queue(&samples) {
                    // The output went away: the session's clock carries on from here.
                    log::warn!("the video's sound stopped: {e}");
                    let mut s = lock(&self.shared);
                    let at = s.anchor_ms.saturating_add(heard);
                    s.anchor_ms = at;
                    s.timing = Timing::Wall(self.clock.now());
                    break;
                }
                p.queued = p.queued.saturating_add((samples.len() / channels) as u64);
            }
        }
        // Frames ahead. Each one decoded is handed over, however late: the owner shows the last
        // one due, so a decoder slower than the clip shows frames at its own rate. One that falls
        // behind skips ahead to a key frame ([`catch_up`]); one that spends a quarter of the sound
        // queued ahead decoding stops to queue more first.
        let started = position(&lock(&self.shared), self.sound.as_ref(), self.clock.as_ref());
        let mut decoded = 0;
        while !p.frames_done && decoded < FRAMES_AHEAD {
            let (pos, room) = {
                let s = lock(&self.shared);
                if s.generation != p.generation {
                    return;
                }
                (position(&s, self.sound.as_ref(), self.clock.as_ref()), s.ahead.len() < FRAMES_AHEAD)
            };
            if !room || (decoded > 0 && pos.saturating_sub(started) >= SOUND_AHEAD_MS / 4) {
                break;
            }
            decoded += 1;
            let cost = {
                let mut c = p.costs;
                c.sort_unstable();
                c.get(1).copied().unwrap_or(0)
            };
            let skip = catch_up(frames, pos, cost);
            let got = match skip {
                Some(key) => frames.frame(key).map(Some),
                None => {
                    let t = Instant::now();
                    let got = frames.next_frame();
                    let took = frames.decode_ms().unwrap_or_else(|| u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX));
                    p.costs.rotate_left(1);
                    if let Some(c) = p.costs.last_mut() {
                        *c = took;
                    }
                    got
                }
            };
            let f = match got {
                Ok(Some(f)) => f,
                Ok(None) => {
                    p.frames_done = true;
                    break;
                }
                Err(e) => {
                    log::warn!("{}: a frame could not be decoded: {}", self.path.display(), e.reason());
                    p.frames_done = true;
                    break;
                }
            };
            if f.time_ms >= end {
                p.frames_done = true;
                break;
            }
            let mut s = lock(&self.shared);
            if s.generation != p.generation {
                return;
            }
            if let Some((key, from)) = skip.zip(p.handed) {
                s.dropped = s.dropped.saturating_add(key.saturating_sub(from).saturating_sub(1) as u64);
            }
            p.handed = Some(f.index);
            s.ahead.push_back((p.generation, Arc::new(f)));
        }
    }
}

/// Where a decoder that fell behind skips to, if it should, when the frame it decoded last is more
/// than [`BEHIND_MS`] behind the position `pos`: the last key frame after it already due (always
/// cheaper than decoding the frames before it); for a decoder slower than the clip (`cost_ms` a
/// frame, the median of the last three), also one due by the time it is decoded (so the picture
/// never waits for it), or, when it is more than [`FAR_BEHIND_MS`] behind, the next key frame
/// however far (the picture waits for it). `None`: decode on (a decoder keeping up or able to, or
/// a clip without a key frame ahead).
fn catch_up(frames: &dyn PlayFrames, pos: u64, cost_ms: u64) -> Option<usize> {
    let last = frames.current_index()?;
    let at = frames.time_of(last)?;
    let behind = pos.saturating_sub(at);
    if behind <= BEHIND_MS {
        return None;
    }
    let next = frames.key_after(last.checked_add(1)?)?;
    let latest_by = |horizon: u64| {
        let mut best = None;
        let mut key = next;
        for _ in 0..frames.frame_count() {
            if frames.time_of(key).is_none_or(|t| t > horizon) {
                break;
            }
            best = Some(key);
            match key.checked_add(1).and_then(|k| frames.key_after(k)) {
                Some(k) if k > key => key = k,
                _ => break,
            }
        }
        best
    };
    if let Some(key) = latest_by(pos) {
        return Some(key);
    }
    let interval = last.checked_add(1).and_then(|n| frames.time_of(n)).map_or(0, |t| t.saturating_sub(at));
    if cost_ms < interval {
        return None;
    }
    latest_by(pos.saturating_add(cost_ms)).or((behind > FAR_BEHIND_MS).then_some(next))
}

#[cfg(test)]
mod tests {
    use super::{collage_notice, is_video_name, playback, trim_of};
    use dalica_library::VideoFacts;

    fn edits(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_video_is_known_by_its_extension_in_any_case() {
        assert!(is_video_name("clip.MP4") && is_video_name("a.mov") && is_video_name("b.m4v") && is_video_name("c.mkv"));
        assert!(!is_video_name("a.jpg") && !is_video_name("mp4") && !is_video_name("") && !is_video_name("clip.mp4.jpg"));
    }

    #[test]
    fn the_trim_is_the_last_readable_trim_entry() {
        assert_eq!(trim_of(&edits(&[])), None);
        assert_eq!(trim_of(&edits(&["rotate(90)", "trim(500,1500)"])), Some((500, 1500)));
        assert_eq!(trim_of(&edits(&["trim(1,2)", "rotate(90)", "trim(0,900)"])), Some((0, 900)));
        for bad in ["trim(1500,500)", "trim(5,5)", "trim(a,b)", "trim(1)", "trim(-1,5)", "trim(0,99999999999999999999)", "trim()"] {
            assert_eq!(trim_of(&edits(&["trim(0,10)", bad])), None, "{bad}");
        }
    }

    #[test]
    fn what_plays_is_answered_from_the_facts_by_codec() {
        let facts = |video: &str, sound: &str| VideoFacts {
            width: 640,
            height: 360,
            video_codec: video.into(),
            sound_codec: sound.into(),
            sound: !sound.is_empty(),
            ..VideoFacts::default()
        };
        let p = playback(&facts("avc1", "mp4a"));
        assert_eq!((p.video.clone(), p.muted()), (Ok(()), None));
        let p = playback(&facts("apcn", "mp4a"));
        assert_eq!(p.video, Err("This video can't be played (ProRes)".to_string()));
        let p = playback(&facts("avc1", "ac-3"));
        assert_eq!((p.video.clone(), p.muted()), (Ok(()), Some("This clip plays without sound (AC-3 sound can't be played)")));
    }

    #[test]
    fn the_collage_notice_counts_the_videos() {
        assert_eq!(collage_notice(0), None);
        assert_eq!(collage_notice(1).as_deref(), Some("1 video left out: videos can't go in a collage yet."));
        assert_eq!(collage_notice(3).as_deref(), Some("3 videos left out: videos can't go in a collage yet."));
    }
}
