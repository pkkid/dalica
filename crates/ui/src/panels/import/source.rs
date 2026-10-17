//! "Import from:" (`organizing.md` §3 item 1): the dropdown along the top of the Import room. Its
//! menu lists the cards and drives `import.sources` gives (from the engine's cached device list,
//! never waiting on the disk), then the recent sources, then Folder… (the folder picker,
//! `import.chooseFolder`). Choosing one lists it (`import.openSource {device}` for a card or a
//! drive, `{path}` for a recent folder).
//!
//! The room asks for the sources at most every [`POLL`] while it is in front, so the menu follows
//! a card plugged in or pulled out while it is open; a card that appears then is listed at once,
//! and with no source listed yet the first card is ([`poll`]).

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use dalica_engine::Session;
use dalica_ui_kit::{Tokens, fonts, widgets};
use egui::{Align2, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::{Action, Snapshot};
use crate::app::App;

/// How often the room asks for the sources while it is in front.
pub const POLL: Duration = Duration::from_secs(1);
/// The dropdown's automation id; its items are `<id>/<n>` (from 0, in the menu's order) and
/// `<id>/folder` (Folder…).
pub const ID: &str = "choice:import.source";
/// The label left of the dropdown.
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
    Recent,
}

impl Kind {
    /// The item's description line.
    pub fn detail(self) -> &'static str {
        match self {
            Kind::Card => "Camera card",
            Kind::Drive => "Removable drive",
            Kind::Recent => "Recent folder",
        }
    }
}

/// One source of the menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub name: String,
    /// The folder listed (a card's `DCIM` folder, a drive's root, the recent folder).
    pub path: PathBuf,
}

impl Item {
    /// What choosing it runs: `import.openSource` with its device or its path.
    pub fn open_params(&self) -> Value {
        match self.kind {
            Kind::Card | Kind::Drive => json!({ "device": self.path }),
            Kind::Recent => json!({ "path": self.path }),
        }
    }
}

/// The sources as last asked.
#[derive(Debug, Default)]
pub struct Sources {
    pub list: Vec<Item>,
    /// The frame clock's time of the last ask.
    asked: Option<f64>,
    /// The cards of the last ask (`None` before the first).
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
            Some(Item { kind, name: s["name"].as_str()?.to_string(), path: PathBuf::from(s["path"].as_str()?) })
        })
        .collect()
}

/// The folder of the first card of an `import.sources` reply.
pub fn first_card(reply: &Value) -> Option<PathBuf> {
    items(reply).into_iter().find(|i| i.kind == Kind::Card).map(|i| i.path)
}

/// Asks for the sources when the last ask is [`POLL`] old (`now`: the frame clock, seconds).
/// Returns the `import.openSource` parameters of a card to list: the first card while no source
/// is listed, or a card plugged in since the last ask.
pub fn poll(session: &mut Session, s: &mut Sources, now: f64) -> Option<Value> {
    if s.asked.is_some_and(|at| (0.0..POLL.as_secs_f64()).contains(&(now - at))) {
        return None;
    }
    s.asked = Some(now);
    let reply = session.execute("import.sources", &json!({})).ok()?;
    s.list = items(&reply);
    let cards: HashSet<PathBuf> = s.list.iter().filter(|i| i.kind == Kind::Card).map(|i| i.path.clone()).collect();
    let open = session.import_session().source().is_some();
    let plugged = s.cards.as_ref().and_then(|before| s.list.iter().find(|i| i.kind == Kind::Card && !before.contains(&i.path)));
    let first = s.list.iter().find(|i| i.kind == Kind::Card).filter(|_| !open);
    let chosen = plugged.or(first).map(Item::open_params);
    s.cards = Some(cards);
    chosen
}

/// Draws "Import from:" and its dropdown in `ui`'s rect (the room's top bar).
pub fn show(app: &App, ui: &mut Ui, t: &Tokens, s: &Sources, snap: &Snapshot, actions: &mut Vec<Action>) {
    let _ = app;
    let rect = ui.max_rect();
    let font = fonts::regular(t.body);
    let label = widgets::label_in(ui.painter(), rect.shrink2(vec2(12.0, 0.0)), Align2::LEFT_CENTER, LABEL, font, t.text_label);
    let shown = snap.source.as_ref().map_or(NOTHING, |src| src.name.as_str());
    let at = pos2(label.max.x + 6.0, rect.center().y - t.metrics.interact_h / 2.0);
    let slot = egui::Rect::from_min_max(at, pos2(rect.max.x - 12.0, at.y + t.metrics.interact_h));
    let current = snap.source.as_ref().map(|src| src.path.clone());
    ui.scope_builder(UiBuilder::new().max_rect(slot), |ui| {
        widgets::dropdown(ui, t, ID, shown, |ui| {
            for (n, item) in s.list.iter().enumerate() {
                let checked = current.as_ref() == Some(&item.path);
                if widgets::dropdown_item(ui, t, &format!("{ID}/{n}"), &item.name, item.kind.detail(), checked).clicked() {
                    actions.push(("import.openSource", item.open_params()));
                }
            }
            if !s.list.is_empty() {
                ui.separator();
            }
            if widgets::dropdown_item(ui, t, &format!("{ID}/folder"), FOLDER, FOLDER_DETAIL, false).clicked() {
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
        assert_eq!(list[1].open_params(), json!({ "device": "/m/EOS_DIGITAL/DCIM" }));
        assert_eq!(list[2].open_params(), json!({ "path": "/home/me/Scans" }));
        assert_eq!(items(&json!(null)), []);
    }
}
