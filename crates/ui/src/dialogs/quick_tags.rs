//! Configure Quick Tags (`organizing.md` §5.2): opened by `quicktags.configure` (the gear of the
//! Tags pane's quick tags, or a click on a quick tag showing "+").
//!
//! ```text
//! Configure Quick Tags
//! Quick tags put a tag on the selected photos with one click …
//! [x] Use the top two buttons for recently used tags
//! [ slot 1        ] [ slot 2        ]          (disabled while the box is ticked)
//! [ slot 3 ] [ slot 4 ] [ slot 5 ]
//! [ slot 6        ] [ slot 7        ]
//! [ slot 8 ] [ slot 9 ] [ slot 10 ]
//! [Fill Empty with Common Tags]
//!                                   [Cancel] [OK]
//! ```
//!
//! The fields sit in the same rows of 2/3/2/3 as the pane's buttons. The draft is `{useRecent,
//! slots}`: the box and the ten slots' text (`""`: unassigned), starting from the stored quick
//! tags (`tags.quick.get`). While the box is ticked, fields 1 and 2 are greyed: those buttons
//! follow the most recently added tags, and the text kept in them comes back when the box is
//! cleared. [Fill Empty with Common Tags] puts the tags used on the most photos into the empty
//! fields that can take one, in order, leaving out tags already in a field. OK runs
//! `tags.quick.set` once (no undo step: quick tags are a setting); it is greyed while a field holds
//! more than a tag may. Cancel changes nothing.
//!
//! Widget ids (`control-protocol.md` §7, the dialog row): `check:dialog.useRecent`,
//! `field:dialog.slot1` … `field:dialog.slot10`, `button:dialog.fillCommon`, `button:dialog.cancel`,
//! `button:dialog.ok`.

use dalica_engine::CmdError;
use dalica_library::MAX_TAG;
use dalica_library::tags::fold;
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target, TextField};
use egui::{RichText, Ui};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::panels::tags::quick::ROWS;
use crate::state::MAX_DRAFT;
use crate::tl;

pub const TITLE: &str = "Configure Quick Tags";
/// The dialog's width (points).
pub const WIDTH: f32 = 460.0;
/// The number of quick tags.
pub const SLOTS: usize = 10;
/// The slots that follow the recently used tags while the box is ticked (1 and 2).
pub const RECENT_SLOTS: usize = 2;
/// The checkbox's label.
pub const USE_RECENT: &str = "Use the top two buttons for recently used tags";
/// The fill button's label.
pub const FILL: &str = "Fill Empty with Common Tags";
/// What the dialog says above the box.
pub const INTRO: &str = "Quick tags add a tag to the selected photos with one click, or with Ctrl+Alt+1 … Ctrl+Alt+0. \
Type the tags you use most below. With the option ticked, the top two buttons show the tags you \
added most recently instead.";
/// Why OK is greyed when a field holds more than a tag may ([`MAX_TAG`] bytes).
pub const TOO_LONG: &str = "A tag is at most 256 bytes.";

pub const USE_RECENT_ID: &str = "check:dialog.useRecent";
pub const FILL_ID: &str = "button:dialog.fillCommon";

/// Slot `n`'s field (1 … 10): `field:dialog.slot<n>`.
pub fn field_id(n: usize) -> String {
    format!("field:dialog.slot{n}")
}

/// The dialog, its draft filled from the stored quick tags (with no library yet: the box ticked,
/// every slot empty).
pub fn dialog(app: &App) -> Dialog {
    let (slots, use_recent) = match app.session() {
        Some(s) => {
            let q = s.settings().quick_tags();
            (q.slots.iter().map(|t| t.clone().unwrap_or_default()).collect::<Vec<_>>(), q.use_recent)
        }
        None => (vec![String::new(); SLOTS], true),
    };
    let mut d = Dialog::new(DialogKind::QuickTags);
    d.fields = draft(&slots, use_recent);
    d
}

/// A draft holding `slots` (padded or cut to ten) and the box.
pub fn draft(slots: &[String], use_recent: bool) -> Map<String, Value> {
    let mut slots: Vec<String> = slots.iter().take(SLOTS).cloned().collect();
    slots.resize(SLOTS, String::new());
    match json!({ "useRecent": use_recent, "slots": slots }) {
        Value::Object(o) => o,
        _ => Map::new(),
    }
}

