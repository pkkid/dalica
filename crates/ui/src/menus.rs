//! Menus: the menu model built from the [catalog](crate::menu_catalog), the UI-only commands
//! ([`UI_COMMANDS`], defined in [`crate::commands`]) and the running app, and the in-window menus
//! drawn from it.
//!
//! - **The model** ([`bar`], [`section_menu`]) is the catalog with the app's state applied: an
//!   item is *live* when its command is registered (a UI command or an engine command; an item
//!   that opens a dialog also needs that dialog), *enabled* when live and its command's `enabled`
//!   accepts the parameters a click would pass, and shows the shortcut in effect. The same model
//!   feeds the menus drawn here, `ui.menu.tree` / `ui.menu.invoke` (`control-protocol.md` §6.10)
//!   and the native macOS menu bar.
//! - **Drawing** ([`show_bar`], [`context_menu`]): every menu level is bounded by the window and
//!   scrolls when it is taller than the room below the menu bar, with arrow strips at its top and
//!   bottom (`design-system.md` §11.2). Disabled items are greyed, never hidden.
//! - **Running an item** ([`activate`]): a dialog item opens its dialog, any other runs its
//!   command through [`App::run`] with the selection where the command takes `ids`.

use dalica_engine::{CmdError, CommandSpec, Context};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Rect, Sense, Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use crate::app::App;
use crate::menu_catalog::{self, Entry, Item, MAC_APP_MENU, MENU_BAR, Mac, Mark, Menu};

pub use crate::commands::{THUMB_SMALL, UI_COMMANDS, UiCommand};

// ---------------------------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------------------------

/// The platform a menu model is built for: macOS moves some items into its application menu and
/// writes `Ctrl` as `Cmd`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Other,
}

impl Platform {
    /// The platform this build runs on.
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") { Platform::Mac } else { Platform::Other }
    }
}

/// What the current section is: it titles the fourth menu and picks its variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SectionKind {
    #[default]
    Folder,
    /// An album or a person.
    Album,
}

/// The current section's kind. Albums and people arrive with their milestones; until then every
/// section is a folder.
pub fn section_kind(_app: &App) -> SectionKind {
    SectionKind::Folder
}

/// Items that open a dialog Dalica already has, and the dialog: `ui.menu.invoke` replies
/// `{dialog: kind}` for them. Other `…` items stay disabled until their dialog exists, even when
/// their command is registered (Help ▸ Rebuild Library… asks first; `engine.execute` runs it with
/// `confirm: true`).
pub const DIALOGS: &[(&str, &str)] = &[("folders.add", "filePicker")];

/// One item as the app shows it now.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemNode {
    /// The command it runs (the album variant's in an album).
    pub id: &'static str,
    /// The label shown (Undo names the action it undoes).
    pub label: String,
    /// The catalog's label.
    pub catalog_label: &'static str,
    /// The fixed parameters of the row (`null` for none).
    pub params: Value,
    /// Its automation id while its menu is open: `menuitem:<id>`, or `menuitem:<id>/<part>`.
    pub widget: String,
    /// The shortcut shown, as the platform writes it.
    pub shortcut: Option<String>,
    pub live: bool,
    pub enabled: bool,
    /// Why it is disabled, when it is.
    pub reason: Option<String>,
    pub mark: Mark,
    pub checked: bool,
    /// The dialog it opens, for an item of [`DIALOGS`].
    pub dialog: Option<&'static str>,
}

/// One row of a menu as shown.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Item(ItemNode),
    Separator,
    Submenu {
        label: &'static str,
        /// `submenu:<menu>/<label>…`
        widget: String,
        children: Vec<Node>,
    },
}

/// One menu of the bar as shown.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuNode {
    /// The English title: the widget id is `menu:<key>`.
    pub key: &'static str,
    /// The title shown (`Album` for the fourth menu in an album).
    pub title: &'static str,
    pub children: Vec<Node>,
}

/// The catalog menus of the bar on `platform`, left to right.
pub fn bar_menus(platform: Platform) -> Vec<&'static Menu> {
    let mac = (platform == Platform::Mac).then_some(&MAC_APP_MENU);
    mac.into_iter().chain(MENU_BAR.iter()).collect()
}

/// The title `menu` shows for a section of `kind`.
pub fn title(menu: &Menu, kind: SectionKind) -> &'static str {
    match (kind, menu.album_title) {
        (SectionKind::Album, Some(t)) => t,
        _ => menu.key,
    }
}

/// What the model needs to know about the app, gathered once per build.
struct Facts<'a> {
    app: &'a App,
    platform: Platform,
    kind: SectionKind,
    /// The engine's commands when no library is open yet (they exist, but wait for it).
    specs: Vec<CommandSpec>,
}

