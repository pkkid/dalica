//! Options ▸ Network (`menus-and-shortcuts.md` §5 Network, `organizing.md` §7, reserved decision
//! 4), on M8.5's form rows, its options from the engine's catalogue (`options.list {tab:
//! "network"}`):
//!
//! ```text
//!                Map:  [ ] Allow map tiles
//!                          The Places pane fetches map pictures from the tile server while it is open.
//!    Map tile server:  [https://tile.openstreetmap.org/{z}/{x}/{y}.png          ]
//!                      A web address with {z}, {x} and {y} for the tile's zoom, column and row.
//!    Map attribution:  [© OpenStreetMap contributors                            ]
//!                      Shown on the map under the tiles.
//!  Geocoding service:  [https://nominatim.openstreetmap.org/search?format=jsonv2&q={query}]
//!                      A web address with {query} for the place typed in "Search for a place".
//!  ───────────────────────────────────────────────────────────
//!              Proxy:  (•) Use the system's proxy settings
//!                      ( ) Manual
//!         Proxy host:  [                    ]                    (enabled for Manual)
//!         Proxy port:  [8080 ]                                   (enabled for Manual)
//!  ───────────────────────────────────────────────────────────
//!  Network log level:  [Errors only ▾]          (disabled: Arrives with the network log)
//! ```
//!
//! Nothing on this tab connects: a change goes into the draft ([`super::general::set`]) and Apply or
//! OK sends it in the dialog's one `options.set`; tiles are fetched only while the Places pane is
//! open and Allow map tiles is on, a place looked up only on Enter in "Search for a place:". The
//! servers are editable for a self-hosted one; an address the map cannot use (not `https://`, or
//! without its `{z}`, `{x}`, `{y}` or `{query}`) is accepted but says why under it in `danger`
//! ([`template_problem`]). The port takes digits only and goes into the draft once it is 1 …
//! 65535. Under automation without `--automation-allow-external`, a change that would let the app
//! reach outside or decide where it connects (ticking Allow map tiles; the tile server, the
//! geocoding service, the proxy: `dalica_engine::automation::EXTERNAL_OPTIONS`) is refused with the
//! reason on the status line, as `options.set` is over `engine.execute` and MCP.
//!
//! Automation ids (`control-protocol.md` §7): `check:option.network.allowMapTiles`, the fields
//! `field:option.network.tileServer`, `field:option.network.tileAttribution`,
//! `field:option.network.geocoder`, `field:option.network.proxyHost` and
//! `field:option.network.proxyPort`, the proxy's radios `choice:option.network.proxy/system` and
//! `choice:option.network.proxy/manual`, and the disabled `choice:option.network.logLevel`.

use dalica_core::geo::Tile;
use dalica_engine::geocode::query_url;
use dalica_engine::map_tiles::tile_url;
use dalica_engine::options::{Kind, Tab};
use dalica_ui_kit::widgets::{Form, TextField, dropdown, dropdown_item, form_group, hint, radio, tooltip};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Id, RichText, Ui};
use serde_json::{Map, Value, json};

use super::export::cut;
use super::file_types::{self, Opt, shown};
use super::general;
use crate::app::App;
use crate::tl;

/// The settings keys of the tab.
pub const ALLOW: &str = "network.allowMapTiles";
pub const TILE_SERVER: &str = "network.tileServer";
pub const ATTRIBUTION: &str = "network.tileAttribution";
pub const GEOCODER: &str = "network.geocoder";
pub const PROXY: &str = "network.proxy";
pub const PROXY_HOST: &str = "network.proxyHost";
pub const PROXY_PORT: &str = "network.proxyPort";
pub const LOG_LEVEL: &str = "network.logLevel";

/// What an address the map cannot use says.
pub const TILE_SERVER_PROBLEM: &str = "The map needs an https:// address with {z}, {x} and {y}.";
pub const GEOCODER_PROBLEM: &str = "The search needs an https:// address with {query}.";
/// The port field's width (points).
const PORT_W: f32 = 72.0;

/// The tab's options with the values in effect.
pub fn rows(app: &App) -> Vec<Opt> {
    file_types::rows(app, Tab::Network)
}

/// Why the map cannot use `value` as option `key`'s address template, `None` when it can (or the
/// option is no template).
pub fn template_problem(key: &str, value: &str) -> Option<&'static str> {
    match key {
        TILE_SERVER => Tile::new(0, 0, 0).ok().and_then(|t| tile_url(value, t).err()).map(|_| TILE_SERVER_PROBLEM),
        GEOCODER => query_url(value, "Lisbon").err().map(|_| GEOCODER_PROBLEM),
        _ => None,
    }
}

