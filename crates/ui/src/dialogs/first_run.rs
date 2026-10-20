//! The first-run panel (`organizing.md` §1): offered once when the window opens a new library, one
//! that has never had a folder (`Session::is_new_library`) — so never for the demo library, which
//! comes with its folders, and never in headless or snapshot sessions, which do not call
//! [`offer`].
//!
//! Two choices and Continue: **"Only look in my Pictures, Documents and Desktop folders"** (the
//! default) adds those that exist as Scan Always with one `folders.set` (one undo step), and the
//! window fills in as the scan finds folders; **"Choose folders…"** opens the Folder Manager
//! instead. Where the standard places are is read on the engine's workers (`folders.browse`), asked
//! for each frame while the panel is open so Continue never waits.
//!
//! Continue then asks, once, whether Dalica should be the default viewer
//! ([`super::default_viewer::offer`], M15.24; under the Folder Manager when that opens), when the
//! system lets it be.
//!
//! Widget ids: `dialog:firstRun`, the radios `choice:dialog.start/places` and
//! `choice:dialog.start/choose`, `button:dialog.continue`. The draft is `{choice: "places" |
//! "choose"}`.

use std::path::{Path, PathBuf};

use dalica_engine::CmdError;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target};
use dalica_ui_kit::{Tokens, fonts, icons};
use egui::{Align, Id, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::folder_manager::listing::{self, Listing, Root};
use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::tl;

/// The panel's width and height (points, `organizing.md` §1: ≈ 640 × 460).
pub const WIDTH: f32 = 640.0;
pub const HEIGHT: f32 = 460.0;
/// The body's height: the panel's less its title, button row and margins.
const BODY_H: f32 = HEIGHT - 88.0;

/// The panel's heading (its title).
pub const HEADING: &str = "Dalica is ready to look for pictures on your computer";
/// The two choices: their draft value, id, label and hint.
pub const CHOICES: [(&str, &str, &str, &str); 2] = [
    (
        "places",
        "choice:dialog.start/places",
        "Only look in my Pictures, Documents and Desktop folders",
        "Choose this if you keep your photos in these folders.",
    ),
    (
        "choose",
        "choice:dialog.start/choose",
        "Choose folders…",
        "Pick the folders you want Dalica to watch. You can change this at any time in the Folder Manager.",
    ),
];
/// The line at the foot of the panel.
pub const FOOTER: &str =
    "Looking for pictures never moves or copies files. You can change which folders are shown at any time with Tools → Folder Manager.";

/// Opens the panel if the library is new and it is not open yet; whether it opened. The window
/// calls it once when it has opened a library.
pub fn offer(app: &mut App) -> bool {
    let new = app.session().is_some_and(dalica_engine::Session::is_new_library);
    new && super::open(app, Dialog::new(DialogKind::FirstRun)).is_ok()
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: tl!("Dalica is ready to look for pictures on your computer"),
        width: WIDTH,
        buttons: vec![Button::new("continue", tl!("Continue"), ButtonKind::Default)],
    }
}

/// A label or hint of [`CHOICES`] in the language in effect.
fn shown(english: &str) -> String {
    match english {
        "Only look in my Pictures, Documents and Desktop folders" => tl!("Only look in my Pictures, Documents and Desktop folders"),
        "Choose this if you keep your photos in these folders." => tl!("Choose this if you keep your photos in these folders."),
        "Choose folders…" => tl!("Choose folders…"),
        "Pick the folders you want Dalica to watch. You can change this at any time in the Folder Manager." => {
            tl!("Pick the folders you want Dalica to watch. You can change this at any time in the Folder Manager.")
        }
        other => crate::i18n::tr(other),
    }
}

/// The choice in the draft: `"places"` unless it says `"choose"`.
fn choice_of(d: &Dialog) -> &'static str {
    if d.fields.get("choice").and_then(Value::as_str) == Some("choose") { "choose" } else { "places" }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    // Where the standard places are, read ahead so Continue never waits.
    let listing = if app.session().is_some() { listing::ask(app, None, false) } else { Listing::Pending };
    if listing == Listing::Pending {
        ui.ctx().request_repaint();
    }
    let chosen = choice_of(d);
    let screen = ui.ctx().content_rect();
    let body_h = BODY_H.min(screen.height() - (HEIGHT - BODY_H) - 80.0).max(200.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), body_h), Sense::hover());
    let mut col = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    col.spacing_mut().item_spacing = vec2(0.0, 0.0);
    col.add_space(8.0);
    for (value, id, label, hint) in CHOICES {
        if radio(&mut col, &t, id, &shown(label), &shown(hint), chosen == value).clicked() {
            d.fields.insert("choice".into(), json!(value));
        }
        if value == "places" {
            places(&mut col, &t, &listing, chosen == value);
        }
        col.add_space(12.0);
    }
    let footer = Rect::from_min_max(pos2(rect.min.x, rect.max.y - 40.0), rect.max);
    let mut foot = ui.new_child(UiBuilder::new().max_rect(footer).layout(Layout::top_down(Align::Min)));
    let footer =
        tl!("Looking for pictures never moves or copies files. You can change which folders are shown at any time with Tools → Folder Manager.");
    foot.add(egui::Label::new(RichText::new(footer).font(fonts::regular(t.small)).color(t.text_dim)).wrap());
}

