//! The Folder Manager (`organizing.md` §2; Tools ▸ Folder Manager…, `folders.manager`): which
//! folders Dalica looks for pictures in. Drawn on the dialog host from its draft
//! ([`draft::Draft`], the dialog's fields); nothing reaches the library before OK.
//!
//! - **Left, "Folders":** the tree ([`tree`]). Roots: the standard places that exist (Pictures,
//!   Documents, Desktop, Home) and the volumes, each with its icon; a folder's subfolders are read
//!   when it is expanded, through `folders.browse` on the engine's workers ([`listing`]), a spinner
//!   row standing in while they are. Every row shows its folder's effective state (Scan Always,
//!   Scan Once, Remove from Dalica), drawn at 50 % when inherited from a parent.
//! - **Right:** the three radios for the selected folder (disabled until a row is selected), the
//!   "Find faces in this folder" toggle, and the watched folders (set to Scan Always), by friendly
//!   name or path; a click reveals one in the tree.
//! - **Buttons:** Help (disabled until there is help), Cancel (Esc: changes nothing) and OK (the
//!   default: applies the draft's changes with one `folders.set`, one undo step).
//! - **Watch Drive:** choosing Scan Always for a volume root or the home folder asks first (a
//!   Watch Drive dialog over this one); Watch Drive sets the radio ([`drive_confirmed`]), Cancel
//!   leaves it as it was. OK then sends `"confirm": true`.
//! - The library's selected folder is pre-selected, its parents expanded and the tree scrolled to
//!   it.
//!
//! Widget ids (`control-protocol.md` §7, the dialog row): the rows `choice:dialog.folder/<root>`
//! and `choice:dialog.folder/<root>/<folder>` (`<root>` and `<folder>` are sixteen hex digits of the
//! FNV-1a hash of the path, [`hash`]; a root's row has no second part), their triangles
//! `button:dialog.expand/…` with the same parts, a spinner row `choice:dialog.folder/…/loading`
//! (painted, under the folder being read), the radios `choice:dialog.mode/always`, `/once`,
//! `/remove`, the toggle `check:dialog.faces`, the watched folders `button:dialog.reveal/<folder>`.

pub mod draft;
pub mod listing;
pub mod tree;

use std::path::{Path, PathBuf};

use dalica_engine::CmdError;
use dalica_ui_kit::icons::{FolderIcon, paint_folder};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, ButtonKind, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Color32, Id, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use draft::{Change, Draft, key};
use listing::{Listing, Root};

/// The dialog's width and height (points, `organizing.md` §2: ≈ 640 × 480).
pub const WIDTH: f32 = 640.0;
pub const HEIGHT: f32 = 480.0;
/// The body's height: the dialog's less its title, button row and margins.
const BODY_H: f32 = HEIGHT - 88.0;
/// The room kept round the dialog in a short window.
const WINDOW_MARGIN: f32 = 40.0;
/// The gap between the two halves.
const SPLIT_GAP: f32 = 16.0;
/// The radio rows (points).
const CHOICE_H: f32 = 24.0;

/// The text above the radios.
pub const INTRO: &str =
    "For each folder, choose whether Dalica looks for pictures in it. Watched folders are checked for new pictures automatically.";
/// The faces toggle's label.
pub const FACES_LABEL: &str = "Find faces in this folder";
/// The faces toggle's id.
pub const FACES_ID: &str = "check:dialog.faces";

/// A folder's state: its mode (`organizing.md` §2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Always,
    Once,
    Remove,
}

impl State {
    pub const ALL: [State; 3] = [State::Always, State::Once, State::Remove];

    /// The mode as `folders.set` writes it.
    pub fn id(self) -> &'static str {
        match self {
            State::Always => "always",
            State::Once => "once",
            State::Remove => "remove",
        }
    }

    pub fn from_id(id: &str) -> Option<State> {
        State::ALL.into_iter().find(|s| s.id() == id)
    }

    /// The radio's label.
    pub fn label(self) -> &'static str {
        match self {
            State::Always => "Scan Always",
            State::Once => "Scan Once",
            State::Remove => "Remove from Dalica",
        }
    }

    pub fn icon(self) -> FolderIcon {
        match self {
            State::Always => FolderIcon::ScanAlways,
            State::Once => FolderIcon::ScanOnce,
            State::Remove => FolderIcon::Remove,
        }
    }

    /// The state icon's colour: Scan Always `accent`, Scan Once `text_label`, Remove `danger`.
    pub fn color(self, t: &Tokens) -> Color32 {
        match self {
            State::Always => t.accent,
            State::Once => t.text_label,
            State::Remove => t.danger,
        }
    }
}