/// Draws the tab's body; changes go into the Options draft.
pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let height = ui.available_height();
    let mut changes = Vec::new();
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical()
        .id_salt("dialog:options/network")
        .max_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| changes = form(ui, &t, &opts, &draft));
    for (i, value) in changes {
        let Some(o) = opts.get(i) else { continue };
        let one = serde_json::Map::from_iter([("values".to_string(), json!({ o.row.key.as_str(): value.clone() }))]);
        if !external_allowed() && dalica_engine::automation::external_option("options.set", &one).is_some() {
            app.state_mut().status = crate::commands::map::EXTERNAL.into();
            continue;
        }
        general::set(app, &o.row, value);
    }
}

/// Whether the app may be told to reach outside now: always for the person's own clicks; under a
/// control request only with `--automation-allow-external`.
fn external_allowed() -> bool {
    crate::control::automated().is_none_or(|c| c.allow_external)
}

/// The option `key` and its place in `opts`.
fn find<'a>(opts: &'a [Opt], key: &str) -> Option<(usize, &'a Opt)> {
    opts.iter().enumerate().find(|(_, o)| o.row.key == key)
}

/// The label of the form row of option `key` (its name and a colon).
fn label(opts: &[Opt], key: &str) -> String {
    find(opts, key).map_or_else(String::new, |(_, o)| general::form_label_of(&o.row))
}

/// A problem of [`template_problem`] in the language in effect.
fn problem_text(english: &str) -> String {
    match english {
        "The map needs an https:// address with {z}, {x} and {y}." => tl!("The map needs an https:// address with {z}, {x} and {y}."),
        "The search needs an https:// address with {query}." => tl!("The search needs an https:// address with {query}."),
        other => crate::i18n::tr(other),
    }
}

/// Draws the form; returns the values changed this pass, by option.
fn form(ui: &mut Ui, t: &Tokens, opts: &[Opt], draft: &Map<String, Value>) -> Vec<(usize, Value)> {
    let (map, proxy) = (general::form_label(&tl!(ctx = "options group", "Map")), general::form_label(&tl!(ctx = "options group", "Proxy")));
    let labels = [
        map.clone(),
        label(opts, TILE_SERVER),
        label(opts, ATTRIBUTION),
        label(opts, GEOCODER),
        proxy.clone(),
        label(opts, PROXY_HOST),
        label(opts, PROXY_PORT),
        label(opts, LOG_LEVEL),
    ];
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let form = Form::new(ui, t, &labels);
    let mut changes = Vec::new();
    if let Some((i, o)) = find(opts, ALLOW) {
        form.row(ui, t, &map, |ui| {
            if let Some(v) = file_types::check(ui, t, &o.row, shown(&o.row, draft)) {
                changes.push((i, v));
            }
        });
    }
    for key in [TILE_SERVER, ATTRIBUTION, GEOCODER] {
        if let Some((i, o)) = find(opts, key) {
            form.row(ui, t, &label(opts, key), |ui| {
                if let Some(v) = text(ui, t, o, shown(&o.row, draft), true) {
                    changes.push((i, v));
                }
            });
        }
    }
    ui.separator();
    ui.add_space(4.0);
    let manual = find(opts, PROXY).is_some_and(|(_, o)| shown(&o.row, draft).as_str() == Some("manual"));
    if let Some((i, o)) = find(opts, PROXY) {
        form_group(ui, o.row.reason.is_none(), |ui| {
            form.row(ui, t, &proxy, |ui| {
                let value = shown(&o.row, draft);
                for (v, l) in &o.row.choices {
                    let r = radio(ui, t, &general::item_id(&o.row.key, v), &general::choice_text(&o.row, v, l), v == value);
                    if r.clicked() && v != value {
                        changes.push((i, v.clone()));
                    }
                }
            });
        });
    }
    if let Some((i, o)) = find(opts, PROXY_HOST) {
        form_group(ui, manual, |ui| {
            form.row(ui, t, &label(opts, PROXY_HOST), |ui| {
                if let Some(v) = text(ui, t, o, shown(&o.row, draft), false) {
                    changes.push((i, v));
                }
            });
        });
    }
    if let Some((i, o)) = find(opts, PROXY_PORT) {
        form_group(ui, manual, |ui| {
            form.row(ui, t, &label(opts, PROXY_PORT), |ui| {
                if let Some(v) = port(ui, t, o, shown(&o.row, draft)) {
                    changes.push((i, v));
                }
            });
        });
    }
    ui.separator();
    ui.add_space(4.0);
    if let Some((i, o)) = find(opts, LOG_LEVEL) {
        form_group(ui, o.row.reason.is_none(), |ui| {
            form.row(ui, t, &label(opts, LOG_LEVEL), |ui| {
                if let Some(v) = choice(ui, t, o, shown(&o.row, draft)) {
                    changes.push((i, v));
                }
            });
        });
    }
    changes
}

