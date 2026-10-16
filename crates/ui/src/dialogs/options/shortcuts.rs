//! Options ▸ Keyboard Shortcuts: the shortcut editor (`menus-and-shortcuts.md` §2.0; also Help ▸
//! Keyboard Shortcuts…, Ctrl+/, `app.shortcuts`):
//!
//! ```text
//! [🔍 Search commands and keys      ]  Context: [All ▾]   [ ] Show commands without a shortcut
//!    Command                        Context     Shortcuts
//!  ● Picture › Star / Unstar        Library     [Ctrl+8      ] [F4          ]  ↺
//!    View › Small Thumbnails        Library     [Ctrl+1      ] [—           ]
//!    …
//! [Reset All]  Click a shortcut to change it: Esc cancels, Backspace removes it.
//!  or, while a key is taken:  Ctrl+1 is already Small Thumbnails in Library.  [Replace] [Cancel]
//!  or, after Reset All:       Restore all default shortcuts?  [Reset All] [Cancel]
//! ```
//!
//! Its state is the draft's `shortcuts` ([`ShortcutEditor`]): the search text, the Context filter,
//! "Show commands without a shortcut", the key cell recording, a conflict waiting for an answer,
//! and the changed bindings and Reset All, which only the dialog's Apply or OK apply ([`apply`]:
//! `app.resetShortcuts`, then one `app.setShortcut` per change); Cancel drops them. The rules of
//! the draft (recording, conflicts, Replace, ↺, Reset All) are [`draft`]'s.
//!
//! - The rows come from `keymap.list {context?, search?, withoutKeys?}` (the engine's search: by
//!   label, menu path and key in any spelling, so "ctrl+1" finds both Ctrl+1 bindings), one per
//!   command and context, with the keys the draft leads to and a mark and ↺ where they differ from
//!   the defaults. UI commands are labelled from `UI_COMMANDS` (the engine knows only their ids),
//!   and the search matches their labels and menu paths here.
//!   The search matches the keys in effect, not the draft's. The list is built again only when the
//!   search, the filter, the keymap or the draft changes, and only the rows in sight are drawn.
//! - A click on a key cell records it ("Press the new shortcut…"; a second click stops): every
//!   key press goes to the editor (`crate::keys::capture_for_editor` hands them over in
//!   `captured`), never to the keymap or the dialog's Esc and Enter. A command without keys
//!   records into the Context filter's context, or Global under All.
//! - A key already taken outlines the cell in red and asks on the bottom line, with [Replace] and
//!   [Cancel] (Esc cancels too: the cell still records). Reset All asks there too.
//!
//! Automation ids (`control-protocol.md` §7): the search field `field:shortcuts.search` and its
//! clear button `button:shortcuts.search/clear`, the Context filter `choice:shortcuts.context`
//! with its items `choice:shortcuts.context/<all|global|library|editView|…>`, the check box
//! `check:shortcuts.showUnbound`; a row `shortcut:<command>/<context>`, its key cells
//! `shortcut:<command>/<context>/0` and `/1` and its ↺ `shortcut:<command>/<context>/restore`; the
//! conflict line `text:shortcuts.conflict` with `button:shortcuts.replace` and
//! `button:shortcuts.cancel`; `button:shortcuts.resetAll`, its question `text:shortcuts.resetAll`
//! with `button:shortcuts.resetAll/confirm` and `button:shortcuts.resetAll/cancel`.
//!
//! The editor follows LightCraft `crates/ui-egui/src/panels/keymap.rs` (`body`, `row`, `capture`)
//! at b3d392b4 and PhotoCraft `crates/ui-egui/src/prefs_ui.rs` (`shortcuts_body`,
//! `shortcut_text`) at 583425b4 (both MIT OR Apache-2.0), rewritten over Dalica's keymap commands,
//! draft and widgets.

pub mod draft;

use std::sync::Arc;

use dalica_engine::keymap::{CONTEXTS, Platform, context_label};
use dalica_engine::{CmdError, Context, Key};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{
    self, KeyCell, ListCell, Target, checkbox, dropdown, dropdown_item, key_cell, key_cell_height, list_columns, list_row, list_row_height,
    list_text, search_field, text_button,
};
use dalica_ui_kit::{Tokens, fonts, icons};
use egui::{Id, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};

