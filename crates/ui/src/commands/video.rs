//! The video player's commands (`editing.md` §11, `viewing.md` §4). The clip itself (its frames,
//! its sound, its trim in the recipe) is the engine's player (M15.25: `video.open`, `video.play`,
//! `video.pause`, `video.seek`, `video.step`, `video.setStart`, `video.setEnd`, `video.volume`,
//! `video.saveFrame`, `video.exportClip`), which these commands drive; the window keeps the
//! player's view state (`video`: playing, the playhead, the trim handle dragged, the volume, full
//! screen), mirrored from the engine each frame ([`sync`]).
//!
//! The ids are the UI's own (`player.*`), never the engine's `video.*`: a UI command of the same
//! id would hide the engine's from `engine.execute` (M13.4's rule). The widgets keep the names the
//! protocol gives them (`region:video`, `button:video.toggle`, `slider:video.position`,
//! `handle:video.trimStart`, `button:video.saveFrame`, …; `control-protocol.md` §7).
//!
//! - **Playing** (`player.play`, `player.pause`, `player.toggle`: the bar's ▶ and the keys `/` and
//!   Space), **stepping** (`player.step {frames}`: `,` and `.`, a frame back or forward, held to
//!   rewind or fast-forward) and **seeking** (`player.seek {ms}`: the scrubber) need the pictures:
//!   a clip of a codec Dalica cannot play refuses them with the reason ("This video can't be played
//!   (ProRes)"), as Save Frame does.
//! - **The trim** (`player.setStart {ms?}`, `player.setEnd {ms?}`: the trim ruler's buttons at the
//!   playhead, its handles at the time dragged to) and **Export Clip** (`player.exportClip
//!   {path?}`: the save picker without `path`) work on the container, so on any clip.
//! - **The volume** (`player.volume {volume}`) is refused with the reason for a clip whose sound
//!   Dalica cannot decode; **Full Screen** (`player.fullScreen {on?}`) is the window's alone.
//! - **Save Frame as Picture** (`player.saveFrame`) writes into `Pictures/Dalica/Video Frames`;
//!   while a control request drives the window that folder keeps to the automation write roots
//!   (`crate::control::refuse_folder`, `control-protocol.md` §5.7) before anything is written.
//!
//! Every one needs a video shown in the edit view ([`shown`]; [`NO_VIDEO`] otherwise), which they
//! open in the engine's player first. The video keys (`/`, Space, `,`, `.`) reach these commands
//! only while one is shown (`crate::keys::capture_video_keys`); in the library and on a still photo
//! the keys are what they were (Space stars the photo in the edit view).

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::picker::{Filter, SaveRequest};
use crate::state::{MAX_LOCATION, MAX_POSITION_MS, VOLUME_MAX};

/// Why a player command cannot run: the edit view shows no video.
pub const NO_VIDEO: &str = "Open a video in the edit view first.";
/// The most frames one step moves (a held `,` or `.` repeats single steps).
pub const MAX_STEP: i32 = 1000;
/// The save picker's formats for Export Clip: the containers the clip is copied in.
pub const CLIP_FILTERS: &[Filter] = &[Filter { name: "Video", extensions: &["mp4", "mov", "m4v", "mkv"] }];

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "player.play",
        label: "Play",
        menu: "",
        keys: &[],
        params: "{}: play the video shown in the edit view (video.play); replies {playing}",
        run: play,
    },
    UiCommand {
        id: "player.pause",
        label: "Pause",
        menu: "",
        keys: &[],
        params: "{}: pause the video shown in the edit view (video.pause); replies {playing}",
        run: pause,
    },
    UiCommand {
        id: "player.toggle",
        label: "Play / Pause",
        menu: "",
        keys: &[],
        params: "{}: play the video shown in the edit view, or pause it while it plays (/ and Space); replies {playing}",
        run: toggle,
    },
    UiCommand {
        id: "player.step",
        label: "Step Frame",
        menu: "",
        keys: &[],
        params: "{frames: −1000 … 1000, not 0}: pause and move by whole frames (, back, . forward; video.step); replies {playing, position}",
        run: step,
    },
    UiCommand {
        id: "player.seek",
        label: "Seek",
        menu: "",
        keys: &[],
        params: "{ms: 0 … 86400000}: move the playhead to this time from the clip's start (video.seek); replies {position}",
        run: seek,
    },
    UiCommand {
        id: "player.setStart",
        label: "Set Start",
        menu: "",
        keys: &[],
        params: "{ms?: 0 … 86400000}: trim the clip to start at ms, else at the playhead (video.setStart; one step of the video's history); replies {trim}",
        run: set_start,
    },
    UiCommand {
        id: "player.setEnd",
        label: "Set End",
        menu: "",
        keys: &[],
        params: "{ms?: 0 … 86400000}: trim the clip to end at ms, else at the playhead (video.setEnd; one step of the video's history); replies {trim}",
        run: set_end,
    },
    UiCommand {
        id: "player.volume",
        label: "Volume",
        menu: "",
        keys: &[],
        params: "{volume: 0 … 100}: the player's volume (0 is muted; video.volume, kept in the setting); replies {volume}",
        run: volume,
    },
    UiCommand {
        id: "player.fullScreen",
        label: "Full Screen",
        menu: "",
        keys: &[],
        params: "{on?: bool}: the video fills the window, or not (toggles without `on`; Esc leaves); replies {fullScreen}",
        run: full_screen,
    },
    UiCommand {
        id: "player.saveFrame",
        label: "Save Frame as Picture",
        menu: "",
        keys: &[],
        params: "{}: save the frame shown as a JPEG in Pictures/Dalica/Video Frames (video.saveFrame); replies {job, pending, folder, name, timeMs}",
        run: save_frame,
    },
    UiCommand {
        id: "player.exportClip",
        label: "Export Clip",
        menu: "",
        keys: &[],
        params: "{path?: string}: export the trimmed clip to `path` (an absolute file path; video.exportClip), or ask where with the save picker; replies the job, or {picking: true}",
        run: export_clip,
    },
];

