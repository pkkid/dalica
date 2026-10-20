//! "Import from:" (`organizing.md` §3 item 1): the dropdown along the top of the Import room. Its
//! menu lists the cards and drives `import.sources` gives (from the engine's cached device list,
//! never waiting on the disk), then the cameras and phones `camera.list` gives (one item per
//! storage; a camera another program holds is shown greyed out with its reason), then the recent
//! sources, then Folder… (the folder picker, `import.chooseFolder`). Choosing one lists it
//! (`import.openSource {device}` for a card or a drive, `{path}` for a recent folder,
//! `camera.open {id, storage}` for a camera).
//!
//! The room asks for the sources at most every [`POLL`] while it is in front, so the menu follows
//! a card or a camera plugged in or pulled out while it is open; one that appears then is listed
//! at once, and with no source listed yet the first card (else the first camera) is ([`poll`]).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use dalica_engine::Session;
use dalica_ui_kit::{Tokens, fonts, widgets};
use egui::{Align2, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::{Action, Snapshot};
use crate::app::App;
use crate::tl;

/// How often the room asks for the sources while it is in front.
pub const POLL: Duration = Duration::from_secs(1);
/// The dropdown's automation id; its items are `<id>/<n>` (from 0, in the menu's order) and
/// `<id>/folder` (Folder…).
pub const ID: &str = "choice:import.source";
/// The label left of the dropdown (English, as the constants below; drawn through the catalogs).
pub const LABEL: &str = "Import from:";
/// The dropdown's text with nothing listed.
pub const NOTHING: &str = "Choose a source";
/// Folder…'s title and description.
pub const FOLDER: &str = "Folder\u{2026}";
pub const FOLDER_DETAIL: &str = "Choose a folder on this computer";

/// What kind of source a menu item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Card,
    Drive,
    Camera,
    Recent,
}

impl Kind {
    /// The item's description line.
    pub fn detail(self) -> String {
        match self {
            Kind::Card => tl!("Camera card"),
            Kind::Drive => tl!("Removable drive"),
            Kind::Camera => tl!("Camera or phone"),
            Kind::Recent => tl!("Recent folder"),
        }
    }
}

/// Why a camera cannot be opened (`dalica_engine::camera::unreachable_reason`), in the language in
/// effect, what the system says of it kept as it says it; any other reason looked up whole by its
/// English.
pub fn unavailable(why: &str) -> String {
    let detail = |prefix: &str, suffix: &str| why.strip_prefix(prefix).and_then(|rest| rest.strip_suffix(suffix));
    if let Some(d) = detail("Another program is using this camera (", "); close it, or import from the folder it shows") {
        tl!("Another program is using this camera ({detail}); close it, or import from the folder it shows", detail = d)
    } else if let Some(d) = detail("Dalica may not open this camera (", ")") {
        tl!("Dalica may not open this camera ({detail})", detail = d)
    } else if let Some(d) = detail("The camera did not answer as expected (", ")") {
        tl!("The camera did not answer as expected ({detail})", detail = d)
    } else {
        crate::i18n::tr(why)
    }
}

/// A camera's storage as a menu item opens it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraRef {
    pub id: String,
    /// The storage (`None`: every storage, for a camera that lists none).
    pub storage: Option<u32>,
}

/// One source of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub name: String,
    /// The folder listed (a card's `DCIM` folder, a drive's root, the recent folder); a camera's
    /// source path (`dalica_engine::camera::source_path`).
    pub path: PathBuf,
    /// The camera storage, for a camera.
    pub camera: Option<CameraRef>,
    /// Why it cannot be opened (a camera another program holds): shown greyed out with it.
    pub unavailable: Option<String>,
}

impl Item {
    /// What choosing it runs: `import.openSource` with its device or its path, `camera.open` for a
    /// camera.
    pub fn open(&self) -> (&'static str, Value) {
        match (&self.kind, &self.camera) {
            (Kind::Camera, Some(c)) => ("camera.open", json!({ "id": c.id, "storage": c.storage })),
            (Kind::Card | Kind::Drive | Kind::Camera, _) => ("import.openSource", json!({ "device": self.path })),
            (Kind::Recent, _) => ("import.openSource", json!({ "path": self.path })),
        }
    }
}

