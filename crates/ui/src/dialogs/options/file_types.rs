//! Options ▸ File Types (`menus-and-shortcuts.md` §5 File Types, `design-system.md` §11.3): which
//! files beside JPEGs the library shows, and the small-picture threshold, on M8.5's form rows:
//!
//! ```text
//!     Show JPEG files and:  [x] PNG
//!                           [x] GIF
//!                           …
//!                           [x] HEIC/HEIF          (disabled: HEIC support isn't included in this build)
//!                           …
//!                           [x] RAW formats  list… (the raw kinds this build reads)
//!                           [x] Videos (MP4, MOV, M4V, MKV)
//!                           A change rescans your folders. Files of a type turned off are
//!                           hidden, never deleted.
//!  ───────────────────────────────────────────────────────────
//! Small-picture threshold:  [ 64] px ──o──|───|───|───────
//!                           Pictures smaller than this on both sides show only with View ▸ …
//!
//!  ◌ Rescanning your folders…                (while the scan an applied change started runs)
//! ```
//!
//! The rows come from the options catalogue (`dalica_engine::options`) with the values the session's
//! settings hold ([`rows`]); a change goes into the draft through [`super::general::set`], so Apply
//! is enabled exactly while the draft differs from the saved settings, and Apply sends it in the
//! dialog's one `options.set`. That command starts the rescan (M10.5): turning a type off takes its
//! files out of what the library shows and keeps everything said about them (stars, captions,
//! edits), so turning it on again brings them back whole. The dialog only sets the options and
//! shows the outcome: while the scan an applied change started runs, the tab says "Rescanning your
//! folders…" beside the activity spinner ([`RESCAN_ID`]).
//!
//! Automation ids (`control-protocol.md` §7): a type `check:option.fileTypes.<type>`, the RAW
//! row's list `button:option.fileTypes.raw/list` and its rows
//! `button:option.fileTypes.raw/list/<kind>` (`tiff`, `cr3`, `raf`, `crw`, `mrw`, `x3f`), the
//! threshold's slider `slider:option.fileTypes.smallPictureThreshold` and its number field
//! `slider:option.fileTypes.smallPictureThreshold/value`, the rescan line
//! `text:option.fileTypes.rescan`.
//!
//! [`rows`], [`shown`], [`check`] and [`number`] are shared with the Export tab ([`super::export`]).

use dalica_engine::keymap::Platform;
use dalica_engine::options::{Applies, Kind, OptionSpec, Tab, on_tab};
use dalica_engine::settings::Settings;
use dalica_engine::{JobKind, Session};
use dalica_image::{Format, RawKind};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Form, SliderSpec, StopsSlider, Target, checkbox_row, dropdown, dropdown_item, form_group, hint, stops_slider};
use dalica_ui_kit::{Tokens, fonts, icons};
use egui::{Id, Rect, Ui, Vec2, pos2, vec2};
use serde_json::{Map, Value, json};

use super::general::{self, Row};
use crate::app::App;
use crate::tl;

/// The tab's groups, in order: the file types, then the threshold (a row labelled with its name).
pub const GROUPS: [&str; 2] = ["Show JPEG files and:", "Small pictures"];

/// The key of the RAW formats option, the row with the list of raw kinds.
pub const RAW: &str = "fileTypes.raw";
/// The key of the small-picture threshold.
pub const THRESHOLD: &str = "fileTypes.smallPictureThreshold";
/// The RAW row's list of raw kinds, and the id prefix of its rows.
pub const RAW_LIST_ID: &str = "button:option.fileTypes.raw/list";
/// The rescan line, while the scan an applied change started runs (its text as the label).
pub const RESCAN_ID: &str = "text:option.fileTypes.rescan";
/// What the rescan line says.
pub const RESCANNING: &str = "Rescanning your folders…";
/// The hint under the file types.
pub const TYPES_HINT: &str = "A change rescans your folders. Files of a type turned off are hidden, never deleted.";
/// The values the threshold's slider snaps to (pixels).
pub const THRESHOLD_STOPS: [f64; 5] = [32.0, 64.0, 128.0, 256.0, 512.0];
/// The widest a number slider's track gets on these tabs (points).
pub const TRACK_MAX: f32 = 320.0;

