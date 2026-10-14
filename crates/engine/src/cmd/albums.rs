//! Album commands (`organizing.md` §4, `data-model.md` §3.2): `album.create`, `album.info.set`,
//! `album.add`, `album.remove`, `album.reorder`, `album.setCover`, `album.delete` and the
//! `albums.list` query.
//!
//! - Each change is one library undo step (none when nothing changes), and its op goes through
//!   [`dalica_library::Library::stamped`] with the session clock, so the photos it touches count as
//!   updated for Recently Updated. Undo and redo restore the model, and through it the sidecars and
//!   `albums.json`, exactly.
//! - Albums are named by id (`"a91c0e"`): an id that is not one is `bad_params`, an album the
//!   library does not have `not_found`. Photos are named by id as everywhere (`not_found` when
//!   unknown).
//! - Order: adding puts the photos after the album's last member, or right `before` / `after` a
//!   member, in the order given; a reorder moves members there. Each takes keys between its
//!   neighbours' (`data-model.md` §3.2), so only the moved photos' sidecars are rewritten; when no
//!   key fits, the whole album is re-keyed inside the same step.
//! - `album.remove` asks first unless Options → "Remove from albums without asking" is on, and
//!   `album.delete` always asks: without `"confirm": true` they refuse as `disabled` with the
//!   question (`control-protocol.md` §5.5). Deleting an album keeps its photos; its memberships go
//!   from the model (the sidecars keep entries of albums the library does not have) and its name
//!   from the folders, so a rebuild never brings it back. Undo gives everything back.
//! - New album ids are `a` and 12 hex digits from a random source seeded by the operating system
//!   (the standard library's `RandomState`), drawn again on a clash.

use std::collections::BTreeSet;
use std::hash::BuildHasher;

use dalica_core::{AlbumId, Date, PhotoId, PosKey};
use dalica_library::{AlbumInfo, Anchor, Library, Op, clean_album_name, clean_description, clean_place};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Effect, History, always, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "album.create",
            label: "New Album",
            menu: "",
            keys: &[],
            params: r#"{"name":string, "date"?:"YYYY-MM-DD"|null, "useNewest"?:bool (default: no date given), "place"?:string, "description"?:string, "ids"?:[PhotoId]}: a new album of the photos, in that order (Album Properties' OK); replies {album, name, count}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: create,
        },
        CommandSpec {
            id: "album.info.set",
            label: "Album Properties",
            menu: "",
            keys: &[],
            params: r#"{"album":AlbumId, "name"?:string, "date"?:"YYYY-MM-DD"|null, "useNewest"?:bool, "place"?:string, "description"?:string}: change the fields given ("" clears a text, null the date)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: set_info,
        },
        CommandSpec {
            id: "album.add",
            label: "Add to Album",
            menu: "",
            keys: &[],
            params: r#"{"album":AlbumId, "ids":[PhotoId], "before"?:PhotoId | "after"?:PhotoId}: add the photos (those already in it stay where they are), at the end or next to a member, in the order given"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: add,
        },
        CommandSpec {
            id: "album.remove",
            label: "Remove from Album",
            menu: "",
            keys: &[],
            params: r#"{"album":AlbumId, "ids":[PhotoId], "confirm"?:bool}: take the photos out of the album (the files stay); asks first unless "Remove from albums without asking" is on: needs "confirm": true"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: remove,
        },
        CommandSpec {
            id: "album.reorder",
            label: "Reorder Album",
            menu: "",
            keys: &[],
            params: r#"{"album":AlbumId, "ids":[PhotoId] (members), "before"?:PhotoId | "after"?:PhotoId}: move the members to the end or next to another member, in the order given"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: reorder,
        },
        CommandSpec {
            id: "album.setCover",
            label: "Set as Album Cover",
            menu: "",
            keys: &[],
            params: r#"{"album":AlbumId, "id":PhotoId (a member)}: the album's cover (the first photo until one is set)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: set_cover,
        },
        CommandSpec {
            id: "album.delete",
            label: "Delete Album",
            menu: "",
            keys: &[],
            params: r#"{"album":AlbumId, "confirm":bool}: delete the album (its photos stay); asks first: needs "confirm": true"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: delete,
        },
        CommandSpec {
            id: "albums.list",
            label: "List Albums",
            menu: "",
            keys: &[],
            params: r#"{}: every album: Starred Photos, Recently Updated, then the user albums by name: [{id, section, name, kind:"starred"|"recent"|"user", count, cover, date}] (a user album adds description, place, useNewest, dateSet)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
    ]
}

