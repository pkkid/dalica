//! The video player's commands (`editing.md` §11, `viewing.md` §4), stubs until M15.26 makes the
//! player act. The player's view state (`video`: playing, the playhead, the trim handle dragged,
//! the volume, full screen) is here; the clip itself (its frames, its sound, its trim in the
//! recipe) is the engine's (M15.25: `video.open`, `video.play`, `video.pause`, `video.seek`,
//! `video.step`, `video.setStart`, `video.setEnd`, `video.volume`, `video.saveFrame`,
//! `video.exportClip`), which these commands will drive.
//!
//! The ids are the UI's own (`player.*`), never the engine's `video.*`: a UI command of the same
//! id would hide the engine's from `engine.execute` (M13.4's rule). The widgets keep the names the
//! protocol gives them (`region:video`, `button:video.toggle`, `slider:video.position`,
//! `handle:video.trimStart`, `button:video.saveFrame`, …; `control-protocol.md` §7).
//!
//! - **View state, acting now**: `player.play`, `player.pause`, `player.toggle` (play or pause,
//!   the bar's ▶ and the keys `/` and Space), `player.seek {ms}` (the scrubber), `player.volume
//!   {volume}` (the volume slider; the speaker toggles 0 and back) and `player.fullScreen {on?}`.
//! - **Waiting for the player** ([`LATER`]): `player.step {frames}` (`,` and `.`, a frame back or
//!   forward), `player.setStart`, `player.setEnd` (the trim ruler's buttons), `player.saveFrame`
//!   (Save Frame as Picture) and `player.exportClip {path?}` (Export Clip, the save picker without
//!   `path`). They check their parameters, then refuse.
//!
//! Every one needs a video shown in the edit view ([`shown`]; [`NO_VIDEO`] otherwise). The video
//! keys (`/`, Space, `,`, `.`) reach these commands only while one is shown
//! (`crate::keys::capture_video_keys`); in the library and on a still photo the keys are what they
//! were (Space stars the photo in the edit view).

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, no_params, parse, parse_object};
use crate::app::App;
use crate::state::{MAX_LOCATION, MAX_POSITION_MS, VOLUME_MAX};

/// Why a player command cannot run: the edit view shows no video.
pub const NO_VIDEO: &str = "Open a video in the edit view first.";
/// What the player's stub commands say until M15.26 makes them act.
pub const LATER: &str = "Arrives with the video player.";
/// The most frames one step moves (a held `,` or `.` repeats single steps).
pub const MAX_STEP: i32 = 1000;

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "player.play",
        label: "Play",
        menu: "",
        keys: &[],
        params: "{}: play the video shown in the edit view; replies {playing}",
        run: play,
    },
    UiCommand {
        id: "player.pause",
        label: "Pause",
        menu: "",
        keys: &[],
        params: "{}: pause the video shown in the edit view; replies {playing}",
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
        params: "{frames: −1000 … 1000, not 0}: move the paused video by whole frames (, back, . forward)",
        run: step,
    },
    UiCommand {
        id: "player.seek",
        label: "Seek",
        menu: "",
        keys: &[],
        params: "{ms: 0 … 86400000}: move the playhead to this time from the clip's start; replies {position}",
        run: seek,
    },
    UiCommand {
        id: "player.setStart",
        label: "Set Start",
        menu: "",
        keys: &[],
        params: "{}: trim the clip to start at the playhead (one step of the video's history)",
        run: set_start,
    },
    UiCommand {
        id: "player.setEnd",
        label: "Set End",
        menu: "",
        keys: &[],
        params: "{}: trim the clip to end at the playhead (one step of the video's history)",
        run: set_end,
    },
    UiCommand {
        id: "player.volume",
        label: "Volume",
        menu: "",
        keys: &[],
        params: "{volume: 0 … 100}: the player's volume (0 is muted); replies {volume}",
        run: volume,
    },
    UiCommand {
        id: "player.fullScreen",
        label: "Full Screen",
        menu: "",
        keys: &[],
        params: "{on?: bool}: the video fills the screen, or not (toggles without `on`); replies {fullScreen}",
        run: full_screen,
    },
    UiCommand {
        id: "player.saveFrame",
        label: "Save Frame as Picture",
        menu: "",
        keys: &[],
        params: "{}: save the frame shown as a JPEG in Pictures/Dalica/Video Frames (video.saveFrame)",
        run: save_frame,
    },
    UiCommand {
        id: "player.exportClip",
        label: "Export Clip",
        menu: "",
        keys: &[],
        params: "{path?: string}: export the trimmed clip to `path` (an absolute file path), or ask where with the save picker (video.exportClip)",
        run: export_clip,
    },
];