/// One option of a tab: its catalogue row, and the dialog's view of it (value saved, reason, …).
#[derive(Debug, Clone)]
pub struct Opt {
    pub spec: &'static OptionSpec,
    pub row: Row,
}

/// The options of `tab` on this platform, in the catalogue's order, with the values in effect: the
/// session's settings, or the defaults (an empty settings file) while no library is open.
pub fn rows(app: &App, tab: Tab) -> Vec<Opt> {
    let empty = Settings::default();
    let settings = app.session().map_or(&empty, Session::settings);
    let here = Platform::current();
    on_tab(tab).filter(|spec| spec.on(here)).map(|spec| Opt { spec, row: row_of(spec, settings) }).collect()
}

/// The dialog's view of `spec` with `settings`.
fn row_of(spec: &OptionSpec, settings: &Settings) -> Row {
    let choices = match spec.kind {
        Kind::Choice(c) => c.iter().map(|(id, label)| (json!(id), (*label).to_string())).collect(),
        _ => Vec::new(),
    };
    Row {
        key: spec.key.to_string(),
        group: spec.group.to_string(),
        label: spec.label.to_string(),
        hint: spec.hint.to_string(),
        kind: spec.kind.id().to_string(),
        saved: (spec.read)(settings),
        reason: spec.unavailable.map(str::to_string),
        choices,
    }
}

/// The value of `row` shown: the draft's, else the saved one.
pub fn shown<'a>(row: &'a Row, draft: &'a Map<String, Value>) -> &'a Value {
    draft.get(&row.key).unwrap_or(&row.saved)
}

/// The automation id of a number option's slider (its field is `<id>/value`).
pub fn slider_id(key: &str) -> String {
    format!("slider:option.{key}")
}

/// An on/off option as a checkbox row (its hint under it; disabled with its reason as the
/// tooltip); returns the new value when a click flipped it.
pub fn check(ui: &mut Ui, t: &Tokens, row: &Row, value: &Value) -> Option<Value> {
    let mut on = value.as_bool().unwrap_or(false);
    let hint_text = (!row.hint.is_empty()).then(|| general::hint_of(row));
    let reason = general::reason_of(row);
    let out = checkbox_row(ui, t, &general::control_id("bool", &row.key), &general::label_of(row), &mut on, hint_text.as_deref(), reason.as_deref());
    out.response.changed().then(|| json!(on))
}

/// A whole-number option as M8.5's slider with stops and its number field (the unit after it),
/// `enabled` or drawn disabled, with the option's hint under it; returns the new value when it
/// changed (a drag snaps at `stops`, a typed number never does).
pub fn number(ui: &mut Ui, t: &Tokens, opt: &Opt, value: &Value, stops: &[f64], enabled: bool) -> Option<Value> {
    let Kind::Int { min, max, unit, .. } = opt.spec.kind else { return None };
    let id = slider_id(&opt.row.key);
    let default = opt.spec.default_value().as_f64().unwrap_or(f64::from(min));
    let (label, unit) = (general::label_of(&opt.row), if unit.is_empty() { String::new() } else { crate::i18n::tr(unit) });
    let spec = SliderSpec { id: &id, label: &label, min: f64::from(min), max: f64::from(max), default, step: 1.0, decimals: 0, unit: &unit };
    let current = value.as_f64().unwrap_or(default);
    let track_w = ui.available_width().min(TRACK_MAX);
    let s = StopsSlider { spec, stops, field: true, track_w };
    let enabled = enabled && opt.row.reason.is_none();
    let out = form_group(ui, enabled, |ui| stops_slider(ui, t, &s, current));
    if !opt.row.hint.is_empty() {
        form_group(ui, enabled, |ui| hint(ui, t, &general::hint_of(&opt.row)));
    }
    let v = out.value.filter(|v| v.is_finite())?;
    // In range after the clamp, so the cast is exact.
    Some(json!(v.round().clamp(f64::from(min), f64::from(max)) as u32))
}

