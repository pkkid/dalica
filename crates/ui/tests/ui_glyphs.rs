//! Every catalogue string the UI draws uses only glyphs its fonts have, so none shows the
//! missing-glyph "?": the options catalogue (tab titles, groups, labels, hints, choices, units and
//! the reasons an option is unavailable), the menu bar (titles, submenus and items, the macOS
//! application menu too), and the commands' labels and menu paths the shortcut editor shows (the
//! engine's and `UI_COMMANDS`). Coverage is egui's, through the fonts `ui-kit` installs, as the
//! key caps' check in `crates/ui-kit/tests/options_widgets.rs` does.

use std::collections::BTreeSet;

use dalica_engine::cmd;
use dalica_engine::options::{self, Kind, Tab};
use dalica_ui::app::UI_COMMANDS;
use dalica_ui::menu_catalog::{Entry, MAC_APP_MENU, MENU_BAR, Menu};
use dalica_ui_kit::{Tokens, fonts};
use egui::Context;

/// Every string of the catalogues, with where it comes from.
fn strings() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut add = |what: String, text: &str| out.push((what, text.to_string()));
    for tab in Tab::ALL {
        add(format!("tab {}", tab.id()), tab.label());
    }
    for o in options::catalogue() {
        let key = o.key;
        add(format!("{key} group"), o.group);
        add(format!("{key} label"), o.label);
        add(format!("{key} hint"), o.hint);
        if let Some(reason) = o.unavailable {
            add(format!("{key} reason"), reason);
        }
        match o.kind {
            Kind::Int { unit, .. } => add(format!("{key} unit"), unit),
            Kind::Choice(choices) => {
                for (id, label) in choices {
                    add(format!("{key} choice {id}"), label);
                }
            }
            Kind::Bool | Kind::Scale { .. } | Kind::Text { .. } | Kind::Folder => {}
        }
    }
    fn entries(what: &str, list: &'static [Entry], add: &mut dyn FnMut(String, &str)) {
        for e in list {
            match e {
                Entry::Item(i) => add(format!("{what} item {}", i.id), i.label),
                Entry::Separator => {}
                Entry::Submenu(label, children) => {
                    add(format!("{what} submenu"), label);
                    entries(&format!("{what} ▸ {label}"), children, add);
                }
            }
        }
    }
    let menus: Vec<&Menu> = MENU_BAR.iter().chain([&MAC_APP_MENU]).collect();
    for m in menus {
        let what = format!("menu {}", m.key);
        add(what.clone(), m.key);
        if let Some(title) = m.album_title {
            add(format!("{what} album title"), title);
        }
        entries(&what, m.entries, &mut add);
        if let Some(album) = m.album_entries {
            entries(&format!("{what} (album)"), album, &mut add);
        }
    }
    for c in cmd::specs() {
        add(format!("command {} label", c.id), c.label);
        add(format!("command {} menu", c.id), c.menu);
    }
    for c in UI_COMMANDS {
        add(format!("UI command {} label", c.id), c.label);
        add(format!("UI command {} menu", c.id), c.menu);
    }
    out
}

#[test]
fn every_catalogue_string_uses_only_glyphs_the_ui_fonts_have() {
    let all = strings();
    assert!(all.iter().any(|(what, _)| what.ends_with("hint")), "the options catalogue is listed");
    assert!(all.iter().any(|(what, _)| what.starts_with("menu ")), "the menus are listed");
    let chars: BTreeSet<char> = all.iter().flat_map(|(_, text)| text.chars()).filter(|c| !c.is_control()).collect();
    let t = Tokens::default();
    let ctx = Context::default();
    fonts::install(&ctx);
    // The fonts are in place from the second frame on.
    ctx.run_ui(dalica_ui_kit::raw_input(100, 100, 1.0), |_| {}).textures_delta.clear();
    let mut lacking = BTreeSet::new();
    ctx.run_ui(dalica_ui_kit::raw_input(100, 100, 1.0), |ui| {
        for font in [fonts::regular(t.body), fonts::semibold(t.body)] {
            // A character the fonts lack is drawn with the replacement glyph, as a surely missing
            // one is ("◻", or "?" when the fonts lack that too: then "?" itself is no loss).
            let uv = |c: char| {
                let g = ui.painter().layout_no_wrap(c.to_string(), font.clone(), egui::Color32::WHITE);
                g.rows.first().and_then(|r| r.glyphs.first()).map(|g| g.uv_rect)
            };
            let none = uv('\u{10FFFD}');
            let replacement = if uv('?') == none { '?' } else { '◻' };
            let missing = |c: char| c != replacement && uv(c) == none;
            assert!(!missing('S') && !missing('›'), "the check sees glyphs the fonts have");
            assert!(missing('\u{10FFFE}'), "the check sees a glyph the fonts lack");
            lacking.extend(chars.iter().copied().filter(|&c| missing(c)));
        }
    })
    .textures_delta
    .clear();
    let bad: Vec<String> =
        all.iter().filter(|(_, text)| text.chars().any(|c| lacking.contains(&c))).map(|(what, text)| format!("{what}: {text:?}")).collect();
    assert!(bad.is_empty(), "glyphs the UI fonts lack ({lacking:?}):\n{}", bad.join("\n"));
}