/// Sixteen lowercase hex digits of the 64-bit FNV-1a hash of `path`'s text: stable across runs and
/// releases, so ids built from it are too.
pub fn hash(path: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn parts(root: &Path, path: &Path) -> String {
    if root == path { hash(root) } else { format!("{}/{}", hash(root), hash(path)) }
}

/// The id of `path`'s row under `root` (the root's own row when they are the same).
pub fn row_id(root: &Path, path: &Path) -> String {
    format!("choice:dialog.folder/{}", parts(root, path))
}

/// The id of that row's disclosure triangle.
pub fn expand_id(root: &Path, path: &Path) -> String {
    format!("button:dialog.expand/{}", parts(root, path))
}

/// The id of a watched folder's row.
pub fn reveal_id(path: &Path) -> String {
    format!("button:dialog.reveal/{}", hash(path))
}

/// The id of a radio.
pub fn mode_id(state: State) -> String {
    format!("choice:dialog.mode/{}", state.id())
}

pub fn look(_app: &App, _d: &Dialog) -> Look {
    Look {
        title: "Folder Manager".into(),
        width: WIDTH,
        buttons: vec![Button::new("help", "Help", ButtonKind::Other).enabled(false), Button::cancel(), Button::new("ok", "OK", ButtonKind::Default)],
    }
}

/// What the library itself sets for `path` (its own rule), as a change would.
fn own(app: &App, path: &Path) -> Change {
    let rule = app.session().and_then(|s| s.folder_rules().get(path).copied());
    Change { mode: rule.and_then(|r| r.mode).map(|m| m.id().to_string()), faces: rule.and_then(|r| r.faces) }
}

/// `path`'s state as the draft would leave it, and whether it is inherited from a parent.
pub fn effective(app: &App, draft: &Draft, path: &Path) -> (State, bool) {
    let rules = app.session().map(|s| s.folder_rules());
    for a in path.ancestors() {
        let drafted = draft.changes.get(&key(a)).and_then(|c| c.mode.as_deref()).and_then(State::from_id);
        let library = || rules.and_then(|r| r.get(a)).and_then(|r| r.mode).and_then(|m| State::from_id(m.id()));
        if let Some(s) = drafted.or_else(library) {
            return (s, a != path);
        }
    }
    (State::Remove, true)
}

/// Whether faces are found in `path` as the draft would leave it (on when nothing says).
pub fn effective_faces(app: &App, draft: &Draft, path: &Path) -> bool {
    let rules = app.session().map(|s| s.folder_rules());
    for a in path.ancestors() {
        let drafted = draft.changes.get(&key(a)).and_then(|c| c.faces);
        if let Some(f) = drafted.or_else(|| rules.and_then(|r| r.get(a)).and_then(|r| r.faces)) {
            return f;
        }
    }
    true
}

/// The open Folder Manager's draft, if it is open.
fn open_draft(app: &App) -> Option<Draft> {
    app.state().dialogs.iter().find(|d| d.kind == DialogKind::FolderManager).map(|d| Draft::read(&d.fields))
}

/// Whether watching `path` asks first: a volume root (a path with no parent, or a volume the
/// tree's roots listed) or the home folder.
fn is_drive(draft: Option<&Draft>, path: &Path) -> bool {
    path.parent().is_none() || draft.is_some_and(|d| d.drives.contains(&key(path)))
}

/// The Watch Drive row's question (`confirm::ASKS`): `folders.set` asks when it sets a volume root
/// or the home folder that is not watched yet to Scan Always. Which folders those are the UI knows
/// from the Folder Manager's roots (and a path with no parent is always one); the engine asks
/// about the rest itself.
pub fn watches_a_drive(app: &App, params: &Value) -> bool {
    let draft = open_draft(app);
    let Some(changes) = params.get("changes").and_then(Value::as_array) else { return false };
    changes.iter().take(dalica_engine::cmd::folders::MAX_CHANGES).any(|c| {
        let Some(path) = c.get("path").and_then(Value::as_str).map(Path::new) else { return false };
        c.get("mode").and_then(Value::as_str) == Some("always") && is_drive(draft.as_ref(), path) && own(app, path).mode.as_deref() != Some("always")
    })
}

/// Watch Drive, asked by the radio: sets the folder the question was about to Scan Always in the
/// Folder Manager's draft, confirmed.
pub fn drive_confirmed(app: &mut App, d: &Dialog) -> Result<Value, CmdError> {
    let path =
        d.fields.get("path").and_then(Value::as_str).map(PathBuf::from).ok_or_else(|| CmdError::BadParams("Watch Drive has no folder".into()))?;
    let own = own(app, &path);
    let slot = app
        .state_mut()
        .dialogs
        .iter_mut()
        .find(|d| d.kind == DialogKind::FolderManager)
        .ok_or_else(|| CmdError::NotFound("the Folder Manager is not open".into()))?;
    let mut draft = Draft::read(&slot.fields);
    set_mode(&mut draft, &path, State::Always, &own);
    draft.confirm(&path);
    draft.write(&mut slot.fields);
    Ok(json!({ "path": path, "mode": State::Always.id() }))
}

fn set_mode(draft: &mut Draft, path: &Path, state: State, own: &Change) {
    let mut change = draft.changes.get(&key(path)).cloned().unwrap_or_default();
    change.mode = Some(state.id().to_string());
    draft.set(path, change, own);
}

/// OK: applies the draft's changes with one `folders.set` (one undo step). With nothing changed it
/// runs nothing.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = Draft::read(&d.fields);
    let changes: Vec<Value> = draft
        .changes
        .iter()
        .filter(|(_, c)| !c.is_empty())
        .take(dalica_engine::cmd::folders::MAX_CHANGES)
        .map(|(path, c)| {
            let mut v = json!({ "path": path });
            if let Some(o) = v.as_object_mut() {
                if let Some(m) = &c.mode {
                    o.insert("mode".into(), json!(m));
                }
                if let Some(f) = c.faces {
                    o.insert("faces".into(), json!(f));
                }
            }
            v
        })
        .collect();
    if changes.is_empty() {
        return Ok(json!({ "changed": 0 }));
    }
    let confirmed = draft.changes.iter().any(|(p, c)| c.mode.as_deref() == Some("always") && draft.confirmed.contains(p));
    let mut params = json!({ "changes": changes });
    if confirmed && let Some(o) = params.as_object_mut() {
        o.insert("confirm".into(), json!(true));
    }
    app.run("folders.set", &params)
}