use crate::app::{App, UI_COMMANDS};
use crate::state::{KeySlot, MAX_DRAFT, SHORTCUT_SLOTS, ShortcutChange, ShortcutEditor};

pub const SEARCH_ID: &str = "field:shortcuts.search";
pub const SEARCH_CLEAR_ID: &str = "button:shortcuts.search/clear";
pub const CONTEXT_ID: &str = "choice:shortcuts.context";
pub const UNBOUND_ID: &str = "check:shortcuts.showUnbound";
pub const CONFLICT_ID: &str = "text:shortcuts.conflict";
pub const REPLACE_ID: &str = "button:shortcuts.replace";
pub const CONFLICT_CANCEL_ID: &str = "button:shortcuts.cancel";
pub const RESET_ID: &str = "button:shortcuts.resetAll";
pub const RESET_QUESTION_ID: &str = "text:shortcuts.resetAll";
pub const RESET_CONFIRM_ID: &str = "button:shortcuts.resetAll/confirm";
pub const RESET_CANCEL_ID: &str = "button:shortcuts.resetAll/cancel";
/// The line shown instead of the list while no library is open.
pub const WAITING_ID: &str = "text:shortcuts.waiting";

pub const SEARCH_HINT: &str = "Search commands and keys";
pub const UNBOUND_LABEL: &str = "Show commands without a shortcut";
pub const RESET_LABEL: &str = "Reset All";
pub const RESET_QUESTION: &str = "Restore all default shortcuts?";
pub const REPLACE_LABEL: &str = "Replace";
pub const CANCEL_LABEL: &str = "Cancel";
pub const HINT: &str = "Click a shortcut to change it: Esc cancels, Backspace removes it.";
pub const ALL_LABEL: &str = "All";
pub const NO_ROWS: &str = "No shortcuts match.";
/// Why the editor shows nothing and Apply is refused.
pub const WAITING: &str = "The library is still opening.";
/// The column titles.
pub const HEADERS: [&str; 3] = ["Command", "Context", "Shortcuts"];

/// The Context column's width, the ↺ column's and the key cells' (points).
const CONTEXT_W: f32 = 92.0;
const RESTORE_W: f32 = 28.0;
const CELL_W: f32 = 150.0;
/// The search field's width (points).
const SEARCH_W: f32 = 240.0;
/// The longest search sent to `keymap.list` (its limit), in bytes.
const SEARCH_BYTES: usize = 256;

/// One row of the editor: a command in a context (`None`: a command without keys, listed with
/// "Show commands without a shortcut"), its menu path and label as shown, and the keys the draft
/// leads to and the defaults, as the keymap writes them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub command: String,
    pub context: Option<Context>,
    pub name: String,
    pub keys: Vec<String>,
    pub defaults: Vec<String>,
    pub editable: bool,
}

impl Row {
    /// Whether the keys differ from the defaults (the mark and ↺).
    pub fn modified(&self) -> bool {
        self.keys != self.defaults
    }

    /// The context its key cells record into: its own, or for a command without keys the
    /// Context filter's (Global under All).
    pub fn cell_context(&self, filter: Option<Context>) -> Context {
        self.context.or(filter).unwrap_or(Context::Global)
    }
}

/// A context as `keymap.list` and the ids name it (`editView`).
pub fn context_name(c: Context) -> String {
    serde_json::to_value(c).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// The automation id of a row.
pub fn row_id(command: &str, context: Context) -> String {
    format!("shortcut:{command}/{}", context_name(context))
}

/// The automation id of a row's key cell in `slot` (0 or 1).
pub fn cell_id(command: &str, context: Context, slot: u8) -> String {
    format!("{}/{slot}", row_id(command, context))
}

/// The automation id of a row's ↺.
pub fn restore_id(command: &str, context: Context) -> String {
    format!("{}/restore", row_id(command, context))
}

/// What a command is called on screen: a UI command's label from `UI_COMMANDS`, else the engine's,
/// else its id.
pub fn label(app: &App, command: &str) -> String {
    UI_COMMANDS
        .iter()
        .find(|c| c.id == command)
        .map(|c| c.label.to_string())
        .or_else(|| app.session().and_then(|s| s.spec(command)).map(|s| s.label.to_string()))
        .unwrap_or_else(|| command.to_string())
}

/// The menu path and label as a row shows them: `Picture/Star / Unstar` with the label `Star /
/// Unstar` is "Picture › Star / Unstar"; no menu path, the label alone.
pub fn display_name(menu: &str, label: &str) -> String {
    let path = match menu.strip_suffix(label) {
        Some(path) => path,
        None if menu.is_empty() => "",
        None => return menu.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" › "),
    };
    let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    parts.push(label);
    parts.join(" › ")
}