impl<'a> Facts<'a> {
    fn new(app: &'a App, platform: Platform) -> Facts<'a> {
        let specs = if app.session().is_some() { Vec::new() } else { dalica_engine::cmd::specs() };
        Facts { app, platform, kind: section_kind(app), specs }
    }

    fn spec(&self, id: &str) -> Option<&CommandSpec> {
        match self.app.session() {
            Some(s) => s.spec(id),
            None => self.specs.iter().find(|s| s.id == id),
        }
    }
}

/// The whole bar on `platform`, as shown now.
pub fn bar(app: &App, platform: Platform) -> Vec<MenuNode> {
    let facts = Facts::new(app, platform);
    bar_menus(platform).into_iter().map(|m| menu_node(&facts, m)).collect()
}

/// One menu of the bar, as shown now.
pub fn menu(app: &App, menu: &Menu, platform: Platform) -> MenuNode {
    menu_node(&Facts::new(app, platform), menu)
}

/// The section context menu (tree row, section header, empty light-table space): the fourth menu's
/// rows (`menus-and-shortcuts.md` §1.4).
pub fn section_menu(app: &App, platform: Platform) -> Vec<Node> {
    let facts = Facts::new(app, platform);
    nodes(&facts, menu_catalog::SECTION, "Folder")
}

fn menu_node(facts: &Facts<'_>, menu: &Menu) -> MenuNode {
    MenuNode { key: menu.key, title: title(menu, facts.kind), children: nodes(facts, menu.entries, menu.key) }
}

/// The rows of `entries` under the widget path `path`; separators tidied after macOS removed the
/// items its application menu holds (none leading, trailing or doubled).
fn nodes(facts: &Facts<'_>, entries: &'static [Entry], path: &str) -> Vec<Node> {
    let mut out: Vec<Node> = Vec::with_capacity(entries.len());
    for e in entries {
        match e {
            Entry::Separator => {
                if !matches!(out.last(), None | Some(Node::Separator)) {
                    out.push(Node::Separator);
                }
            }
            Entry::Item(i) => {
                if facts.platform == Platform::Mac && i.mac == Mac::AppMenu {
                    continue;
                }
                out.push(Node::Item(item_node(facts, i)));
            }
            Entry::Submenu(label, children) => {
                let path = format!("{path}/{label}");
                let children = nodes(facts, children, &path);
                out.push(Node::Submenu { label, widget: format!("submenu:{path}"), children });
            }
        }
    }
    if matches!(out.last(), Some(Node::Separator)) {
        out.pop();
    }
    out
}

/// The parameters a click on the row passes to `id`: its fixed ones, plus the selection when the
/// command takes `ids`.
pub fn params_for(app: &App, id: &str, fixed: &Value) -> Value {
    let mut obj: Map<String, Value> = match crate::keys::params_for(app, id) {
        Value::Object(o) => o,
        _ => Map::new(),
    };
    if let Value::Object(f) = fixed {
        for (k, v) in f {
            obj.insert(k.clone(), v.clone());
        }
    }
    Value::Object(obj)
}

fn item_node(facts: &Facts<'_>, i: &'static Item) -> ItemNode {
    let app = facts.app;
    let album = facts.kind == SectionKind::Album;
    let id = if album { i.album_id.unwrap_or(i.id) } else { i.id };
    let params: Value = if i.params.is_empty() { Value::Null } else { serde_json::from_str(i.params).unwrap_or(Value::Null) };
    let dialog = DIALOGS.iter().find(|(c, _)| *c == id).map(|(_, d)| *d);
    let ui_command = UI_COMMANDS.iter().any(|c| c.id == id);
    let spec = facts.spec(id);
    let registered = ui_command || spec.is_some();
    let live = registered && (dialog.is_some() || !i.label.ends_with('…'));
    let refuse = |why: &str| (false, Some(why.to_string()));
    let (enabled, reason) = if !live {
        refuse("Not available yet.")
    } else if album && i.folder_only {
        refuse("Not available for albums.")
    } else if dialog.is_some() {
        match app.can_pick_folder() {
            Ok(()) => (true, None),
            Err(why) => (false, Some(why)),
        }
    } else if ui_command {
        (true, None)
    } else {
        match (app.session(), spec) {
            (Some(session), Some(spec)) => match (spec.enabled)(session, &params_for(app, id, &params)) {
                Ok(()) => (true, None),
                Err(d) => (false, Some(d.0)),
            },
            _ => refuse("The library is still opening."),
        }
    };
    let undo = app.session().map(dalica_engine::Session::undo_stack);
    let label = match (id, undo.and_then(|u| if id == "edit.undo" { u.undo_label() } else { u.redo_label() })) {
        ("edit.undo", Some(action)) => format!("Undo {action}"),
        ("edit.redo", Some(action)) => format!("Redo {action}"),
        _ => i.label.to_string(),
    };
    let widget = match i.part {
        Some(part) => format!("menuitem:{id}/{part}"),
        None => format!("menuitem:{id}"),
    };
    let checked = checked(app, id, &params);
    ItemNode {
        id,
        label,
        catalog_label: i.label,
        params,
        widget,
        shortcut: shortcut(facts, i, id, registered),
        live,
        enabled,
        reason,
        mark: i.mark,
        checked,
        dialog,
    }
}

/// The shortcut a row shows: for a registered command (the engine's or a UI command, both in the
/// session's keymap), the keymap's binding in effect (so a rebinding shows here, and a removed one
/// shows nothing); otherwise, and before a library is open, the catalog's default. Rows
/// the catalog gives no shortcut (Batch Edit's rotations) show none. macOS writes `Ctrl` as `Cmd`.
fn shortcut(facts: &Facts<'_>, i: &Item, id: &str, registered: bool) -> Option<String> {
    i.shortcut?;
    let mac = facts.platform == Platform::Mac;
    let text = match facts.app.session().filter(|_| registered) {
        Some(s) => {
            let keymap = s.keymap();
            let key = [Context::Library, Context::Global].into_iter().find_map(|c| keymap.keys_for(id, c).into_iter().next());
            key?.to_string()
        }
        None => if mac { i.mac_shortcut.or(i.shortcut) } else { i.shortcut }?.to_string(),
    };
    Some(if mac { text.replace("Ctrl+", "Cmd+") } else { text })
}

/// Whether a checkable row (or a row of the view-options popup) running `id` with its fixed
/// `params` is on: the view state answers ([`crate::commands::view::checked`]); rows of commands
/// that do not exist yet are off.
pub fn checked(app: &App, id: &str, params: &Value) -> bool {
    crate::commands::view::checked(app.state(), id, params).unwrap_or(false)
}

/// How `ui.menu.invoke` names an item.
#[derive(Clone, Debug, PartialEq)]
pub enum Lookup {
    /// Labels from the menu title down (`["Picture", "Batch Edit", "Rotate Clockwise"]`), as shown
    /// or as in the catalog, a trailing `…` optional.
    Path(Vec<String>),
    /// The first item running `id` whose fixed parameters all appear in `params`.
    Id { id: String, params: Value },
}

/// Splits a menu path at `/`; `\/` is a literal slash and `\\` a backslash. `Err` for an empty
/// path or label, or a stray backslash.
pub fn split_path(path: &str) -> Result<Vec<String>, String> {
    let mut parts = vec![String::new()];
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(e @ ('/' | '\\')) => {
                    if let Some(p) = parts.last_mut() {
                        p.push(e);
                    }
                }
                _ => return Err("a `\\` in a menu path escapes `/` or `\\` only".into()),
            },
            '/' => parts.push(String::new()),
            c => {
                if let Some(p) = parts.last_mut() {
                    p.push(c);
                }
            }
        }
    }
    if parts.iter().any(|p| p.trim().is_empty()) {
        return Err("a menu path is labels separated by `/`, none empty".into());
    }
    Ok(parts)
}