/// What the right side and the tree asked for this frame.
#[derive(Default)]
struct Asked {
    select: Option<PathBuf>,
    reveal: Option<PathBuf>,
    toggle: Option<PathBuf>,
    mode: Option<State>,
    faces: Option<bool>,
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let mut draft = Draft::read(&d.fields);
    if !draft.ready {
        listing::clear(&ctx);
        let folder = app.state().folder.and_then(|id| app.session().and_then(|s| s.library().folder(id)).map(|f| f.path.clone()));
        draft.selected = folder.as_deref().map(key);
        draft.reveal = draft.selected.clone();
        draft.ready = true;
    }
    let screen = ctx.content_rect();
    let body_h = BODY_H.min(screen.height() - (HEIGHT - BODY_H) - 2.0 * WINDOW_MARGIN).max(160.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), body_h), Sense::hover());
    if app.session().is_none() {
        ui.painter().text(rect.left_top(), egui::Align2::LEFT_TOP, "The library is still opening.", fonts::regular(t.body), t.text_dim);
        draft.write(&mut d.fields);
        return;
    }
    let roots: Vec<Root> = match listing::get(app, &ctx, None) {
        Listing::Roots(r) => r.to_vec(),
        _ => Vec::new(),
    };
    let drives: Vec<String> = roots.iter().filter(|r| r.drive).map(|r| key(&r.path)).collect();
    if !roots.is_empty() && draft.drives != drives {
        draft.drives = drives;
    }
    // Revealing a folder: expand its parents under the root nearest to it.
    let reveal = draft.reveal.as_deref().map(PathBuf::from);
    let reveal_root = reveal.as_deref().and_then(|p| roots.iter().filter(|r| p.starts_with(&r.path)).max_by_key(|r| r.path.components().count()));
    if let (Some(p), Some(r)) = (reveal.as_deref(), reveal_root) {
        for a in p.ancestors().skip(1).take_while(|a| a.starts_with(&r.path)) {
            if !draft.is_expanded(a) {
                draft.set_expanded(a, true);
            }
        }
    }
    let rows = tree::rows(app, &ctx, &draft, &roots);
    let mut scroll_to = None;
    if let Some(p) = reveal.as_deref() {
        let found = reveal_root
            .and_then(|r| rows.iter().position(|row| *row.root == r.path && row.path == p && matches!(row.kind, tree::Kind::Folder { .. })));
        if found.is_some() || (!roots.is_empty() && !rows.iter().any(|r| r.kind == tree::Kind::Loading)) {
            scroll_to = found;
            draft.reveal = None;
        }
    }

    let half = ((rect.width() - SPLIT_GAP) / 2.0).floor();
    let left = Rect::from_min_size(rect.min, vec2(half, rect.height()));
    let right = Rect::from_min_max(pos2(left.max.x + SPLIT_GAP, rect.min.y), rect.max);
    let mut asked = Asked::default();

    // Left: the tree.
    let label_h = 20.0;
    ui.painter().text(left.left_top(), egui::Align2::LEFT_TOP, "Folders", fonts::semibold(t.body), t.text);
    let well = Rect::from_min_max(pos2(left.min.x, left.min.y + label_h), left.max);
    let out = {
        let app_ref: &App = app;
        let draft_ref = &draft;
        tree::show(ui, &t, well, &rows, draft.selected.as_deref(), scroll_to, |p| effective(app_ref, draft_ref, p))
    };
    if let Some(row) = out.clicked.and_then(|i| rows.get(i)) {
        asked.select = Some(row.path.clone());
    }
    if let Some(row) = out.toggled.and_then(|i| rows.get(i)) {
        asked.toggle = Some(row.path.clone());
    }

    // Right: the radios, the faces toggle and the watched folders.
    let mut side = ui.new_child(UiBuilder::new().max_rect(right).layout(Layout::top_down(Align::Min)));
    side.spacing_mut().item_spacing = vec2(0.0, 0.0);
    side.add(egui::Label::new(RichText::new(INTRO).font(fonts::regular(t.body)).color(t.text_label)).wrap());
    side.add_space(12.0);
    side.label(RichText::new("For the selected folder:").font(fonts::semibold(t.body)).color(t.text));
    side.add_space(4.0);
    let selected = draft.selected.as_deref().map(PathBuf::from);
    let current = selected.as_deref().map(|p| effective(app, &draft, p));
    for s in State::ALL {
        let checked = current.is_some_and(|(c, _)| c == s);
        // A click sets the folder's own mode, unless it sets this one already.
        let own_already = current == Some((s, false));
        if choice(&mut side, &t, &mode_id(s), s, checked, selected.is_some()).clicked() && !own_already {
            asked.mode = Some(s);
        }
    }
    side.add_space(6.0);
    let (line, _) = side.allocate_exact_size(vec2(side.available_width(), 1.0), Sense::hover());
    side.painter().rect_filled(line, 0, t.divider);
    side.add_space(6.0);
    let mut faces = selected.as_deref().is_some_and(|p| effective_faces(app, &draft, p));
    let r = side.add_enabled(selected.is_some(), egui::Checkbox::new(&mut faces, RichText::new(FACES_LABEL).font(fonts::regular(t.body))));
    widgets::register(&ctx, FACES_ID, Role::Toggle, FACES_LABEL, Target::Widget(&r), faces);
    if r.clicked() && selected.is_some() {
        asked.faces = Some(faces);
    }
    side.add_space(12.0);
    side.label(RichText::new("Watched folders").font(fonts::semibold(t.body)).color(t.text));
    side.add_space(4.0);
    let rest = Rect::from_min_max(side.cursor().min, right.max);
    let watched = watched(app, &draft, &roots);
    if let Some(p) = watched_list(&mut side, &t, rest, &watched) {
        asked.select = Some(p.clone());
        asked.reveal = Some(p);
    }

    // Apply what was asked to the draft.
    if let Some(p) = asked.toggle {
        let open = !draft.is_expanded(&p);
        draft.set_expanded(&p, open);
    }
    if let Some(p) = asked.select {
        draft.selected = Some(key(&p));
    }
    if let Some(p) = asked.reveal {
        draft.reveal = Some(key(&p));
    }
    let mut ask_drive = None;
    if let (Some(p), Some(s)) = (selected.as_deref(), asked.mode) {
        let own = own(app, p);
        if s == State::Always && is_drive(Some(&draft), p) && own.mode.as_deref() != Some("always") && !draft.is_confirmed(p) {
            ask_drive = Some(p.to_path_buf());
        } else {
            set_mode(&mut draft, p, s, &own);
        }
    }
    if let (Some(p), Some(f)) = (selected.as_deref(), asked.faces) {
        let own = own(app, p);
        let mut change = draft.changes.get(&key(p)).cloned().unwrap_or_default();
        change.faces = Some(f);
        draft.set(p, change, &own);
    }
    draft.write(&mut d.fields);
    if let Some(p) = ask_drive {
        let mut ask = Dialog::new(DialogKind::WatchDrive);
        ask.fields.insert("path".into(), json!(key(&p)));
        if let Err(e) = super::open(app, ask) {
            app.state_mut().status = e.to_string();
        }
    }
}