/// The sources as last asked.
#[derive(Debug, Default)]
pub struct Sources {
    pub list: Vec<Item>,
    /// The frame clock's time of the last ask.
    asked: Option<f64>,
    /// The cards and the cameras' storages of the last ask (`None` before the first).
    cards: Option<HashSet<PathBuf>>,
}

/// The sources of an `import.sources` reply, in its order; entries it cannot read are left out.
pub fn items(reply: &Value) -> Vec<Item> {
    let Some(list) = reply["sources"].as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|s| {
            let kind = match s["kind"].as_str()? {
                "card" => Kind::Card,
                "drive" => Kind::Drive,
                "recent" => Kind::Recent,
                _ => return None,
            };
            Some(Item { kind, name: s["name"].as_str()?.to_string(), path: PathBuf::from(s["path"].as_str()?), camera: None, unavailable: None })
        })
        .collect()
}

/// The camera items of a `camera.list` reply: one per storage ("Name (Storage)" when it has
/// several), or one greyed out with its reason when it cannot be opened; entries it cannot read
/// are left out.
pub fn camera_items(reply: &Value) -> Vec<Item> {
    let Some(list) = reply["cameras"].as_array() else { return Vec::new() };
    let mut out = Vec::new();
    for c in list {
        let (Some(id), Some(name)) = (c["id"].as_str(), c["name"].as_str()) else { continue };
        let reason = c["reason"].as_str().map(str::to_string);
        let storages: Vec<(u32, String)> = c["storages"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| Some((u32::try_from(s["id"].as_u64()?).ok()?, s["name"].as_str()?.to_string()))).collect())
            .unwrap_or_default();
        let item = |storage: Option<u32>, shown: String| Item {
            kind: Kind::Camera,
            name: shown,
            path: dalica_engine::camera::source_path(id, storage),
            camera: Some(CameraRef { id: id.to_string(), storage }),
            unavailable: reason.clone(),
        };
        match storages.as_slice() {
            _ if reason.is_some() => out.push(item(None, name.to_string())),
            [] => out.push(item(None, name.to_string())),
            [(st, _)] => out.push(item(Some(*st), name.to_string())),
            more => out.extend(more.iter().map(|(st, label)| item(Some(*st), format!("{name} ({label})")))),
        }
    }
    out
}

/// The folder of the first card of an `import.sources` reply.
pub fn first_card(reply: &Value) -> Option<PathBuf> {
    items(reply).into_iter().find(|i| i.kind == Kind::Card).map(|i| i.path)
}

/// Asks for the sources when the last ask is [`POLL`] old (`now`: the frame clock, seconds).
/// Returns the `import.openSource` parameters of a card to list: the first card while no source
/// is listed, or a card plugged in since the last ask.
///
/// A camera to list is opened here (`camera.open`) and `None` returned.
pub fn poll(session: &mut Session, s: &mut Sources, now: f64) -> Option<Value> {
    if s.asked.is_some_and(|at| (0.0..POLL.as_secs_f64()).contains(&(now - at))) {
        return None;
    }
    s.asked = Some(now);
    let reply = session.execute("import.sources", &json!({})).ok()?;
    let mut list = items(&reply);
    let cameras = session.execute("camera.list", &json!({})).map(|r| camera_items(&r)).unwrap_or_default();
    // The cameras after the cards and drives, before the recent folders.
    let at = list.iter().position(|i| i.kind == Kind::Recent).unwrap_or(list.len());
    list.splice(at..at, cameras);
    s.list = list;
    let openable = |i: &&Item| matches!(i.kind, Kind::Card | Kind::Camera) && i.unavailable.is_none();
    let cards: HashSet<PathBuf> = s.list.iter().filter(openable).map(|i| i.path.clone()).collect();
    let open = session.import_session().source().is_some();
    let plugged = s.cards.as_ref().and_then(|before| s.list.iter().filter(openable).find(|i| !before.contains(&i.path)));
    let first = s.list.iter().find(|i| i.kind == Kind::Card).or_else(|| s.list.iter().find(openable)).filter(|_| !open);
    let chosen = plugged.or(first).map(Item::open);
    s.cards = Some(cards);
    match chosen? {
        ("import.openSource", params) => Some(params),
        (id, params) => {
            // A camera gone meanwhile is no error: the menu shows it no more at the next ask.
            let _ = session.execute(id, &params);
            None
        }
    }
}