/// Whether keys show as macOS writes them.
fn mac() -> bool {
    Platform::current() == Platform::Mac
}

/// `key` (as the keymap writes it) as the screen shows it.
fn shown_key(key: &str) -> String {
    widgets::key_text(key, mac())
}

/// What the list is built from; it is built again when this changes.
#[derive(Debug, Clone, PartialEq)]
struct Source {
    search: String,
    context: Option<Context>,
    show_unbound: bool,
    commands: usize,
    overrides: Vec<(String, Context, Vec<Key>)>,
    changes: Vec<ShortcutChange>,
    reset_all: bool,
}

/// The rows of the editor for `e`, sorted by name then context: `keymap.list`'s, with the keys the
/// draft leads to. Empty while no library is open.
pub fn rows(app: &mut App, e: &ShortcutEditor) -> Vec<Row> {
    let mut search = e.search.trim().to_string();
    while search.len() > SEARCH_BYTES {
        search.pop();
    }
    let mut params = json!({ "search": search, "withoutKeys": e.show_unbound });
    if let Some(c) = e.context {
        params["context"] = json!(context_name(c));
    }
    let Some(listed) = app.session_mut().and_then(|s| s.execute("keymap.list", &params).ok()) else { return Vec::new() };
    let mut listed: Vec<Value> = listed["rows"].as_array().cloned().unwrap_or_default();
    // The engine knows a UI command by its id only: its label and menu path are matched here.
    let needle = search.to_lowercase();
    if !needle.is_empty() {
        params["search"] = json!("");
        let all = app.session_mut().and_then(|s| s.execute("keymap.list", &params).ok()).unwrap_or_default();
        let seen = |r: &Value, listed: &[Value]| listed.iter().any(|l| l["command"] == r["command"] && l["context"] == r["context"]);
        for r in all["rows"].as_array().into_iter().flatten() {
            let named = UI_COMMANDS
                .iter()
                .find(|c| r["command"].as_str() == Some(c.id))
                .is_some_and(|c| c.label.to_lowercase().contains(&needle) || c.menu.to_lowercase().contains(&needle));
            if named && !seen(r, &listed) {
                listed.push(r.clone());
            }
        }
    }
    let Some(base) = app.session().map(|s| s.keymap()) else { return Vec::new() };
    let draft = draft::keymap(base, e);
    let strings = |ks: Vec<Key>| ks.iter().map(Key::to_string).collect::<Vec<_>>();
    let draft_rows = draft.rows(true);
    let mut out = Vec::new();
    for r in &listed {
        let Some(command) = r["command"].as_str() else { continue };
        let (label, menu) = match UI_COMMANDS.iter().find(|c| c.id == command) {
            Some(c) => (c.label, c.menu),
            None => (r["label"].as_str().unwrap_or(command), r["menu"].as_str().unwrap_or("")),
        };
        let name = display_name(menu, label);
        let editable = r["editable"].as_bool().unwrap_or(false);
        let row = |context: Option<Context>, keys: Vec<String>, defaults: Vec<String>| Row {
            command: command.to_string(),
            context,
            name: name.clone(),
            keys,
            defaults,
            editable,
        };
        match serde_json::from_value::<Option<Context>>(r["context"].clone()).ok().flatten() {
            Some(c) => out.push(row(Some(c), strings(draft.keys_for(command, c)), strings(draft.default_keys_for(command, c)))),
            // A command without keys: the contexts the draft gives it keys in, if any.
            None => {
                let given: Vec<Context> = draft_rows
                    .iter()
                    .filter(|d| d.command == command)
                    .filter_map(|d| d.context)
                    .filter(|c| e.context.is_none_or(|f| f == *c))
                    .collect();
                if given.is_empty() {
                    out.push(row(None, Vec::new(), Vec::new()));
                }
                for c in given {
                    out.push(row(Some(c), strings(draft.keys_for(command, c)), strings(draft.default_keys_for(command, c))));
                }
            }
        }
    }
    out.sort_by_cached_key(|r| (r.name.to_lowercase(), r.context));
    out
}

