//! The macOS menu bar (`design-system.md` §11.1): generated from the same menu model as the
//! in-window menus ([`dalica_ui::menus::bar`] for the Mac layout), so both always list the same
//! items. [`native_bar`] turns the model into a platform-neutral item list, which is tested on every
//! OS; on macOS a thin layer hands that list to `muda` and routes its clicks back through
//! [`dalica_ui::menus::activate`], exactly as a click in the in-window menu runs.
//!
//! Shortcuts stay with Dalica's own keymap on every OS (one dispatcher, context-aware), so the
//! native items carry no key equivalents, except the two the system provides itself: Hide Dalica
//! and Quit Dalica are macOS's own items (Cmd+H, Cmd+Q); quitting closes the window the way its
//! close button does, so the library is flushed first.

// Only macOS shows the native menu; elsewhere this module is its tested model.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use dalica_ui::App;
use dalica_ui::menus::{self, ItemNode, MenuNode, Node, Platform};

/// What a native item does when chosen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Runs the model item with this id through `menus::activate`.
    Command,
    /// macOS's own Hide item.
    Hide,
    /// macOS's own Quit item.
    Quit,
}

/// One row of a native menu.
#[derive(Clone, Debug, PartialEq)]
pub enum NativeItem {
    Item {
        /// The menu's key and the model item's widget id, `"<menu> menuitem:<command id>[/<part>]"`:
        /// unique (one command may be in two menus), so a click maps back to its item.
        id: String,
        label: String,
        enabled: bool,
        /// `Some` for an item with a check mark or radio dot.
        checked: Option<bool>,
        role: Role,
    },
    Separator,
    Submenu {
        label: String,
        children: Vec<NativeItem>,
    },
}

/// One menu of the native bar.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMenu {
    pub title: String,
    pub items: Vec<NativeItem>,
}

/// The native bar for the menu model `menus` (built for `Platform::Mac`).
pub fn native_bar(menus: &[MenuNode]) -> Vec<NativeMenu> {
    menus.iter().map(|m| NativeMenu { title: m.title.to_string(), items: items(m.key, &m.children) }).collect()
}

fn items(menu: &str, nodes: &[Node]) -> Vec<NativeItem> {
    nodes
        .iter()
        .map(|n| match n {
            Node::Separator => NativeItem::Separator,
            Node::Submenu { label, children, .. } => NativeItem::Submenu { label: (*label).to_string(), children: items(menu, children) },
            Node::Item(i) => {
                let role = match i.id {
                    "app.quit" => Role::Quit,
                    "app.hide" => Role::Hide,
                    _ => Role::Command,
                };
                let checked = (i.mark != dalica_ui::menu_catalog::Mark::None).then_some(i.checked);
                // The system's own items always work.
                let enabled = i.enabled || role != Role::Command;
                NativeItem::Item { id: format!("{menu} {}", i.widget), label: i.label.clone(), enabled, checked, role }
            }
        })
        .collect()
}

/// Whether two bars have the same shape (menus, rows, ids, roles, submenu labels), so the native
/// menu can be updated in place instead of rebuilt.
pub fn same_shape(a: &[NativeMenu], b: &[NativeMenu]) -> bool {
    fn rows(a: &[NativeItem], b: &[NativeItem]) -> bool {
        a.len() == b.len()
            && a.iter().zip(b).all(|pair| match pair {
                (NativeItem::Separator, NativeItem::Separator) => true,
                (NativeItem::Submenu { label: la, children: ca }, NativeItem::Submenu { label: lb, children: cb }) => la == lb && rows(ca, cb),
                (NativeItem::Item { id: ia, role: ra, checked: ka, .. }, NativeItem::Item { id: ib, role: rb, checked: kb, .. }) => {
                    ia == ib && ra == rb && ka.is_some() == kb.is_some()
                }
                _ => false,
            })
    }
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.title == y.title && rows(&x.items, &y.items))
}

/// The model item a native item stands for, by its native id (`"<menu> <widget id>"`).
pub fn find_item<'m>(menus: &'m [MenuNode], id: &str) -> Option<&'m ItemNode> {
    fn walk<'m>(nodes: &'m [Node], id: &str) -> Option<&'m ItemNode> {
        nodes.iter().find_map(|n| match n {
            Node::Item(i) if i.widget == id => Some(i),
            Node::Submenu { children, .. } => walk(children, id),
            _ => None,
        })
    }
    let (menu, widget) = id.split_once(' ')?;
    menus.iter().find(|m| m.key == menu).and_then(|m| walk(&m.children, widget))
}