fn label_matches(shown: &str, catalog: &str, wanted: &str) -> bool {
    let bare = |s: &str| s.trim().trim_end_matches('…').trim_end_matches("...").trim().to_string();
    bare(shown) == bare(wanted) || bare(catalog) == bare(wanted)
}

/// The item `lookup` names in `menus`, if any.
pub fn find<'m>(menus: &'m [MenuNode], lookup: &Lookup) -> Option<&'m ItemNode> {
    match lookup {
        Lookup::Path(parts) => {
            let (first, rest) = parts.split_first()?;
            let menu = menus.iter().find(|m| label_matches(m.title, m.key, first))?;
            find_path(&menu.children, rest)
        }
        Lookup::Id { id, params } => menus.iter().find_map(|m| find_id(&m.children, id, params)),
    }
}

/// The item at `path` under `nodes`.
pub fn find_path<'m>(nodes: &'m [Node], path: &[String]) -> Option<&'m ItemNode> {
    let (first, rest) = path.split_first()?;
    nodes.iter().find_map(|n| match n {
        Node::Item(i) if rest.is_empty() && label_matches(&i.label, i.catalog_label, first) => Some(i),
        Node::Submenu { label, children, .. } if !rest.is_empty() && label_matches(label, label, first) => find_path(children, rest),
        _ => None,
    })
}

/// The first item under `nodes` running `id` whose fixed parameters all appear in `params`.
pub fn find_id<'m>(nodes: &'m [Node], id: &str, params: &Value) -> Option<&'m ItemNode> {
    let fits = |fixed: &Value| match fixed {
        Value::Object(f) => f.iter().all(|(k, v)| params.get(k) == Some(v)),
        _ => true,
    };
    nodes.iter().find_map(|n| match n {
        Node::Item(i) if i.id == id && fits(&i.params) => Some(i),
        Node::Submenu { children, .. } => find_id(children, id, params),
        _ => None,
    })
}

/// The bar as `ui.menu.tree` returns it (`control-protocol.md` §6.10).
pub fn tree_json(menus: &[MenuNode]) -> Value {
    Value::Array(
        menus.iter().map(|m| json!({ "label": m.title, "widget": format!("menu:{}", m.key), "children": nodes_json(&m.children) })).collect(),
    )
}

/// Rows as `ui.menu.tree` returns them.
pub fn nodes_json(nodes: &[Node]) -> Value {
    Value::Array(
        nodes
            .iter()
            .map(|n| match n {
                Node::Separator => json!({ "separator": true }),
                Node::Submenu { label, widget, children } => {
                    json!({ "label": label, "widget": widget, "children": nodes_json(children) })
                }
                Node::Item(i) => {
                    let mut o = Map::new();
                    o.insert("id".into(), i.id.into());
                    o.insert("label".into(), i.label.clone().into());
                    if let Some(s) = &i.shortcut {
                        o.insert("shortcut".into(), s.clone().into());
                    }
                    o.insert("enabled".into(), i.enabled.into());
                    o.insert("live".into(), i.live.into());
                    o.insert("widget".into(), i.widget.clone().into());
                    if !i.params.is_null() {
                        o.insert("params".into(), i.params.clone());
                    }
                    match i.mark {
                        Mark::None => {}
                        Mark::Check => {
                            o.insert("checked".into(), i.checked.into());
                        }
                        Mark::Radio => {
                            o.insert("checked".into(), i.checked.into());
                            o.insert("radio".into(), true.into());
                        }
                    }
                    if let Some(d) = i.dialog {
                        o.insert("dialog".into(), d.into());
                    }
                    if let Some(r) = &i.reason {
                        o.insert("reason".into(), r.clone().into());
                    }
                    Value::Object(o)
                }
            })
            .collect(),
    )
}

/// Runs a menu item as a click does: a dialog item opens its dialog (`{dialog: kind}`), any other
/// runs its command with [`params_for`] plus `extra` (the caller's parameters win). A disabled or
/// not-yet-live item is refused with the reason, which the status line shows.
pub fn activate(app: &mut App, item: &ItemNode, extra: &Value) -> Result<Value, CmdError> {
    if !item.enabled {
        let why = item.reason.clone().unwrap_or_else(|| format!("{} is not available now.", item.label));
        app.state_mut().status = why.clone();
        return Err(CmdError::Disabled(why));
    }
    if let Some(dialog) = item.dialog {
        app.add_folder();
        return Ok(json!({ "dialog": dialog }));
    }
    let mut params = params_for(app, item.id, &item.params);
    if let (Value::Object(p), Value::Object(e)) = (&mut params, extra) {
        for (k, v) in e {
            p.insert(k.clone(), v.clone());
        }
    }
    app.run(item.id, &params)
}

// ---------------------------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------------------------

/// Height of a scroll-arrow strip.
pub const ARROW: f32 = 16.0;
/// Gap kept between a menu and the window's edge.
const EDGE: f32 = 4.0;
/// How fast resting the pointer on a scroll arrow scrolls, in points per second.
const SPEED: f32 = 600.0;
/// How far a click on a scroll arrow scrolls: three rows.
const STEP: f32 = 3.0 * 26.0;
/// The narrowest a menu is.
const MIN_WIDTH: f32 = 220.0;
/// The left and right padding of a row; the check mark and radio dot sit in the left one.
const GUTTER: f32 = 24.0;

