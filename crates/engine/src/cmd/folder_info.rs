//! Folder information: `folder.info.set` (description, date, place, music; `organizing.md` §10).
//! Folder Properties' OK and the section header's inline description editor commit through it.
//!
//! Only the fields given change: a description or place is trimmed and cut to its cap
//! ([`dalica_library::clean_description`], [`dalica_library::clean_place`]), and "" clears it; a
//! date is a calendar day (`"2026-06-04"`), and `"oldest"` or `null` goes back to the date of the
//! folder's oldest photo (the default). The music is the folder of music files the folder's
//! slideshows play (`viewing.md` §1.5): an absolute path to a folder that is there, looked at on
//! the session's I/O worker (never on the thread that owns the session), trimmed
//! ([`dalica_library::clean_music`]); "" clears it, and anything else (a file, a path that is not
//! there, a relative one) is `bad_params`. The hidden flag is left alone (`folder.hide`). One
//! library undo step for every field given, and none when nothing changes.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use dalica_core::{Date, FolderId};
use dalica_library::{Op, clean_description, clean_music, clean_place};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::{Msg, Session};
use crate::settings::folder_path;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "folder.info.set",
        label: "Folder Properties",
        menu: "",
        keys: &[],
        params: r#"{"id":FolderId, "description"?:string, "date"?:"YYYY-MM-DD"|"oldest"|null, "place"?:string, "music"?:string (an absolute path to a folder of music, "" for none)}: change the fields given ("" clears a text; "oldest" or null uses the oldest photo's date; a music path that is not a folder is refused)"#,
        effect: Effect::Library,
        history: History::Library,
        enabled: always,
        run: set,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetInfo {
    id: FolderId,
    description: Option<String>,
    /// Absent: unchanged; present (even `null`): the new date.
    #[serde(default, deserialize_with = "present")]
    date: Option<Value>,
    place: Option<String>,
    music: Option<String>,
}

/// A field that is there, `null` included (serde's `Option` reads `null` as absent).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// The date `v` gives: a day, or `None` for the oldest photo's.
fn date(v: &Value) -> Result<Option<Date>, CmdError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s == "oldest" => Ok(None),
        Value::String(s) => s.parse::<Date>().map(Some).map_err(|e| CmdError::BadParams(format!("date: {e}"))),
        _ => Err(CmdError::BadParams("date must be \"YYYY-MM-DD\", \"oldest\" or null".into())),
    }
}

/// The music folder `text` names, as stored: "" for none, else an absolute path to a folder that
/// is there (followed through links), looked at on the I/O worker.
fn music(s: &mut Session, text: &str) -> Result<String, CmdError> {
    let text = clean_music(text);
    if text.is_empty() {
        return Ok(text);
    }
    let path = PathBuf::from(&text);
    if folder_path(&path).is_none() {
        return Err(CmdError::BadParams("music must be an absolute folder path or \"\"".into()));
    }
    let (tx, rx) = mpsc::channel();
    let looked = path.clone();
    s.send(Msg::Run(Box::new(move || {
        let _ = tx.send(is_folder(&looked));
    })));
    match rx.recv() {
        Ok(Ok(())) => Ok(text),
        Ok(Err(why)) => Err(CmdError::BadParams(format!("music: {}: {why}", path.display()))),
        Err(_) => Err(CmdError::Internal("the I/O worker stopped".into())),
    }
}

/// Whether `path` is a folder (a link to one counts), and why not.
fn is_folder(path: &Path) -> Result<(), String> {
    match std::fs::metadata(path) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => Err("not a folder".into()),
        Err(e) => Err(format!("no folder there ({e})")),
    }
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetInfo { id, description, date: new_date, place, music: new_music } = params(p)?;
    if s.library().folder(id).is_none() {
        return Err(CmdError::NotFound(format!("no folder {id}")));
    }
    let new_date = new_date.as_ref().map(date).transpose()?;
    let new_music = new_music.as_deref().map(|m| music(s, m)).transpose()?;
    let old = s.library().folder_info(id).clone();
    let mut info = old.clone();
    if let Some(d) = description {
        info.description = clean_description(&d);
    }
    if let Some(d) = new_date {
        info.date = d;
    }
    if let Some(pl) = place {
        info.place = clean_place(&pl);
    }
    if let Some(m) = new_music {
        info.music = m;
    }
    let changed = info != old;
    if changed {
        s.apply(Op::SetFolderInfo { folders: vec![(id, info.clone())] })?;
    }
    Ok(json!({ "id": id, "description": info.description, "date": info.date, "place": info.place, "music": info.music, "changed": changed }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn bad_params() {
        let (_dirs, mut s, ids) = library(1);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        assert!(matches!(s.execute("folder.info.set", &json!({ "id": 999, "description": "x" })), Err(CmdError::NotFound(_))));
        for bad in [
            json!({}),
            json!({ "description": "x" }),
            json!({ "id": folder, "date": "2026-02-30" }),
            json!({ "id": folder, "date": "yesterday" }),
            json!({ "id": folder, "date": 20260604 }),
            json!({ "id": folder, "date": "2026-06-04T10:00:00" }),
            json!({ "id": folder, "place": 1 }),
            json!({ "id": folder, "music": 1 }),
            json!({ "id": folder, "music": "relative/music" }),
            json!({ "id": folder, "hidden": true }),
            json!({ "id": -1 }),
            json!({ "id": "folder:1" }),
        ] {
            assert!(matches!(s.execute("folder.info.set", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert_eq!(s.undo_stack().len(), 0);
    }
}
