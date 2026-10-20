//! Save Frame as Picture and Export Clip (`editing.md` §11, `creating.md` §4; the jobs are
//! `crate::video_output`): `video.saveFrame` (the frame shown, a JPEG into the Video Frames
//! project) and `video.exportClip {id?, path?}` (the trimmed part copied without re-encoding),
//! each an `export` job of one step; no undo step (each makes a new file, as an export does). Over
//! automation `path` (`Session::export_clip_destination`) and the Video Frames folder
//! (`Session::video_frames_dir`) are checked against the write roots (`control-protocol.md` §5.7).

use std::path::PathBuf;

use dalica_core::PhotoId;
use serde::Deserialize;

use crate::registry::{CommandSpec, Effect, History, always, params};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "video.saveFrame",
            label: "Save Frame as Picture",
            menu: "",
            keys: &[],
            params: r#"{}: saves the frame the player shows as a JPEG (the clip's full size, upright, with the video's rotation; quality 92; dated the clip's date plus the frame's time) into <Pictures>/Dalica/Video Frames, marked as the Video Frames project, named "<clip> <hh-mm-ss>.jpg" (-001, … when taken; never replaces a file), as an export job; refused with the reason for a clip Dalica cannot play. Replies {job, pending:true, folder, name, timeMs}; the job's result is {path, photo, width, height}"#,
            effect: Effect::Files,
            history: History::None,
            enabled: always,
            run: |s, p| {
                params::<Empty>(p)?;
                s.start_save_frame()
            },
        },
        CommandSpec {
            id: "video.exportClip",
            label: "Export Clip",
            menu: "",
            keys: &[],
            params: r#"{"id"?:PhotoId (default: the player's video), "path"?:string (an absolute file path in a folder that exists; default: "<clip> trimmed.<ext>" beside the clip)}: copies the trimmed part (the whole clip without a trim) into a new file without re-encoding, from the key frame at or before the start; works on any clip, played or not; never replaces a file. Replies {job, pending:true, path, startMs, endMs (the trim asked)}; the job's result is {path, photo, startMs, endMs, lengthMs} with the real cut"#,
            effect: Effect::Files,
            history: History::None,
            enabled: always,
            run: |s, p| {
                let Clip { id, path } = params(p)?;
                s.start_export_clip(id, path.as_deref())
            },
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clip {
    #[serde(default)]
    id: Option<PhotoId>,
    #[serde(default)]
    path: Option<PathBuf>,
}