/// [`rows`], kept between frames while what they are built from stays the same.
fn cached_rows(app: &mut App, ctx: &egui::Context, e: &ShortcutEditor) -> Arc<Vec<Row>> {
    let Some(s) = app.session() else { return Arc::new(Vec::new()) };
    let overrides = s.keymap().overrides().iter().map(|o| (o.command.clone(), o.context, o.keys.clone())).collect();
    let source = Source {
        search: e.search.clone(),
        context: e.context,
        show_unbound: e.show_unbound,
        commands: s.keymap().commands().len(),
        overrides,
        changes: e.changes.clone(),
        reset_all: e.reset_all,
    };
    let memory = Id::new("dialog:options/shortcuts/rows");
    let kept: Option<(Arc<Source>, Arc<Vec<Row>>)> = ctx.data(|d| d.get_temp(memory));
    if let Some((was, rows)) = kept
        && *was == source
    {
        return rows;
    }
    let rows = Arc::new(rows(app, e));
    ctx.data_mut(|d| d.insert_temp(memory, (Arc::new(source), rows.clone())));
    rows
}

/// What a frame of the editor asked for, done after it is drawn.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    Search(String),
    Filter(Option<Context>),
    Unbound(bool),
    Record(KeySlot),
    Replace,
    CancelConflict,
    Restore(String, Context),
    AskReset(bool),
    ConfirmReset,
}

/// Whether the Reset All question is up: kept while the tab is drawn frame after frame, so it is
/// gone when the dialog opens again.
#[derive(Debug, Clone, Copy, Default)]
struct Asking {
    on: bool,
    pass: u64,
}

fn asking_id() -> Id {
    Id::new("dialog:options/shortcuts/asking")
}

fn asking(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let a: Asking = ctx.data(|d| d.get_temp(asking_id())).unwrap_or_default();
    a.on && a.pass.saturating_add(1) >= pass
}

fn set_asking(ctx: &egui::Context, on: bool) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(asking_id(), Asking { on, pass }));
}

pub fn body(app: &mut App, ui: &mut Ui) {
    let Some(mut e) = app.state().options.as_ref().map(|o| o.shortcuts.clone()) else { return };
    let t = *app.tokens();
    let captured = std::mem::take(&mut e.captured);
    let mut full = false;
    if let Some(s) = app.session() {
        for key in &captured {
            full |= draft::press(s.keymap(), &mut e, key) == draft::Pressed::Full;
        }
    }
    let ctx = ui.ctx().clone();
    let asked = asking(&ctx);
    set_asking(&ctx, asked);
    let mut actions = Vec::new();
    if app.session().is_none() {
        line(ui, &t, WAITING_ID, WAITING, t.text_label);
    } else {
        let rows = cached_rows(app, &ctx, &e);
        let conflict_label = e.conflict.as_ref().map(|c| label(app, &c.command)).unwrap_or_default();
        toolbar(ui, &t, &e, &mut actions);
        ui.add_space(8.0);
        let bottom_h = t.metrics.button_h.max(key_cell_height(&t)) + 8.0;
        let list_h = (ui.available_height() - bottom_h).max(list_row_height(&t));
        list(ui, &t, &e, &rows, list_h, &mut actions);
        ui.add_space(8.0);
        bottom(ui, &t, &e, asked, &conflict_label, &mut actions);
    }
    let base = if actions.is_empty() { None } else { app.session().map(|s| s.keymap().clone()) };
    for action in actions {
        match action {
            Action::Search(s) => e.search = s.chars().take(MAX_DRAFT).collect(),
            Action::Filter(c) => e.context = c,
            Action::Unbound(on) => e.show_unbound = on,
            Action::Record(cell) => {
                let again = e.recording.as_ref() == Some(&cell);
                draft::cancel(&mut e);
                if !again {
                    e.recording = Some(cell);
                }
            }
            Action::Replace => {
                if let Some(b) = &base {
                    full |= !draft::replace(b, &mut e);
                }
            }
            Action::CancelConflict => draft::cancel(&mut e),
            Action::Restore(command, context) => {
                if let Some(b) = &base {
                    full |= draft::restore(b, &mut e, &command, context) == draft::Pressed::Full;
                }
            }
            Action::AskReset(on) => set_asking(&ctx, on),
            Action::ConfirmReset => {
                set_asking(&ctx, false);
                if let Some(b) = &base {
                    draft::reset_all(b, &mut e);
                }
            }
        }
    }
    if let Some(o) = app.state_mut().options.as_mut() {
        if full {
            o.error = Some(draft::FULL.into());
        }
        if o.shortcuts != e || !captured.is_empty() {
            o.shortcuts = e;
        }
    }
}

