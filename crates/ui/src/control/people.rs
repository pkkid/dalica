//! People's names withheld from the window's replies (`control-protocol.md` §6.4, §6.6–§6.7,
//! §6.10, `faces.md` §7): without `--automation-allow-people` (`Config::allow_people`)
//! `ui.inspect`, `ui.state`, `ui.widgets` and `ui.menu.tree` carry no person's name or nickname, and
//! `ui.menu.invoke {path}` matches a person's row by the placeholder only. This is the one list of
//! what is withheld, as the engine's `automation::PEOPLE_REPLIES` is for the commands' replies.
//!
//! A fixed placeholder ([`PLACEHOLDER`]) stands where a name was; the rest of a label is kept (a
//! tree row's count, a People pane row's photos, a suggestion's "?"), and every widget id stays,
//! so a client still finds, clicks and drags by id. What is withheld is chosen by **widget id, role
//! and dialog kind**, never by matching a reply's text against the library's names: a folder or a
//! caption renamed to a guessed name would then come back changed, telling the guess right (a
//! guess-oracle, `faces.md` §7). Withheld, without the permission:
//!
//! - **Labels** ([`widgets`]): the People group's rows (`tree:person:<id>`, "Ana (12)" →
//!   "Person (12)"), a person section's header, icon and title (`section:person:<id>`, `/icon`,
//!   `/title`), the People pane's rows (`person:<id>`, "Ana, 3 photos" → "Person, 3 photos"), face
//!   tiles (`face:<face>`, `face:<face>/in/<id>`: a name, or "Ana?" → "Person?"; "Unnamed face"
//!   stays), the edit view's face boxes (`facebox:<face>`; "Face" stays), the people the name
//!   fields offer (`field:faces.name/<n>/option/<k>`, `field:people.name/<face>/option/<k>`,
//!   `field:faces.manual/option/<k>`), the menus' people rows (`menuitem:faces.moveToPerson/<id>`,
//!   `menuitem:people.merge/<id>`, in `ui.widgets` and `ui.inspect`'s `popup`), the search banner
//!   while the People pane filters by people (`banner:search`: "Showing photos with Person and
//!   Person") and a face drag's pill (`drag:pill`).
//! - **Menus** ([`nodes`]): the same people rows in `ui.menu.tree` (the face and person menus) and
//!   in the model `ui.menu.invoke` looks a path up in.
//! - **The view state** ([`state`]): the names typed in the Unnamed grid (`unnamed.names`), Add a
//!   Person Manually's name (`peoplePane.manual.name`), and the drafts of the dialogs that hold a
//!   name (New Person, Rename Person: `fields.name`, `fields.nickname`, and their command's), in
//!   `dialogs` and in `ui.inspect`'s `dialog` ([`dialog`]).
//! - **`ui.inspect`'s own** ([`inspect`]): the tooltip over a widget whose label is withheld (a
//!   person section's icon says their name), and the drag's `pill` while faces are dragged.
//!
//! Screenshots show what the window shows (`control-protocol.md` §11). With the permission every
//! reply is unchanged.

use std::collections::HashSet;

use dalica_core::PersonId;
use dalica_ui_kit::WidgetInfo;
use serde_json::Value;

use super::Config;
use crate::app::App;
use crate::menus::Node;
use crate::state::{DialogKind, DropAt, FaceRef};

/// What stands where a name was.
pub const PLACEHOLDER: &str = "Person";

/// A face tile's label when it shows no name (ui-kit's `face_tile` with `FaceLabel::None`).
const UNNAMED_FACE: &str = "Unnamed face";
/// A face box's label when it has no name (ui-kit's `face_box`).
const UNNAMED_BOX: &str = "Face";
/// The filter banner's words before the people's names (`commands::people_pane::banner`).
const BANNER_PREFIX: &str = "Showing photos with ";

/// The name fields whose autocomplete offers the people (`<field>/option/<k>`): the Unnamed grid's,
/// the People pane's and Add a Person Manually's.
const NAME_FIELDS: &[&str] = &["field:faces.name/", "field:people.name/", "field:faces.manual"];
/// The menu rows that name a person (`<prefix><PersonId>`): Move to Person ▸ and Merge Into ▸.
const PERSON_ROWS: &[&str] = &["menuitem:faces.moveToPerson/", "menuitem:people.merge/"];
/// The dialogs whose draft holds a name, and the keys that hold it.
const NAME_DIALOGS: &[DialogKind] = &[DialogKind::NewPerson, DialogKind::RenamePerson];
const NAME_KEYS: &[&str] = &["name", "nickname"];