/// The player's commands grey their buttons while no video is shown; those that need the
/// pictures also while its codec is one Dalica cannot play, and the volume while its sound is.
pub const CHECKS: &[(&str, Check)] = &[
    ("player.play", pictures),
    ("player.pause", pictures),
    ("player.toggle", pictures),
    ("player.step", pictures),
    ("player.seek", pictures),
    ("player.setStart", video_shown),
    ("player.setEnd", video_shown),
    ("player.volume", sound),
    ("player.fullScreen", video_shown),
    ("player.saveFrame", pictures),
    ("player.exportClip", video_shown),
];

/// The video the edit view shows, if its photo is a video (`dalica_engine::video::is_video`: the
/// library's word, else a video's extension): the player's commands and the video keys act on it.
pub fn shown(app: &App) -> Option<PhotoId> {
    let state = app.state();
    if !state.edit_photo_shown() {
        return None;
    }
    let photo = state.edit.photo?;
    let library = app.session()?.library();
    dalica_engine::video::is_video(library, photo).then_some(photo)
}

/// What the engine's player says of the video open in it (`video.state`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Player {
    pub id: Option<PhotoId>,
    /// Why Dalica cannot play its pictures (its codec, or a stream its decoder refused), if so.
    pub reason: Option<String>,
    /// Why it plays without sound, if its sound cannot be decoded.
    pub muted: Option<String>,
    /// The worker has opened it.
    pub ready: bool,
    pub playing: bool,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub frame_count: u64,
    /// `(start, end)` in milliseconds, when the recipe trims it.
    pub trim: Option<(u64, u64)>,
    /// A seek or step not answered yet, or the clip still opening.
    pub pending: bool,
    pub dropped: u64,
    pub volume: u8,
}

impl Player {
    /// The engine's `video.state` reply read.
    pub fn of(v: &Value) -> Player {
        let u = |k: &str| v[k].as_u64().unwrap_or(0);
        let s = |k: &str| v[k].as_str().map(str::to_string);
        let trim = v["trim"]["startMs"].as_u64().zip(v["trim"]["endMs"].as_u64());
        Player {
            id: v["id"].as_u64().map(PhotoId::new),
            reason: s("reason"),
            muted: s("muted"),
            ready: v["ready"] == true,
            playing: v["playing"] == true,
            position_ms: u("positionMs"),
            duration_ms: u("durationMs"),
            frame_count: u("frameCount"),
            trim,
            pending: v["pending"] == true,
            dropped: u("dropped"),
            volume: u8::try_from(u("volume").min(u64::from(VOLUME_MAX))).unwrap_or(VOLUME_MAX),
        }
    }

    /// Why the pictures cannot be used now: the clip's reason, or that it is still opening.
    pub fn pictures_refused(&self) -> Option<String> {
        if let Some(why) = &self.reason {
            return Some(why.clone());
        }
        (!self.ready).then(|| crate::i18n::tr(dalica_engine::video::OPENING))
    }