/// The ten slots of a draft (anything that is not text read as empty).
pub fn slots(fields: &Map<String, Value>) -> Vec<String> {
    let list = fields.get("slots").and_then(Value::as_array);
    (0..SLOTS).map(|i| list.and_then(|l| l.get(i)).and_then(Value::as_str).unwrap_or("").to_string()).collect()
}

fn use_recent(fields: &Map<String, Value>) -> bool {
    fields.get("useRecent").and_then(Value::as_bool).unwrap_or(true)
}

/// Whether slot `i` (0-based) takes a tag in the dialog: all but the first two while the box is
/// ticked.
fn editable(i: usize, use_recent: bool) -> bool {
    !use_recent || i >= RECENT_SLOTS
}

/// Why OK cannot run, if it cannot: a field holding more than a tag may.
pub fn refused(fields: &Map<String, Value>) -> Option<&'static str> {
    slots(fields).iter().any(|s| s.trim().len() > MAX_TAG).then_some(TOO_LONG)
}

/// The tags in use, the ones on the most photos first (ties in alphabetical order, ignoring
/// case), from `tags.list`. Asked on a click of the fill button only.
pub fn common_tags(app: &mut App) -> Vec<String> {
    let Some(s) = app.session_mut() else { return Vec::new() };
    let Ok(reply) = s.execute("tags.list", &json!({})) else { return Vec::new() };
    let mut tags: Vec<(u64, String, String)> = reply
        .get("tags")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|t| Some((t.get("count")?.as_u64()?, t.get("tag")?.as_str()?.to_string())))
                .map(|(count, tag)| (count, fold(&tag), tag))
                .collect()
        })
        .unwrap_or_default();
    tags.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    tags.into_iter().map(|(_, _, tag)| tag).collect()
}

/// Puts `common` (most common first) into the empty slots that take a tag, in order, leaving out
/// tags already in one of those slots (ignoring case). Returns how many slots it filled.
pub fn fill_empty(slots: &mut [String], use_recent: bool, common: &[String]) -> usize {
    let mut have: Vec<String> =
        slots.iter().enumerate().filter(|(i, s)| editable(*i, use_recent) && !s.trim().is_empty()).map(|(_, s)| fold(s.trim())).collect();
    let mut next = common.iter().filter(|t| !t.trim().is_empty());
    let mut filled = 0;
    for (i, slot) in slots.iter_mut().enumerate() {
        if !editable(i, use_recent) || !slot.trim().is_empty() {
            continue;
        }
        let Some(tag) = next.by_ref().find(|t| !have.contains(&fold(t.trim()))) else { break };
        have.push(fold(tag.trim()));
        *slot = tag.trim().to_string();
        filled += 1;
    }
    filled
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let ok = Button::new("ok", tl!("OK"), ButtonKind::Default).enabled(refused(&d.fields).is_none());
    Look { title: tl!("Configure Quick Tags"), width: WIDTH, buttons: vec![Button::cancel(), ok] }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let gap = 6.0;
    ui.spacing_mut().item_spacing.y = gap;
    let intro = tl!(
        "Quick tags add a tag to the selected photos with one click, or with Ctrl+Alt+1 … Ctrl+Alt+0. Type the tags you use most below. With the option ticked, the top two buttons show the tags you added most recently instead."
    );
    ui.label(RichText::new(intro).font(fonts::regular(t.body)).color(t.text_label));
    ui.add_space(4.0);

    let recent = use_recent(&d.fields);
    let mut on = recent;
    let label = tl!("Use the top two buttons for recently used tags");
    let r = ui.checkbox(&mut on, RichText::new(&label).font(fonts::regular(t.body)));
    widgets::register(ui.ctx(), USE_RECENT_ID, Role::Toggle, &label, Target::Widget(&r), on);
    if on != recent {
        d.fields.insert("useRecent".into(), Value::Bool(on));
    }

    let mut slots = slots(&d.fields);
    let before = slots.clone();
    let width = ui.available_width();
    let mut i = 0usize;
    let recently = tl!("Recently used");
    for (row, &n) in ROWS.iter().enumerate() {
        let w = ((width - gap * n.saturating_sub(1) as f32) / n as f32).floor();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for _ in 0..n {
                let Some(text) = slots.get_mut(i) else { return };
                let hint = if editable(i, on) { "" } else { recently.as_str() };
                ui.add_enabled_ui(editable(i, on), |ui| {
                    TextField::new(&field_id(i + 1), hint, w).select_on_focus(true).show(ui, &t, text);
                });
                if let Some((at, _)) = text.char_indices().nth(MAX_DRAFT) {
                    text.truncate(at);
                }
                i += 1;
            }
        });
        if row == 0 {
            ui.add_space(4.0);
        }
    }

    ui.add_space(2.0);
    let can_fill = slots.iter().enumerate().any(|(i, s)| editable(i, on) && s.trim().is_empty());
    let fill = ui.add_enabled_ui(can_fill, |ui| widgets::text_button(ui, &t, FILL_ID, &tl!("Fill Empty with Common Tags"), false)).inner;
    if fill.clicked() {
        let common = common_tags(app);
        fill_empty(&mut slots, on, &common);
    }
    if slots != before {
        d.fields.insert("slots".into(), json!(slots));
    }
    if refused(&d.fields).is_some() {
        ui.label(RichText::new(tl!("A tag is at most {max} bytes.", max = MAX_TAG)).font(fonts::regular(t.small)).color(t.caution));
    }
}

