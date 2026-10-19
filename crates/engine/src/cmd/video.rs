//! The video in the edit view (`editing.md` §11, `viewing.md` §4; the state is `crate::video`):
//! `video.inspect {id}` (M15.16), `video.open`, `video.play`, `video.pause`, `video.seek {ms}`,
//! `video.step {frames}`, `video.setStart`, `video.setEnd` and `video.volume` (M15.25). Empty
//! until M15.16.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