/// A field that is there, `null` included (serde's `Option` reads `null` as absent).
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// The day `v` gives, or `None` for `null`.
fn date(v: &Value) -> Result<Option<Date>, CmdError> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) => s.parse::<Date>().map(Some).map_err(|e| CmdError::BadParams(format!("date: {e}"))),
        _ => Err(CmdError::BadParams("date must be \"YYYY-MM-DD\" or null".into())),
    }
}

/// The album id `text` spells, of an album the library has.
pub(crate) fn known_album(s: &Session, text: &str) -> Result<AlbumId, CmdError> {
    let id = AlbumId::parse(text).map_err(|_| {
        let shown: String = text.chars().take(40).collect();
        CmdError::BadParams(format!("album must be an album id (\"a\" and 4 to 16 hex digits), not {shown:?}"))
    })?;
    if s.library().album(&id).is_none() {
        return Err(CmdError::NotFound(format!("no album {id}")));
    }
    Ok(id)
}

/// The command error for a library refusal.
fn refused(e: dalica_library::Error) -> CmdError {
    use dalica_library::Error as E;
    match e {
        E::UnknownPhoto(_) | E::UnknownAlbum(_) | E::UnknownFolder(_) => CmdError::NotFound(e.to_string()),
        E::TooMany { .. } | E::TooLong { .. } => CmdError::Limit(e.to_string()),
        other => CmdError::BadParams(other.to_string()),
    }
}

/// `op` with the last-changed time of every photo it touches (Recently Updated).
fn stamped(s: &Session, op: Op) -> Op {
    s.library().stamped(op, s.clock().now())
}

/// Where `before` / `after` put dropped photos (at most one of the two).
fn anchor(before: Option<PhotoId>, after: Option<PhotoId>) -> Result<Option<Anchor>, CmdError> {
    match (before, after) {
        (Some(_), Some(_)) => Err(CmdError::BadParams("give before or after, not both".into())),
        (Some(b), None) => Ok(Some(Anchor::Before(b))),
        (None, Some(a)) => Ok(Some(Anchor::After(a))),
        (None, None) => Ok(None),
    }
}

/// A random 64-bit number: the standard library's hasher keys, seeded by the operating system and
/// different for every call.
fn draw() -> u64 {
    std::collections::hash_map::RandomState::new().hash_one(0u8)
}

/// `n` keys in order for a new album: the first key, then each one after the last.
fn fresh_keys(n: usize) -> Result<Vec<PosKey>, CmdError> {
    let mut out: Vec<PosKey> = Vec::with_capacity(n);
    for _ in 0..n {
        let key = match out.last() {
            None => PosKey::first(),
            Some(k) => k.after().map_err(|e| CmdError::Limit(e.to_string()))?,
        };
        out.push(key);
    }
    Ok(out)
}

const NO_PHOTOS: &str = "Select some photos first.";

// --- album.create -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Create {
    name: String,
    #[serde(default, deserialize_with = "present")]
    date: Option<Value>,
    use_newest: Option<bool>,
    place: Option<String>,
    description: Option<String>,
    ids: Option<Vec<PhotoId>>,
}

fn create(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Create { name, date: when, use_newest, place, description, ids } = params(p)?;
    let name = clean_album_name(&name).ok_or_else(|| CmdError::BadParams("name the album: the name is empty".into()))?;
    let when = when.as_ref().map(date).transpose()?.flatten();
    let info = AlbumInfo {
        name: name.clone(),
        description: description.as_deref().map(clean_description).unwrap_or_default(),
        date: when,
        use_newest: use_newest.unwrap_or(when.is_none()),
        place: place.as_deref().map(clean_place).unwrap_or_default(),
    };
    let ids = known(s, ids.unwrap_or_default())?;
    let id = s.library().new_album_id(draw).map_err(refused)?;
    let keys = fresh_keys(ids.len())?;
    let mut ops = vec![Op::CreateAlbums { albums: vec![(id.clone(), info)] }];
    if !ids.is_empty() {
        ops.push(Op::AddMembers { members: ids.iter().zip(keys).map(|(&photo, key)| (id.clone(), photo, key)).collect() });
    }
    let op = stamped(s, Op::Batch { ops });
    s.apply(op)?;
    Ok(json!({ "album": id, "name": name, "count": ids.len() }))
}