/// The height a menu level's rows may take: from `top` (the bottom of the menu bar, or the window's
/// top for a context menu) down to the window's bottom, less `frame` (the popup's margins and
/// stroke) and a small gap at both ends. Never less than the scroll arrows need.
pub fn room(screen: Rect, top: f32, frame: f32) -> f32 {
    // `max` then `min`, never `clamp`: an empty screen rect has its top below its bottom.
    let top = if top.is_finite() { top.max(screen.top()).min(screen.bottom()) } else { screen.top() };
    let r = screen.bottom() - EDGE - (top + EDGE) - frame;
    if r.is_finite() { r.max(4.0 * ARROW) } else { 4.0 * ARROW }
}

/// Is the pointer over `rect` on this ui's layer (not under a submenu drawn on top)?
fn pointer_on(ui: &Ui, rect: Rect) -> bool {
    ui.ctx().pointer_hover_pos().is_some_and(|p| rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id()))
}

/// Draws one level of a menu (`depth` 1 for a menu of the bar or a context menu), bounded by the
/// window: when its rows do not fit the room below `top` they scroll, with arrow strips at the top
/// and bottom that scroll while the pointer rests on them or on a click, and the wheel. The strips
/// register as `button:menu.scrollUp/<depth>` and `button:menu.scrollDown/<depth>`.
pub fn level(ui: &mut Ui, depth: usize, top: f32, rows: impl FnOnce(&mut Ui)) {
    let ctx = ui.ctx().clone();
    let frame = ui.spacing().menu_margin.sum().y + 2.0;
    let key = ui.id().with(("dalica-menu-level", depth));
    // The rows' height last frame: does this level overflow?
    let content: Option<f32> = ctx.data(|d| d.get_temp(key));
    let room = room(ctx.content_rect(), top, frame);
    let over = content.is_some_and(|h| h > room + 0.5);
    let up = over.then(|| ui.allocate_exact_size(vec2(0.0, ARROW), Sense::hover()).0);
    let height = if over { room - 2.0 * ARROW } else { room };
    // A popup's ui is only as tall as the popup was last frame and a scroll area never grows past
    // that: ask for the room the window has; auto-shrink then fits the area to its rows.
    let out = egui::ScrollArea::vertical()
        .id_salt(("dalica-menu-level", depth))
        .max_height(height)
        .min_scrolled_height(height)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, rows);
    let down = over.then(|| ui.allocate_exact_size(vec2(0.0, ARROW), Sense::hover()).0);
    ctx.data_mut(|d| d.insert_temp(key, out.content_size.y));
    let (Some(up), Some(down)) = (up, down) else { return };
    let tokens = dalica_ui_kit::Tokens::default();
    let x = ui.min_rect().x_range();
    let max = (out.content_size.y - out.inner_rect.height()).max(0.0);
    let offset = out.state.offset.y;
    let dt = ctx.input(|i| i.stable_dt).clamp(1.0 / 120.0, 0.1);
    let mut target = offset;
    for (strip, dir, name) in [(up, -1.0_f32, "menu.scrollUp"), (down, 1.0, "menu.scrollDown")] {
        let strip = Rect::from_x_y_ranges(x, strip.y_range());
        let live = if dir < 0.0 { offset > 0.5 } else { offset < max - 0.5 };
        let r = ui.interact(strip, key.with(name), Sense::click());
        let label = if dir < 0.0 { "Scroll menu up" } else { "Scroll menu down" };
        widgets::register(&ctx, &format!("button:{name}/{depth}"), Role::Button, label, Target::Widget(&r), false);
        let c = strip.center();
        let (h, w) = (4.5, 6.0);
        let tri = if dir < 0.0 {
            vec![pos2(c.x - w, c.y + h * 0.5), pos2(c.x + w, c.y + h * 0.5), pos2(c.x, c.y - h * 0.5)]
        } else {
            vec![pos2(c.x - w, c.y - h * 0.5), pos2(c.x + w, c.y - h * 0.5), pos2(c.x, c.y + h * 0.5)]
        };
        let colour = if live { tokens.text } else { tokens.text_disabled };
        ui.painter().add(egui::Shape::convex_polygon(tri, colour, egui::Stroke::NONE));
        let on = pointer_on(ui, strip);
        if live && on {
            target = (target + dir * SPEED * dt).clamp(0.0, max);
            ctx.request_repaint();
        }
        if r.clicked() {
            target = (target + dir * STEP).clamp(0.0, max);
        }
        // The wheel scrolls over the arrows too.
        if on {
            let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                target = (target - wheel).clamp(0.0, max);
            }
        }
    }
    if target != offset {
        let mut state = out.state;
        state.offset.y = target;
        state.store(&ctx, out.id);
    }
}

/// Draws the rows of a menu level; a clicked item lands in `clicked` and closes the menu.
fn rows(ui: &mut Ui, nodes: &[Node], clicked: &mut Option<ItemNode>, depth: usize, top: f32) {
    ui.set_min_width(MIN_WIDTH);
    ui.spacing_mut().button_padding.x = GUTTER;
    let tokens = dalica_ui_kit::Tokens::default();
    for n in nodes {
        match n {
            Node::Separator => {
                ui.separator();
            }
            Node::Submenu { label, widget, children } => {
                let button = egui::Button::new(*label).right_text("›");
                let (r, inner) = egui::containers::menu::SubMenuButton::from_button(button)
                    .ui(ui, |ui| level(ui, depth + 1, top, |ui| self::rows(ui, children, clicked, depth + 1, top)));
                widgets::register(ui.ctx(), widget, Role::MenuItem, label, Target::Widget(&r), inner.is_some());
            }
            Node::Item(i) => {
                let mut b = egui::Button::new(i.label.as_str());
                if let Some(s) = &i.shortcut {
                    b = b.shortcut_text(s.as_str());
                }
                let r = ui.add_enabled(i.enabled, b);
                if i.checked {
                    let colour = if i.enabled { tokens.text } else { tokens.text_disabled };
                    let c = pos2(r.rect.left() + GUTTER * 0.5, r.rect.center().y);
                    match i.mark {
                        Mark::Check => {
                            let pts = vec![pos2(c.x - 4.0, c.y), pos2(c.x - 1.0, c.y + 3.5), pos2(c.x + 4.5, c.y - 4.0)];
                            ui.painter().add(egui::Shape::line(pts, egui::Stroke::new(1.6, colour)));
                        }
                        Mark::Radio => {
                            ui.painter().circle_filled(c, 3.0, colour);
                        }
                        Mark::None => {}
                    }
                }
                widgets::register(ui.ctx(), &i.widget, Role::MenuItem, &i.label, Target::Widget(&r), i.checked);
                if r.clicked() {
                    *clicked = Some(i.clone());
                    ui.close();
                }
            }
        }
    }
}