/// What the RAW row's list says for `kind`: its row id part, the file extensions and the makers.
pub fn raw_kind(kind: RawKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        RawKind::TiffBased => {
            ("tiff", "DNG, CR2, NEF, ARW, PEF, ORF, RW2, SRW", "TIFF-based: Adobe, Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Samsung")
        }
        RawKind::Cr3 => ("cr3", "CR3", "Canon"),
        RawKind::Raf => ("raf", "RAF", "Fujifilm"),
        RawKind::Crw => ("crw", "CRW", "Canon, older cameras"),
        RawKind::Mrw => ("mrw", "MRW", "Minolta"),
        RawKind::X3f => ("x3f", "X3F", "Sigma"),
    }
}

/// The raw kinds this build reads, in the list's order.
pub fn raw_kinds() -> Vec<RawKind> {
    RawKind::ALL.into_iter().filter(|k| Format::Raw(*k).supported()).collect()
}

/// Whether the tab says "Rescanning your folders…": a scan is running and either the line already
/// shows or the saved values of the options that rescan changed since the last frame (`before`:
/// none on the tab's first frame), which is what an Apply does.
pub fn rescanning(before: Option<&[Value]>, now: &[Value], showing: bool, scan_running: bool) -> bool {
    scan_running && (showing || before.is_some_and(|b| b != now))
}

/// What the tab remembers between frames: the saved values of the options that rescan, and whether
/// the rescan line shows.
#[derive(Clone, Default)]
struct Seen {
    saved: Option<Vec<Value>>,
    showing: bool,
}

pub fn body(app: &mut App, ui: &mut Ui) {
    let opts = rows(app, Tab::FileTypes);
    let draft = app.state().options.as_ref().map(|o| o.values.clone()).unwrap_or_default();
    let t = *app.tokens();
    let scan_running = app.session().is_some_and(|s| s.jobs().iter().any(|j| j.kind == JobKind::Scan && !j.state.is_finished()));
    let saved: Vec<Value> = opts.iter().filter(|o| o.spec.applies == Applies::Rescan).map(|o| o.row.saved.clone()).collect();
    let memory = Id::new("dialog:options/fileTypes/seen");
    let seen: Seen = ui.data(|d| d.get_temp(memory)).unwrap_or_default();
    let showing = rescanning(seen.saved.as_deref(), &saved, seen.showing, scan_running);
    ui.data_mut(|d| d.insert_temp(memory, Seen { saved: Some(saved), showing }));

    let height = ui.available_height();
    let line_h = if showing { t.metrics.text_field_h + 8.0 } else { 0.0 };
    let mut changes: Vec<(usize, Value)> = Vec::new();
    // A solid bar, shown whenever the tab is taller than the dialog (as on General).
    ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
    egui::ScrollArea::vertical()
        .id_salt("dialog:options/fileTypes")
        .max_height((height - line_h).max(0.0))
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| changes = form(ui, &t, &opts, &draft));
    if showing {
        rescan_line(ui, &t);
    }
    for (i, value) in changes {
        if let Some(o) = opts.get(i) {
            general::set(app, &o.row, value);
        }
    }
}

/// Draws the tab's rows; returns the values changed this pass, by option.
fn form(ui: &mut Ui, t: &Tokens, opts: &[Opt], draft: &Map<String, Value>) -> Vec<(usize, Value)> {
    let numbers: Vec<String> = opts.iter().filter(|o| o.row.kind != "bool").map(|o| general::form_label_of(&o.row)).collect();
    let types = tl!("Show JPEG files and:");
    let labels: Vec<&str> = std::iter::once(types.as_str()).chain(numbers.iter().map(String::as_str)).collect();
    let form = Form::new(ui, t, &labels);
    let mut changes = Vec::new();
    form.row(ui, t, &types, |ui| {
        for (i, o) in opts.iter().enumerate().filter(|(_, o)| o.row.kind == "bool") {
            let value = shown(&o.row, draft);
            let changed = if o.row.key == RAW {
                ui.horizontal(|ui| {
                    let c = check(ui, t, &o.row, value);
                    raw_list(ui, t);
                    c
                })
                .inner
            } else {
                check(ui, t, &o.row, value)
            };
            if let Some(v) = changed {
                changes.push((i, v));
            }
        }
        ui.add_space(2.0);
        hint(ui, t, &tl!("A change rescans your folders. Files of a type turned off are hidden, never deleted."));
    });
    for (i, o) in opts.iter().enumerate().filter(|(_, o)| o.row.kind != "bool") {
        ui.separator();
        ui.add_space(4.0);
        let stops: &[f64] = if o.row.key == THRESHOLD { &THRESHOLD_STOPS } else { &[] };
        if let Some(v) = form.row(ui, t, &general::form_label_of(&o.row), |ui| number(ui, t, o, shown(&o.row, draft), stops, true)) {
            changes.push((i, v));
        }
    }
    changes
}

