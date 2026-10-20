//! The face context menu (`organizing.md` §6.3, `menus-and-shortcuts.md` §4): a right-click on a
//! face in the Unnamed grid (and on a person's Zoom to Faces cells, M13.25) selects the face first
//! when it is not selected, then opens this menu for the faces selected. Its rows are
//! [`menus::face_menu`]'s (Move to New Person…, Move to Person ▸, Remove from Person, Set as Person
//! Thumbnail, Ignore Face); this module fills Move to Person ▸ with the people by name, each row
//! running `faces.moveToPerson {person}` (`menuitem:faces.moveToPerson/<PersonId>`), or one greyed
//! row "No people yet" (`menuitem:faces.moveToPerson/none`). `ui.menu.tree {context: "face"}`
//! returns the same rows ([`nodes`]) without opening the menu.

use dalica_core::PersonId;
use dalica_library::PeopleSort;
use egui::Response;
use serde_json::{Value, json};

use crate::app::App;
use crate::menu_catalog::Mark;
use crate::menus::{self, ItemNode, Node, Platform};
use crate::tl;

/// The submenu the people are listed in.
pub const MOVE_TO_PERSON: &str = "Move to Person";
/// The row standing in for the people while there are none.
pub const NO_PEOPLE: &str = "No people yet";

/// The people by name (ignoring case), with their names.
pub fn people_by_name(app: &App) -> Vec<(PersonId, String)> {
    let Some(session) = app.session() else { return Vec::new() };
    let lib = session.library();
    lib.people_sorted(PeopleSort::Name).into_iter().filter_map(|id| lib.person(&id).map(|p| p.name.clone()).map(|name| (id, name))).collect()
}

/// The face menu's rows for the faces selected, Move to Person ▸ filled.
pub fn nodes(app: &App, platform: Platform) -> Vec<Node> {
    let mut rows = menus::face_menu(app, platform);
    finish(app, &mut rows);
    rows
}

/// Fills Move to Person ▸ with a row per person (or the greyed [`NO_PEOPLE`]).
pub fn finish(app: &App, nodes: &mut [Node]) {
    for node in nodes.iter_mut() {
        if let Node::Submenu { label, children, .. } = node
            && *label == MOVE_TO_PERSON
        {
            let ready = crate::commands::enabled(app, "faces.moveToPerson", &Value::Null);
            let people = people_by_name(app);
            *children = if people.is_empty() {
                vec![row("none", tl!("No people yet"), Value::Null, Err(tl!("Name a face first: a person is made when you type a name.")))]
            } else {
                people.into_iter().map(|(id, name)| row(id.as_str(), name, json!({ "person": id }), ready.clone())).collect()
            };
        }
    }
}

fn row(part: &str, label: String, params: Value, ready: Result<(), String>) -> Node {
    let (enabled, reason) = match ready {
        Ok(()) => (true, None),
        Err(why) => (false, Some(why)),
    };
    Node::Item(ItemNode {
        id: "faces.moveToPerson",
        label,
        // No catalog label: a menu path finds the row by the person's name only.
        catalog_label: "",
        params,
        widget: format!("menuitem:faces.moveToPerson/{part}"),
        shortcut: None,
        live: true,
        enabled,
        reason,
        mark: Mark::None,
        checked: false,
        dialog: None,
    })
}

/// Draws the face menu on `response` (a face tile's) while it is open, and runs the row clicked.
/// Returns the command id run, if any.
pub fn show(response: &Response, app: &mut App) -> Option<&'static str> {
    let rows = nodes(app, Platform::current());
    menus::context_menu(response, app, &rows)
}
