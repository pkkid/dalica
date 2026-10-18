//! Photo commands: stars and inspection, and the helpers the other photo areas share (`ids`
//! parameters: [`Ids`], [`has_ids`], [`known`]). A star change stamps the photos' last-changed
//! time (Recently Updated); `photo.inspect` lists the albums each photo is in.

use dalica_core::PhotoId;
use dalica_library::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.star.toggle",
            label: "Star / Unstar",
            menu: "Picture/Star / Unstar",
            // Two keys per context: the edit view's are Ctrl+8 and Space (`editing.md` §12), F4
            // is the Library's second.
            keys: &[(Context::Library, "Ctrl+8"), (Context::Library, "F4"), (Context::EditView, "Ctrl+8"), (Context::EditView, "Space")],
            params: r#"{"ids":[PhotoId]}: stars them all, or unstars them if all are starred"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: star_toggle,
        },
        CommandSpec {
            id: "photo.star.set",
            label: "Set Star",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "on":bool}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: star_set,
        },
        CommandSpec {
            id: "photo.inspect",
            label: "Inspect Photos",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: everything known about each photo: the file's fields, its caption, tags, hidden flag, recipe (edits, redo), effective date, displayed size (the file's, grown by the recipe's frames and turned by its rotations and flips; a crop not counted), albums ([{id, name, key}]), geotagged (the library's word, else the file's GPS) and place ([lat, lon] of its geotag, or null)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: crate::registry::always,
            run: inspect,
        },
    ]
}

/// The parameters of a command that takes only photos.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ids {
    pub ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdsOn {
    ids: Vec<PhotoId>,
    on: bool,
}

/// The most ids one command takes.
pub(crate) const MAX_IDS: usize = 100_000;

/// Disabled with "Select some photos first." when `ids` is present and empty. Malformed params
/// are left to `run`, which reports them as bad parameters.
pub(crate) fn has_ids(_: &Session, p: &Value) -> Result<(), Disabled> {
    match p.get("ids").and_then(Value::as_array) {
        Some(ids) if ids.is_empty() => Err(Disabled("Select some photos first.".into())),
        _ => Ok(()),
    }
}

/// The ids, de-duplicated, all known; `NotFound` names the first unknown one.
pub(crate) fn known(s: &Session, ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if s.library().photo(id).is_none() {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        }
        if seen.insert(id) {
            out.push(id);
        }
    }
    Ok(out)
}

fn set(s: &mut Session, ids: Vec<PhotoId>, on: bool) -> Result<Value, CmdError> {
    let changed: Vec<(PhotoId, bool)> = ids.iter().filter(|id| s.library().photo(**id).is_some_and(|p| p.star != on)).map(|id| (*id, on)).collect();
    if !changed.is_empty() {
        // Starred photos count as updated (Recently Updated, `organizing.md` §4.1).
        let op = s.library().stamped(Op::SetStars { stars: changed }, s.clock().now());
        s.apply(op)?;
    }
    Ok(json!({ "ids": ids, "on": on }))
}

fn star_toggle(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let all_starred = ids.iter().all(|id| s.library().photo(*id).is_some_and(|p| p.star));
    set(s, ids, !all_starred)
}

fn star_set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let IdsOn { ids, on } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    set(s, ids, on)
}

fn inspect(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let ids = known(s, ids)?;
    let lib = s.library();
    let photos: Vec<Value> = ids
        .iter()
        .filter_map(|id| {
            let p = lib.photo(*id)?;
            let folder = lib.folder(p.folder)?;
            let info = lib.info(p.id);
            let (dw, dh) = displayed_size(&info.edits, p.width, p.height);
            Some(json!({
                "id": p.id, "folder": p.folder, "name": p.name, "path": folder.path.join(&p.name),
                "size": p.size, "mtime": p.mtime, "width": p.width, "height": p.height, "date": p.date, "star": p.star,
                "caption": info.caption, "tags": info.tags, "hidden": info.hidden,
                "recipe": { "edits": info.edits, "redo": info.redo },
                "dateSet": info.date, "effectiveDate": lib.effective_date(p.id),
                "displayed": { "width": dw, "height": dh },
                "albums": crate::cmd::albums::albums_of(lib, p.id),
                "geotagged": lib.is_geotagged(p.id), "place": lib.geotag(p.id),
            }))
        })
        .collect();
    Ok(Value::Array(photos))
}