/// The RAW row's "list…": a dropdown of the raw kinds this build reads, by extension and maker.
fn raw_list(ui: &mut Ui, t: &Tokens) {
    dropdown(ui, t, RAW_LIST_ID, &tl!("list…"), |ui| {
        for kind in raw_kinds() {
            let (part, extensions, makers) = raw_kind(kind);
            dropdown_item(ui, t, &format!("{RAW_LIST_ID}/{part}"), extensions, &makers_text(makers), false);
        }
    });
}

/// The activity indicator's spinner and "Rescanning your folders…", under the rows.
/// The makers of [`raw_kind`] in the language in effect (the companies' names stay as they are).
fn makers_text(english: &str) -> String {
    match english {
        "TIFF-based: Adobe, Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Samsung" => {
            tl!("TIFF-based: {makers}", makers = "Adobe, Canon, Nikon, Sony, Pentax, Olympus, Panasonic, Samsung")
        }
        "Canon, older cameras" => tl!("{maker}, older cameras", maker = "Canon"),
        other => other.to_string(),
    }
}

fn rescan_line(ui: &mut Ui, t: &Tokens) {
    ui.add_space(8.0);
    let rescanning = tl!("Rescanning your folders…");
    let galley = ui.painter().layout_no_wrap(rescanning.clone(), fonts::regular(t.body), t.text_label);
    let glyph = t.metrics.icon_toolbar;
    let (_, rect) = ui.allocate_space(vec2(glyph + 8.0 + galley.size().x, t.metrics.text_field_h));
    let time = ui.input(|i| i.time);
    let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
    let icon = Rect::from_center_size(pos2(rect.min.x + glyph / 2.0, rect.center().y), Vec2::splat(glyph));
    icons::paint_activity(ui.painter(), icon, t.icon, time.rem_euclid(1.0) as f32);
    ui.painter().galley(pos2(rect.min.x + glyph + 8.0, rect.center().y - galley.size().y / 2.0).round(), galley, t.text_label);
    // The spinner's next step (eight a turn, a turn a second).
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(125));
    widgets::register(ui.ctx(), RESCAN_ID, Role::Other, &rescanning, Target::Painted { rect, clip: ui.clip_rect() }, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rescan_line_shows_from_an_apply_until_the_scan_ends() {
        let (off, on) = ([json!(false)], [json!(true)]);
        assert!(!rescanning(None, &on, false, true), "the tab's first frame: a scan that was running before");
        assert!(!rescanning(Some(&on), &on, false, true), "nothing applied");
        assert!(rescanning(Some(&on), &off, false, true), "applied, and the scan runs");
        assert!(rescanning(Some(&off), &off, true, true), "still running");
        assert!(!rescanning(Some(&off), &off, true, false), "the scan ended");
        assert!(!rescanning(Some(&on), &off, false, false), "applied, no scan (no watched folders)");
    }

    #[test]
    fn the_rows_are_the_catalogues_with_the_defaults_while_no_library_is_open() {
        let app = App::new(None);
        let opts = rows(&app, Tab::FileTypes);
        let keys: Vec<&str> = opts.iter().map(|o| o.row.key.as_str()).collect();
        assert_eq!((keys.first(), keys.last()), (Some(&"fileTypes.png"), Some(&THRESHOLD)));
        assert!(opts.iter().filter(|o| o.row.kind == "bool").all(|o| o.row.saved == json!(true)));
        let threshold = opts.iter().find(|o| o.row.key == THRESHOLD).unwrap();
        assert_eq!(threshold.row.saved, threshold.spec.default_value());
        // Every raw kind this build reads is listed once, under its own id.
        let mut parts: Vec<&str> = raw_kinds().into_iter().map(|k| raw_kind(k).0).collect();
        let listed = parts.len();
        parts.sort_unstable();
        parts.dedup();
        assert_eq!((parts.len(), listed > 0), (listed, true));
    }
}