/// Whether people's names are withheld from this control client's replies.
pub(super) fn withheld(config: &Config) -> bool {
    !config.allow_people
}

/// Gives the widgets that show a person's name the placeholder instead (module docs); returns the
/// ids of those it changed.
pub(super) fn widgets(app: &App, list: &mut [WidgetInfo]) -> HashSet<String> {
    let ids: HashSet<&str> = list.iter().map(|w| w.id.as_str()).collect();
    let changes: Vec<(usize, String)> = list.iter().enumerate().filter_map(|(i, w)| label(app, &ids, w).map(|l| (i, l))).collect();
    let mut changed = HashSet::with_capacity(changes.len());
    for (i, l) in changes {
        if let Some(w) = list.get_mut(i) {
            w.label = l;
            changed.insert(w.id.clone());
        }
    }
    changed
}

/// The label widget `w` shows in place of its own, when its own holds a name.
fn label(app: &App, ids: &HashSet<&str>, w: &WidgetInfo) -> Option<String> {
    let id = w.id.as_str();
    if let Some(rest) = id.strip_prefix("tree:person:") {
        // "Ana (12)": the count stays.
        return PersonId::parse(rest).is_ok().then(|| keep_suffix(&w.label, tree_count));
    }
    if let Some(rest) = id.strip_prefix("section:person:") {
        let (person, part) = rest.split_once('/').map_or((rest, None), |(p, part)| (p, Some(part)));
        return (PersonId::parse(person).is_ok() && matches!(part, None | Some("icon" | "title"))).then(|| PLACEHOLDER.to_string());
    }
    if let Some(rest) = id.strip_prefix("person:") {
        // The People pane's row: "Ana, 3 photos"; its photos stay.
        return PersonId::parse(rest).is_ok().then(|| keep_suffix(&w.label, pane_count));
    }
    if let Some(rest) = id.strip_prefix("face:") {
        let tile = match rest.split('/').collect::<Vec<_>>().as_slice() {
            [face] => face.parse::<FaceRef>().is_ok(),
            [face, "in", person] => face.parse::<FaceRef>().is_ok() && PersonId::parse(person).is_ok(),
            _ => false,
        };
        if !tile || w.label == UNNAMED_FACE {
            return None;
        }
        // A suggestion ("Ana?") has its ✓.
        let suggested = ids.contains(format!("{id}/confirm").as_str());
        return Some(if suggested { format!("{PLACEHOLDER}?") } else { PLACEHOLDER.to_string() });
    }
    if let Some(rest) = id.strip_prefix("facebox:") {
        return (rest.parse::<FaceRef>().is_ok() && w.label != UNNAMED_BOX).then(|| PLACEHOLDER.to_string());
    }
    if NAME_FIELDS.iter().any(|f| id.starts_with(f)) && id.contains("/option/") {
        return Some(PLACEHOLDER.to_string());
    }
    if person_row(id) {
        return Some(PLACEHOLDER.to_string());
    }
    if id == crate::panels::search_banner::BANNER_ID {
        return banner(app);
    }
    if id == crate::drag::PILL_ID {
        return face_pill(app);
    }
    None
}

/// Whether menu row `widget` names a person (`menuitem:faces.moveToPerson/<PersonId>`, …).
fn person_row(widget: &str) -> bool {
    PERSON_ROWS.iter().any(|p| widget.strip_prefix(p).is_some_and(|rest| PersonId::parse(rest).is_ok()))
}

/// [`PLACEHOLDER`] followed by the end of `label` that `suffix` finds (a count), or alone.
fn keep_suffix(label: &str, suffix: fn(&str) -> Option<&str>) -> String {
    format!("{PLACEHOLDER}{}", suffix(label).unwrap_or(""))
}

/// A tree row's count: the label's last " (<digits>)".
fn tree_count(label: &str) -> Option<&str> {
    let at = label.rfind(" (")?;
    let tail = label.get(at..)?;
    let digits = tail.strip_prefix(" (")?.strip_suffix(')')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(tail)
}

/// A People pane row's photos: the label's last ", <n> photo(s)".
fn pane_count(label: &str) -> Option<&str> {
    let at = label.rfind(", ")?;
    let tail = label.get(at..)?;
    let (digits, noun) = tail.strip_prefix(", ")?.split_once(' ')?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) && matches!(noun, "photo" | "photos")).then_some(tail)
}

