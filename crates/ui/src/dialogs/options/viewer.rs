//! Options ▸ Photo Viewer (`viewing.md` §3.3, `menus-and-shortcuts.md` §5 Photo Viewer), on M8.5's
//! form rows, its options from the engine's catalogue (`options.list {tab: "viewer"}`):
//!
//! ```text
//!  File Types:  (•) Use Dalica as the default viewer for these file types:
//!               [x] JPEG (Image Viewer)   [ ] PNG (Image Viewer)   [ ] GIF (Firefox)  …
//!               [Current] [Select All] [Select None]
//!               ( ) Don't use Dalica as a viewer
//!  ───────────────────────────────────────────────────────────
//!   Interface:  (•) Mouse wheel zooms (hold Ctrl to navigate)
//!               ( ) Mouse wheel moves through photos (hold Ctrl to zoom)
//!               [x] Hide controls automatically
//!               [x] Translucent background
//! ```
//!
//! **File Types** (M15.24) lists the types the engine's `viewer.fileTypes` gives, each with the
//! application that opens it now, as the operating system says ("JPEG (Image Viewer)"; the type
//! alone when it does not say). The list is asked once when the part is first drawn and kept while
//! it is drawn every frame ([`listing`]; a part not drawn for a frame asks again). The types
//! checked are a draft (`viewer.defaultViewer`, the types last chosen to start with): a checkbox
//! adds or takes a type away, [Current] checks the types Dalica opens now, [Select All] every one,
//! [Select None] none; "Don't use Dalica as a viewer" is the same as none, and "Use Dalica…" with
//! none checks every type. OK or Apply runs `viewer.setDefault {types}` with them ([`apply`]); where
//! only the user may choose (Windows, macOS), what to choose shows as a toast. A system with no way
//! to do it draws the part disabled with its reason. The Interface options are live: a change goes
//! into the draft ([`super::general::set`]) and the dialog's Apply or OK sends it in its one
//! `options.set`; the viewer reads them every frame. The viewer's own Settings dialog
//! (`crate::dialogs::viewer_settings`) draws the same two parts ([`file_types`], [`interface`]).
//!
//! Automation ids (`control-protocol.md` §7): the wheel's radios
//! `choice:option.viewer.wheel/zoom` and `choice:option.viewer.wheel/navigate`, the checkboxes
//! `check:option.viewer.hideControls` and `check:option.viewer.translucent`; the File Types part's
//! radios `choice:option.viewer.defaultViewer/on` and `/off`, a type
//! `check:option.viewer.fileType/<type>` (`jpeg`, `png`, … as `viewer.fileTypes` names them) and
//! the buttons `button:option.viewer.defaultViewer/<name>` (`current`, `selectAll`, `selectNone`).

use dalica_engine::options::Tab;
use dalica_engine::{CmdError, Session};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{Form, ToastKind, checkbox, form_group, radio, text_button, tooltip};
use egui::{Id, Ui};
use serde_json::{Map, Value, json};

use super::file_types::{self, Opt, shown};
use super::general;
use crate::app::App;

/// The tab's groups, in order.
pub const GROUPS: [&str; 2] = ["File Types", "Interface"];
/// The settings keys of the tab.
pub const DEFAULT_VIEWER: &str = "viewer.defaultViewer";
pub const WHEEL: &str = "viewer.wheel";
/// The File Types part's radios.
pub const USE_DALICA: &str = "Use Dalica as the default viewer for these file types:";
pub const DONT_USE: &str = "Don't use Dalica as a viewer";
/// The engine's commands behind the File Types part.
pub const FILE_TYPES: &str = "viewer.fileTypes";
pub const SET_DEFAULT: &str = "viewer.setDefault";

/// One type of the File Types part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRow {
    /// The engine's id (`jpeg`).
    pub id: String,
    /// Its name ("JPEG").
    pub name: String,
    /// The application that opens it now, by name, if the system says.
    pub handler: Option<String>,
    /// Whether that application is Dalica.
    pub dalica: bool,
}

impl TypeRow {
    /// Its checkbox's label: "JPEG (Image Viewer)", or "JPEG".
    pub fn label(&self) -> String {
        match &self.handler {
            Some(h) => format!("{} ({h})", self.name),
            None => self.name.clone(),
        }
    }
}

/// What the File Types part shows: the types with their handlers, or why it cannot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listing {
    pub types: Vec<TypeRow>,
    /// Why the system cannot make Dalica the default viewer, when it cannot.
    pub reason: Option<String>,
}