/// Under the first choice, the folders it watches: each standard place found, with its icon and
/// name (dimmed while the other choice is chosen); a spinner while they are looked for.
fn places(ui: &mut Ui, t: &Tokens, listing: &Listing, chosen: bool) {
    let row_h = t.metrics.tree_row_h;
    let side = t.metrics.icon_menu;
    let x = ui.cursor().min.x + TEXT_X;
    ui.add_space(4.0);
    let (color, text) = if chosen { (t.icon, t.text_label) } else { (t.text_disabled, t.text_disabled) };
    let found: Vec<Root> = match listing {
        Listing::Roots(roots) => {
            let wanted = standard_places(roots);
            roots.iter().filter(|r| wanted.contains(&r.path)).cloned().collect()
        }
        _ => Vec::new(),
    };
    if *listing == Listing::Pending {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
        let spin = Rect::from_min_size(pos2(x, rect.center().y - side / 2.0), vec2(side, side));
        icons::paint_activity(ui.painter(), spin, t.icon, ui.input(|i| i.time) as f32);
        ui.painter().text(
            pos2(spin.max.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            tl!("Looking for your folders…"),
            fonts::regular(t.body),
            t.text_dim,
        );
        return;
    }
    if found.is_empty() {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
        let none = tl!("None of these folders exists here: Continue opens the Folder Manager.");
        ui.painter().text(pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, none, fonts::regular(t.body), t.text_dim);
        return;
    }
    for place in &found {
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
        let icon = Rect::from_min_size(pos2(x, rect.center().y - side / 2.0), vec2(side, side));
        icons::paint_folder(ui.painter(), icon, place.icon, color);
        ui.painter().text(pos2(icon.max.x + 8.0, rect.center().y), egui::Align2::LEFT_CENTER, &place.name, fonts::regular(t.body), text);
    }
}

/// Where a choice's label, hint and places start (points from its left edge).
const TEXT_X: f32 = 26.0;

/// A choice: a radio with its label, and its hint under the label; the whole block clickable.
fn radio(ui: &mut Ui, t: &Tokens, id: &str, label: &str, hint: &str, checked: bool) -> egui::Response {
    let w = ui.available_width();
    let text_x = TEXT_X;
    let hint_g = ui.painter().layout(hint.to_string(), fonts::regular(t.body), t.text_dim, w - text_x);
    let h = 22.0 + hint_g.size().y + 2.0;
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let r = ui.interact(rect, Id::new(id), Sense::click());
    let p = ui.painter();
    let c = pos2(rect.min.x + 8.0, rect.min.y + 11.0);
    p.circle(c, 7.0, t.field, egui::Stroke::new(1.0, if r.hovered() { t.text_label } else { t.button_border }));
    if checked {
        p.circle_filled(c, 3.5, t.accent);
    }
    let color = if r.hovered() || checked { t.text } else { t.text_label };
    p.text(pos2(rect.min.x + text_x, c.y), egui::Align2::LEFT_CENTER, label, fonts::semibold(t.body), color);
    p.galley(pos2(rect.min.x + text_x, rect.min.y + 22.0), hint_g, t.text_dim);
    widgets::register(ui.ctx(), id, Role::Toggle, label, Target::Widget(&r), checked);
    r
}

/// The folders "Only look in my Pictures, Documents and Desktop folders" adds: those places that
/// exist, never the home folder itself, and never a folder inside another one listed.
pub fn standard_places(roots: &[Root]) -> Vec<PathBuf> {
    let candidates: Vec<&Path> = roots.iter().filter(|r| r.place && !r.drive).map(|r| r.path.as_path()).collect();
    let mut out: Vec<PathBuf> = Vec::new();
    for c in &candidates {
        if !candidates.iter().any(|o| o != c && c.starts_with(o)) && !out.iter().any(|p| p == c) {
            out.push(c.to_path_buf());
        }
    }
    out
}

/// Continue: adds the standard places as Scan Always (one `folders.set`), or opens the Folder
/// Manager for "Choose folders…" (and when none of the places exists).
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    if app.session().is_none() {
        return Err(CmdError::Disabled("The library is still opening.".into()));
    }
    let places = if choice_of(d) == "choose" {
        Vec::new()
    } else {
        match listing::ask(app, None, false) {
            Listing::Roots(r) => standard_places(&r),
            Listing::Failed(_) => Vec::new(),
            Listing::Pending | Listing::Children(_) => {
                return Err(CmdError::Disabled("Dalica is still looking for your folders; try again in a moment.".into()));
            }
        }
    };
    if places.is_empty() {
        if app.state().dialogs.iter().any(|d| d.kind == DialogKind::FolderManager) {
            return Ok(json!({ "dialog": DialogKind::FolderManager.name() }));
        }
        let asked = super::default_viewer::offer(app);
        let mut r = super::open(app, Dialog::new(DialogKind::FolderManager))?;
        if let Some(o) = r.as_object_mut() {
            o.insert("defaultViewer".into(), json!(asked));
        }
        return Ok(r);
    }
    let changes: Vec<Value> = places.iter().map(|p| json!({ "path": p, "mode": "always" })).collect();
    let mut r = app.run("folders.set", &json!({ "changes": changes }))?;
    let asked = super::default_viewer::offer(app);
    if let Some(o) = r.as_object_mut() {
        o.insert("added".into(), json!(places));
        o.insert("defaultViewer".into(), json!(asked));
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_ui_kit::icons::FolderIcon;

    fn root(path: &str, place: bool, drive: bool) -> Root {
        Root { path: PathBuf::from(path), name: path.into(), icon: FolderIcon::Pictures, place, drive }
    }

    #[test]
    fn the_standard_places_are_the_existing_ones_never_home_nor_nested() {
        let roots = [
            root("/h/Pictures", true, false),
            root("/h/Pictures/Docs", true, false),
            root("/h/Desktop", true, false),
            root("/h", true, true),
            root("/mnt/card", false, true),
        ];
        assert_eq!(standard_places(&roots), [PathBuf::from("/h/Pictures"), PathBuf::from("/h/Desktop")]);
        assert!(standard_places(&[]).is_empty());
    }
}
