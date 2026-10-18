//! Geotags (`organizing.md` §7, `data-model.md` §5, `xmp-interop.md` §2; the state is
//! `crate::places`).
//!
//! - **`photo.geo.set {ids, geo: [lat, lon] | null, confirm?}`** (the Places pane's drop and pin,
//!   an agent's `set_metadata {geo}`): gives the photos a point (stored at six decimals) or, with
//!   `null`, removes their location (the model's `Geo::Removed`: the photo is no longer geotagged,
//!   whatever its file's GPS says). One library undo step, none when nothing changes. Changing the
//!   location of more than [`MOVE_ASKS_PAST`] photos that already have one, or removing it from
//!   more than [`CLEAR_ASKS_PAST`], asks first: without `"confirm": true` it refuses with the
//!   question (`needs_confirm` over automation). Clearing changes only the photos that are
//!   geotagged.
//! - **`geo.clear {ids, confirm?}`** (Tools ▸ Geotag ▸ Clear Location…): `photo.geo.set` with
//!   `null`, under its own name ("Undo Clear Location"); disabled when none of the photos has a
//!   location.
//! - **`geo.copy {id}`** (Copy Location): keeps the photo's place for this session (nothing is
//!   written); disabled for a photo without one.
//! - **`geo.paste {ids}`** (Paste Location): gives the photos the copied place, one undo step
//!   ("Undo Paste Location"); it does not ask (the menu item is the user's word); disabled while
//!   nothing is copied.
//! - **`places.list {ids | section}`** (query): the geotags of the photos selected or of a section
//!   shown (`"folder:<id>"`, `"album:<id>"`, `"album:starred"`, `"album:recent"`), with the number
//!   of distinct places among them.
//!
//! **What reaches the files** (`metadata.rs`): every change of a geotag a command makes, undo and
//! redo included, is written into the photo file through the metadata write queue while Options →
//! General → Files → Write captions, tags and places is on (EXIF GPS and its XMP mirror; a removed
//! location deletes both); with it off nothing is written and the folder sidecar keeps the word.

use std::collections::BTreeSet;

use dalica_core::PhotoId;
use dalica_core::geo::LatLon;
use dalica_library::{AlbumRef, Geo, Op, Section};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

/// Changing the location of more photos that already have one than this asks first.
pub const MOVE_ASKS_PAST: usize = 20;
/// Removing the location from more photos than this asks first.
pub const CLEAR_ASKS_PAST: usize = 5;
/// The undo step of Paste Location ("Undo Paste Location").
pub const PASTE_LABEL: &str = "Paste Location";
/// The undo step of Clear Location… ("Undo Clear Location").
pub const CLEAR_LABEL: &str = "Clear Location";
/// Paste Location with nothing copied.
pub const NOTHING_COPIED: &str = "Copy a photo's location first (Tools ▸ Geotag ▸ Copy Location).";
/// Copy Location on a photo without a location.
pub const NO_LOCATION: &str = "The photo has no location to copy.";
/// Clear Location… on photos without a location.
pub const NONE_LOCATED: &str = "None of these photos has a location.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.geo.set",
            label: "Set Location",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "geo":[lat, lon]|null, "confirm"?:bool}: gives the photos a geotag (degrees, latitude -90..90 and longitude -180..180, kept at six decimals) or, with null, removes their location (clearing changes only the photos that are geotagged); one undo step; changing the location of more than 20 photos that already have one, or removing it from more than 5, asks first: needs "confirm": true; written into the files while Write captions, tags and places is on. Replies {ids, geo, changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: set,
        },
        CommandSpec {
            id: "geo.clear",
            label: CLEAR_LABEL,
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "confirm"?:bool}: Clear Location…: removes the location of the photos that have one (also from their files while Write captions, tags and places is on); one undo step; removing it from more than 5 photos asks first: needs "confirm": true. Replies {ids, geo: null, changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: clear,
        },
        CommandSpec {
            id: "geo.copy",
            label: "Copy Location",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: Copy Location: keeps the photo's geotag for Paste Location (this session only; nothing is written); disabled for a photo without one. Replies {id, geo:[lat, lon]}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: crate::registry::always,
            run: copy,
        },
        CommandSpec {
            id: "geo.paste",
            label: PASTE_LABEL,
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: Paste Location: gives the photos the place Copy Location kept, one undo step; disabled while nothing is copied. Replies {ids, geo, changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: paste_enabled,
            run: paste,
        },
        CommandSpec {
            id: "places.list",
            label: "List Places",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]} | {"section":"folder:<id>"|"album:<id>"|"album:starred"|"album:recent"}: the geotags of the photos named, or of the section's photos: {total (photos looked at), photos:[{id, geo:[lat, lon]}] (the geotagged ones, in the order given), places (distinct points among them), removed (photos whose location the user removed)}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: crate::registry::always,
            run: list,
        },
    ]
}

