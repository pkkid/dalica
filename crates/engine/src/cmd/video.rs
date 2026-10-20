//! The video in the edit view (`editing.md` §11, `viewing.md` §4; the state and the rules are
//! `crate::video`): `video.inspect {id}` (M15.16); `video.open`, `video.play`, `video.pause`,
//! `video.seek {ms}`, `video.step {frames}`, `video.setStart`, `video.setEnd` and `video.volume`
//! follow with playback (M15.25).
//!
//! - **`video.inspect {id}`** (a query): the video's path, kind and duration, its container's facts
//!   (the stored size, the quarter turns, the size shown, the codecs, whether it has sound, the
//!   frame rate in frames per 1 000 s), whether Dalica can play it (`playable`, with the `reason`
//!   when it cannot) and its trim (`{startMs, endMs}` or `null`). A still is refused (`disabled`,
//!   "<name> is not a video."), an unknown id is `not_found`. The owner thread only reads the
//!   library: the facts were read on scan.

use dalica_core::PhotoId;
use serde::Deserialize;
use serde_json::Value;

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "video.inspect",
        label: "Inspect Video",
        menu: "",
        keys: &[],
        params: r#"{"id":PhotoId}: the video's facts: {id, name, path, kind, durationMs, width, height (stored), rotation (quarter turns clockwise), displayed {width, height}, videoCodec, soundCodec, sound, frameRateMilli (frames per 1000 s; 0 unknown), playable, reason (why not, else null), trim ({startMs, endMs} or null)}; a still is disabled"#,
        effect: Effect::Query,
        history: History::None,
        enabled: always,
        run: inspect,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
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
