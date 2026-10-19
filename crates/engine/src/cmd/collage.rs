//! The Collage room (`creating.md` §3, `roadmap.md` M14 → *Collage*; the state is
//! `crate::collage`): `create.collage {ids | section}`, `collage.inspect`, `collage.set`,
//! `collage.clips.add` / `.clips.remove`, `collage.place` / `.unplace`, `collage.move`,
//! `collage.swap`, `collage.order`, `collage.background`, `collage.frameCenter`,
//! `collage.scramble`, `collage.shuffle`, `collage.reset`, `collage.undo` / `.redo` (the room's own
//! history) and `collage.close {discard?}`. Empty until M14.16.

use crate::registry::CommandSpec;

pub fn specs() -> Vec<CommandSpec> {
    Vec::new()
}