/// A text option, typed in and kept to its length, its hint under it (and, for an address the map
/// cannot use, why); `wide` fills the row. Returns the new text when it changed.
fn text(ui: &mut Ui, t: &Tokens, o: &Opt, value: &Value, wide: bool) -> Option<Value> {
    let Kind::Text { max_bytes } = o.spec.kind else { return None };
    let before = value.as_str().unwrap_or_default().to_string();
    let mut typed = before.clone();
    let w = if wide { ui.available_width().max(60.0) } else { ui.available_width().clamp(60.0, 260.0) };
    let available = o.row.reason.is_none();
    let field = form_group(ui, available, |ui| TextField::new(&general::control_id("folder", &o.row.key), "", w).show(ui, t, &mut typed));
    if let Some(reason) = general::reason_of(&o.row) {
        tooltip(field.response, t, &reason, None);
    }
    let kept = cut(&typed, max_bytes);
    if let Some(problem) = template_problem(&o.row.key, kept) {
        ui.label(RichText::new(problem_text(problem)).font(fonts::regular(t.small)).color(t.danger));
    } else if !o.row.hint.is_empty() {
        form_group(ui, available, |ui| hint(ui, t, &general::hint_of(&o.row)));
    }
    (kept != before).then(|| json!(kept))
}

/// The proxy port: digits only, at most five; the draft takes it once it is a port (1 … 65535).
/// What is being typed is kept while the field has the focus, so it can be emptied and retyped.
fn port(ui: &mut Ui, t: &Tokens, o: &Opt, value: &Value) -> Option<Value> {
    let Kind::Int { min, max, .. } = o.spec.kind else { return None };
    let id = general::control_id("folder", &o.row.key);
    let typing = Id::new((&id, "typing"));
    let saved = value.as_u64().map(|v| v.to_string()).unwrap_or_default();
    let mut typed = ui.data(|d| d.get_temp::<String>(typing)).unwrap_or_else(|| saved.clone());
    let field = TextField::new(&id, "", PORT_W).show(ui, t, &mut typed);
    typed.retain(|c| c.is_ascii_digit());
    typed.truncate(5);
    if field.response.has_focus() {
        ui.data_mut(|d| d.insert_temp(typing, typed.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(typing));
    }
    let n: u32 = typed.parse().ok()?;
    ((min..=max).contains(&n) && typed != saved).then(|| json!(n))
}

/// A dropdown of `o`'s choices showing `value` (disabled with its reason as the tooltip); returns
/// the choice picked this pass.
fn choice(ui: &mut Ui, t: &Tokens, o: &Opt, value: &Value) -> Option<Value> {
    let text = o.row.choices.iter().find(|(v, _)| v == value).map_or_else(|| value.to_string(), |(v, l)| general::choice_text(&o.row, v, l));
    let mut picked = None;
    let r = dropdown(ui, t, &general::control_id(&o.row.kind, &o.row.key), &text, |ui| {
        for (v, label) in &o.row.choices {
            if dropdown_item(ui, t, &general::item_id(&o.row.key, v), &general::choice_text(&o.row, v, label), "", v == value).clicked() {
                picked = Some(v.clone());
            }
        }
    });
    if let Some(reason) = general::reason_of(&o.row) {
        tooltip(r, t, &reason, None);
    }
    picked.filter(|p| p != value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_engine::map_tiles::{DEFAULT_GEOCODER, DEFAULT_TILE_SERVER};

    #[test]
    fn the_rows_are_the_catalogues_network_options() {
        let opts = rows(&App::new(None));
        let keys: Vec<&str> = opts.iter().map(|o| o.row.key.as_str()).collect();
        assert_eq!(keys, [ALLOW, TILE_SERVER, ATTRIBUTION, GEOCODER, PROXY, PROXY_HOST, PROXY_PORT, LOG_LEVEL]);
        let live: Vec<&str> = opts.iter().filter(|o| o.row.reason.is_none()).map(|o| o.row.key.as_str()).collect();
        assert_eq!(live, [ALLOW, TILE_SERVER, ATTRIBUTION, GEOCODER, PROXY, PROXY_HOST, PROXY_PORT], "only the log level waits");
    }

    #[test]
    fn an_address_the_map_cannot_use_says_why() {
        assert_eq!(template_problem(TILE_SERVER, DEFAULT_TILE_SERVER), None);
        assert_eq!(template_problem(GEOCODER, DEFAULT_GEOCODER), None);
        assert_eq!(template_problem(TILE_SERVER, "https://maps.example/{z}/{x}.png"), Some(TILE_SERVER_PROBLEM));
        assert_eq!(template_problem(TILE_SERVER, "http://maps.example/{z}/{x}/{y}.png"), Some(TILE_SERVER_PROBLEM));
        assert_eq!(template_problem(GEOCODER, "https://geo.example/search"), Some(GEOCODER_PROBLEM));
        assert_eq!(template_problem(ATTRIBUTION, "anything"), None);
    }
}