/// The search banner while the People pane filters by people: "Showing photos with Person and
/// Person" (one placeholder per person named); `None` (the banner as it is) otherwise.
fn banner(app: &App) -> Option<String> {
    let n = crate::commands::people_pane::names(app, &app.state().people_pane.selected).len();
    (n > 0).then(|| format!("{BANNER_PREFIX}{}", crate::commands::people_pane::listed(&vec![PLACEHOLDER.to_string(); n])))
}

/// A face drag's pill with the placeholder for the person under it, when the pill names them (its
/// text is not the pill without a name, made from the counts alone); `None` for a drag of photos.
fn face_pill(app: &App) -> Option<String> {
    let d = app.state().drag.as_ref().filter(|d| !d.faces.is_empty())?;
    let unnamed = crate::drag::face_pill(d.op, d.faces.len(), None);
    let names = matches!(d.at, Some(DropAt::Person(_))) && d.pill != unnamed;
    Some(if names { crate::drag::face_pill(d.op, d.faces.len(), Some(PLACEHOLDER)) } else { unnamed })
}

/// The menu rows that name a person, given the placeholder, at any depth.
pub(super) fn nodes(nodes: &mut [Node]) {
    for n in nodes {
        match n {
            Node::Item(item) if person_row(&item.widget) => item.label = PLACEHOLDER.to_string(),
            Node::Submenu { children, .. } => self::nodes(children),
            _ => {}
        }
    }
}

/// The view state (`ui.state`, `ui.set`'s reply, the view state in `ui.inspect`) without the names
/// typed: `unnamed.names`, `peoplePane.manual.name`, and the name dialogs' drafts in `dialogs`.
pub(super) fn state(v: &mut Value) {
    if let Some(Value::Object(names)) = v.pointer_mut("/unnamed/names") {
        names.values_mut().for_each(withhold);
    }
    if let Some(name) = v.pointer_mut("/peoplePane/manual/name") {
        withhold(name);
    }
    if let Some(Value::Array(dialogs)) = v.get_mut("dialogs") {
        dialogs.iter_mut().for_each(dialog);
    }
}

/// A dialog as `ui.state`'s `dialogs` or `ui.inspect`'s `dialog` lists it (`{kind, fields,
/// command?}`): for a dialog that holds a name (by its kind), the name and nickname drafted, in its
/// fields and in its command's params.
pub(super) fn dialog(d: &mut Value) {
    let names = d.get("kind").and_then(Value::as_str).is_some_and(|k| NAME_DIALOGS.iter().any(|n| n.name() == k));
    if !names {
        return;
    }
    for at in ["/fields", "/command/params"] {
        if let Some(Value::Object(fields)) = d.pointer_mut(at) {
            for key in NAME_KEYS {
                if let Some(v) = fields.get_mut(*key) {
                    withhold(v);
                }
            }
        }
    }
}

/// `ui.inspect`'s own fields: the top dialog's draft, the drag's pill while faces are dragged, and
/// the tooltip over a widget whose label was withheld (`changed`, from [`widgets`]).
pub(super) fn inspect(app: &App, v: &mut Value, hovered: Option<&str>, changed: &HashSet<String>) {
    if let Some(d) = v.get_mut("dialog") {
        dialog(d);
    }
    if let (Some(pill), Some(text)) = (v.pointer_mut("/drag/pill"), face_pill(app)) {
        *pill = Value::String(text);
    }
    if hovered.is_some_and(|h| changed.contains(h))
        && let Some(tip) = v.get_mut("tooltip").filter(|t| !t.is_null())
    {
        *tip = Value::String(PLACEHOLDER.to_string());
    }
}

/// A non-empty string becomes the placeholder; anything else stays.
fn withhold(v: &mut Value) {
    if v.as_str().is_some_and(|s| !s.is_empty()) {
        *v = Value::String(PLACEHOLDER.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The counts a label keeps are found by their shape at its end, never by the name before them.
    #[test]
    fn a_label_keeps_its_count_and_loses_the_rest() {
        assert_eq!(keep_suffix("Ana (12)", tree_count), "Person (12)");
        assert_eq!(keep_suffix("Ana (Lisbon) (3)", tree_count), "Person (3)");
        assert_eq!(keep_suffix("Ana (Lisbon)", tree_count), "Person");
        assert_eq!(keep_suffix("Ana ()", tree_count), "Person");
        assert_eq!(keep_suffix("Ana, Rui, 3 photos", pane_count), "Person, 3 photos");
        assert_eq!(keep_suffix("Ana, 1 photo", pane_count), "Person, 1 photo");
        assert_eq!(keep_suffix("Ana, Rui", pane_count), "Person");
        assert_eq!(keep_suffix("", pane_count), "Person");
    }
}