/// A radio for state `s`: the circle, the state icon and the label, the whole row clickable.
fn choice(ui: &mut Ui, t: &Tokens, id: &str, s: State, checked: bool, enabled: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, CHOICE_H), Sense::hover());
    let r = ui.interact(rect, Id::new(id), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let c = pos2(rect.min.x + 8.0, rect.center().y);
    let ring = if !enabled {
        t.text_disabled
    } else if r.hovered() {
        t.text_label
    } else {
        t.button_border
    };
    p.circle(c, 7.0, t.field, egui::Stroke::new(1.0, ring));
    if checked {
        p.circle_filled(c, 3.5, if enabled { t.accent } else { t.text_disabled });
    }
    let side = t.metrics.icon_menu;
    let icon = Rect::from_min_size(pos2(rect.min.x + 22.0, rect.center().y - side / 2.0), vec2(side, side));
    let color = if enabled { s.color(t) } else { t.text_disabled };
    paint_folder(p, icon, s.icon(), color);
    let text = if enabled { t.text_label } else { t.text_disabled };
    p.text(pos2(icon.max.x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, s.label(), fonts::regular(t.body), text);
    widgets::register(ui.ctx(), id, Role::Toggle, s.label(), Target::Widget(&r), checked);
    r
}

/// The watched folders as the draft would leave them (set to Scan Always), with the name shown:
/// a standard place's name, else the path.
fn watched(app: &App, draft: &Draft, roots: &[Root]) -> Vec<(PathBuf, String)> {
    let mut paths: Vec<PathBuf> = app
        .session()
        .map(|s| s.folder_rules().iter().filter(|(_, r)| r.mode.map(|m| m.id()) == Some("always")).map(|(p, _)| p.to_path_buf()).collect())
        .unwrap_or_default();
    paths.retain(|p| draft.changes.get(&key(p)).and_then(|c| c.mode.as_deref()).is_none_or(|m| m == "always"));
    for (p, c) in &draft.changes {
        let p = PathBuf::from(p);
        if c.mode.as_deref() == Some("always") && !paths.contains(&p) {
            paths.push(p);
        }
    }
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let name = roots.iter().find(|r| r.place && r.path == p).map_or_else(|| p.display().to_string(), |r| r.name.clone());
            (p, name)
        })
        .collect()
}