/// The player's commands grey their buttons while no video is shown, and those that wait for
/// M15.26 with [`LATER`] while one is.
pub const CHECKS: &[(&str, Check)] = &[
    ("player.play", video_shown),
    ("player.pause", video_shown),
    ("player.toggle", video_shown),
    ("player.step", waits),
    ("player.seek", video_shown),
    ("player.setStart", waits),
    ("player.setEnd", waits),
    ("player.volume", video_shown),
    ("player.fullScreen", video_shown),
    ("player.saveFrame", waits),
    ("player.exportClip", waits),
];

/// The video the edit view shows, if its photo is a video (the library's facts say so; M15.16
/// gives the session a photo's kind): the player's commands and the video keys act on it.
pub fn shown(app: &App) -> Option<PhotoId> {
    let state = app.state();
    if !state.edit_photo_shown() {
        return None;
    }
    let photo = state.edit.photo?;
    let library = app.session()?.library();
    library.photo(photo)?;
    library.facts(photo).video.then_some(photo)
}

/// The player back to idle (its volume kept) once no video is shown: the edit view went back to the
/// library or on to a still photo. Once a frame, before anything is drawn.
pub fn settle(app: &mut App) {
    let video = app.state().video;
    if video != video.idle() && shown(app).is_none() {
        app.state_mut().video = video.idle();
    }
}

/// Whether the edit view shows a video; [`NO_VIDEO`] when it does not.
pub fn video_shown(app: &App, _: &Value) -> Result<(), String> {
    shown(app).map(|_| ()).ok_or_else(|| NO_VIDEO.to_string())
}

/// [`NO_VIDEO`] while no video is shown, else [`LATER`].
fn waits(app: &App, params: &Value) -> Result<(), String> {
    video_shown(app, params)?;
    Err(LATER.into())
}

fn need_video(app: &App) -> Result<(), CmdError> {
    video_shown(app, &Value::Null).map_err(CmdError::Disabled)
}

/// A player command that waits for M15.26: its parameters checked, a video shown, then [`LATER`].
fn later(app: &App) -> Result<Value, CmdError> {
    need_video(app)?;
    Err(CmdError::Disabled(LATER.into()))
}

fn set_playing(app: &mut App, playing: Option<bool>) -> Result<Value, CmdError> {
    need_video(app)?;
    let video = &mut app.state_mut().video;
    video.playing = playing.unwrap_or(!video.playing);
    Ok(json!({ "playing": video.playing }))
}

fn play(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    set_playing(app, Some(true))
}

fn pause(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    set_playing(app, Some(false))
}

fn toggle(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    set_playing(app, None)
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
    later(app)
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
    need_video(app)?;
    app.state_mut().video.position = p.ms;
    Ok(json!({ "position": p.ms }))
}

fn set_start(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    later(app)
}

fn set_end(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    later(app)
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
    need_video(app)?;
    app.state_mut().video.volume = p.volume;
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
    need_video(app)?;
    let video = &mut app.state_mut().video;
    video.full_screen = p.on.unwrap_or(!video.full_screen);
    Ok(json!({ "fullScreen": video.full_screen }))
}

fn save_frame(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    later(app)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    #[serde(default)]
    path: Option<std::path::PathBuf>,
}

fn export_clip(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Export = parse_object(params)?;
    if let Some(path) = &p.path
        && (!path.is_absolute() || path.as_os_str().len() > MAX_LOCATION)
    {
        return Err(CmdError::BadParams(format!("`path` is an absolute file path of at most {MAX_LOCATION} bytes")));
    }
    later(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    #[test]
    fn the_player_needs_a_video_and_its_stubs_check_their_params() {
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
            ("player.saveFrame", json!({})),
        ] {
            assert!(matches!(app.run(id, &params), Err(CmdError::Disabled(m)) if m == NO_VIDEO), "{id}");
        }
        assert_eq!(app.state().video, crate::state::VideoView::default());
        assert_eq!(shown(&app), None);
        // Bad parameters are refused before anything else.
        for (id, bad) in [
            ("player.step", json!({ "frames": 0 })),
            ("player.step", json!({ "frames": 1001 })),
            ("player.seek", json!({ "ms": MAX_POSITION_MS + 1 })),
            ("player.seek", json!({ "ms": -1 })),
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
}