/// The in-window menu bar (Windows and Linux): one title per menu of the model (`menu:<key>`), each
/// opening its menu bounded by the window. A clicked item runs through [`activate`]; returns its
/// command id (its result or refusal is on the status line). It always draws the Windows/Linux
/// layout: on macOS the Mac layout belongs to the system menu bar (`design-system.md` §11.1), and
/// a render must not depend on the OS it runs on.
pub fn show_bar(ui: &mut Ui, app: &mut App) -> Option<&'static str> {
    let platform = Platform::Other;
    let top = ui.max_rect().bottom();
    let mut clicked: Option<ItemNode> = None;
    {
        let app: &App = app;
        let kind = section_kind(app);
        egui::MenuBar::new().ui(ui, |ui| {
            for m in bar_menus(platform) {
                let shown = title(m, kind);
                let r = ui.menu_button(shown, |ui| {
                    let model = menu(app, m, platform);
                    level(ui, 1, top, |ui| rows(ui, &model.children, &mut clicked, 1, top));
                });
                widgets::register(ui.ctx(), &format!("menu:{}", m.key), Role::Menu, shown, Target::Widget(&r.response), r.inner.is_some());
            }
        });
    }
    let item = clicked?;
    let _ = activate(app, &item, &Value::Null);
    Some(item.id)
}