/// The size the photo shows at: its file's displayed size (`width × height`, after the EXIF
/// orientation) grown by the recipe's frames (Border, Drop Shadow, …) and turned by what its
/// rotations and flips add up to; a crop is not counted (`edit::shown_size`). A recipe `edit`
/// cannot read (more operations than it holds) counts as none.
pub(crate) fn displayed_size(edits: &[String], width: u32, height: u32) -> (u32, u32) {
    dalica_edit::shown_size(edits, width, height, dalica_edit::Cropping::Ignored)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn star_toggle_stars_all_or_unstars_all_and_undo_restores() {
        let (_dirs, mut s, ids) = library(4);
        let before = s.library().model().clone();
        s.execute("photo.star.toggle", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert!(s.library().photo(ids[0]).unwrap().star && s.library().photo(ids[1]).unwrap().star);
        // Mixed selection: stars the rest.
        s.execute("photo.star.toggle", &json!({ "ids": [ids[1], ids[2]] })).unwrap();
        assert!(s.library().photo(ids[2]).unwrap().star);
        // All starred: unstars.
        s.execute("photo.star.toggle", &json!({ "ids": [ids[1], ids[2]] })).unwrap();
        assert!(!s.library().photo(ids[1]).unwrap().star && !s.library().photo(ids[2]).unwrap().star);
        let after = s.library().model().clone();
        for _ in 0..3 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(s.library().model(), &before);
        for _ in 0..3 {
            s.execute("edit.redo", &json!({})).unwrap();
        }
        assert_eq!(s.library().model(), &after);
    }

    #[test]
    fn star_set_is_one_undo_step_even_for_many_photos() {
        let (_dirs, mut s, ids) = library(5);
        let before = s.library().model().clone();
        s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
        assert_eq!(s.library().starred().len(), 5);
        assert_eq!(s.undo_stack().len(), 1);
        // Setting what is already set changes nothing and records no step.
        s.execute("photo.star.set", &json!({ "ids": ids, "on": true })).unwrap();
        assert_eq!(s.undo_stack().len(), 1);
        s.execute("edit.undo", &json!(null)).unwrap();
        assert_eq!(s.library().model(), &before);
    }

    #[test]
    fn disabled_and_bad_params() {
        let (_dirs, mut s, ids) = library(2);
        for (id, extra) in [("photo.star.toggle", json!({})), ("photo.star.set", json!({ "on": true }))] {
            let with = |ids: serde_json::Value| {
                let mut p = extra.clone();
                p["ids"] = ids;
                p
            };
            assert_eq!(s.execute(id, &with(json!([]))), Err(CmdError::Disabled("Select some photos first.".into())));
            assert!(matches!(s.execute(id, &with(json!([999]))), Err(CmdError::NotFound(_))));
        }
        assert!(matches!(s.execute("photo.star.set", &json!({ "ids": [ids[0]] })), Err(CmdError::BadParams(_))));
        assert!(matches!(s.execute("photo.star.toggle", &json!({ "ids": [ids[0]], "extra": 1 })), Err(CmdError::BadParams(_))));
        assert_eq!(s.undo_stack().len(), 0);
        assert!(s.library().starred().is_empty());
    }

    #[test]
    fn inspect_reports_every_field_and_changes_nothing() {
        let (_dirs, mut s, ids) = library(2);
        let rev = s.library().revision();
        let v = s.execute("photo.inspect", &json!({ "ids": [ids[1], ids[1]] })).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        for key in [
            "id",
            "folder",
            "name",
            "path",
            "size",
            "mtime",
            "width",
            "height",
            "date",
            "star",
            "caption",
            "tags",
            "hidden",
            "recipe",
            "dateSet",
            "effectiveDate",
            "displayed",
            "albums",
        ] {
            assert!(arr[0].get(key).is_some(), "{key}");
        }
        assert_eq!((arr[0]["width"].as_u64(), arr[0]["height"].as_u64()), (Some(120), Some(80)));
        assert_eq!(arr[0]["displayed"], json!({ "width": 120, "height": 80 }));
        assert_eq!(arr[0]["recipe"], json!({ "edits": [], "redo": [] }));
        assert_eq!((&arr[0]["caption"], &arr[0]["tags"], &arr[0]["hidden"]), (&json!(""), &json!([]), &json!(false)));
        assert_eq!(arr[0]["albums"], json!([]));
        assert_eq!(s.library().revision(), rev);
        assert_eq!(s.undo_stack().len(), 0);
    }

    #[test]
    fn displayed_size_turns_with_the_recipe_and_grows_with_its_frames() {
        let t = |e: &[&str]| super::displayed_size(&e.iter().map(|s| s.to_string()).collect::<Vec<_>>(), 120, 80);
        assert_eq!(t(&[]), (120, 80));
        assert_eq!(t(&["rotate(90)"]), (80, 120));
        assert_eq!(t(&["rotate(90)", "future_op(1)", "flip_h()"]), (80, 120));
        assert_eq!(t(&["rotate(180)"]), (120, 80));
        assert_eq!(t(&["rotate(45)"]), (120, 80), "an operation edit does not know is no turn");
        // A Border (5 % of the short side, 4 px) grows it, turned by a later rotation; a crop is
        // not counted, as before frames.
        assert_eq!(t(&["border(#000000,50,#ffffff,0,0,0)", "rotate(90)"]), (88, 128));
        assert_eq!(t(&["crop(0.25,0,0.75,1)"]), (120, 80));
    }
}