/// The watched list in `rect`; the folder clicked, if any.
fn watched_list(ui: &mut Ui, t: &Tokens, rect: Rect, watched: &[(PathBuf, String)]) -> Option<PathBuf> {
    tree::well(ui, t, rect);
    let inner = rect.shrink(1.0);
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    if watched.is_empty() {
        let at = pos2(inner.min.x + 8.0, inner.min.y + t.metrics.tree_row_h / 2.0);
        child.painter().text(at, egui::Align2::LEFT_CENTER, "No folders are watched.", fonts::regular(t.body), t.text_dim);
        return None;
    }
    let mut clicked = None;
    let row_h = t.metrics.tree_row_h;
    egui::ScrollArea::vertical()
        .id_salt("dialog:folderManager/watched")
        .max_height(inner.height())
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show_rows(&mut child, row_h, watched.len(), |ui, range| {
            let w = ui.available_width();
            for (path, name) in watched.get(range).unwrap_or_default() {
                let (rect, _) = ui.allocate_exact_size(vec2(w, row_h), Sense::hover());
                let id = reveal_id(path);
                let r = ui.interact(rect, Id::new(&id), Sense::click());
                let p = ui.painter();
                if r.hovered() {
                    p.rect_filled(rect, 0, t.hover.gamma_multiply(0.6));
                }
                let side = t.metrics.icon_menu;
                let icon = Rect::from_min_size(pos2(rect.min.x + 6.0, rect.center().y - side / 2.0), vec2(side, side));
                paint_folder(p, icon, FolderIcon::ScanAlways, State::Always.color(t));
                let tx = icon.max.x + 6.0;
                let g = tree::one_line(ui, name, fonts::regular(t.body), t.text_label, rect.max.x - tx - 4.0);
                p.galley(pos2(tx, rect.center().y - g.size().y / 2.0).round(), g, t.text_label);
                widgets::register(ui.ctx(), &id, Role::Button, name, Target::Widget(&r), false);
                if r.clicked() {
                    clicked = Some(path.clone());
                }
            }
        });
    clicked
}
