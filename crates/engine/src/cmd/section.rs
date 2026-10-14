//! Commands on a light-table section (a folder or an album as a whole: the fourth menu, Folder or
//! Album).
//!
//! A section is named as in its widget id (`control-protocol.md` §5.4): `"folder:12"`,
//! `"album:a91c0e"` (a user album), `"album:starred"` (Starred Photos) or `"album:recent"`
//! (Recently Updated); from a menu or a shortcut the selection's photos name their folders instead
//! ([`section`], [`section_photos`]). `"album:search"` (the search results) is search's (M9.11)
//! and refused here until then.
//!
//! - `section.refreshThumbs` (Refresh Thumbnails, `organizing.md` §9): the section's thumbnails are
//!   made again from the files (their cache entries made dead first, sketches too, failures tried
//!   again), in the background; photos whose folder cannot be reached keep what they have. A
//!   folder section is all its photos; an album section its photos.
//! - `section.sort` (Sort Folder By ▸ / Sort Album By ▸, `organizing.md` §4.3, §10): on a folder
//!   it sets the folder's photo sort (Date, Name, Size; Reverse Order turns its direction around),
//!   written to its sidecar; on a user album it rewrites every member's position key in the order
//!   asked (`data-model.md` §3.2), or in its own order turned around for Reverse Order. One library
//!   undo step, none when nothing changes. Starred Photos and Recently Updated keep their own order.

use std::collections::BTreeSet;
use std::str::FromStr;

use dalica_core::{AlbumId, FolderId, PhotoId, PosKey};
use dalica_library::{AlbumRef, Op, PhotoSort, Section, SortBy};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "section.refreshThumbs",
            label: "Refresh Thumbnails",
            menu: "Folder/Refresh Thumbnails",
            keys: &[],
            params: r#"{"section"?:"folder:<FolderId>"|"album:<AlbumId>"|"album:starred"|"album:recent", "ids"?:[PhotoId]}: make the thumbnails of the section (or of the folders of the photos `ids` names) again, in the background"#,
            effect: Effect::Library,
            history: History::None,
            enabled: has_target,
            run: refresh_thumbs,
        },
        CommandSpec {
            id: "section.sort",
            label: "Sort",
            menu: "Folder/Sort By",
            keys: &[],
            params: r#"{"section"?:"folder:<FolderId>"|"album:<AlbumId>", "ids"?:[PhotoId] (their folders), "by"?:"date"|"name"|"size", "reverse"?:bool}: Sort By ▸ on a folder (its photo sort; what is not given is kept, and "reverse": true alone turns its direction around) or a user album (its manual order rewritten by the key, or turned around by "reverse": true alone); one undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_target,
            run: sort,
        },
    ]
}

/// The most photos named at once.
const MAX_IDS: usize = 100_000;

const NO_TARGET: &str = "Select a folder or a photo first.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    section: Option<String>,
    ids: Option<Vec<PhotoId>>,
}

fn has_target(_: &Session, p: &Value) -> Result<(), Disabled> {
    let none = p.get("section").is_none_or(Value::is_null) && p.get("ids").and_then(Value::as_array).is_none_or(Vec::is_empty);
    if none { Err(Disabled(NO_TARGET.into())) } else { Ok(()) }
}

/// The section `text` names, as its widget id spells it; a folder or user album the library does
/// not have is `not_found`, and the search album (until M9.11) `disabled`.
pub(crate) fn section(s: &Session, text: &str) -> Result<Section, CmdError> {
    let bad = || {
        let shown: String = text.chars().take(40).collect();
        CmdError::BadParams(format!("section must be \"folder:<id>\", \"album:<id>\", \"album:starred\" or \"album:recent\", not {shown:?}"))
    };
    let section = match text.strip_prefix("folder:") {
        Some(n) => Section::Folder(FolderId::new(n.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(bad)?)),
        None => Section::from_str(text).map_err(|_| bad())?,
    };
    match &section {
        Section::Folder(id) if s.library().folder(*id).is_none() => Err(CmdError::NotFound(format!("no folder {id}"))),
        Section::Album(AlbumRef::User(a)) if s.library().album(a).is_none() => Err(CmdError::NotFound(format!("no album {a}"))),
        Section::Album(AlbumRef::Search) => Err(CmdError::Disabled("There are no search results.".into())),
        _ => Ok(section),
    }
}

/// The photos of album section `album` in its order, hidden ones included: a user album's members
/// by position key, Starred Photos by date, Recently Updated newest first (the days Options →
/// General sets).
pub(crate) fn section_photos(s: &Session, album: &AlbumRef) -> Vec<PhotoId> {
    let lib = s.library();
    match album {
        AlbumRef::User(a) => lib.album_members(a).to_vec(),
        AlbumRef::Starred => lib.starred_photos().to_vec(),
        AlbumRef::Recent => lib.recently_updated(s.clock().now(), s.settings().organize().recent_days),
        AlbumRef::Search => Vec::new(),
    }
}

