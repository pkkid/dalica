//! The video in the edit view (`editing.md` §11, `viewing.md` §4; the state and the rules are
//! `crate::video`): `video.inspect {id}`, and the player: `video.open {id}`, `video.close`,
//! `video.state`, `video.play`, `video.pause`, `video.seek {ms}`, `video.step {frames}`,
//! `video.setStart {ms?}`, `video.setEnd {ms?}` and `video.volume {volume}`. The window's own
//! player commands (`player.*`, `crates/ui/src/commands/video.rs`) call these.
//!
//! - **`video.inspect {id}`** (a query): the video's path, kind and duration, its container's facts
//!   (the stored size, the quarter turns, the size shown, the codecs, whether it has sound, the
//!   frame rate in frames per 1 000 s), whether Dalica plays it (`playable`, with the `reason`
//!   when it cannot: "This video can't be played (ProRes)"), its sound (`soundPlays`, with `muted`
//!   the reason it plays without) and its trim (`{startMs, endMs}` or `null`). A still is refused
//!   (`disabled`, "<name> is not a video."), an unknown id is `not_found`. The owner thread only
//!   reads the library: the facts were read on scan.
//! - **The player** (`crate::video`): every command answers the player's state as `video.state`
//!   does. Its frames are decoded on its own worker; the owner thread never touches the file.

use dalica_core::PhotoId;
use serde::Deserialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;
use crate::settings::VOLUME_RANGE;
use crate::video::MAX_STEP;

/// What every player command answers.
const STATE: &str = "the player's state: {open, id, playable, reason, sound (plays|muted|none), muted (why), ready, playing, positionMs, durationMs, frameCount, trim ({startMs, endMs} or null), frame ({index, timeMs, width, height} or null), pending, dropped, volume}";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "video.inspect",
            label: "Inspect Video",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: the video's facts: {id, name, path, kind, durationMs, width, height (stored), rotation (quarter turns clockwise), displayed {width, height}, videoCodec, soundCodec, sound, frameRateMilli (frames per 1000 s; 0 unknown), playable, reason (why not, else null), soundPlays (plays|muted|none), muted (why it plays without sound, else null), trim ({startMs, endMs} or null)}; a still is disabled"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: inspect,
        },
        CommandSpec {
            id: "video.open",
            label: "Open Video",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: opens the video in the player (closing the one open); a clip Dalica cannot play opens with the reason ("This video can't be played (ProRes)"), which its playback commands then give; answers the player's state"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                let Id { id } = params(p)?;
                s.video_open(id)
            },
        },
        CommandSpec {
            id: "video.close",
            label: "Close Video",
            menu: "",
            keys: &[],
            params: "{}: closes the player's video, stopping its sound",
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                params::<Empty>(p)?;
                s.video_close();
                s.video_state()
            },
        },
        CommandSpec {
            id: "video.state",
            label: "Video State",
            menu: "",
            keys: &[],
            params: STATE,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                params::<Empty>(p)?;
                s.video_state()
            },
        },
        CommandSpec {
            id: "video.play",
            label: "Play",
            menu: "",
            keys: &[],
            params: "{}: plays from the position (from the trim's start when outside it) to the trim's end, the sound's clock driving the frames; refused for a clip Dalica cannot play, with the reason",
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                params::<Empty>(p)?;
                s.video_play()
            },
        },
        CommandSpec {
            id: "video.pause",
            label: "Pause",
            menu: "",
            keys: &[],
            params: "{}: pauses at the position",
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                params::<Empty>(p)?;
                s.video_pause()
            },
        },
        CommandSpec {
            id: "video.seek",
            label: "Go to Time",
            menu: "",
            keys: &[],
            params: r#"{"ms":int}: shows the frame at ms milliseconds (the last one at or before it; playing on from there when it was playing); the frame follows (pending)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                let Seek { ms } = params(p)?;
                s.video_seek(ms)
            },
        },
        CommandSpec {
            id: "video.step",
            label: "Step Frames",
            menu: "",
            keys: &[],
            params: r#"{"frames":int}: pauses and shows the frame this many frames from the one shown (negative: back; at most 100000 either way); the frame follows (pending)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: |s, p| {
                let Step { frames } = params(p)?;
                if frames == 0 || frames.unsigned_abs() > MAX_STEP.unsigned_abs() {
                    return Err(CmdError::BadParams(format!("frames: a whole number from -{MAX_STEP} to {MAX_STEP}, not 0")));
                }
                s.video_step(frames)
            },
        },
        CommandSpec {
            id: "video.setStart",
            label: "Set Start",
            menu: "",
            keys: &[],
            params: r#"{"ms"?:int}: the trim starts at ms (default: the position), its end kept; trim(start,end) in the video's recipe, one step of its history; works on any clip"#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: always,
            run: |s, p| {
                let At { ms } = params(p)?;
                s.video_set_trim(false, ms)
            },
        },
        CommandSpec {
            id: "video.setEnd",
            label: "Set End",
            menu: "",
            keys: &[],
            params: r#"{"ms"?:int}: the trim ends at ms (default: the position), its start kept; trim(start,end) in the video's recipe, one step of its history; works on any clip"#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: always,
            run: |s, p| {
                let At { ms } = params(p)?;
                s.video_set_trim(true, ms)
            },
        },
        CommandSpec {
            id: "video.volume",
            label: "Volume",
            menu: "",
            keys: &[],
            params: r#"{"volume":int}: the player's volume, 0–100, kept in the setting video.volume; refused with the reason for a clip that plays muted"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: |s, p| {
                let Volume { volume } = params(p)?;
                let volume = u8::try_from(volume).ok().filter(|v| VOLUME_RANGE.contains(v));
                let volume = volume.ok_or_else(|| CmdError::BadParams("volume: a whole number from 0 to 100".into()))?;
                s.video_volume(volume)
            },
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seek {
    ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    frames: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct At {
    #[serde(default)]
    ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Volume {
    volume: u64,
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    let lib = s.library();
    let photo = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    if !crate::video::is_video(lib, id) {
        return Err(CmdError::Disabled(format!("{} is not a video.", photo.name)));
    }
    crate::video::inspect(lib, id).ok_or_else(|| CmdError::NotFound(format!("no folder for photo {id}")))
}