/// A context menu on `response` (right-click) showing `nodes`, bounded by the window like every
/// menu. A clicked item runs through [`activate`]; returns its command id.
pub fn context_menu(response: &egui::Response, app: &mut App, nodes: &[Node]) -> Option<&'static str> {
    let mut clicked: Option<ItemNode> = None;
    let top = response.ctx.content_rect().top();
    response.context_menu(|ui| level(ui, 1, top, |ui| rows(ui, nodes, &mut clicked, 1, top)));
    let item = clicked?;
    let _ = activate(app, &item, &Value::Null);
    Some(item.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;
    use crate::state::{Pane, THUMB_DEFAULT, UiState};

    #[test]
    fn select_set_replaces_or_toggles_and_keeps_an_anchor() {
        let (_dir, s, ids) = session_with_photos(4);
        let mut app = App::new(Some(s));
        let n = |i: usize| ids[i].get();
        assert_eq!(app.run("select.set", &json!({ "ids": [n(0), n(1)] })).unwrap(), json!({ "selection": [n(0), n(1)] }));
        assert_eq!(app.state().selection.anchor, Some(ids[1]));
        app.run("select.set", &json!({ "ids": [n(1), n(2)], "mode": "toggle" })).unwrap();
        assert_eq!(app.state().selection.ids, [ids[0], ids[2]]);
        assert_eq!(app.state().selection.anchor, Some(ids[2]));
        // Toggling the anchor off moves it to the last selected photo.
        app.run("select.set", &json!({ "ids": [n(2)], "mode": "toggle" })).unwrap();
        assert_eq!(app.state().selection.anchor, Some(ids[0]));
        app.run("select.set", &json!({ "ids": [n(3), n(3)] })).unwrap();
        assert_eq!(app.state().selection.ids, [ids[3]], "duplicates collapse");
        app.run("select.none", &json!({})).unwrap();
        assert!(app.state().selection.ids.is_empty() && app.state().selection.anchor.is_none());
        // Engine commands are forwarded.
        let r = app.run("photo.star.toggle", &json!({ "ids": [n(0)] })).unwrap();
        assert!(r.to_string().contains("true"), "{r}");
    }

    #[test]
    fn hostile_params_and_unknown_ids_are_errors_never_panics() {
        let (_dir, s, ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        for (id, params) in [
            ("select.set", json!({})),
            ("select.set", json!({ "ids": "1" })),
            ("select.set", json!({ "ids": [-1] })),
            ("select.set", json!({ "ids": [1.5] })),
            ("select.set", json!({ "ids": [], "mode": "sideways" })),
            ("select.set", json!({ "ids": [], "extra": 1 })),
            ("select.set", json!([1, 2])),
            ("select.set", Value::Null),
            ("select.none", json!({ "all": true })),
            ("select.none", json!("x")),
        ] {
            assert!(matches!(app.run(id, &params), Err(CmdError::BadParams(_))), "{id} {params}");
        }
        assert!(matches!(app.run("select.set", &json!({ "ids": [ids[0].get() + 1000] })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("select.set", &json!({ "ids": [u64::MAX] })), Err(CmdError::NotFound(_))));
        assert!(matches!(app.run("no.such.command", &json!({})), Err(CmdError::UnknownCommand(_))));
        assert!(app.state().selection.ids.is_empty(), "nothing changed");
        assert!(!app.state().status.is_empty(), "the last failure is on the status line");
    }

    #[test]
    fn ui_commands_do_not_clash_with_engine_commands() {
        let engine = dalica_engine::cmd::specs();
        for c in UI_COMMANDS {
            assert!(!engine.iter().any(|s| s.id == c.id), "{} is both", c.id);
            assert!(UI_COMMANDS.iter().filter(|o| o.id == c.id).count() == 1);
        }
    }

    fn items(nodes: &[Node]) -> Vec<&ItemNode> {
        let mut out = Vec::new();
        for n in nodes {
            match n {
                Node::Item(i) => out.push(i),
                Node::Submenu { children, .. } => out.extend(items(children)),
                Node::Separator => {}
            }
        }
        out
    }

    #[test]
    fn registered_commands_are_live_and_the_rest_disabled() {
        let (_dir, s, ids) = session_with_photos(2);
        let mut app = App::new(Some(s));
        let menus = bar(&app, Platform::Other);
        let all: Vec<&ItemNode> = menus.iter().flat_map(|m| items(&m.children)).collect();
        let live: Vec<&str> = all.iter().filter(|i| i.live).map(|i| i.id).collect();
        assert_eq!(
            live,
            [
                "folders.add",
                "app.quit",
                "edit.undo",
                "edit.redo",
                "select.none",
                "view.thumbsSmall",
                "view.thumbsNormal",
                "pane.properties",
                "pane.tags",
                "pane.people",
                "pane.places",
                "view.smallPictures",
                "view.hiddenPictures",
                "view.caption",
                "view.caption",
                "view.caption",
                "view.caption",
                "view.caption",
                "view.caption",
                "view.folderView",
                "view.folderView",
                "window.fullscreen",
                "pane.properties",
                "photo.star.toggle"
            ]
        );
        for i in all.iter().filter(|i| !i.live) {
            assert!(!i.enabled && i.reason.as_deref() == Some("Not available yet."), "{}", i.widget);
        }
        // Rebuild Library… is registered but asks first, and its dialog is not built yet.
        let rebuild = find(&menus, &Lookup::Id { id: "library.rebuild".into(), params: Value::Null }).unwrap();
        assert!(!rebuild.live && !rebuild.enabled);
        // Without a selection Star is live but disabled, with the command's reason; Undo too.
        let star = find(&menus, &Lookup::Path(vec!["Picture".into(), "Star / Unstar".into()])).unwrap();
        assert_eq!((star.live, star.enabled, star.reason.as_deref()), (true, false, Some("Select some photos first.")));
        assert_eq!(star.shortcut.as_deref(), Some("Ctrl+8"));
        let undo = find(&menus, &Lookup::Id { id: "edit.undo".into(), params: Value::Null }).unwrap();
        assert_eq!((undo.label.as_str(), undo.enabled), ("Undo", false));
        // Select None needs no library state.
        assert!(find(&menus, &Lookup::Id { id: "select.none".into(), params: Value::Null }).unwrap().enabled);
        // With a selection Star runs, and Undo then names the step.
        app.run("select.set", &json!({ "ids": [ids[0].get()] })).unwrap();
        let star = find(&bar(&app, Platform::Other), &Lookup::Id { id: "photo.star.toggle".into(), params: Value::Null }).cloned().unwrap();
        assert!(star.enabled);
        let r = activate(&mut app, &star, &Value::Null).unwrap();
        assert!(r.to_string().contains("true"), "{r}");
        let menus = bar(&app, Platform::Other);
        let undo = find(&menus, &Lookup::Id { id: "edit.undo".into(), params: Value::Null }).unwrap();
        assert!(undo.enabled && undo.label.starts_with("Undo ") && undo.label.len() > 5, "{}", undo.label);
        // Found by the label shown and by the catalog's.
        assert!(find(&menus, &Lookup::Path(vec!["Edit".into(), undo.label.clone()])).is_some());
        assert!(find(&menus, &Lookup::Path(vec!["Edit".into(), "Undo".into()])).is_some());
    }

    #[test]
    fn without_a_library_engine_items_wait_and_ui_items_run() {
        let app = App::new(None);
        let menus = bar(&app, Platform::Other);
        let star = find(&menus, &Lookup::Id { id: "photo.star.toggle".into(), params: Value::Null }).unwrap();
        assert_eq!((star.live, star.enabled, star.reason.as_deref()), (true, false, Some("The library is still opening.")));
        let add = find(&menus, &Lookup::Id { id: "folders.add".into(), params: Value::Null }).unwrap();
        assert!(add.live && !add.enabled, "no library, no picker");
        assert!(find(&menus, &Lookup::Id { id: "select.none".into(), params: Value::Null }).unwrap().enabled);
    }

    #[test]
    fn disabled_items_are_refused_with_their_reason_on_the_status_line() {
        let (_dir, s, _) = session_with_photos(1);
        let mut app = App::new(Some(s));
        let menus = bar(&app, Platform::Other);
        let star = find(&menus, &Lookup::Id { id: "photo.star.toggle".into(), params: Value::Null }).cloned().unwrap();
        assert!(matches!(activate(&mut app, &star, &Value::Null), Err(CmdError::Disabled(m)) if m == "Select some photos first."));
        assert_eq!(app.state().status, "Select some photos first.");
        let album = find(&menus, &Lookup::Id { id: "album.new".into(), params: Value::Null }).cloned().unwrap();
        assert!(matches!(activate(&mut app, &album, &Value::Null), Err(CmdError::Disabled(_))));
    }

    #[test]
    fn macos_moves_exit_options_and_about_into_the_application_menu() {
        let app = App::new(None);
        let other = bar(&app, Platform::Other);
        let mac = bar(&app, Platform::Mac);
        assert_eq!(other.iter().map(|m| m.key).collect::<Vec<_>>(), ["File", "Edit", "View", "Folder", "Picture", "Create", "Tools", "Help"]);
        assert_eq!(mac.len(), other.len() + 1);
        assert_eq!(mac[0].key, "Dalica");
        let ids = |m: &MenuNode| items(&m.children).iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids(&mac[0]), ["app.about", "app.preferences", "app.hide", "app.quit"]);
        for (menu, gone) in [("File", "app.quit"), ("Tools", "app.preferences"), ("Help", "app.about")] {
            let o = other.iter().find(|m| m.key == menu).unwrap();
            let m = mac.iter().find(|m| m.key == menu).unwrap();
            assert!(ids(o).contains(&gone) && !ids(m).contains(&gone), "{menu}");
            assert!(!matches!(m.children.last(), Some(Node::Separator)), "{menu} ends with an item on macOS");
        }
        // Ctrl is written Cmd; Redo has its own macOS key.
        let shortcut = |menus: &[MenuNode], id: &str| find(menus, &Lookup::Id { id: id.into(), params: Value::Null }).unwrap().shortcut.clone();
        assert_eq!(shortcut(&other, "album.new").as_deref(), Some("Ctrl+N"));
        assert_eq!(shortcut(&mac, "album.new").as_deref(), Some("Cmd+N"));
        assert_eq!(shortcut(&other, "edit.redo").as_deref(), Some("Ctrl+Y"));
        assert_eq!(shortcut(&mac, "edit.redo").as_deref(), Some("Cmd+Shift+Z"));
        assert_eq!(shortcut(&mac, "photo.rename").as_deref(), Some("F2"));
    }

    #[test]
    fn a_rebinding_shows_in_the_menu_and_a_removed_key_shows_none() {
        let (_dir, mut s, _) = session_with_photos(1);
        s.execute("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": "Ctrl+Shift+8" })).unwrap();
        let mut app = App::new(Some(s));
        let star = |app: &App| {
            find(&bar(app, Platform::Other), &Lookup::Id { id: "photo.star.toggle".into(), params: Value::Null }).unwrap().shortcut.clone()
        };
        assert_eq!(star(&app).as_deref(), Some("Ctrl+Shift+8"));
        assert_eq!(
            find(&bar(&app, Platform::Mac), &Lookup::Id { id: "photo.star.toggle".into(), params: Value::Null }).unwrap().shortcut.as_deref(),
            Some("Cmd+Shift+8")
        );
        app.run("app.setShortcut", &json!({ "command": "photo.star.toggle", "context": "library", "key": null })).unwrap();
        assert_eq!(star(&app), None, "a removed key is not shown");
        app.run("app.resetShortcuts", &json!({})).unwrap();
        assert_eq!(star(&app).as_deref(), Some("Ctrl+8"));
    }

    /// The UI commands' defaults join the engine's in one conflict check, on every platform:
    /// Ctrl+1 is Small Thumbnails in Library and may mean something else in Edit view.
    #[test]
    fn no_default_conflicts_with_ui_commands_on_any_platform() {
        use dalica_engine::keymap::{Keymap, Platform as KeyPlatform};
        let specs = dalica_engine::cmd::specs();
        for p in KeyPlatform::ALL {
            let mut map = Keymap::from_specs_on(&specs, p).unwrap();
            map.add_commands(crate::keys::ui_keys(), p).unwrap();
            assert_eq!(map.conflicts(), [], "{p:?}");
            assert!(UI_COMMANDS.iter().all(|c| map.knows(c.id)), "{p:?}");
            for (id, ctx, k) in [
                ("window.fullscreen", Context::Global, "F11"),
                ("view.thumbsSmall", Context::Library, "Ctrl+1"),
                ("view.thumbsNormal", Context::Library, "Ctrl+2"),
                ("pane.toggle", Context::Global, "Ctrl+0"),
                ("select.none", Context::Library, "Ctrl+D"),
            ] {
                assert_eq!(map.keys_for(id, ctx), [dalica_engine::Key::parse(k).unwrap()], "{id} on {p:?}");
            }
        }
        // Every catalog shortcut of a UI command is its first default key (its first row, whatever
        // that row's parameters).
        let menus = bar(&App::new(None), Platform::Other);
        let rows: Vec<&ItemNode> = menus.iter().flat_map(|m| items(&m.children)).collect();
        for c in UI_COMMANDS.iter().filter(|c| !c.menu.is_empty()) {
            let (_, first) = c.keys.first().copied().unwrap_or((Context::Global, ""));
            let row = rows.iter().find(|r| r.id == c.id).unwrap_or_else(|| panic!("{} has a menu path but no row", c.id));
            assert_eq!(row.shortcut.as_deref().unwrap_or(""), first, "{}", c.id);
        }
    }

    /// The done-when of M3.4: rebinding a UI command with `app.setShortcut` changes its menu item.
    #[test]
    fn rebinding_a_ui_command_changes_its_menu_item() {
        let (_dir, s, _) = session_with_photos(1);
        let mut app = App::new(Some(s));
        let shown =
            |app: &App, p: Platform, id: &str| find(&bar(app, p), &Lookup::Id { id: id.into(), params: Value::Null }).unwrap().shortcut.clone();
        assert_eq!(shown(&app, Platform::Other, "view.thumbsSmall").as_deref(), Some("Ctrl+1"));
        assert_eq!(shown(&app, Platform::Other, "window.fullscreen").as_deref(), Some("F11"));
        app.run("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": "Ctrl+Shift+1" })).unwrap();
        app.run("app.setShortcut", &json!({ "command": "window.fullscreen", "context": "global", "key": "F12" })).unwrap();
        assert_eq!(shown(&app, Platform::Other, "view.thumbsSmall").as_deref(), Some("Ctrl+Shift+1"));
        assert_eq!(shown(&app, Platform::Mac, "view.thumbsSmall").as_deref(), Some("Cmd+Shift+1"));
        assert_eq!(shown(&app, Platform::Other, "window.fullscreen").as_deref(), Some("F12"));
        // A conflicting key is refused and the menu keeps the old one.
        let r = app.run("app.setShortcut", &json!({ "command": "view.thumbsNormal", "context": "library", "key": "Ctrl+Shift+1" }));
        assert!(matches!(&r, Err(CmdError::BadParams(m)) if m.contains("already view.thumbsSmall")), "{r:?}");
        assert_eq!(shown(&app, Platform::Other, "view.thumbsNormal").as_deref(), Some("Ctrl+2"));
        app.run("app.setShortcut", &json!({ "command": "view.thumbsSmall", "context": "library", "key": null })).unwrap();
        assert_eq!(shown(&app, Platform::Other, "view.thumbsSmall"), None, "a removed key is not shown");
        app.run("app.resetShortcuts", &json!({})).unwrap();
        assert_eq!(shown(&app, Platform::Other, "view.thumbsSmall").as_deref(), Some("Ctrl+1"));
        assert_eq!(shown(&app, Platform::Other, "window.fullscreen").as_deref(), Some("F11"));
        // A session handed over later learns the UI commands too.
        let (_dir2, s2, _) = session_with_photos(1);
        let mut later = App::new(None);
        later.set_session(s2);
        later.run("app.setShortcut", &json!({ "command": "pane.toggle", "context": "global", "key": "Ctrl+Alt+0" })).unwrap();
        let key = dalica_engine::Key::parse("Ctrl+Alt+0").unwrap();
        assert_eq!(later.session().unwrap().keymap().resolve(&key, Context::Library, false).as_deref(), Some("pane.toggle"));
    }

    #[test]
    fn view_state_commands_set_the_state_and_refuse_parameters() {
        let mut app = App::new(None);
        let radio = |app: &App, id: &str| find(&bar(app, Platform::Other), &Lookup::Id { id: id.into(), params: Value::Null }).unwrap().checked;
        assert!(radio(&app, "view.thumbsNormal") && !radio(&app, "view.thumbsSmall"));
        assert_eq!(app.run("view.thumbsSmall", &json!({})).unwrap(), json!({ "thumbSize": 96.0 }));
        assert_eq!(app.state().thumb_size, THUMB_SMALL);
        assert!(radio(&app, "view.thumbsSmall") && !radio(&app, "view.thumbsNormal"));
        app.run("view.thumbsNormal", &Value::Null).unwrap();
        assert_eq!(app.state().thumb_size, THUMB_DEFAULT);
        assert_eq!(app.run("pane.toggle", &json!({})).unwrap(), json!({ "pane": "properties" }));
        assert_eq!(app.state().pane, Some(Pane::Properties));
        app.state_mut().pane = Some(Pane::Tags);
        app.run("pane.toggle", &json!({})).unwrap();
        assert_eq!(app.state().pane, None);
        assert_eq!(app.run("window.fullscreen", &json!({})).unwrap(), json!({ "fullScreen": true }));
        assert!(app.state().full_screen);
        app.run("window.fullscreen", &json!({})).unwrap();
        assert!(!app.state().full_screen);
        let before = app.state().clone();
        for id in ["view.thumbsSmall", "view.thumbsNormal", "pane.toggle", "window.fullscreen", "select.none"] {
            for params in [json!({ "size": 96 }), json!([1]), json!("x"), json!(7), json!(true)] {
                assert!(matches!(app.run(id, &params), Err(CmdError::BadParams(_))), "{id} {params}");
            }
        }
        let after = app.state().clone();
        assert_eq!(UiState { status: String::new(), ..after }, UiState { status: String::new(), ..before }, "nothing changed");
    }

    #[test]
    fn the_album_variant_retitles_the_fourth_menu_and_swaps_its_commands() {
        let app = App::new(None);
        let facts = Facts { app: &app, platform: Platform::Other, kind: SectionKind::Album, specs: dalica_engine::cmd::specs() };
        let m = menu_node(&facts, &MENU_BAR[3]);
        assert_eq!((m.key, m.title), ("Folder", "Album"));
        let all = items(&m.children);
        assert!(all.iter().any(|i| i.id == "album.delete") && !all.iter().any(|i| i.id == "folder.delete"));
        assert_eq!(section_kind(&app), SectionKind::Folder, "every section is a folder until albums exist");
        assert_eq!(bar(&app, Platform::Other)[3].title, "Folder");
    }

    #[test]
    fn paths_split_at_slashes_with_escapes() {
        assert_eq!(split_path("Picture/Star \\/ Unstar").unwrap(), ["Picture", "Star / Unstar"]);
        assert_eq!(split_path("Picture/Batch Edit/Rotate Clockwise").unwrap(), ["Picture", "Batch Edit", "Rotate Clockwise"]);
        assert_eq!(split_path("a\\\\b").unwrap(), ["a\\b"]);
        for bad in ["", "/", "File/", "/File", "File//Save", "File\\x", "File\\", "  "] {
            assert!(split_path(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn ids_with_parameters_find_their_variant() {
        let app = App::new(None);
        let menus = bar(&app, Platform::Other);
        let date = find(&menus, &Lookup::Id { id: "view.caption".into(), params: json!({ "caption": "date" }) }).unwrap();
        assert_eq!((date.label.as_str(), date.widget.as_str()), ("Date", "menuitem:view.caption/date"));
        let first = find(&menus, &Lookup::Id { id: "view.caption".into(), params: Value::Null });
        assert!(first.is_none(), "a variant row needs its parameters");
        let path = find(&menus, &Lookup::Path(split_path("Picture/Batch Edit/Rotate Clockwise").unwrap())).unwrap();
        assert_eq!(path.widget, "menuitem:photo.rotate.cw/batch");
        assert!(find(&menus, &Lookup::Path(vec!["Picture".into(), "Batch Edit".into()])).is_none(), "a submenu is not an item");
        assert!(find(&menus, &Lookup::Path(vec!["Nope".into()])).is_none());
        assert!(find(&menus, &Lookup::Path(vec![])).is_none());
        assert!(find(&menus, &Lookup::Path(vec!["File".into(), "New Album".into()])).is_some(), "a trailing … is optional");
        assert!(find(&menus, &Lookup::Path(vec!["File".into(), "New Album...".into()])).is_some());
    }

    #[test]
    fn room_keeps_a_level_below_the_bar_and_inside_the_window() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(900.0, 600.0));
        assert_eq!(room(screen, 24.0, 10.0), 600.0 - EDGE - 24.0 - EDGE - 10.0);
        assert_eq!(room(screen, 0.0, 10.0), 600.0 - 2.0 * EDGE - 10.0);
        assert_eq!(room(screen, f32::NAN, 10.0), room(screen, 0.0, 10.0));
        assert_eq!(room(screen, 5000.0, 10.0), 4.0 * ARROW, "never less than the arrows need");
        assert_eq!(room(Rect::NOTHING, 24.0, 10.0), 4.0 * ARROW);
        assert_eq!(room(screen, -100.0, 10.0), room(screen, 0.0, 10.0));
    }
}