/// The search field, the Context filter and "Show commands without a shortcut".
fn toolbar(ui: &mut Ui, t: &Tokens, e: &ShortcutEditor, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut text = e.search.clone();
        if search_field(ui, t, SEARCH_ID, SEARCH_CLEAR_ID, &mut text, SEARCH_HINT, SEARCH_W).changed {
            actions.push(Action::Search(text));
        }
        ui.add_space(10.0);
        ui.add(egui::Label::new(RichText::new("Context:").font(fonts::regular(t.body)).color(t.text_label)));
        let shown = e.context.map_or(ALL_LABEL, context_label);
        dropdown(ui, t, CONTEXT_ID, shown, |ui| {
            if dropdown_item(ui, t, &format!("{CONTEXT_ID}/all"), ALL_LABEL, "", e.context.is_none()).clicked() {
                actions.push(Action::Filter(None));
            }
            for c in CONTEXTS {
                if dropdown_item(ui, t, &format!("{CONTEXT_ID}/{}", context_name(c)), context_label(c), "", e.context == Some(c)).clicked() {
                    actions.push(Action::Filter(Some(c)));
                }
            }
        });
        ui.add_space(10.0);
        let mut on = e.show_unbound;
        if checkbox(ui, t, UNBOUND_ID, UNBOUND_LABEL, &mut on).changed() {
            actions.push(Action::Unbound(on));
        }
    });
}

/// The column widths of a row `width` wide: the name takes what the others leave.
fn widths(width: f32) -> [f32; 5] {
    let rest = width - widgets::LIST_ROW_GUTTER - CONTEXT_W - 2.0 * CELL_W - RESTORE_W;
    [rest.max(80.0), CONTEXT_W, CELL_W, CELL_W, RESTORE_W]
}

/// The column titles, then the rows (only those in sight are drawn).
fn list(ui: &mut Ui, t: &Tokens, e: &ShortcutEditor, rows: &[Row], height: f32, actions: &mut Vec<Action>) {
    let w = widths(ui.available_width());
    let header_h = (t.small * 1.25 + 4.0).ceil();
    let (_, header) = ui.allocate_space(vec2(ui.available_width(), header_h));
    let cols = list_columns(header, &w);
    for (title, col) in HEADERS.iter().zip(&cols) {
        ui.painter().text(pos2(col.min.x, col.center().y), egui::Align2::LEFT_CENTER, *title, fonts::semibold(t.small), t.text_dim);
    }
    ui.add_space(2.0);
    let height = (height - header_h - 2.0).max(list_row_height(t));
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    let area = egui::ScrollArea::vertical()
        .id_salt("dialog:options/shortcuts")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL);
    if rows.is_empty() {
        area.show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(NO_ROWS).font(fonts::regular(t.body)).color(t.text_dim)));
        });
        return;
    }
    area.show_rows(ui, list_row_height(t), rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for row in rows.get(range).into_iter().flatten() {
            row_ui(ui, t, e, row, &w, actions);
        }
    });
}