impl Listing {
    /// Reads a `viewer.fileTypes` reply (rows it cannot read are left out).
    pub fn from_reply(reply: &Value) -> Listing {
        let rows = reply.get("types").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        let types = rows
            .iter()
            .filter_map(|r| {
                let text = |k: &str| r.get(k).and_then(Value::as_str).map(|s| s.chars().filter(|c| !c.is_control()).take(80).collect::<String>());
                Some(TypeRow {
                    id: text("type")?,
                    name: text("name")?,
                    handler: r
                        .get("handler")
                        .and_then(|h| h.get("name"))
                        .and_then(Value::as_str)
                        .map(|s| s.chars().filter(|c| !c.is_control()).take(80).collect()),
                    dalica: r.get("dalica").and_then(Value::as_bool).unwrap_or(false),
                })
            })
            .take(64)
            .collect();
        Listing { types, reason: None }
    }

    /// The types Dalica opens now ([Current]).
    pub fn current(&self) -> Vec<String> {
        self.types.iter().filter(|t| t.dalica).map(|t| t.id.clone()).collect()
    }

    /// Every type, in the list's order ([Select All]).
    pub fn all(&self) -> Vec<String> {
        self.types.iter().map(|t| t.id.clone()).collect()
    }

    /// `types` in the list's order, each once, unknown ones left out.
    pub fn ordered(&self, types: &[String]) -> Vec<String> {
        self.types.iter().filter(|t| types.contains(&t.id)).map(|t| t.id.clone()).collect()
    }
}

/// The cached listing, the pass it was last drawn in and the types saved when it was asked.
#[derive(Clone)]
struct Cached {
    pass: u64,
    saved: Vec<String>,
    listing: Listing,
}

fn cache_id() -> Id {
    Id::new("dalica.viewer.fileTypes")
}

/// The File Types part's listing: asked of the engine (`viewer.fileTypes`, which asks the system
/// off the UI thread and waits for it at most `OS_WAIT`) when the part was not drawn in the
/// previous pass or the types chosen changed since (a `viewer.setDefault`), else the one kept.
pub fn listing(app: &mut App, ctx: &egui::Context) -> Listing {
    let pass = ctx.cumulative_pass_nr();
    let saved = saved(app);
    if let Some(c) = ctx.data(|d| d.get_temp::<Cached>(cache_id()))
        && c.pass.saturating_add(1) >= pass
        && c.saved == saved
    {
        ctx.data_mut(|d| d.insert_temp(cache_id(), Cached { pass, ..c.clone() }));
        return c.listing;
    }
    let listing = match app.run(FILE_TYPES, &json!({})) {
        Ok(reply) => Listing::from_reply(&reply),
        Err(e) => Listing { types: Vec::new(), reason: Some(e.to_string()) },
    };
    ctx.data_mut(|d| d.insert_temp(cache_id(), Cached { pass, saved, listing: listing.clone() }));
    listing
}

/// The types Dalica was last chosen for (`viewer.defaultViewer`).
pub fn saved(app: &App) -> Vec<String> {
    app.session().map(Session::settings).map(|s| s.viewer().default_viewer.iter().map(|t| t.id().to_string()).collect()).unwrap_or_default()
}

/// A JSON list of type ids as strings (anything else left out).
pub fn ids_of(v: Option<&Value>) -> Option<Vec<String>> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).take(64).collect())
}

/// Runs `viewer.setDefault {types}`; where only the user may choose, says what to choose (a toast).
pub fn set_default(app: &mut App, types: &[String]) -> Result<Value, CmdError> {
    let reply = app.run(SET_DEFAULT, &json!({ "types": types }))?;
    if let Some(message) = reply.get("message").and_then(Value::as_str) {
        app.toast(message, ToastKind::Info);
    }
    Ok(reply)
}

/// Options' Apply: takes the File Types draft out of `values` (`options.set` does not take it) and,
/// when it differs from the saved types, runs `viewer.setDefault` with it.
pub fn apply(app: &mut App, values: &mut Map<String, Value>) -> Result<Value, CmdError> {
    let Some(types) = values.remove(DEFAULT_VIEWER) else { return Ok(Value::Null) };
    let types = ids_of(Some(&types)).unwrap_or_default();
    if types == saved(app) { Ok(Value::Null) } else { set_default(app, &types) }
}

