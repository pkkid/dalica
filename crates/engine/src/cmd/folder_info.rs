//! Folder information: `folder.info.set` (description, date, place; `organizing.md` §10). Folder
//! Properties' OK and the section header's inline description editor commit through it.
//!
//! Only the fields given change: a description or place is trimmed and cut to its cap
//! ([`dalica_library::clean_description`], [`dalica_library::clean_place`]), and "" clears it; a
//! date is a calendar day (`"2026-06-04"`), and `"oldest"` or `null` goes back to the date of the
//! folder's oldest photo (the default). The hidden flag is left alone (`folder.hide`). One library
//! undo step, and none when nothing changes.

use dalica_core::{Date, FolderId};
use dalica_library::{Op, clean_description, clean_place};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "folder.info.set",
        label: "Folder Properties",
        menu: "",
        keys: &[],
        params: r#"{"id":FolderId, "description"?:string, "date"?:"YYYY-MM-DD"|"oldest"|null, "place"?:string}: change the fields given ("" clears a text; "oldest" or null uses the oldest photo's date)"#,
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

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetInfo { id, description, date: new_date, place } = params(p)?;
    if s.library().folder(id).is_none() {
        return Err(CmdError::NotFound(format!("no folder {id}")));
    }
    let new_date = new_date.as_ref().map(date).transpose()?;
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
    let changed = info != old;
    if changed {
        s.apply(Op::SetFolderInfo { folders: vec![(id, info.clone())] })?;
    }
    Ok(json!({ "id": id, "description": info.description, "date": info.date, "place": info.place, "changed": changed }))
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
            json!({ "id": folder, "hidden": true }),
            json!({ "id": -1 }),
            json!({ "id": "folder:1" }),
        ] {
            assert!(matches!(s.execute("folder.info.set", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert_eq!(s.undo_stack().len(), 0);
    }
}