/// One row: the name, the context, the two key cells and ↺ when modified.
fn row_ui(ui: &mut Ui, t: &Tokens, e: &ShortcutEditor, row: &Row, w: &[f32; 5], actions: &mut Vec<Action>) {
    let context = row.cell_context(e.context);
    let cell = |slot: u8| KeySlot { command: row.command.clone(), context, slot };
    let recording_here = e.recording.as_ref().is_some_and(|r| r.command == row.command && r.context == context);
    let keys: Vec<String> = row.keys.iter().map(|k| shown_key(k)).collect();
    let context_text = row.context.map_or("—", context_label);
    let label = format!("{}, {}: {}", row.name, context_text, if keys.is_empty() { widgets::NO_KEY_TEXT.to_string() } else { keys.join(", ") });
    let id = row_id(&row.command, context);
    let enabled = row.editable;
    list_row(ui, t, &id, &label, w, recording_here, row.modified(), |ui, c: &ListCell| match c.column {
        0 => {
            list_text(ui, t, c, &row.name, false);
        }
        1 => {
            list_text(ui, t, c, context_text, true);
        }
        2 | 3 => {
            let slot: u8 = if c.column == 2 { 0 } else { 1 };
            let here = cell(slot);
            let conflict = e.conflict.as_ref().filter(|x| x.cell == here);
            let recording = e.recording.as_ref() == Some(&here) && conflict.is_none();
            let key: Vec<&str> = match conflict {
                Some(x) => vec![x.key.as_str()],
                None => row.keys.get(usize::from(slot)).map(String::as_str).into_iter().collect(),
            };
            let look = KeyCell { keys: &key, mac: mac(), recording, conflict: conflict.is_some(), width: c.rect.width() - widgets::LIST_COLUMN_GAP };
            let r = ui.add_enabled_ui(enabled && slot < SHORTCUT_SLOTS, |ui| key_cell(ui, t, &cell_id(&row.command, context, slot), &look)).inner;
            if r.clicked() {
                actions.push(Action::Record(here));
            }
        }
        _ if row.modified() && enabled => {
            let defaults: Vec<String> = row.defaults.iter().map(|k| shown_key(k)).collect();
            let tip = if defaults.is_empty() {
                "Restore the default: no shortcut".to_string()
            } else {
                format!("Restore the default: {}", defaults.join(", "))
            };
            if restore_button(ui, t, &restore_id(&row.command, context), &tip).clicked() {
                actions.push(Action::Restore(row.command.clone(), context));
            }
        }
        _ => {}
    });
}

/// ↺: a 20 × 20 button with the Restore glyph (`icon`, `text` hovered), `hover × 0.7` behind it
/// hovered. Registered as `id` (role `iconButton`, labelled `tip`).
fn restore_button(ui: &mut Ui, t: &Tokens, id: &str, tip: &str) -> egui::Response {
    let (_, rect) = ui.allocate_space(Vec2::splat(20.0));
    let r = ui.interact(rect, Id::new(id), Sense::click());
    let p = ui.painter();
    if r.hovered() {
        p.rect_filled(rect, egui::CornerRadius::same(t.radius), t.hover.gamma_multiply(0.7));
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let glyph = Rect::from_center_size(rect.center(), Vec2::splat(14.0));
    icons::paint_options(p, glyph, icons::OptionsIcon::Restore, if r.hovered() { t.text } else { t.icon });
    widgets::register(ui.ctx(), id, Role::IconButton, tip, Target::Widget(&r), false);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    r.on_hover_text(tip)
}

/// A line of text in `color` registered as `id` (its text as the label), cut to the room left.
fn line(ui: &mut Ui, t: &Tokens, id: &str, text: &str, color: egui::Color32) {
    let r = ui.add(egui::Label::new(RichText::new(text).font(fonts::regular(t.body)).color(color)).truncate());
    widgets::register(ui.ctx(), id, Role::Other, text, Target::Painted { rect: r.rect, clip: ui.clip_rect() }, false);
}

/// The room kept for a question's two buttons after its text (points).
const QUESTION_BUTTONS_W: f32 = 180.0;

/// A question on the bottom line: its text (cut short so the buttons always fit), then its two
/// buttons; returns which was clicked.
fn question(ui: &mut Ui, t: &Tokens, (id, text, color): (&str, &str, egui::Color32), yes: (&str, &str), no: &str) -> Option<bool> {
    let room = (ui.available_width() - QUESTION_BUTTONS_W).max(40.0);
    ui.allocate_ui(vec2(room, t.metrics.button_h), |ui| line(ui, t, id, text, color));
    let yes = text_button(ui, t, yes.0, yes.1, false).clicked();
    let no = text_button(ui, t, no, CANCEL_LABEL, false).clicked();
    if yes {
        Some(true)
    } else if no {
        Some(false)
    } else {
        None
    }
}

/// The bottom line: the conflict's question, else Reset All's question, else Reset All and the hint.
fn bottom(ui: &mut Ui, t: &Tokens, e: &ShortcutEditor, asked: bool, conflict_label: &str, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(c) = &e.conflict {
            let text = draft::sentence(&shown_key(&c.key), conflict_label, c.context);
            match question(ui, t, (CONFLICT_ID, &text, t.danger), (REPLACE_ID, REPLACE_LABEL), CONFLICT_CANCEL_ID) {
                Some(true) => actions.push(Action::Replace),
                Some(false) => actions.push(Action::CancelConflict),
                None => {}
            }
        } else if asked {
            match question(ui, t, (RESET_QUESTION_ID, RESET_QUESTION, t.text), (RESET_CONFIRM_ID, RESET_LABEL), RESET_CANCEL_ID) {
                Some(true) => actions.push(Action::ConfirmReset),
                Some(false) => actions.push(Action::AskReset(false)),
                None => {}
            }
        } else {
            if text_button(ui, t, RESET_ID, RESET_LABEL, false).clicked() {
                actions.push(Action::AskReset(true));
            }
            ui.add(egui::Label::new(RichText::new(HINT).font(fonts::regular(t.small)).color(t.text_dim)).truncate());
        }
    });
}

