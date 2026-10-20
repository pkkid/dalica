//! Keyboard-only use of the window (M15.29, `ui-layout.md` → Keyboard and screen readers), headless:
//! F10 and Alt open the menu bar and the arrows walk it; every enabled row of every menu is reached
//! from the keyboard and Enter runs the focused one; every shortcut is a key a keyboard can press;
//! Tab and Shift+Tab walk the window's controls region by region in reading order, and cycle each
//! dialog's controls in reading order without leaving it; a focused control takes its own Enter
//! and Space; a focused cell leaves the arrows to the selection and the focus follows it.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, LARGE, demo_open, new_driver, widget};
use dalica_ui::a11y::keyboard::reading_order;
use dalica_ui::menus::{self, Node, Platform};
use dalica_ui::state::{Dialog, DialogKind, View};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry::Role;
use egui::{Id, Rect};
use serde_json::json;

fn demo(dir: &Path) -> Driver {
    let session = demo_open(dir, Arc::new(dalica_core::SystemClock));
    let mut d = new_driver(App::new(Some(session)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

/// The automation id of the focused widget; `-` for none, the egui id for an unregistered one.
fn focused(d: &Driver) -> String {
    match d.ctx().memory(|m| m.focused()) {
        None => "-".into(),
        Some(f) => d.widgets().into_iter().find(|w| w.egui_id == Some(f)).map_or_else(|| format!("{f:?}"), |w| w.id),
    }
}

fn press(d: &mut Driver, key: &str) {
    d.key(key).unwrap();
}

/// The bar's menu titles, left to right, with their egui ids.
fn titles(d: &Driver) -> Vec<(String, Id)> {
    d.widgets().into_iter().filter(|w| w.role == Role::Menu && w.id.starts_with("menu:")).filter_map(|w| Some((w.id, w.egui_id?))).collect()
}

/// The bar's menu that is open, by its title's id.
fn open_menu(d: &Driver) -> Option<String> {
    titles(d).into_iter().find(|(_, id)| egui::Popup::is_id_open(d.ctx(), id.with("popup"))).map(|(name, _)| name)
}

#[test]
fn f10_opens_the_menu_bar_and_the_arrows_walk_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    assert_eq!(open_menu(&d), None);
    press(&mut d, "F10");
    assert_eq!((open_menu(&d).as_deref(), focused(&d).as_str()), (Some("menu:File"), "menu:File"));
    press(&mut d, "Right");
    assert_eq!((open_menu(&d).as_deref(), focused(&d).as_str()), (Some("menu:Edit"), "menu:Edit"));
    press(&mut d, "Left");
    press(&mut d, "Left");
    assert_eq!(open_menu(&d).as_deref(), Some("menu:Help"), "Left wraps round to the last menu");
    press(&mut d, "Right");
    assert_eq!(open_menu(&d).as_deref(), Some("menu:File"), "and Right back to the first");
    let rows: Vec<String> = d.widgets().into_iter().filter(|w| w.role == Role::MenuItem && w.enabled).map(|w| w.id).collect();
    assert!(rows.len() > 3, "the File menu's rows: {rows:?}");
    press(&mut d, "Down");
    assert_eq!(focused(&d), rows[0], "Down goes to the first row");
    press(&mut d, "Down");
    assert_eq!(focused(&d), rows[1]);
    press(&mut d, "Up");
    press(&mut d, "Up");
    assert_eq!(&focused(&d), rows.last().unwrap(), "Up wraps to the last row");
    assert_eq!(d.state().view, View::Library, "the arrows moved no photo selection");
    assert!(d.state().selection.ids.is_empty());
    press(&mut d, "Esc");
    assert_eq!(open_menu(&d), None, "Esc closes the menu");
    press(&mut d, "F10");
    assert!(open_menu(&d).is_some());
    press(&mut d, "F10");
    assert_eq!(open_menu(&d), None, "F10 closes it again");
}

#[cfg(not(target_os = "macos"))]
#[test]
fn alt_alone_opens_the_menu_bar_and_alt_with_a_key_does_not() {
    use crate::accessibility::frame;
    use egui::{Event, Modifiers};
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let alt = Modifiers::ALT;
    frame(&mut d, vec![Event::ModifiersChanged(alt)]);
    frame(&mut d, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    d.step();
    assert_eq!(open_menu(&d).as_deref(), Some("menu:File"), "Alt pressed and released alone");
    press(&mut d, "Esc");
    assert_eq!((open_menu(&d), focused(&d).as_str()), (None, "menu:File"), "Esc closes the menu, the title keeps the focus");
    press(&mut d, "Esc");
    assert_eq!(focused(&d), "-", "a second Esc leaves the menu bar");
    // Alt with another key is that key's chord (Alt+Enter: Properties), not the menu bar.
    let enter = |pressed| Event::Key { key: egui::Key::Enter, physical_key: None, pressed, repeat: false, modifiers: alt };
    frame(&mut d, vec![Event::ModifiersChanged(alt)]);
    frame(&mut d, vec![enter(true)]);
    frame(&mut d, vec![enter(false)]);
    frame(&mut d, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    d.step();
    assert_eq!(open_menu(&d), None, "Alt+Enter opens no menu");
    // Ctrl+Alt (the quick preview's hold) neither.
    let ctrl_alt = Modifiers { ctrl: true, command: true, ..alt };
    frame(&mut d, vec![Event::ModifiersChanged(alt)]);
    frame(&mut d, vec![Event::ModifiersChanged(ctrl_alt)]);
    frame(&mut d, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    d.step();
    assert_eq!(open_menu(&d), None, "Ctrl+Alt opens no menu");
}

/// Every enabled row of `nodes` (submenus' rows too) and every submenu, by widget id.
fn enabled_rows(nodes: &[Node], out: &mut BTreeSet<String>) {
    for n in nodes {
        match n {
            Node::Item(i) if i.enabled => {
                out.insert(i.widget.clone());
            }
            Node::Submenu { widget, children, .. } => {
                out.insert(widget.clone());
                enabled_rows(children, out);
            }
            _ => {}
        }
    }
}

#[test]
fn every_enabled_menu_row_is_reached_from_the_keyboard_and_enter_runs_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.settle(FRAMES).unwrap();
    let bar = menus::bar(d.app(), Platform::Other);
    let mut missed = Vec::new();
    for (i, m) in bar.iter().enumerate() {
        let mut want = BTreeSet::new();
        enabled_rows(&m.children, &mut want);
        press(&mut d, "F10");
        for _ in 0..i {
            press(&mut d, "Right");
        }
        assert_eq!(open_menu(&d), Some(format!("menu:{}", m.key)));
        let mut seen: Vec<String> = Vec::new();
        let mut opened = BTreeSet::new();
        for _ in 0..400 {
            press(&mut d, "Down");
            let at = focused(&d);
            // A submenu's row opens it with Enter; its rows follow it.
            if at.starts_with("submenu:") && opened.insert(at.clone()) {
                press(&mut d, "Enter");
                seen.push(at);
                continue;
            }
            if seen.first() == Some(&at) {
                break;
            }
            seen.push(at);
        }
        let seen: BTreeSet<String> = seen.into_iter().collect();
        missed.extend(want.difference(&seen).map(|w| format!("{}: {w}", m.key)));
        press(&mut d, "Esc");
        d.settle(FRAMES).unwrap();
        assert_eq!(open_menu(&d), None);
    }
    assert!(missed.is_empty(), "rows the keyboard never reached:\n{}", missed.join("\n"));

    // Help ▸ About Dalica…: Down to it, Enter runs it.
    press(&mut d, "F10");
    press(&mut d, "Left");
    assert_eq!(open_menu(&d).as_deref(), Some("menu:Help"));
    for _ in 0..40 {
        press(&mut d, "Down");
        if focused(&d) == "menuitem:app.about" {
            break;
        }
    }
    assert_eq!(focused(&d), "menuitem:app.about");
    press(&mut d, "Enter");
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().dialogs.last().map(|x| x.kind), Some(DialogKind::About), "Enter ran the focused row");
    assert_eq!(open_menu(&d), None);
}

#[test]
fn every_shortcut_is_a_key_a_keyboard_can_press() {
    let dir = tempfile::tempdir().unwrap();
    let d = demo(dir.path());
    let keymap = d.app().session().unwrap().keymap();
    let bindings = keymap.bindings();
    assert!(bindings.len() > 100, "{} bindings", bindings.len());
    let mut bad = Vec::new();
    for b in &bindings {
        let name = match b.key.code.as_str() {
            "Esc" => "Escape",
            "Plus" => "+",
            other => other,
        };
        let Some(key) = egui::Key::from_name(name) else {
            bad.push(format!("{} ({}): no key {:?}", b.command, b.key, b.key.code));
            continue;
        };
        let modifiers = dalica_ui::headless::modifiers(b.key.ctrl, b.key.shift, b.key.alt, false);
        if dalica_ui::keys::from_egui(key, modifiers).as_ref() != Some(&b.key) {
            bad.push(format!("{} ({}): pressed, it reads {:?}", b.command, b.key, dalica_ui::keys::from_egui(key, modifiers)));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// The window's regions of the last frame, in reading order.
fn regions(d: &Driver) -> Vec<(String, Rect)> {
    let rects = d
        .widgets()
        .into_iter()
        .filter(|w| w.id.starts_with("region:"))
        .map(|w| (w.id.clone(), Rect::from_min_size(egui::pos2(w.rect[0], w.rect[1]), egui::vec2(w.rect[2], w.rect[3]))))
        .collect();
    reading_order(rects)
}

/// Tab (or Shift+Tab) round the controls once, from where the focus is: the stops in order, with
/// their egui rects (unclipped) from the last frame.
fn walk(d: &mut Driver, key: &str) -> Vec<(String, Rect)> {
    let mut stops: Vec<(String, Rect)> = Vec::new();
    for _ in 0..600 {
        press(d, key);
        let at = focused(d);
        if stops.first().is_some_and(|s| s.0 == at) {
            return stops;
        }
        let id = d.ctx().memory(|m| m.focused()).unwrap();
        let rect = d.ctx().viewport(|v| v.prev_pass.widgets.get(id).map(|w| w.rect)).unwrap_or(Rect::NOTHING);
        stops.push((at, rect));
    }
    panic!("Tab never came round: {stops:?}");
}

#[test]
fn tab_walks_the_windows_controls_region_by_region_in_reading_order_and_shift_tab_walks_back() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    d.run("pane.properties", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let regions = regions(&d);
    let region_of = |r: Rect| regions.iter().position(|(_, rr)| rr.contains(r.center())).unwrap_or(regions.len());
    let stops = walk(&mut d, "Tab");
    assert_eq!(stops.first().map(|s| s.0.as_str()), Some("menu:File"), "Tab starts at the menu bar");
    let order: Vec<usize> = stops.iter().map(|(_, r)| region_of(*r)).collect();
    assert!(order.windows(2).all(|w| w[0] <= w[1]), "region by region: {:?}", stops.iter().zip(&order).collect::<Vec<_>>());
    let visited: BTreeSet<&str> = order.iter().filter_map(|i| regions.get(*i)).map(|(n, _)| n.as_str()).collect();
    for region in
        ["region:menuBar", "region:toolbar", "region:tree", "region:lightTable", "region:sidePane", "region:bottomControls", "region:actionButtons"]
    {
        assert!(visited.contains(region), "Tab reaches {region}: {visited:?}");
    }
    // Within a region, rows top to bottom: never back up by more than half a row.
    for w in stops.windows(2) {
        if region_of(w[0].1) == region_of(w[1].1) {
            assert!(
                w[1].1.top() >= w[0].1.top() - w[0].1.height() / 2.0 || w[1].1.left() > w[0].1.left(),
                "Tab goes back up from {:?} to {:?}",
                w[0],
                w[1]
            );
        }
    }
    // Shift+Tab walks the same stops the other way.
    let back = walk(&mut d, "Shift+Tab");
    // The focus sits on the first stop: Shift+Tab goes to the last, back round to the first.
    let mut reversed: Vec<&str> = back.iter().map(|s| s.0.as_str()).collect();
    reversed.reverse();
    let forward: Vec<&str> = stops.iter().map(|s| s.0.as_str()).collect();
    assert_eq!(reversed, forward, "Shift+Tab is Tab backwards");
}

/// Plants `home` with `folders` empty folders as the session's home (its only standard place
/// that exists), so the Folder Manager's tree does not depend on the machine's folders.
fn plant_home(d: &mut Driver, home: &Path, folders: usize) {
    for i in 0..folders {
        std::fs::create_dir_all(home.join(format!("Folder {i:03}"))).unwrap();
    }
    d.app_mut().session_mut().unwrap().set_places(dalica_scan::Places::under(home), Vec::new());
}

/// Expands the open Folder Manager's Home root and waits until its `folders` folders are listed.
fn expand_home(d: &mut Driver, home: &Path, folders: usize) {
    use dalica_ui::dialogs::folder_manager::{expand_id, row_id};
    d.click(&expand_id(home, home)).unwrap();
    let rows = |d: &Driver| d.widgets().iter().filter(|w| w.role == Role::TreeRow).count();
    for _ in 0..400 {
        if rows(d) > 10 {
            break;
        }
        d.step();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    d.settle(FRAMES).unwrap();
    assert!(rows(d) > 10, "Home's {folders} folders are listed");
    let last = home.join(format!("Folder {:03}", folders - 1));
    assert!(d.widgets().iter().all(|w| w.id != row_id(home, &last)), "the tree is taller than its well");
}

#[test]
fn tab_cycles_every_dialogs_controls_in_reading_order_without_leaving_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    // The Folder Manager's tree taller than its well, whatever folders the machine has: a planted
    // home of 300 folders, expanded once the dialog opens.
    let home = dir.path().join("home");
    plant_home(&mut d, &home, 300);
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    let mut problems = Vec::new();
    let mut walked = 0;
    for kind in DialogKind::ALL {
        d.app_mut().state_mut().dialogs.clear();
        d.settle(FRAMES).unwrap();
        if dalica_ui::dialogs::open(d.app_mut(), Dialog::new(kind)).is_err() {
            continue;
        }
        d.settle(FRAMES).unwrap();
        if d.state().dialogs.last().map(|x| x.kind) != Some(kind) {
            continue;
        }
        if kind == DialogKind::FolderManager {
            expand_home(&mut d, &home, 300);
        }
        let Some(dialog) = d.widgets().into_iter().find(|w| w.id == format!("dialog:{}", kind.name())) else {
            problems.push(format!("{kind:?}: not drawn"));
            continue;
        };
        let bounds = Rect::from_min_size(egui::pos2(dialog.rect[0], dialog.rect[1]), egui::vec2(dialog.rect[2], dialog.rect[3]));
        // The dialog's registered controls that take the focus: on its layer.
        let layer = d.ctx().viewport(|v| dialog.egui_id.and_then(|id| v.prev_pass.widgets.get(id)).map(|w| w.layer_id));
        let named: HashMap<Id, String> = d.widgets().into_iter().filter(|w| w.enabled).filter_map(|w| Some((w.egui_id?, w.id))).collect();
        let takes: BTreeSet<String> = d.ctx().viewport(|v| {
            v.prev_pass
                .widgets
                .layers()
                .flat_map(|(_, l)| l.iter())
                .filter(|w| Some(w.layer_id) == layer && w.enabled && w.sense.is_focusable() && bounds.contains(w.rect.center()))
                .filter_map(|w| named.get(&w.id).cloned())
                .collect()
        });
        if takes.is_empty() {
            problems.push(format!("{kind:?}: no control takes the focus"));
            continue;
        }
        walked += 1;
        let stops = walk(&mut d, "Tab");
        let ids: BTreeSet<String> = stops.iter().map(|s| s.0.clone()).collect();
        let missed: Vec<&String> = takes.difference(&ids).collect();
        if !missed.is_empty() {
            problems.push(format!("{kind:?}: Tab never reaches {missed:?}"));
        }
        // A stop registered outside the dialog left it (an unregistered one is a control scrolled
        // out of the dialog's body, which has no automation id while hidden).
        let foreign: Vec<&String> = stops.iter().map(|s| &s.0).filter(|s| !takes.contains(*s) && !s.starts_with("Id")).collect();
        if !foreign.is_empty() {
            problems.push(format!("{kind:?}: Tab leaves the dialog for {foreign:?}"));
        }
        // Reading order, from wherever the walk started (the dialog may give a field the focus):
        // one wrap from the last control back to the first, no other step back.
        let mut wraps = Vec::new();
        for w in stops.windows(2) {
            let (a, b) = (w[0].1, w[1].1);
            if b.top() < a.top() - a.height() / 2.0 || (b.top() < a.center().y && b.left() < a.left()) {
                wraps.push(format!("{} to {}", w[0].0, w[1].0));
            }
        }
        let first = stops.first().map(|s| s.1);
        let last = stops.last().map(|s| s.1);
        let walk_wraps =
            matches!((last, first), (Some(a), Some(b)) if b.top() < a.top() - a.height() / 2.0 || (b.top() < a.center().y && b.left() < a.left()));
        if wraps.len() + usize::from(walk_wraps) > 1 {
            problems.push(format!("{kind:?}: Tab is not in reading order: steps back from {wraps:?}"));
        }
        let mut back: Vec<String> = walk(&mut d, "Shift+Tab").into_iter().map(|s| s.0).collect();
        back.reverse();
        let forward: Vec<String> = stops.into_iter().map(|s| s.0).collect();
        if back != forward {
            problems.push(format!("{kind:?}: Shift+Tab is not Tab backwards: {back:?} against {forward:?}"));
        }
    }
    d.app_mut().state_mut().dialogs.clear();
    assert!(walked > 30, "{walked} dialogs walked");
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn a_focused_control_takes_its_own_space_and_enter() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let target = "button:filter.starred";
    for _ in 0..60 {
        press(&mut d, "Tab");
        if focused(&d) == target {
            break;
        }
    }
    assert_eq!(focused(&d), target);
    assert!(!widget(&d, target).active);
    press(&mut d, "Space");
    d.settle(FRAMES).unwrap();
    assert!(widget(&d, target).active, "Space toggled the focused filter");
    press(&mut d, "Enter");
    d.settle(FRAMES).unwrap();
    assert!(!widget(&d, target).active, "Enter toggled it back");
    assert_eq!(d.state().view, View::Library, "Enter was the filter's, not View and Edit's");
}

#[test]
fn arrows_on_a_focused_cell_move_the_selection_the_focus_follows_and_shortcuts_still_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    for _ in 0..80 {
        press(&mut d, "Tab");
        if focused(&d) == "thumb:1" {
            break;
        }
    }
    assert_eq!(focused(&d), "thumb:1");
    // Nothing selected: Right selects the section's first photo, then the next one.
    press(&mut d, "Right");
    press(&mut d, "Right");
    d.settle(FRAMES).unwrap();
    assert_eq!(d.state().selection.ids.iter().map(|i| i.get()).collect::<Vec<_>>(), [2], "Right selected the next photo");
    assert_eq!(focused(&d), "thumb:2", "the focus followed");
    press(&mut d, "Down");
    d.settle(FRAMES).unwrap();
    let anchor = d.state().selection.anchor.unwrap().get();
    assert_eq!(focused(&d), format!("thumb:{anchor}"), "and Down too");
    assert_ne!(anchor, 2);
    // The view's shortcuts still work with a cell focused (only a text field takes them).
    press(&mut d, "Ctrl+8");
    let starred = d.app().session().unwrap().library().photo(dalica_core::PhotoId::new(anchor)).unwrap().star;
    assert!(starred, "Ctrl+8 starred the selected photo");
}

/// The keyboard's marks, painted for a look at 1400 × 900 and 900 × 600: the File menu opened with
/// F10 and walked to a row, and a dialog's button reached with Tab (`keyboard-*.png` in the test
/// output and `DALICA_SHOTS`).
#[test]
fn the_keyboards_focus_is_painted_for_a_look() {
    for (w, h) in [LARGE, crate::common::SMALL] {
        let dir = tempfile::tempdir().unwrap();
        let session = demo_open(dir.path(), Arc::new(dalica_core::SystemClock));
        let mut d = new_driver(App::new(Some(session)), w, h, 1.0);
        d.settle(FRAMES).unwrap();
        press(&mut d, "F10");
        press(&mut d, "Down");
        press(&mut d, "Down");
        d.settle(FRAMES).unwrap();
        crate::common::shot(&d, &format!("keyboard-menu-{w}x{h}"));
        press(&mut d, "Esc");
        press(&mut d, "Esc");
        d.run("app.preferences", &json!({})).unwrap();
        d.settle(FRAMES).unwrap();
        for _ in 0..60 {
            press(&mut d, "Tab");
            if focused(&d) == "button:dialog.cancel" {
                break;
            }
        }
        d.settle(FRAMES).unwrap();
        crate::common::shot(&d, &format!("keyboard-dialog-{w}x{h}"));
    }
}

#[test]
fn tab_into_a_dialog_bodys_lower_rows_scrolls_them_into_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    d.run("app.preferences", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    let row = "check:option.files.writeDates";
    assert!(d.widgets().iter().all(|w| w.id != row), "{row} starts out of sight");
    for _ in 0..80 {
        press(&mut d, "Tab");
        if focused(&d) == row || focused(&d).contains(row) {
            break;
        }
    }
    d.settle(FRAMES).unwrap();
    let shown = widget(&d, row);
    let dialog = widget(&d, "dialog:options");
    let (r, b) = (shown.rect, dialog.rect);
    assert!(r[1] >= b[1] && r[1] + r[3] <= b[1] + b[3], "the focused row is scrolled into the dialog: {r:?} in {b:?}");
    assert_eq!(focused(&d), row);
}

#[test]
fn in_the_edit_view_a_focused_control_keeps_the_views_shortcuts() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let first = d.app().session().unwrap().library().photos().next().unwrap().id;
    d.run("select.set", &json!({ "ids": [first.get()] })).unwrap();
    d.run("view.edit", &json!({})).unwrap();
    d.settle(FRAMES).unwrap();
    for _ in 0..80 {
        press(&mut d, "Tab");
        if focused(&d) == "tool:crop" {
            break;
        }
    }
    assert_eq!(focused(&d), "tool:crop");
    press(&mut d, "Ctrl+8");
    d.settle(FRAMES).unwrap();
    assert!(d.app().session().unwrap().library().photo(first).unwrap().star, "Ctrl+8 starred the photo with a tool button focused");
    assert_eq!(focused(&d), "tool:crop");
}

/// The library tree's row with photos that Home (`first`) or End goes to, by automation id.
fn end_row(d: &Driver, first: bool) -> String {
    let model = dalica_ui::panels::folders::current_model(d.ctx());
    model.step(None, first).and_then(|i| model.row(i)).map(|r| r.id.clone()).unwrap()
}

/// Whether the widget `id` is drawn whole inside the tree pane.
fn in_pane(d: &Driver, id: &str) -> bool {
    let pane = widget(d, "region:tree").rect;
    d.widgets().iter().any(|w| w.id == id && w.rect[1] >= pane[1] - 0.5 && w.rect[1] + w.rect[3] <= pane[1] + pane[3] + 0.5)
}

#[test]
fn a_tree_taller_than_its_pane_is_one_tab_stop_whose_arrows_reach_every_row() {
    use dalica_engine::synthetic::{self, Spec};
    let dir = tempfile::tempdir().unwrap();
    let s = synthetic::open(dir.path(), &Spec::new(1_500, 150, synthetic::SEED), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    let info = dalica_ui::panels::folders::info(d.ctx());
    assert!(info.height > 3.0 * info.view_h, "the tree is taller than its pane: {info:?}");
    let is_row = |d: &Driver, id: &str| d.widgets().iter().any(|w| w.id == id && w.role == Role::TreeRow);

    // Tab enters the tree once, and comes round.
    let stops = walk(&mut d, "Tab");
    let rows: Vec<&str> = stops.iter().map(|s| s.0.as_str()).filter(|s| is_row(&d, s)).collect();
    assert_eq!(rows.len(), 1, "the tree's rows are one stop: {rows:?}");
    let first = end_row(&d, true);
    assert_eq!(rows[0], first, "nothing selected: the first row");

    // Into the tree: End and Home reach its last and first rows, scrolled into view.
    for _ in 0..80 {
        press(&mut d, "Tab");
        if focused(&d) == first {
            break;
        }
    }
    assert_eq!(focused(&d), first);
    press(&mut d, "End");
    let last = end_row(&d, false);
    assert_eq!(focused(&d), last, "End: the last row");
    assert!(in_pane(&d, &last), "the last row is scrolled into view");
    assert!(dalica_ui::panels::folders::info(d.ctx()).offset > 0.0);
    press(&mut d, "Home");
    assert_eq!(focused(&d), first, "Home: the first row");
    assert_eq!(dalica_ui::panels::folders::info(d.ctx()).offset, 0.0);
    press(&mut d, "Down");
    let model = dalica_ui::panels::folders::current_model(d.ctx());
    let second = model.step(model.step(None, true), true).and_then(|i| model.row(i)).unwrap();
    assert_eq!(focused(&d), second.id, "Down: the next row");
    assert_eq!(d.state().selection.section, second.section, "the selection follows the focus");
    for _ in 0..40 {
        press(&mut d, "Down");
    }
    let deep = focused(&d);
    assert!(is_row(&d, &deep) && in_pane(&d, &deep), "Down walks past the pane's first screen: {deep}");

    // Tab leaves the tree; with the tree scrolled back to the top, Shift+Tab comes back to the
    // row, scrolled into view again.
    press(&mut d, "Tab");
    assert!(!is_row(&d, &focused(&d)), "Tab left the tree for {}", focused(&d));
    d.scroll(Some("region:tree"), egui::vec2(0.0, 20_000.0)).unwrap();
    d.settle(FRAMES).unwrap();
    assert_eq!(dalica_ui::panels::folders::info(d.ctx()).offset, 0.0);
    assert!(!in_pane(&d, &deep), "{deep} is scrolled out of sight");
    press(&mut d, "Shift+Tab");
    assert_eq!(focused(&d), deep, "Shift+Tab: back to the tree's row");
    assert!(in_pane(&d, &deep), "brought into view");
}

#[test]
fn the_folder_managers_tree_is_one_tab_stop_whose_arrows_walk_and_fold_it() {
    use crate::common::try_widget;
    use dalica_ui::dialogs::folder_manager::row_id;
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
    let home = dir.path().join("home");
    plant_home(&mut d, &home, 300);
    dalica_ui::dialogs::open(d.app_mut(), Dialog::new(DialogKind::FolderManager)).unwrap();
    d.settle(FRAMES).unwrap();
    expand_home(&mut d, &home, 300);
    let root = row_id(&home, &home);
    let folder = |i: usize| row_id(&home, &home.join(format!("Folder {i:03}")));
    for _ in 0..40 {
        press(&mut d, "Tab");
        if focused(&d) == root {
            break;
        }
    }
    assert_eq!(focused(&d), root, "Tab reaches the tree at its first row");
    press(&mut d, "Down");
    assert_eq!(focused(&d), folder(0));
    press(&mut d, "End");
    assert_eq!(focused(&d), folder(299), "End: the last folder, scrolled into view");
    assert!(try_widget(&d, &folder(299)).is_some());
    press(&mut d, "Left");
    assert_eq!(focused(&d), root, "Left on a closed folder: its parent");
    press(&mut d, "Left");
    assert!(try_widget(&d, &folder(0)).is_none(), "Left on an open folder collapses it");
    assert_eq!(focused(&d), root);
    press(&mut d, "Right");
    d.settle(FRAMES).unwrap();
    assert!(try_widget(&d, &folder(0)).is_some(), "Right expands it again");
    press(&mut d, "Right");
    assert_eq!(focused(&d), folder(0), "Right on an open folder: its first subfolder");
    let selected = d.state().dialogs.last().map(|x| dalica_ui::dialogs::folder_manager::draft::Draft::read(&x.fields).selected);
    assert_eq!(selected, Some(Some(home.join("Folder 000").to_string_lossy().into_owned())), "the selection follows the focus");
    // A screen reader is told the focused row: a tree item in the tree, with its level, place and
    // state.
    let t = crate::accessibility::tree(&mut d);
    let (id, node) = t.widget(&d, &folder(0)).unwrap();
    assert_eq!(t.update.focus, id, "the focused row is the focused node");
    assert_eq!(
        (node.role(), node.level(), node.position_in_set(), node.size_of_set()),
        (egui::accesskit::Role::TreeItem, Some(2), Some(1), Some(300))
    );
    assert_eq!(node.is_selected(), Some(true));
    assert_eq!(t.ancestor(id, egui::accesskit::Role::Tree).and_then(|n| n.label()), Some("Folders"));
    let (_, root_node) = t.widget(&d, &root).unwrap();
    assert_eq!((root_node.level(), root_node.is_expanded()), (Some(1), Some(true)));
    // Tab leaves the tree for the radios; Shift+Tab comes back to the same row.
    press(&mut d, "Tab");
    assert!(!focused(&d).starts_with("choice:dialog.folder/"), "Tab left the tree for {}", focused(&d));
    press(&mut d, "Shift+Tab");
    assert_eq!(focused(&d), folder(0));
    d.app_mut().state_mut().dialogs.clear();
}
