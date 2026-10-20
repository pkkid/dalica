//! Keyboard-only use of the window (M15.29, `ui-layout.md` → Keyboard and screen readers), headless:
//! F10 and Alt open the menu bar and the arrows walk it; every enabled row of every menu is reached
//! from the keyboard and Enter runs the focused one; every shortcut is a key a keyboard can press;
//! Tab and Shift+Tab walk the window's controls region by region in reading order, and cycle each
//! dialog's controls in reading order without leaving it; a focused control takes its own Enter
//! and Space; a focused cell leaves the arrows to the selection and the focus follows it.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use crate::accessibility::frame;
use crate::common::{FRAMES, LARGE, demo_open, new_driver, widget};
use dalica_ui::a11y::keyboard::reading_order;
use dalica_ui::menus::{self, Node, Platform};
use dalica_ui::state::{Dialog, DialogKind, View};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry::Role;
use egui::{Event, Id, Modifiers, Rect};
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

#[test]
fn tab_cycles_every_dialogs_controls_in_reading_order_without_leaving_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = demo(dir.path());
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