    /// The trim as `(start, end)`: the recipe's, else the whole clip.
    pub fn bounds(&self) -> (u64, u64) {
        self.trim.unwrap_or((0, self.duration_ms))
    }
}

/// The engine's player opened on the video shown, if it is not already; the video shown, or
/// [`NO_VIDEO`].
fn ensure_open(app: &mut App) -> Result<PhotoId, CmdError> {
    let id = shown(app).ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
    if session.video().open() != Some(id) {
        session.execute("video.open", &json!({ "id": id }))?;
    }
    Ok(id)
}

/// The engine's player on the video shown (opened first when it is not), or `None` while no video
/// is shown.
pub fn player(app: &mut App) -> Option<Player> {
    ensure_open(app).ok()?;
    let v = app.session_mut()?.execute("video.state", &json!({})).ok()?;
    Some(Player::of(&v))
}

/// Runs the engine's player command `id` on the video shown and takes its answer into the view
/// state.
fn engine(app: &mut App, id: &str, params: Value) -> Result<Player, CmdError> {
    ensure_open(app)?;
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
    let reply = session.execute(id, &params)?;
    let p = Player::of(&reply);
    absorb(app, &p);
    Ok(p)
}

/// The view state follows what the engine's player says.
fn absorb(app: &mut App, p: &Player) {
    let video = &mut app.state_mut().video;
    video.playing = p.playing;
    video.position = p.position_ms.min(MAX_POSITION_MS);
    video.volume = p.volume;
}

/// The view state as the player last took it from the engine, kept to tell a change made from
/// outside (`ui.set`) from the engine's own progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mirror {
    photo: PhotoId,
    playing: bool,
    position: u64,
    volume: u8,
}

fn mirror_id() -> egui::Id {
    egui::Id::new("video.player.mirror")
}

/// Once a frame while the edit view shows a video: what `ui.set` changed in the view state since
/// the last frame (playing, the playhead, the volume) goes to the engine's player; then the view
/// state takes the engine's position, playing and volume (play stops by itself at the trim's end).
/// Returns the player as it is now; `None` while no video is shown.
pub fn sync(app: &mut App, ctx: &egui::Context) -> Option<Player> {
    let photo = shown(app)?;
    let mut p = player(app)?;
    let seen: Option<Mirror> = ctx.data(|d| d.get_temp(mirror_id()));
    let want = app.state().video;
    if let Some(m) = seen.filter(|m| m.photo == photo) {
        let mut pushed = false;
        if want.volume != m.volume && want.volume != p.volume {
            pushed |= push(app, "video.volume", json!({ "volume": want.volume }));
        }
        if want.position != m.position && want.position != p.position_ms {
            pushed |= push(app, "video.seek", json!({ "ms": want.position }));
        }
        if want.playing != m.playing && want.playing != p.playing {
            pushed |= push(app, if want.playing { "video.play" } else { "video.pause" }, json!({}));
        }
        if pushed {
            p = player(app)?;
        }
    }
    absorb(app, &p);
    let now = app.state().video;
    ctx.data_mut(|d| d.insert_temp(mirror_id(), Mirror { photo, playing: now.playing, position: now.position, volume: now.volume }));
    Some(p)
}

/// Runs an engine command for [`sync`]; a refusal is left for the mirror to show.
fn push(app: &mut App, id: &str, params: Value) -> bool {
    app.session_mut().is_some_and(|s| s.execute(id, &params).is_ok())
}

/// Once a frame, before anything is drawn: the player back to idle (its volume kept) and the
/// engine's player closed once no video is shown (the edit view went back to the library or on to
/// a still photo); the engine's player opened on the video when one is.
pub fn settle(app: &mut App) {
    let video = app.state().video;
    let showing = shown(app);
    if showing.is_none() {
        if video != video.idle() {
            app.state_mut().video = video.idle();
        }
        if let Some(s) = app.session_mut()
            && s.video().open().is_some()
        {
            let _ = s.execute("video.close", &json!({}));
        }
    } else {
        let _ = ensure_open(app);
    }
}

/// Whether the edit view shows a video; [`NO_VIDEO`] when it does not.
pub fn video_shown(app: &App, _: &Value) -> Result<(), String> {
    shown(app).map(|_| ()).ok_or_else(|| NO_VIDEO.to_string())
}