/// Runs the native item `id` (its native id) as a click on it in the in-window menu runs: through
/// [`menus::activate`], so a disabled item is refused with its reason on the status line. `false`
/// when no item of the current model has that id (the menu changed since it was shown).
pub fn run(app: &mut App, id: &str) -> bool {
    let model = menus::bar(app, Platform::Mac);
    let Some(item) = find_item(&model, id).cloned() else { return false };
    let _ = menus::activate(app, &item, &serde_json::Value::Null);
    true
}

/// The label as `muda` takes it: `&` marks a mnemonic there, so a literal one is doubled.
pub fn escape(label: &str) -> String {
    label.replace('&', "&&")
}

#[cfg(target_os = "macos")]
pub use mac::NativeMenuBar;

/// The `muda` layer (macOS only): builds the system menu bar from [`native_bar`]'s list, updates
/// it in place while its shape holds, and hands clicks back as widget ids.
#[cfg(target_os = "macos")]
mod mac {
    use std::collections::HashMap;

    use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    use super::{NativeItem, NativeMenu, Role, escape, same_shape};

    enum Handle {
        Plain(MenuItem),
        Check(CheckMenuItem),
    }

    /// The system menu bar while the window is open.
    pub struct NativeMenuBar {
        menu: Menu,
        shown: Vec<NativeMenu>,
        handles: HashMap<String, Handle>,
    }

    impl NativeMenuBar {
        /// Builds `bar` and installs it as the application's menu bar. Call on the main thread.
        pub fn new(bar: Vec<NativeMenu>) -> NativeMenuBar {
            let (menu, handles) = build(&bar);
            menu.init_for_nsapp();
            NativeMenuBar { menu, shown: bar, handles }
        }

        /// Shows `bar`: labels, enabled and checked states in place when the shape is the same,
        /// else a new menu bar.
        pub fn update(&mut self, bar: Vec<NativeMenu>) {
            if bar == self.shown {
                return;
            }
            if same_shape(&bar, &self.shown) {
                for m in &bar {
                    update_rows(&self.handles, &m.items);
                }
            } else {
                let (menu, handles) = build(&bar);
                self.menu.remove_for_nsapp();
                menu.init_for_nsapp();
                self.menu = menu;
                self.handles = handles;
            }
            self.shown = bar;
        }

        /// The widget ids of the items chosen since the last call.
        pub fn clicked(&self) -> Vec<String> {
            let mut out = Vec::new();
            while let Ok(e) = MenuEvent::receiver().try_recv() {
                out.push(e.id.0);
            }
            out
        }
    }

    fn update_rows(handles: &HashMap<String, Handle>, rows: &[NativeItem]) {
        for r in rows {
            match r {
                NativeItem::Item { id, label, enabled, checked, .. } => match handles.get(id) {
                    Some(Handle::Plain(i)) => {
                        i.set_text(escape(label));
                        i.set_enabled(*enabled);
                    }
                    Some(Handle::Check(i)) => {
                        i.set_text(escape(label));
                        i.set_enabled(*enabled);
                        i.set_checked(checked.unwrap_or(false));
                    }
                    None => {}
                },
                NativeItem::Submenu { children, .. } => update_rows(handles, children),
                NativeItem::Separator => {}
            }
        }
    }

    fn build(bar: &[NativeMenu]) -> (Menu, HashMap<String, Handle>) {
        let menu = Menu::new();
        let mut handles = HashMap::new();
        for m in bar {
            let sub = Submenu::new(escape(&m.title), true);
            fill(&sub, &m.items, &mut handles);
            if let Err(e) = menu.append(&sub) {
                log::warn!("dalica: the menu {} could not be added: {e}", m.title);
            }
        }
        (menu, handles)
    }