/// Applies the editor's draft through the engine's commands: Reset All (`app.resetShortcuts`),
/// then each change in order (`app.setShortcut {command, context, key, slot}`), after checking on
/// a copy of the keymap that every one of them applies, so a refused change applies nothing. The
/// menus show the new keys at once (they read the session's keymap). Nothing to do when the draft
/// changes no binding; refused (`disabled`) while no library is open.
pub fn apply(app: &mut App, e: &ShortcutEditor) -> Result<Value, CmdError> {
    if !e.changed() {
        return Ok(json!({}));
    }
    let Some(base) = app.session().map(|s| s.keymap().clone()) else { return Err(CmdError::Disabled(WAITING.into())) };
    draft::plan(&base, e, &|c| label(app, c)).map_err(CmdError::BadParams)?;
    if e.reset_all {
        app.run("app.resetShortcuts", &json!({}))?;
    }
    for c in &e.changes {
        app.run("app.setShortcut", &json!({ "command": c.command, "context": c.context, "key": c.key, "slot": c.slot }))?;
    }
    if let Some(o) = app.state_mut().options.as_mut() {
        draft::cancel(&mut o.shortcuts);
    }
    Ok(json!({ "resetAll": e.reset_all, "changes": e.changes.len() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_name_commands_by_menu_path_and_label() {
        assert_eq!(display_name("Picture/Star / Unstar", "Star / Unstar"), "Picture › Star / Unstar");
        assert_eq!(display_name("", "Select Left"), "Select Left");
        assert_eq!(display_name("View/Thumbnail Caption/Date", "Date Caption"), "View › Thumbnail Caption › Date");
        assert_eq!(row_id("photo.star.toggle", Context::EditView), "shortcut:photo.star.toggle/editView");
        assert_eq!(cell_id("a.b", Context::Global, 1), "shortcut:a.b/global/1");
        let keyless = Row { command: "x".into(), context: None, name: "X".into(), keys: Vec::new(), defaults: Vec::new(), editable: true };
        assert_eq!(
            (keyless.cell_context(None), keyless.cell_context(Some(Context::Library)), keyless.modified()),
            (Context::Global, Context::Library, false)
        );
        // A UI command is labelled from UI_COMMANDS; with no library open an engine command by its id.
        let app = App::new(None);
        assert_eq!((label(&app, "view.thumbsSmall"), label(&app, "photo.star.toggle")), ("Small Thumbnails".into(), "photo.star.toggle".into()));
    }
}