// --- album.info.set -----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SetInfo {
    album: String,
    name: Option<String>,
    #[serde(default, deserialize_with = "present")]
    date: Option<Value>,
    use_newest: Option<bool>,
    place: Option<String>,
    description: Option<String>,
}

fn set_info(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetInfo { album, name, date: when, use_newest, place, description } = params(p)?;
    let album = known_album(s, &album)?;
    let when = when.as_ref().map(date).transpose()?;
    let old = s.library().album(&album).cloned().unwrap_or_default();
    let mut info = old.clone();
    if let Some(n) = name {
        info.name = clean_album_name(&n).ok_or_else(|| CmdError::BadParams("name the album: the name is empty".into()))?;
    }
    if let Some(d) = when {
        info.date = d;
    }
    if let Some(u) = use_newest {
        info.use_newest = u;
    }
    if let Some(pl) = place {
        info.place = clean_place(&pl);
    }
    if let Some(d) = description {
        info.description = clean_description(&d);
    }
    let changed = info != old;
    if changed {
        let op = stamped(s, Op::SetAlbumInfo { albums: vec![(album.clone(), info.clone())] });
        s.apply(op)?;
    }
    Ok(json!({ "album": album, "name": info.name, "changed": changed }))
}

// --- album.add, album.reorder -------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Place {
    album: String,
    ids: Vec<PhotoId>,
    before: Option<PhotoId>,
    after: Option<PhotoId>,
}

/// The album, the photos (known, repeats dropped, not empty) and the anchor of `p`.
fn place(s: &Session, p: &Value) -> Result<(AlbumId, Vec<PhotoId>, Option<Anchor>), CmdError> {
    let Place { album, ids, before, after } = params(p)?;
    let album = known_album(s, &album)?;
    let at = anchor(before, after)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled(NO_PHOTOS.into()));
    }
    if let Some(Anchor::Before(a) | Anchor::After(a)) = at
        && s.library().photo(a).is_none()
    {
        return Err(CmdError::NotFound(format!("no photo {a}")));
    }
    Ok((album, ids, at))
}

fn add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let (album, ids, at) = place(s, p)?;
    let added = ids.iter().filter(|id| s.library().member_key(&album, **id).is_none()).count();
    let op = s.library().album_add_op(&album, &ids, at).map_err(refused)?;
    if added > 0 {
        let op = stamped(s, op);
        s.apply(op)?;
    }
    Ok(json!({ "album": album, "added": added, "count": s.library().album_count(&album) }))
}

fn reorder(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let (album, ids, at) = place(s, p)?;
    let op = s.library().album_reorder_op(&album, &ids, at).map_err(refused)?;
    let before = s.library().album_members(&album).to_vec();
    let op = stamped(s, op);
    s.apply(op)?;
    let moved = before != s.library().album_members(&album);
    Ok(json!({ "album": album, "moved": ids.len(), "changed": moved }))
}

// --- album.remove -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    album: String,
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
}

/// "Remove the 3 selected pictures from this album? The files are not deleted."
pub fn remove_question(n: usize) -> String {
    match n {
        1 => "Remove the selected picture from this album? The file is not deleted.".into(),
        n => format!("Remove the {n} selected pictures from this album? The files are not deleted."),
    }
}

fn remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Remove { album, ids, confirm } = params(p)?;
    let album = known_album(s, &album)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled(NO_PHOTOS.into()));
    }
    let members: Vec<PhotoId> = ids.iter().copied().filter(|id| s.library().member_key(&album, *id).is_some()).collect();
    if members.is_empty() {
        return Err(CmdError::Disabled(
            if ids.len() == 1 { "The photo is not in this album." } else { "None of these photos is in this album." }.into(),
        ));
    }
    if !confirm && !s.settings().organize().remove_from_albums_without_asking {
        return Err(CmdError::Disabled(format!("{} Send \"confirm\": true to go ahead.", remove_question(members.len()))));
    }
    let op = stamped(s, Op::RemoveMembers { members: members.iter().map(|&id| (album.clone(), id)).collect() });
    s.apply(op)?;
    Ok(json!({ "album": album, "removed": members.len(), "count": s.library().album_count(&album) }))
}