/// OK: `tags.quick.set {slots, useRecent}` (each slot trimmed; `""` unassigned).
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    if let Some(why) = refused(&d.fields) {
        return Err(CmdError::Disabled(why.into()));
    }
    let slots: Vec<String> = slots(&d.fields).iter().map(|s| s.trim().to_string()).collect();
    app.run("tags.quick.set", &json!({ "slots": slots, "useRecent": use_recent(&d.fields) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn fill_empty_takes_the_most_common_tags_not_already_there() {
        let common = strings(&["beach", "Sea", "tram", "lisbon", "bus"]);
        // The box ticked: slots 1 and 2 are left alone; SEA is already in a slot.
        let mut slots = strings(&["", "x", "SEA", "", "", "", "", "", "", ""]);
        assert_eq!(fill_empty(&mut slots, true, &common), 4);
        assert_eq!(slots, strings(&["", "x", "SEA", "beach", "tram", "lisbon", "bus", "", "", ""]));
        // The box cleared: every slot takes one, and a tag held in slot 2 is not offered again.
        let mut slots = strings(&["", "tram", "", "", "", "", "", "", "", ""]);
        assert_eq!(fill_empty(&mut slots, false, &common), 4);
        assert_eq!(slots, strings(&["beach", "tram", "Sea", "lisbon", "bus", "", "", "", "", ""]));
        // Nothing to fill, or nothing to fill with.
        assert_eq!(fill_empty(&mut slots, false, &[]), 0);
        let mut full = vec!["t".to_string(); SLOTS];
        assert_eq!(fill_empty(&mut full, false, &common), 0);
    }

    #[test]
    fn the_draft_reads_and_refuses_what_is_not_a_tag() {
        let mut f = draft(&strings(&["a", "b"]), false);
        assert_eq!(slots(&f).len(), SLOTS);
        assert_eq!(slots(&f)[..3], strings(&["a", "b", ""]));
        assert!(!use_recent(&f));
        assert_eq!(refused(&f), None);
        f.insert("slots".into(), json!(["x".repeat(MAX_TAG + 1)]));
        assert_eq!(refused(&f), Some(TOO_LONG));
        assert!(TOO_LONG.contains(&MAX_TAG.to_string()), "the message names the limit");
        f.insert("slots".into(), json!([1, null, " y "]));
        assert_eq!(slots(&f)[..3], strings(&["", "", " y "]), "odd entries read as empty");
        assert!(use_recent(&Map::new()), "the box is ticked unless the draft says otherwise");
        assert_eq!((field_id(1), field_id(10)), ("field:dialog.slot1".to_string(), "field:dialog.slot10".to_string()));
    }

    #[test]
    fn the_most_common_tags_come_first() {
        let (_dir, s, ids) = crate::app::tests::session_with_photos(3);
        let mut app = App::new(Some(s));
        let n: Vec<u64> = ids.iter().map(|i| i.get()).collect();
        app.run("photo.tags.add", &json!({ "ids": n, "tags": "zebra" })).unwrap();
        app.run("photo.tags.add", &json!({ "ids": [n[0], n[1]], "tags": "Bus, apple" })).unwrap();
        app.run("photo.tags.add", &json!({ "ids": [n[2]], "tags": "cat" })).unwrap();
        assert_eq!(common_tags(&mut app), strings(&["zebra", "apple", "Bus", "cat"]));
        assert_eq!(common_tags(&mut App::new(None)), Vec::<String>::new());
    }
}