/// `geo` given as a point or as `null`: `Some(None)` for `null`, `None` when left out.
fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<LatLon>>, D::Error> {
    Option::<LatLon>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetParams {
    ids: Vec<PhotoId>,
    #[serde(default, deserialize_with = "present")]
    geo: Option<Option<LatLon>>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClearParams {
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListParams {
    ids: Option<Vec<PhotoId>>,
    section: Option<String>,
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SetParams { ids, geo, confirm } = params(p)?;
    let Some(geo) = geo else { return Err(CmdError::BadParams("missing field `geo` ([lat, lon], or null to remove the location)".into())) };
    change(s, ids, geo, Ask::Confirm(confirm))
}

fn clear(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let ClearParams { ids, confirm } = params(p)?;
    let ids = known(s, ids)?;
    if !ids.is_empty() && !ids.iter().any(|id| s.library().is_geotagged(*id)) {
        return Err(CmdError::Disabled(NONE_LOCATED.into()));
    }
    s.name_step(CLEAR_LABEL);
    change(s, ids, None, Ask::Confirm(confirm))
}

fn copy(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    if s.library().photo(id).is_none() {
        return Err(CmdError::NotFound(format!("no photo {id}")));
    }
    let point = s.library().geotag(id).ok_or_else(|| CmdError::Disabled(NO_LOCATION.into()))?;
    s.places.copy(point);
    Ok(json!({ "id": id, "geo": point.round6() }))
}

fn paste_enabled(s: &Session, p: &Value) -> Result<(), Disabled> {
    has_ids(s, p)?;
    match s.places().copied_point() {
        Some(_) => Ok(()),
        None => Err(Disabled(NOTHING_COPIED.into())),
    }
}

fn paste(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let point = s.places().copied_point().ok_or_else(|| CmdError::Disabled(NOTHING_COPIED.into()))?;
    s.name_step(PASTE_LABEL);
    change(s, ids, Some(point), Ask::Never)
}

/// Whether a change asks first.
#[derive(Clone, Copy)]
enum Ask {
    /// Past the counts, unless `true` (the user said yes).
    Confirm(bool),
    /// Never (Paste Location: the menu item is the user's word).
    Never,
}

/// Gives `ids` the point `to`, or with `None` removes their location (only from the photos that
/// are geotagged), as one op; asks first per `ask`.
fn change(s: &mut Session, ids: Vec<PhotoId>, to: Option<LatLon>, ask: Ask) -> Result<Value, CmdError> {
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let lib = s.library();
    let target = match to {
        Some(p) => Geo::At(p.round6()),
        None => Geo::Removed,
    };
    let changed: Vec<PhotoId> = match target {
        Geo::At(_) => ids.iter().copied().filter(|id| lib.geo(*id) != Some(target)).collect(),
        Geo::Removed => ids.iter().copied().filter(|id| lib.is_geotagged(*id)).collect(),
    };
    if let Ask::Confirm(false) = ask {
        let question = match target {
            Geo::At(_) => {
                let moved = changed.iter().filter(|id| lib.is_geotagged(**id)).count();
                (moved > MOVE_ASKS_PAST).then(|| format!("Change the location of {}?", photos(moved)))
            }
            Geo::Removed => (changed.len() > CLEAR_ASKS_PAST).then(|| format!("Remove the location from {}?", photos(changed.len()))),
        };
        if let Some(q) = question {
            return Err(CmdError::Disabled(format!("{q} Send \"confirm\": true to go ahead.")));
        }
    }
    let n = changed.len();
    if n > 0 {
        s.apply(Op::SetGeo { geo: changed.into_iter().map(|id| (id, Some(target))).collect() })?;
    }
    Ok(json!({ "ids": ids, "geo": target.point(), "changed": n }))
}

/// "1 photo", "21 photos".
fn photos(n: usize) -> String {
    if n == 1 { "1 photo".into() } else { format!("{n} photos") }
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let ListParams { ids, section } = params(p)?;
    let ids = match (ids, section) {
        (Some(_), Some(_)) => return Err(CmdError::BadParams("give either \"ids\" or \"section\", not both".into())),
        (None, None) => return Err(CmdError::Disabled("Select some photos or a section first.".into())),
        (Some(ids), None) => known(s, ids)?,
        (None, Some(text)) => match crate::cmd::section::section(s, &text)? {
            Section::Folder(folder) => s.library().photos_in(folder).into_iter().map(|p| p.id).collect(),
            Section::Album(album) => {
                if matches!(album, AlbumRef::Search) {
                    return Err(CmdError::Disabled("There are no search results.".into()));
                }
                crate::cmd::section::section_photos(s, &album)
            }
        },
    };
    let lib = s.library();
    let tagged = lib.geotags_of(&ids);
    let places: BTreeSet<(i64, i64)> = tagged.iter().map(|(_, p)| micro(p)).collect();
    let removed = ids.iter().filter(|id| lib.geo(**id) == Some(Geo::Removed)).count();
    let photos: Vec<Value> = tagged.iter().map(|(id, p)| json!({ "id": id, "geo": p })).collect();
    Ok(json!({ "total": ids.len(), "photos": photos, "places": places.len(), "removed": removed }))
}

/// A point in whole micro-degrees (a geotag is stored at six decimals), for counting places.
fn micro(p: &LatLon) -> (i64, i64) {
    // Both values are on the globe (at most 180), so the products fit an `i64`.
    let m = |v: f64| (v * 1e6).round() as i64;
    (m(p.lat()), m(p.lon()))
}