/// Draws "Import from:" and its dropdown in `ui`'s rect (the room's top bar).
pub fn show(app: &App, ui: &mut Ui, t: &Tokens, s: &Sources, snap: &Snapshot, actions: &mut Vec<Action>) {
    let _ = app;
    let rect = ui.max_rect();
    let font = fonts::regular(t.body);
    let label = widgets::label_in(ui.painter(), rect.shrink2(vec2(12.0, 0.0)), Align2::LEFT_CENTER, &tl!("Import from:"), font, t.text_label);
    let nothing = tl!("Choose a source");
    let shown = snap.source.as_ref().map_or(nothing.as_str(), |src| src.name.as_str());
    let at = pos2(label.max.x + 6.0, rect.center().y - t.metrics.interact_h / 2.0);
    let slot = egui::Rect::from_min_max(at, pos2(rect.max.x - 12.0, at.y + t.metrics.interact_h));
    let current = snap.source.as_ref().map(|src| src.path.clone());
    ui.scope_builder(UiBuilder::new().max_rect(slot), |ui| {
        widgets::dropdown(ui, t, ID, shown, |ui| {
            for (n, item) in s.list.iter().enumerate() {
                let checked = current.as_ref() == Some(&item.path);
                let detail = item.unavailable.as_deref().map_or_else(|| item.kind.detail(), unavailable);
                let clicked = ui
                    .add_enabled_ui(item.unavailable.is_none(), |ui| {
                        widgets::dropdown_item(ui, t, &format!("{ID}/{n}"), &item.name, &detail, checked).clicked()
                    })
                    .inner;
                if clicked {
                    actions.push(item.open());
                }
            }
            if !s.list.is_empty() {
                ui.separator();
            }
            if widgets::dropdown_item(ui, t, &format!("{ID}/folder"), &tl!("Folder\u{2026}"), &tl!("Choose a folder on this computer"), false)
                .clicked()
            {
                actions.push(("import.chooseFolder", json!({})));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sources_reply_becomes_the_menu() {
        let reply = json!({ "sources": [
            { "kind": "drive", "name": "USB STICK", "path": "/m/USB STICK", "root": "/m/USB STICK" },
            { "kind": "card", "name": "EOS_DIGITAL", "path": "/m/EOS_DIGITAL/DCIM", "root": "/m/EOS_DIGITAL" },
            { "kind": "recent", "name": "Scans", "path": "/home/me/Scans" },
            { "kind": "phone", "name": "?", "path": "/x" },
            { "kind": "card" },
        ] });
        let list = items(&reply);
        assert_eq!(
            list.iter().map(|i| (i.kind, i.name.as_str())).collect::<Vec<_>>(),
            [(Kind::Drive, "USB STICK"), (Kind::Card, "EOS_DIGITAL"), (Kind::Recent, "Scans")]
        );
        assert_eq!(first_card(&reply), Some(PathBuf::from("/m/EOS_DIGITAL/DCIM")));
        assert_eq!(list[1].open(), ("import.openSource", json!({ "device": "/m/EOS_DIGITAL/DCIM" })));
        assert_eq!(list[2].open(), ("import.openSource", json!({ "path": "/home/me/Scans" })));
        assert_eq!(items(&json!(null)), []);
    }

    #[test]
    fn cameras_become_one_item_per_storage_and_a_held_one_is_greyed_out_with_its_reason() {
        let reply = json!({ "cameras": [
            { "id": "a", "name": "Canon EOS R6", "access": "open", "reason": null, "storages": [{ "id": 65537, "name": "SD1" }, { "id": 131073, "name": "SD2" }] },
            { "id": "b", "name": "Pixel 7", "access": "open", "reason": null, "storages": [{ "id": 65537, "name": "Internal" }] },
            { "id": "c", "name": "Nikon Z6", "access": "held", "reason": "Another program is using this camera", "storages": [] },
            { "name": "no id" },
        ] });
        let list = camera_items(&reply);
        assert_eq!(list.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Canon EOS R6 (SD1)", "Canon EOS R6 (SD2)", "Pixel 7", "Nikon Z6"]);
        assert_eq!(list[1].open(), ("camera.open", json!({ "id": "a", "storage": 131073 })));
        assert_eq!(list[2].open(), ("camera.open", json!({ "id": "b", "storage": 65537 })));
        assert_eq!(list[3].unavailable.as_deref(), Some("Another program is using this camera"));
        assert!(list[..3].iter().all(|i| i.kind == Kind::Camera && i.unavailable.is_none()));
    }
}