// --- album.setCover -----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cover {
    album: String,
    id: PhotoId,
}

fn set_cover(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Cover { album, id } = params(p)?;
    let album = known_album(s, &album)?;
    if s.library().photo(id).is_none() {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    if s.library().member_key(&album, id).is_none() {
        return Err(CmdError::Disabled("The photo is not in this album.".into()));
    }
    let changed = s.library().cover_set(&album) != Some(id);
    if changed {
        let op = stamped(s, Op::SetCovers { covers: vec![(album.clone(), Some(id))] });
        s.apply(op)?;
    }
    Ok(json!({ "album": album, "cover": id, "changed": changed }))
}

// --- album.delete -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delete {
    album: String,
    #[serde(default)]
    confirm: bool,
}

/// "Delete the album "Trams"? The pictures in it are not deleted."
pub fn delete_question(name: &str) -> String {
    format!("Delete the album \"{name}\"? The pictures in it are not deleted.")
}

fn delete(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Delete { album, confirm } = params(p)?;
    let album = known_album(s, &album)?;
    let name = s.library().album(&album).map(|a| a.name.clone()).unwrap_or_default();
    if !confirm {
        return Err(CmdError::Disabled(format!("{} Send \"confirm\": true to go ahead.", delete_question(&name))));
    }
    let photos = s.library().album_count(&album);
    let op = stamped(s, Op::DeleteAlbums { ids: vec![album.clone()] });
    s.apply(op)?;
    Ok(json!({ "album": album, "name": name, "photos": photos }))
}

// --- albums.list --------------------------------------------------------------------------------

/// One row of `albums.list` for a user album.
fn user_row(lib: &Library, id: &AlbumId, info: &AlbumInfo) -> Value {
    let stats = lib.album_stats(id);
    json!({
        "id": id, "section": format!("album:{id}"), "name": info.name, "kind": "user",
        "count": stats.count, "cover": stats.cover, "date": stats.date,
        "description": info.description, "place": info.place, "useNewest": info.use_newest, "dateSet": info.date,
    })
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    match p {
        Value::Null => {}
        Value::Object(o) if o.is_empty() => {}
        _ => return Err(CmdError::BadParams("albums.list takes no parameters".into())),
    }
    let lib = s.library();
    let starred = lib.starred_photos();
    let recent = lib.recently_updated(s.clock().now(), s.settings().organize().recent_days);
    let mut rows = vec![
        json!({ "id": "starred", "section": "album:starred", "name": "Starred Photos", "kind": "starred", "count": starred.len(), "cover": starred.first(), "date": null }),
        json!({ "id": "recent", "section": "album:recent", "name": "Recently Updated", "kind": "recent", "count": recent.len(), "cover": recent.first(), "date": null }),
    ];
    let mut users: Vec<(&AlbumId, &AlbumInfo)> = lib.albums().collect();
    users.sort_by(|(a, x), (b, y)| x.name.to_lowercase().cmp(&y.name.to_lowercase()).then_with(|| x.name.cmp(&y.name)).then_with(|| a.cmp(b)));
    rows.extend(users.into_iter().map(|(id, info)| user_row(lib, id, info)));
    Ok(Value::Array(rows))
}