/// The tab's options with the values in effect.
pub fn rows(app: &App) -> Vec<Opt> {
    file_types::rows(app, Tab::PhotoViewer)
}

/// Draws the tab's body: both parts; changes go into the Options draft.
pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let listing = listing(app, ui.ctx());
    let selected = ids_of(draft.get(DEFAULT_VIEWER)).unwrap_or_else(|| saved(app));
    let t = *app.tokens();
    let labels: Vec<String> = GROUPS.iter().map(|g| format!("{g}:")).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let form = Form::new(ui, &t, &labels);
    if let Some(next) = file_types(ui, &t, &form, &listing, &selected)
        && let Some(o) = opts.iter().find(|o| o.row.key == DEFAULT_VIEWER)
    {
        // Compared with the saved types, so going back takes it out of the draft.
        let row = general::Row { saved: json!(saved(app)), ..o.row.clone() };
        general::set(app, &row, json!(next));
    }
    ui.separator();
    ui.add_space(4.0);
    for (i, value) in interface(ui, &t, &form, &opts, &draft) {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
}

/// The File Types part with `selected` checked; returns the new selection when a click changed it.
/// Drawn disabled, with the reason as every row's tooltip, when the system cannot.
pub fn file_types(ui: &mut Ui, t: &Tokens, form: &Form, listing: &Listing, selected: &[String]) -> Option<Vec<String>> {
    let reason = listing.reason.clone().unwrap_or_default();
    let on = format!("choice:option.{DEFAULT_VIEWER}/on");
    let off = format!("choice:option.{DEFAULT_VIEWER}/off");
    let mut next: Option<Vec<String>> = None;
    form_group(ui, listing.reason.is_none(), |ui| {
        form.row(ui, t, &format!("{}:", GROUPS[0]), |ui| {
            if tooltip(radio(ui, t, &on, USE_DALICA, !selected.is_empty()), t, &reason, None).clicked() && selected.is_empty() {
                next = Some(listing.all());
            }
            ui.horizontal_wrapped(|ui| {
                for ty in &listing.types {
                    let mut checked = selected.contains(&ty.id);
                    let r = checkbox(ui, t, &format!("check:option.viewer.fileType/{}", ty.id), &ty.label(), &mut checked);
                    if tooltip(r, t, &reason, None).clicked() {
                        let mut chosen: Vec<String> = selected.iter().filter(|s| **s != ty.id).cloned().collect();
                        if checked {
                            chosen.push(ty.id.clone());
                        }
                        next = Some(listing.ordered(&chosen));
                    }
                }
            });
            ui.horizontal(|ui| {
                for (name, label) in [("current", "Current"), ("selectAll", "Select All"), ("selectNone", "Select None")] {
                    if tooltip(text_button(ui, t, &format!("button:option.{DEFAULT_VIEWER}/{name}"), label, false), t, &reason, None).clicked() {
                        next = Some(match name {
                            "current" => listing.current(),
                            "selectAll" => listing.all(),
                            _ => Vec::new(),
                        });
                    }
                }
            });
            if tooltip(radio(ui, t, &off, DONT_USE, selected.is_empty()), t, &reason, None).clicked() {
                next = Some(Vec::new());
            }
        });
    });
    next.filter(|n| n.as_slice() != selected)
}

/// The Interface part: the wheel's two radios and the two checkboxes; returns the values changed
/// this pass, by option.
pub fn interface(ui: &mut Ui, t: &Tokens, form: &Form, opts: &[Opt], draft: &Map<String, Value>) -> Vec<(usize, Value)> {
    let mut changes = Vec::new();
    form.row(ui, t, &format!("{}:", GROUPS[1]), |ui| {
        for (i, o) in opts.iter().enumerate().filter(|(_, o)| o.row.group == GROUPS[1]) {
            let value = shown(&o.row, draft);
            match o.row.kind.as_str() {
                "bool" => {
                    if let Some(v) = file_types::check(ui, t, &o.row, value) {
                        changes.push((i, v));
                    }
                }
                _ => {
                    for (choice, label) in &o.row.choices {
                        let r =
                            form_group(ui, o.row.reason.is_none(), |ui| radio(ui, t, &general::item_id(&o.row.key, choice), label, value == choice));
                        if r.clicked() && value != choice {
                            changes.push((i, choice.clone()));
                        }
                    }
                }
            }
        }
    });
    changes
}