/// What Dalica plays of the video shown, from the library's facts (`dalica_engine::video::playback`).
fn playback(app: &App) -> Result<dalica_engine::video::Playback, String> {
    let id = shown(app).ok_or_else(|| NO_VIDEO.to_string())?;
    let library = app.session().ok_or_else(|| NO_VIDEO.to_string())?.library();
    let empty = dalica_library::VideoFacts::default();
    Ok(dalica_engine::video::playback(library.video_facts(id).unwrap_or(&empty)))
}

/// A video shown whose pictures Dalica plays; its reason when they are not.
fn pictures(app: &App, _: &Value) -> Result<(), String> {
    playback(app)?.video
}

/// A video shown whose sound is not refused; its reason when it is.
fn sound(app: &App, _: &Value) -> Result<(), String> {
    match playback(app)?.muted() {
        Some(why) => Err(why.to_string()),
        None => Ok(()),
    }
}

fn play(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let p = engine(app, "video.play", json!({}))?;
    Ok(json!({ "playing": p.playing }))
}

fn pause(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let p = engine(app, "video.pause", json!({}))?;
    Ok(json!({ "playing": p.playing }))
}

fn toggle(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    ensure_open(app)?;
    let playing = app.session().is_some_and(|s| s.video().is_playing());
    let p = engine(app, if playing { "video.pause" } else { "video.play" }, json!({}))?;
    Ok(json!({ "playing": p.playing }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    frames: i32,
}

fn step(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Step = parse(params)?;
    if p.frames == 0 || !(-MAX_STEP..=MAX_STEP).contains(&p.frames) {
        return Err(CmdError::BadParams(format!("`frames` is −{MAX_STEP} … {MAX_STEP}, not 0 (−1 is a frame back)")));
    }
    let p = engine(app, "video.step", json!({ "frames": p.frames }))?;
    Ok(json!({ "playing": p.playing, "position": p.position_ms }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seek {
    ms: u64,
}

fn seek(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Seek = parse(params)?;
    if p.ms > MAX_POSITION_MS {
        return Err(CmdError::BadParams(format!("`ms` is 0 … {MAX_POSITION_MS}, not {}", p.ms)));
    }
    let p = engine(app, "video.seek", json!({ "ms": p.ms }))?;
    Ok(json!({ "position": p.position_ms }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct At {
    #[serde(default)]
    ms: Option<u64>,
}

/// Set Start (`end` false) or Set End at `{ms?}`.
fn set_trim(app: &mut App, params: &Value, end: bool) -> Result<Value, CmdError> {
    let At { ms } = parse_object(params)?;
    if let Some(ms) = ms
        && ms > MAX_POSITION_MS
    {
        return Err(CmdError::BadParams(format!("`ms` is 0 … {MAX_POSITION_MS}, not {ms}")));
    }
    let params = match ms {
        Some(ms) => json!({ "ms": ms }),
        None => json!({}),
    };
    let p = engine(app, if end { "video.setEnd" } else { "video.setStart" }, params)?;
    let (start, end) = p.bounds();
    Ok(json!({ "trim": { "startMs": start, "endMs": end } }))
}

fn set_start(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    set_trim(app, params, false)
}

fn set_end(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    set_trim(app, params, true)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Volume {
    volume: u8,
}

fn volume(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Volume = parse(params)?;
    if p.volume > VOLUME_MAX {
        return Err(CmdError::BadParams(format!("`volume` is 0 … {VOLUME_MAX}, not {}", p.volume)));
    }
    let p = engine(app, "video.volume", json!({ "volume": p.volume }))?;
    Ok(json!({ "volume": p.volume }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OnOff {
    #[serde(default)]
    on: Option<bool>,
}

fn full_screen(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: OnOff = parse_object(params)?;
    shown(app).ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
    let video = &mut app.state_mut().video;
    video.full_screen = p.on.unwrap_or(!video.full_screen);
    Ok(json!({ "fullScreen": video.full_screen }))
}

/// Save Frame as Picture: the Video Frames folder kept to the automation write roots while a
/// control request drives the window (nothing written when it is outside them), then the engine's
/// `video.saveFrame`; its file name goes to the status line.
fn save_frame(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    ensure_open(app)?;
    if let Some(s) = app.session()
        && let Some(folder) = s.video_frames_dir()
    {
        crate::control::refuse_folder(&folder, "path", s.roots())?;
    }
    let session = app.session_mut().ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
    let reply = session.execute("video.saveFrame", &json!({}))?;
    if let Some(name) = reply["name"].as_str() {
        app.state_mut().status = crate::tl!("Saving {name} in Video Frames", name = name);
    }
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    #[serde(default)]
    path: Option<std::path::PathBuf>,
}

/// Export Clip: to `path`, or the save picker opened on "<clip> trimmed.<ext>" beside the clip,
/// whose answer runs this command again with the path chosen.
fn export_clip(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Export = parse_object(params)?;
    if let Some(path) = &p.path
        && (!path.is_absolute() || path.as_os_str().len() > MAX_LOCATION)
    {
        return Err(CmdError::BadParams(format!("`path` is an absolute file path of at most {MAX_LOCATION} bytes")));
    }
    let id = shown(app).ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
    if let Some(path) = p.path {
        let session = app.session_mut().ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?;
        return session.execute("video.exportClip", &json!({ "id": id, "path": path }));
    }
    let library = app.session().ok_or_else(|| CmdError::Disabled(NO_VIDEO.into()))?.library();
    let photo = library.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let folder = library.folder(photo.folder).map(|f| f.path.clone());
    let (stem, ext) = dalica_core::names::split_ext(&photo.name);
    let base = format!("{stem} {}", dalica_engine::video_output::TRIMMED);
    let name = match ext {
        Some(ext) => format!("{base}.{ext}"),
        None => base,
    };
    let request = SaveRequest { title: "Export Clip", name, folder, filters: CLIP_FILTERS, command: "player.exportClip", params: json!({}) };
    app.pick_save(request).map_err(CmdError::Disabled)?;
    Ok(json!({ "picking": true }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_player_needs_a_video_and_checks_its_params_first() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
        // A still photo: no command acts, and the view state stays idle.
        for (id, params) in [
            ("player.play", json!({})),
            ("player.toggle", json!({})),
            ("player.seek", json!({ "ms": 10 })),
            ("player.volume", json!({ "volume": 5 })),
            ("player.fullScreen", json!({})),
            ("player.step", json!({ "frames": 1 })),
            ("player.setStart", json!({})),
            ("player.saveFrame", json!({})),
            ("player.exportClip", json!({})),
        ] {
            assert!(matches!(app.run(id, &params), Err(CmdError::Disabled(m)) if m == NO_VIDEO), "{id}");
        }
        assert_eq!(app.state().video, crate::state::VideoView::default());
        assert_eq!(shown(&app), None);
        assert_eq!(app.session().unwrap().video().open(), None, "nothing opened in the engine");
        // Bad parameters are refused before anything else.
        for (id, bad) in [
            ("player.step", json!({ "frames": 0 })),
            ("player.step", json!({ "frames": 1001 })),
            ("player.seek", json!({ "ms": MAX_POSITION_MS + 1 })),
            ("player.seek", json!({ "ms": -1 })),
            ("player.setEnd", json!({ "ms": MAX_POSITION_MS + 1 })),
            ("player.setStart", json!({ "at": 3 })),
            ("player.volume", json!({ "volume": 101 })),
            ("player.exportClip", json!({ "path": "clip.mp4" })),
            ("player.play", json!({ "on": true })),
        ] {
            assert!(matches!(app.run(id, &bad), Err(CmdError::BadParams(_))), "{id} {bad}");
        }
        // No library at all: no video.
        let bare = App::new(None);
        assert_eq!(video_shown(&bare, &Value::Null), Err(NO_VIDEO.to_string()));
    }

    #[test]
    fn the_engines_state_reads_into_the_player() {
        let v = json!({
            "open": true, "id": 7, "playable": false, "reason": "This video can't be played (ProRes)",
            "sound": "muted", "muted": "no sound", "ready": true, "playing": true, "positionMs": 1500,
            "durationMs": 4000, "frameCount": 100, "trim": { "startMs": 500, "endMs": 3000 },
            "frame": null, "pending": false, "dropped": 2, "volume": 250,
        });
        let p = Player::of(&v);
        assert_eq!(p.id, Some(PhotoId::new(7)));
        assert_eq!((p.position_ms, p.duration_ms, p.trim, p.bounds()), (1500, 4000, Some((500, 3000)), (500, 3000)));
        assert_eq!(p.pictures_refused().as_deref(), Some("This video can't be played (ProRes)"));
        assert_eq!(p.volume, VOLUME_MAX, "a volume past the range is its top");
        let closed = Player::of(&json!({ "open": false, "id": null, "volume": 80 }));
        assert_eq!((closed.id, closed.bounds(), closed.volume), (None, (0, 0), 80));
        assert!(closed.pictures_refused().is_some(), "not ready: still opening");
        assert_eq!(Player::of(&json!("nonsense")), Player::default());
    }
}
