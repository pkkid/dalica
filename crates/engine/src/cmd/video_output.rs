//! Save Frame as Picture and Export Clip (`editing.md` §11, `creating.md`; the state is
//! `crate::video_output`): `video.saveFrame` and `video.exportClip {path?}`. Empty until M15.25.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