/// The user albums photo `id` is in, by id: `{id, name, key}` each (`photo.inspect`).
pub(crate) fn albums_of(lib: &Library, id: PhotoId) -> Value {
    let albums: BTreeSet<(&AlbumId, &PosKey)> = lib.albums_of(id).into_iter().collect();
    Value::Array(
        albums
            .into_iter()
            .map(|(a, k)| json!({ "id": a, "name": lib.album(a).map(|i| i.name.as_str()).unwrap_or_default(), "key": k.as_str() }))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    fn new_album(s: &mut crate::session::Session, ids: &[dalica_core::PhotoId]) -> String {
        let v = s.execute("album.create", &json!({ "name": "Trams", "ids": ids })).unwrap();
        v["album"].as_str().unwrap().to_string()
    }

    #[test]
    fn create_add_reorder_remove_and_delete_are_one_step_each_and_undo_exactly() {
        let (_dirs, mut s, ids) = library(4);
        let m0 = s.library().model().clone();
        let a = new_album(&mut s, &[ids[2], ids[0]]);
        let album = dalica_core::AlbumId::parse(&a).unwrap();
        assert_eq!(s.library().album_members(&album), [ids[2], ids[0]]);
        assert_eq!(s.undo_stack().undo_label(), Some("New Album"));
        s.execute("album.add", &json!({ "album": a, "ids": [ids[1], ids[0]], "before": ids[0] })).unwrap();
        assert_eq!(s.library().album_members(&album), [ids[2], ids[1], ids[0]], "a member stays where it is");
        s.execute("album.reorder", &json!({ "album": a, "ids": [ids[0]], "before": ids[2] })).unwrap();
        assert_eq!(s.library().album_members(&album), [ids[0], ids[2], ids[1]]);
        s.execute("album.setCover", &json!({ "album": a, "id": ids[1] })).unwrap();
        assert_eq!(s.library().album_cover(&album), Some(ids[1]));
        s.execute("album.remove", &json!({ "album": a, "ids": [ids[1]], "confirm": true })).unwrap();
        assert_eq!(s.library().album_cover(&album), Some(ids[0]), "the cover left with its photo");
        s.execute("album.info.set", &json!({ "album": a, "name": " Lisbon trams ", "place": "Lisbon" })).unwrap();
        assert_eq!(s.library().album(&album).unwrap().name, "Lisbon trams");
        s.execute("album.delete", &json!({ "album": a, "confirm": true })).unwrap();
        assert!(s.library().album(&album).is_none());
        assert_eq!(s.undo_stack().len(), 7);
        let end = s.library().model().clone();
        for _ in 0..7 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(s.library().model(), &m0);
        for _ in 0..7 {
            s.execute("edit.redo", &json!({})).unwrap();
        }
        assert_eq!(s.library().model(), &end);
    }

    #[test]
    fn remove_and_delete_ask_first() {
        let (_dirs, mut s, ids) = library(2);
        let a = new_album(&mut s, &ids);
        let steps = s.undo_stack().len();
        let asked = s.execute("album.remove", &json!({ "album": a, "ids": [ids[0], ids[1]] }));
        assert_eq!(
            asked,
            Err(CmdError::Disabled(
                "Remove the 2 selected pictures from this album? The files are not deleted. Send \"confirm\": true to go ahead.".into()
            ))
        );
        let asked = s.execute("album.delete", &json!({ "album": a }));
        assert_eq!(
            asked,
            Err(CmdError::Disabled("Delete the album \"Trams\"? The pictures in it are not deleted. Send \"confirm\": true to go ahead.".into()))
        );
        assert_eq!(s.undo_stack().len(), steps);
        // "Remove from albums without asking".
        s.change_settings(|st| {
            let mut o = st.organize();
            o.remove_from_albums_without_asking = true;
            st.set_organize(&o);
        });
        s.execute("album.remove", &json!({ "album": a, "ids": [ids[0]] })).unwrap();
        assert_eq!(s.library().album_count(&dalica_core::AlbumId::parse(&a).unwrap()), 1);
    }

    #[test]
    fn graceful_failures() {
        let (_dirs, mut s, ids) = library(2);
        let a = new_album(&mut s, &[ids[0]]);
        let (model, steps) = (s.library().model().clone(), s.undo_stack().len());
        let cases: Vec<(&str, Value, &str)> = vec![
            ("album.create", json!({}), "bad_params"),
            ("album.create", json!({ "name": "  \n" }), "bad_params"),
            ("album.create", json!({ "name": "x", "date": "2026-02-30" }), "bad_params"),
            ("album.create", json!({ "name": "x", "date": 3 }), "bad_params"),
            ("album.create", json!({ "name": "x", "ids": [999] }), "not_found"),
            ("album.create", json!({ "name": "x", "useNewest": "yes" }), "bad_params"),
            ("album.info.set", json!({ "album": "x" }), "bad_params"),
            ("album.info.set", json!({ "album": "a9999" }), "not_found"),
            ("album.info.set", json!({ "album": a, "name": "" }), "bad_params"),
            ("album.info.set", json!({ "album": a, "date": "soon" }), "bad_params"),
            ("album.add", json!({ "album": a, "ids": [] }), "disabled"),
            ("album.add", json!({ "album": a, "ids": [999] }), "not_found"),
            ("album.add", json!({ "album": "a9999", "ids": [ids[1]] }), "not_found"),
            ("album.add", json!({ "album": a, "ids": [ids[1]], "before": ids[0], "after": ids[0] }), "bad_params"),
            ("album.add", json!({ "album": a, "ids": [ids[1]], "before": ids[1] }), "bad_params"),
            ("album.add", json!({ "album": a, "ids": [ids[1]], "after": 999 }), "not_found"),
            ("album.add", json!({ "album": 7, "ids": [ids[1]] }), "bad_params"),
            ("album.reorder", json!({ "album": a, "ids": [ids[1]] }), "bad_params"),
            ("album.reorder", json!({ "album": a, "ids": [ids[0]], "before": ids[0] }), "bad_params"),
            ("album.remove", json!({ "album": a, "ids": [ids[1]], "confirm": true }), "disabled"),
            ("album.remove", json!({ "album": a, "ids": [] }), "disabled"),
            ("album.remove", json!({ "album": a, "ids": [ids[0]], "confirm": "yes" }), "bad_params"),
            ("album.setCover", json!({ "album": a, "id": ids[1] }), "disabled"),
            ("album.setCover", json!({ "album": a, "id": 999 }), "not_found"),
            ("album.setCover", json!({ "album": a }), "bad_params"),
            ("album.delete", json!({ "album": "a9999", "confirm": true }), "not_found"),
            ("album.delete", json!({ "album": "A1234", "confirm": true }), "bad_params"),
            ("albums.list", json!({ "x": 1 }), "bad_params"),
        ];
        for (id, p, code) in cases {
            let e = s.execute(id, &p).unwrap_err();
            assert_eq!(e.code(), code, "{id} {p}: {e}");
        }
        for id in ["album.create", "album.info.set", "album.add", "album.remove", "album.reorder", "album.setCover", "album.delete", "albums.list"] {
            hostile(&mut s, id);
        }
        assert_eq!(s.library().model(), &model);
        assert_eq!(s.undo_stack().len(), steps);
    }

    #[test]
    fn nothing_changed_is_no_step() {
        let (_dirs, mut s, ids) = library(2);
        let a = new_album(&mut s, &[ids[0]]);
        let steps = s.undo_stack().len();
        assert_eq!(s.execute("album.add", &json!({ "album": a, "ids": [ids[0]] })).unwrap()["added"], json!(0));
        assert_eq!(s.execute("album.info.set", &json!({ "album": a, "name": "Trams" })).unwrap()["changed"], json!(false));
        s.execute("album.setCover", &json!({ "album": a, "id": ids[0] })).unwrap();
        assert_eq!(s.execute("album.setCover", &json!({ "album": a, "id": ids[0] })).unwrap()["changed"], json!(false));
        assert_eq!(s.undo_stack().len(), steps + 1);
    }

    #[test]
    fn list_shows_the_automatic_albums_then_the_users_by_name() {
        let (_dirs, mut s, ids) = library(3);
        s.execute("album.create", &json!({ "name": "zebra", "ids": [ids[1]] })).unwrap();
        s.execute("album.create", &json!({ "name": "Apple", "date": "2026-06-04", "place": "Sintra" })).unwrap();
        s.execute("photo.star.set", &json!({ "ids": [ids[2]], "on": true })).unwrap();
        let v = s.execute("albums.list", &json!(null)).unwrap();
        let rows = v.as_array().unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Starred Photos", "Recently Updated", "Apple", "zebra"]);
        assert_eq!((rows[0]["count"].clone(), rows[0]["cover"].clone()), (json!(1), json!(ids[2])));
        assert_eq!(rows[2]["kind"], "user");
        assert_eq!(
            (rows[2]["count"].clone(), rows[2]["useNewest"].clone(), rows[2]["dateSet"].clone()),
            (json!(0), json!(false), json!("2026-06-04"))
        );
        assert_eq!(rows[3]["cover"], json!(ids[1]));
        assert_eq!(s.undo_stack().len(), 3);
    }
}