/// What `section` and `ids` name: the section, and the folders of the photos.
fn target(s: &Session, text: Option<&str>, ids: Option<&[PhotoId]>) -> Result<(Option<Section>, BTreeSet<FolderId>), CmdError> {
    let named = text.map(|t| section(s, t)).transpose()?;
    let ids = ids.unwrap_or_default();
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut folders = BTreeSet::new();
    for id in ids {
        let photo = s.library().photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        folders.insert(photo.folder);
    }
    if named.is_none() && folders.is_empty() {
        return Err(CmdError::Disabled(NO_TARGET.into()));
    }
    Ok((named, folders))
}

fn refresh_thumbs(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Target { section: text, ids } = params(p)?;
    let (named, mut folders) = target(s, text.as_deref(), ids.as_deref())?;
    let mut photos: Vec<PhotoId> = Vec::new();
    match &named {
        Some(Section::Folder(id)) => {
            folders.insert(*id);
        }
        Some(Section::Album(album)) => photos = section_photos(s, album),
        None => {}
    }
    let seen: BTreeSet<PhotoId> = photos.iter().copied().collect();
    photos.extend(s.library().photos().filter(|p| folders.contains(&p.folder) && !seen.contains(&p.id)).map(|p| p.id));
    let queued = s.refresh_thumbnails(&photos);
    Ok(json!({ "folders": folders, "photos": queued }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SortParams {
    section: Option<String>,
    ids: Option<Vec<PhotoId>>,
    by: Option<SortBy>,
    reverse: Option<bool>,
}

fn sort(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SortParams { section: text, ids, by, reverse } = params(p)?;
    let (named, mut folders) = target(s, text.as_deref(), ids.as_deref())?;
    // Without a key, only "reverse": true asks for something: Reverse Order.
    let turn = by.is_none() && reverse == Some(true);
    if by.is_none() && !turn {
        return Err(CmdError::BadParams("give \"by\" (date, name or size), or \"reverse\": true".into()));
    }
    match named {
        Some(Section::Album(AlbumRef::User(album))) => sort_album(s, &album, by, reverse.unwrap_or(false)),
        Some(Section::Album(_)) => Err(CmdError::Disabled("Starred Photos and Recently Updated keep their own order.".into())),
        Some(Section::Folder(id)) => {
            folders.insert(id);
            sort_folders(s, &folders, by, reverse, turn)
        }
        None => sort_folders(s, &folders, by, reverse, turn),
    }
}

/// Sort Album By ▸: every member's key rewritten in the order `by` gives (`reverse` turning it
/// around), or without `by` in the album's own order turned around (Reverse Order).
fn sort_album(s: &mut Session, album: &AlbumId, by: Option<SortBy>, reverse: bool) -> Result<Value, CmdError> {
    let lib = s.library();
    let before = lib.album_members(album).to_vec();
    let op = match by {
        Some(by) => lib.album_sort_op(album, PhotoSort::new(by, reverse)).map_err(|e| CmdError::BadParams(e.to_string()))?,
        None => {
            let mut keys: Vec<(AlbumId, PhotoId, PosKey)> = Vec::with_capacity(before.len());
            let mut last: Option<PosKey> = None;
            for &id in before.iter().rev() {
                let key = match &last {
                    None => PosKey::first(),
                    Some(k) => k.after().map_err(|e| CmdError::Limit(e.to_string()))?,
                };
                last = Some(key.clone());
                keys.push((album.clone(), id, key));
            }
            Op::SetMemberKeys { keys }
        }
    };
    // Keys that are already the album's change nothing.
    let same = matches!(&op, Op::SetMemberKeys { keys } if keys.iter().all(|(a, p, k)| lib.member_key(a, *p) == Some(k)));
    if !same {
        let op = lib.stamped(op, s.clock().now());
        s.apply(op)?;
    }
    let changed = before != s.library().album_members(album);
    Ok(json!({ "section": format!("album:{album}"), "changed": changed }))
}

/// Sort Folder By ▸ on `folders`: `by` with `reverse` (each kept when not given), or each folder's
/// direction turned around (`turn`).
fn sort_folders(s: &mut Session, folders: &BTreeSet<FolderId>, by: Option<SortBy>, reverse: Option<bool>, turn: bool) -> Result<Value, CmdError> {
    let lib = s.library();
    let mut sorts = Vec::new();
    for &id in folders {
        let own = lib.folder_sort(id);
        let new = if turn { PhotoSort::new(own.by, !own.reverse) } else { PhotoSort::new(by.unwrap_or(own.by), reverse.unwrap_or(own.reverse)) };
        if new != own {
            sorts.push((id, new));
        }
    }
    let changed = !sorts.is_empty();
    if changed {
        s.apply(Op::SetFolderSorts { sorts })?;
    }
    let shown: Vec<Value> = folders.iter().map(|id| json!({ "folder": id, "sort": s.library().folder_sort(*id) })).collect();
    Ok(json!({ "folders": shown, "changed": changed }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::{hostile, library};
    use crate::error::CmdError;

    #[test]
    fn refresh_makes_a_folders_thumbnails_again() {
        let (_dirs, mut s, ids) = library(3);
        let before = s.thumbnail_stats().decodes;
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let v = s.execute("section.refreshThumbs", &json!({ "section": format!("folder:{folder}") })).unwrap();
        assert_eq!(v["photos"], json!(3));
        s.wait_idle();
        let stats = s.thumbnail_stats();
        assert_eq!(stats.decodes - before, 3, "each photo decoded again: {stats:?}");
        assert_eq!((stats.scanned, stats.failed), (3, 0));
        // By a photo of the folder, as the menu passes the selection.
        s.execute("section.refreshThumbs", &json!({ "ids": [ids[2]] })).unwrap();
        s.wait_idle();
        assert_eq!(s.thumbnail_stats().decodes - before, 6);
        assert_eq!(s.undo_stack().len(), 0);
    }

    #[test]
    fn refresh_takes_album_sections() {
        let (_dirs, mut s, ids) = library(3);
        let a = s.execute("album.create", &json!({ "name": "Trams", "ids": [ids[1]] })).unwrap()["album"].clone();
        s.execute("photo.star.set", &json!({ "ids": [ids[0], ids[2]], "on": true })).unwrap();
        let before = s.thumbnail_stats().decodes;
        let v = s.execute("section.refreshThumbs", &json!({ "section": format!("album:{}", a.as_str().unwrap()) })).unwrap();
        assert_eq!(v["photos"], json!(1));
        s.wait_idle();
        assert_eq!(s.thumbnail_stats().decodes - before, 1);
        assert_eq!(s.execute("section.refreshThumbs", &json!({ "section": "album:starred" })).unwrap()["photos"], json!(2));
        s.wait_idle();
        assert_eq!(s.execute("section.refreshThumbs", &json!({ "section": "album:recent" })).unwrap()["photos"], json!(3));
        s.wait_idle();
        assert!(matches!(s.execute("section.refreshThumbs", &json!({ "section": "album:search" })), Err(CmdError::Disabled(_))));
        assert!(matches!(s.execute("section.refreshThumbs", &json!({ "section": "album:a9999" })), Err(CmdError::NotFound(_))));
    }

    #[test]
    fn disabled_and_hostile_params() {
        let (_dirs, mut s, _) = library(1);
        for id in ["section.refreshThumbs", "section.sort"] {
            for empty in [json!({}), json!(null), json!({ "ids": [] }), json!({ "section": null })] {
                assert_eq!(s.execute(id, &empty), Err(CmdError::Disabled("Select a folder or a photo first.".into())), "{id} {empty}");
            }
            // `section.sort` needs a key too.
            let with = |mut p: serde_json::Value| {
                if id == "section.sort" {
                    p["by"] = json!("name");
                }
                p
            };
            assert!(matches!(s.execute(id, &with(json!({ "section": "folder:999" }))), Err(CmdError::NotFound(_))));
            assert!(matches!(s.execute(id, &with(json!({ "ids": [999] }))), Err(CmdError::NotFound(_))));
            for bad in ["folder:", "folder:0", "folder:-1", "folder:x", "album:1", "", "folder:99999999999999999999999", "album:", "starred"] {
                assert!(matches!(s.execute(id, &with(json!({ "section": bad }))), Err(CmdError::BadParams(_))), "{id} {bad}");
            }
            assert!(matches!(s.execute(id, &json!({ "section": 3 })), Err(CmdError::BadParams(_))));
            hostile(&mut s, id);
        }
        let folder = format!("folder:{}", s.library().folders().next().unwrap().id);
        for bad in [json!({ "section": folder }), json!({ "section": folder, "reverse": false }), json!({ "section": folder, "by": "colour" })] {
            assert!(matches!(s.execute("section.sort", &bad), Err(CmdError::BadParams(_))), "{bad}");
        }
        assert!(matches!(s.execute("section.sort", &json!({ "section": "album:starred", "by": "name" })), Err(CmdError::Disabled(_))));
        assert_eq!(s.undo_stack().len(), 0);
    }

    #[test]
    fn sort_folder_by_and_reverse_order() {
        let (_dirs, mut s, ids) = library(3);
        let folder = s.library().photo(ids[0]).unwrap().folder;
        let section = format!("folder:{folder}");
        s.execute("section.sort", &json!({ "section": section, "by": "name" })).unwrap();
        assert_eq!(s.library().folder_sort(folder), dalica_library::PhotoSort::new(dalica_library::SortBy::Name, false));
        s.execute("section.sort", &json!({ "ids": [ids[1]], "reverse": true })).unwrap();
        assert_eq!(s.library().photos_sorted(folder), [ids[2], ids[1], ids[0]]);
        s.execute("section.sort", &json!({ "section": section, "reverse": true })).unwrap();
        assert_eq!(s.library().folder_sort(folder), dalica_library::PhotoSort::new(dalica_library::SortBy::Name, false), "turned back");
        let v = s.execute("section.sort", &json!({ "section": section, "by": "name" })).unwrap();
        assert_eq!(v["changed"], json!(false));
        assert_eq!(s.undo_stack().len(), 3);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.library().folder_sort(folder).reverse);
    }
}