    fn fill(sub: &Submenu, rows: &[NativeItem], handles: &mut HashMap<String, Handle>) {
        for r in rows {
            let added = match r {
                NativeItem::Separator => sub.append(&PredefinedMenuItem::separator()),
                NativeItem::Submenu { label, children } => {
                    let child = Submenu::new(escape(label), true);
                    fill(&child, children, handles);
                    sub.append(&child)
                }
                NativeItem::Item { label, role: Role::Quit, .. } => sub.append(&PredefinedMenuItem::quit(Some(label.as_str()))),
                NativeItem::Item { label, role: Role::Hide, .. } => sub.append(&PredefinedMenuItem::hide(Some(label.as_str()))),
                NativeItem::Item { id, label, enabled, checked: Some(on), role: Role::Command } => {
                    let item = CheckMenuItem::with_id(id.as_str(), escape(label), *enabled, *on, None);
                    let added = sub.append(&item);
                    handles.insert(id.clone(), Handle::Check(item));
                    added
                }
                NativeItem::Item { id, label, enabled, checked: None, role: Role::Command } => {
                    let item = MenuItem::with_id(id.as_str(), escape(label), *enabled, None);
                    let added = sub.append(&item);
                    handles.insert(id.clone(), Handle::Plain(item));
                    added
                }
            };
            if let Err(e) = added {
                log::warn!("dalica: a menu item could not be added: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_items(nodes: &[Node], out: &mut Vec<(String, String, bool)>) {
        for n in nodes {
            match n {
                Node::Item(ItemNode { widget, label, enabled, .. }) => out.push((widget.clone(), label.clone(), *enabled)),
                Node::Submenu { label, children, .. } => {
                    out.push((format!("submenu {label}"), String::new(), true));
                    model_items(children, out);
                }
                Node::Separator => out.push(("-".into(), String::new(), true)),
            }
        }
    }

    fn native_items(rows: &[NativeItem], out: &mut Vec<(String, String, bool)>) {
        for r in rows {
            match r {
                NativeItem::Item { id, label, enabled, .. } => out.push((id.clone(), label.clone(), *enabled)),
                NativeItem::Submenu { label, children } => {
                    out.push((format!("submenu {label}"), String::new(), true));
                    native_items(children, out);
                }
                NativeItem::Separator => out.push(("-".into(), String::new(), true)),
            }
        }
    }

    /// The `roadmap.md` M3.6 done-when: the menu model and the macOS menu list the same items, in
    /// the same order, with the same labels and states (the system's own Hide and Quit always on).
    #[test]
    fn the_menu_model_and_the_macos_menu_list_the_same_items() {
        let app = App::new(None);
        let model = menus::bar(&app, Platform::Mac);
        let native = native_bar(&model);
        assert_eq!(native.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), model.iter().map(|m| m.title).collect::<Vec<_>>());
        assert_eq!(native[0].title, "Dalica", "the application menu comes first");
        let mut ids = std::collections::HashSet::new();
        for (m, n) in model.iter().zip(&native) {
            let (mut a, mut b) = (Vec::new(), Vec::new());
            model_items(&m.children, &mut a);
            native_items(&n.items, &mut b);
            assert_eq!(a.len(), b.len(), "{}", m.title);
            for (x, y) in a.iter().zip(&b) {
                let item = x.0.starts_with("menuitem:");
                let native_id = if item { format!("{} {}", m.key, x.0) } else { x.0.clone() };
                assert_eq!((&native_id, &x.1), (&y.0, &y.1), "{}", m.title);
                let system = x.0 == "menuitem:app.quit" || x.0 == "menuitem:app.hide";
                assert_eq!(y.2, x.2 || system, "{}: {}", m.title, y.0);
                if item {
                    assert!(ids.insert(y.0.clone()), "{} is listed twice: a click could not find its item", y.0);
                    // It maps back to the model item it stands for.
                    assert_eq!(find_item(&model, &y.0).map(|i| i.widget.as_str()), Some(x.0.as_str()));
                }
            }
        }
        assert!(ids.len() > 50, "{}", ids.len());
        let quit = native[0].items.iter().find(|i| matches!(i, NativeItem::Item { id, .. } if id == "Dalica menuitem:app.quit"));
        assert!(matches!(quit, Some(NativeItem::Item { role: Role::Quit, enabled: true, .. })));
    }

    #[test]
    fn a_native_click_runs_its_item_like_an_in_window_click() {
        let dir = tempfile::tempdir().unwrap();
        let session = dalica_engine::Session::open(&dir.path().join("library"), std::sync::Arc::new(dalica_core::SystemClock)).unwrap();
        let mut app = App::new(Some(session));
        assert!(run(&mut app, "View menuitem:view.thumbsSmall"));
        assert_eq!(app.state().thumb_size, menus::THUMB_SMALL);
        assert!(run(&mut app, "View menuitem:window.fullscreen"));
        assert!(app.state().full_screen);
        // A disabled item is refused with its reason; an unknown id runs nothing.
        assert!(run(&mut app, "Picture menuitem:photo.star.toggle"));
        assert!(!app.state().status.is_empty());
        for unknown in ["File menuitem:no.such.command", "Nope menuitem:view.thumbsSmall", "menuitem:view.thumbsSmall", ""] {
            assert!(!run(&mut app, unknown), "{unknown}");
        }
    }

    #[test]
    fn a_change_of_state_keeps_the_shape_and_a_new_item_does_not() {
        let app = App::new(None);
        let a = native_bar(&menus::bar(&app, Platform::Mac));
        let mut b = a.clone();
        if let Some(NativeItem::Item { enabled, label, .. }) = b[1].items.iter_mut().find(|i| matches!(i, NativeItem::Item { .. })) {
            *enabled = !*enabled;
            label.push('!');
        }
        assert!(same_shape(&a, &b) && a != b);
        b[1].items.push(NativeItem::Separator);
        assert!(!same_shape(&a, &b));
        assert!(!same_shape(&a, &a[1..]));
        assert_eq!(escape("Tom & Jerry"), "Tom && Jerry");
    }
}
