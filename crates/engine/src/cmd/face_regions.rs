//! People names in photo files (`xmp-interop.md` §6): `faces.writeXmp {ids, confirm}` (Tools ▸
//! Experimental ▸ Write People Names to Photos…) writes the named faces of a selection into the
//! files once, as MWG face regions, whatever Store people names in photos
//! (`people.storeNamesInPhotos`) says; with that option on, every change to a photo's named faces is
//! written on its own (`metadata.rs`, the people names field).
//!
//! - It asks first ([`QUESTION`]): without `"confirm": true` it refuses as `disabled` with the
//!   question. The files change, so a read-only session refuses it (`Effect::Files`).
//! - Only the photos with faces are written; the others are counted as skipped. Unnamed and ignored
//!   faces are never written, and the files' other regions (pets, focus areas) are kept.
//! - The writes go through the metadata write queue, as a "Writing metadata" job: no undo step (the
//!   names stay in the library either way), a failed write a toast naming the file.

use dalica_core::PhotoId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "faces.writeXmp",
        label: "Write People Names to Photos…",
        menu: "Tools/Experimental/Write People Names to Photos…",
        keys: &[],
        params: r#"{"ids":[PhotoId], "confirm":bool}: writes the named faces of the photos into their files once, as MWG face regions, whatever Store people names in photos says (unnamed and ignored faces never; the files' other regions kept); asks first: needs "confirm": true. Photos without faces are skipped. Replies {queued, skipped}; the writes run as the "Writing metadata" job (no undo step)"#,
        effect: Effect::Files,
        history: History::None,
        enabled: has_ids,
        run: write_xmp,
    }]
}

/// What Write People Names to Photos… asks first, for `n` photos.
pub fn question(n: usize) -> String {
    let what = if n == 1 { "the selected picture".to_string() } else { format!("the {n} selected pictures") };
    format!("Write the names of the people in {what} into the files? Other applications will show them.")
}

/// The question for one photo (the documented example).
pub const QUESTION: &str = "Write the names of the people in the selected picture into the files? Other applications will show them.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteXmp {
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
}

fn write_xmp(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let WriteXmp { ids, confirm } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    if !confirm {
        return Err(CmdError::Disabled(format!("{} Send \"confirm\": true to go ahead.", question(ids.len()))));
    }
    let queued = s.queue_people_writes(&ids);
    Ok(json!({ "queued": queued, "skipped": ids.len().saturating_sub(queued) }))
}
