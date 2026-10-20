//! The geotag confirmations (`organizing.md` §7): changing the location of more than 20 photos
//! ("Change the location of %d photos?" [Change Location] [Cancel]) and removing it, from more than
//! 5 in the Places pane and always from Tools ▸ Geotag ▸ Clear Location… ("Remove the location from
//! %d photos?" [Remove Location] [Cancel]). Their buttons run `photo.geo.set {ids, geo, confirm:
//! true}` and `geo.clear {ids, confirm: true}`, one library undo step, and close the bubble that
//! asked (`crate::commands::places`). Their drafts (`dialogs[].fields`) hold `ids`, and for a
//! change `at`, the point.

use dalica_core::PhotoId;
use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Map, Value};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::commands::places;
use crate::state::{LatLon, is_lat_lon};
use crate::{tl, tln};

pub const CHANGE_TITLE: &str = "Change Location";
pub const CLEAR_TITLE: &str = "Clear Location";
/// The dialogs' width (points).
pub const WIDTH: f32 = 420.0;

/// A dialog of `kind` (one of this module's) with `draft`.
pub fn dialog(kind: DialogKind, draft: Map<String, Value>) -> Dialog {
    let mut d = Dialog::new(kind);
    d.fields = draft;
    d
}

/// How many photos the draft names.
fn count(d: &Dialog) -> usize {
    d.fields.get("ids").and_then(Value::as_array).map_or(0, Vec::len)
}

/// The question, with the count: "Remove the location from 6 photos?".
pub fn question(d: &Dialog) -> String {
    let n = count(d);
    match d.kind {
        DialogKind::ChangeLocation => tln!(n, "Change the location of {n} photo?", "Change the location of {n} photos?"),
        _ => tln!(n, "Remove the location from {n} photo?", "Remove the location from {n} photos?"),
    }
}

/// The default button of each kind: its action and label.
fn default_button(kind: DialogKind) -> (&'static str, &'static str) {
    match kind {
        DialogKind::ChangeLocation => ("change", "Change Location"),
        _ => ("remove", "Remove Location"),
    }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let (title, label) = match d.kind {
        DialogKind::ChangeLocation => (tl!("Change Location"), tl!("Change Location")),
        _ => (tl!("Clear Location"), tl!("Remove Location")),
    };
    Look { title, width: WIDTH, buttons: vec![Button::new(default_button(d.kind).0, label, ButtonKind::Default), Button::cancel()] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::text_lines(app, ui, &[question(d)]);
}

/// The draft's photos and, for a change, its point; why not when it names none or is broken.
fn draft(d: &Dialog) -> Result<(Vec<PhotoId>, Option<LatLon>), CmdError> {
    let bad = |what: &str| CmdError::BadParams(format!("the {} dialog's draft has no valid `{what}`", d.kind.name()));
    let ids: Vec<PhotoId> = match d.fields.get("ids") {
        None => Vec::new(),
        Some(v) => serde_json::from_value(v.clone()).map_err(|_| bad("ids"))?,
    };
    if ids.is_empty() {
        return Err(CmdError::Disabled(places::SELECT_FIRST.into()));
    }
    let at = match d.kind {
        DialogKind::ChangeLocation => {
            let at: LatLon = d.fields.get("at").cloned().and_then(|v| serde_json::from_value(v).ok()).ok_or_else(|| bad("at"))?;
            if !is_lat_lon(at) {
                return Err(bad("at"));
            }
            Some(at)
        }
        _ => None,
    };
    Ok((ids, at))
}

pub fn act(app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    if action != default_button(d.kind).0 {
        return Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())));
    }
    match draft(d)? {
        (ids, Some(at)) => places::set_location(app, ids, at, true),
        (ids, None) => places::clear_location(app, ids, true),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::dialogs;

    #[test]
    fn the_buttons_change_or_remove_the_location_once_confirmed() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        let draft = Map::from_iter([("ids".into(), json!(ids)), ("at".into(), json!([38.7116, -9.1389]))]);
        dialogs::open(&mut app, dialog(DialogKind::ChangeLocation, draft)).unwrap();
        assert_eq!(question(dialogs::top(&app).unwrap()), "Change the location of 2 photos?");
        dialogs::answer(&mut app, None).unwrap();
        assert!(app.state().dialogs.is_empty());
        assert!(places::some_located(&app, &ids));
        dialogs::open(&mut app, dialog(DialogKind::ClearLocation, Map::from_iter([("ids".into(), json!(ids))]))).unwrap();
        dialogs::answer(&mut app, Some("remove")).unwrap();
        assert!(!places::some_located(&app, &ids));
        // A broken draft is refused, and the dialog stays.
        dialogs::open(&mut app, dialog(DialogKind::ChangeLocation, Map::from_iter([("ids".into(), json!(ids)), ("at".into(), json!([99, 0]))])))
            .unwrap();
        assert!(matches!(dialogs::answer(&mut app, None), Err(CmdError::BadParams(_))));
        assert_eq!(app.state().dialogs.len(), 1);
    }
}
