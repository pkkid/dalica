//! Help ▸ Stats for Nerds… (`menus-and-shortcuts.md` §1.8, M16.36, issue #756): a read-only window
//! for the curious, opened by `help.statsForNerds`: how much memory and disk Dalica uses, where
//! its files live, what it is doing, and the one clean-up that is always safe. It has one button,
//! Close (Esc).
//!
//! - **Its data** is the engine's `app.diagnostics` (`control-protocol.md` §6.18), the query the
//!   CLI and MCP give too, asked about once a second while the window is open ([`REFRESH`]); the
//!   engine answers from memory and measures the disk on a helper thread, so drawing the window
//!   never touches the disk. The window adds what only it knows (the [`Rendering`] group, and the
//!   textures in the Memory group).
//! - **Groups** (memory, the thumbnail cache, the library folder, the photo folders' backups, the
//!   library, work in progress, rendering, the build) are collapsible: `header:statsForNerds.<id>`,
//!   the open ones kept in the draft's `open` (Memory only when the window opens, so it starts
//!   short).
//! - **Copy as Text** (`button:statsForNerds.copy`) puts the whole page on the clipboard as plain
//!   text, for a bug report (paths included; nothing is sent anywhere), and keeps it in the
//!   draft's `copied`.
//! - **Clear Thumbnail Cache…** (`button:statsForNerds.clearCache`) asks inside the window first
//!   ("It is rebuilt as you browse; nothing about your photos is lost."), then runs
//!   `app.clearThumbnailCache` (a background job). **Count Backups** runs `app.diagnostics
//!   {countBackups: true}`; there is no delete for the originals here. **Open Folder** shows the
//!   cache's or the library's folder in the file manager (`toast.showFolder`).
//! - **Resizable:** the grip at the bottom right (`dialog:statsForNerds/grip`) drags the window's
//!   width and the groups' height, kept in the draft's `width` and `height`.

use std::fmt::Write as _;

use dalica_engine::CmdError;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::{fonts, widgets};
use egui::{Id, Label, RichText, Sense, Stroke, Ui, vec2};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::i18n::format::{decimal, file_size, number};
use crate::tl;

pub const TITLE: &str = "Stats for Nerds";
/// The window's width and its groups' height by default, and the least and most they take
/// (points).
pub const WIDTH: f32 = 640.0;
pub const HEIGHT: f32 = 440.0;
pub const MIN_WIDTH: f32 = 420.0;
pub const MAX_WIDTH: f32 = 1600.0;
pub const MIN_HEIGHT: f32 = 160.0;
pub const MAX_HEIGHT: f32 = 1600.0;
/// What the frame, title, top row and button row take besides the groups, and the room kept
/// above and below the window (points).
const CHROME_H: f32 = 130.0;
const SCREEN_MARGIN: f32 = 48.0;
/// The label column's least width, so every group's values line up (points).
const LABEL_W: f32 = 200.0;
/// How often the window asks the engine again (seconds).
pub const REFRESH: f64 = 1.0;
/// The frames whose intervals the Rendering group sums up.
pub const FRAMES: usize = 240;

pub const COPY_ID: &str = "button:statsForNerds.copy";
pub const CLEAR_ID: &str = "button:statsForNerds.clearCache";
pub const CLEAR_CONFIRM_ID: &str = "button:statsForNerds.clearCache/confirm";
pub const CLEAR_CANCEL_ID: &str = "button:statsForNerds.clearCache/cancel";
pub const COUNT_ID: &str = "button:statsForNerds.countBackups";
pub const OPEN_CACHE_ID: &str = "button:statsForNerds.openCache";
pub const OPEN_LIBRARY_ID: &str = "button:statsForNerds.openLibrary";
pub const GRIP_ID: &str = "dialog:statsForNerds/grip";

/// The groups, in order: their ids (`header:statsForNerds.<id>`).
pub const GROUPS: [&str; 8] = ["memory", "thumbnailCache", "libraryFolder", "photoFolders", "library", "work", "rendering", "build"];

/// The note under the photo folders' backups.
pub const BACKUPS_NOTE: &str = "These are your originals from before Save. Delete them only if you're sure you won't want Undo Save.";
/// The question Clear Thumbnail Cache… asks.
pub const CLEAR_QUESTION: &str = "Clear the thumbnail cache? It is rebuilt as you browse; nothing about your photos is lost.";

pub fn dialog() -> Dialog {
    Dialog::new(DialogKind::StatsForNerds)
}

/// The width the draft asks for (clamped).
fn width(d: &Dialog) -> f32 {
    d.fields.get("width").and_then(Value::as_f64).map_or(WIDTH, |w| w as f32).clamp(MIN_WIDTH, MAX_WIDTH)
}

/// The groups' height the draft asks for, less on a screen `screen_h` points tall.
pub fn groups_height(d: &Dialog, screen_h: f32) -> f32 {
    let asked = d.fields.get("height").and_then(Value::as_f64).map_or(HEIGHT, |h| h as f32).clamp(MIN_HEIGHT, MAX_HEIGHT);
    asked.min(screen_h - CHROME_H - SCREEN_MARGIN).max(MIN_HEIGHT)
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    Look { title: tl!("Stats for Nerds"), width: width(d), buttons: vec![Button::close()] }
}

/// The dialog has no button but Close.
pub fn act(_app: &mut App, d: &Dialog, action: &str) -> Result<Value, CmdError> {
    Err(CmdError::NotFound(format!("the {} dialog has no button `dialog.{action}`", d.kind.name())))
}

/// What only the window knows.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rendering {
    /// Live textures and their bytes.
    pub textures: u64,
    pub texture_bytes: u64,
    /// Thumbnail textures and their bytes.
    pub thumb_textures: u64,
    pub thumb_texture_bytes: u64,
    /// Frames per second, and the 95th percentile and longest interval between frames (ms), over
    /// the last [`FRAMES`] frames; `None` before two frames.
    pub fps: Option<f64>,
    pub interval_p95_ms: Option<f64>,
    pub interval_max_ms: Option<f64>,
    /// The window's size in points and its pixels per point.
    pub width: f32,
    pub height: f32,
    pub pixels_per_point: f32,
}

/// One group as shown and copied.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: &'static str,
    pub title: String,
    pub rows: Vec<(String, String)>,
    /// A line under the rows.
    pub note: Option<String>,
}

fn title(id: &str) -> String {
    match id {
        "memory" => tl!("Memory"),
        "thumbnailCache" => tl!("Disk: the thumbnail cache"),
        "libraryFolder" => tl!("Disk: the library folder"),
        "photoFolders" => tl!("Disk: in your photo folders"),
        "library" => tl!("Library"),
        "work" => tl!("Work in progress"),
        "rendering" => tl!("Rendering"),
        _ => tl!("Build"),
    }
}

fn u(v: &Value) -> Option<u64> {
    v.as_u64()
}

/// A size, or what stands for one not measured yet.
fn size_or(v: &Value, measuring: bool) -> String {
    match v.as_u64() {
        Some(b) => file_size(b),
        None if measuring => tl!("measuring…"),
        None => tl!("unknown"),
    }
}

fn count(v: &Value) -> String {
    v.as_u64().map_or_else(|| tl!("unknown"), number)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// The groups of `data` (`app.diagnostics`'s answer; `null` before the first) and `r`.
pub fn groups(data: &Value, r: &Rendering) -> Vec<Group> {
    let measuring = data["measuring"].as_bool().unwrap_or(true);
    let mut out = Vec::with_capacity(GROUPS.len());
    for id in GROUPS {
        let mut rows: Vec<(String, String)> = Vec::new();
        let mut note = None;
        match id {
            "memory" => {
                let m = &data["memory"];
                let resident = match m["residentBytes"].as_u64() {
                    Some(b) => file_size(b),
                    None if measuring => tl!("measuring…"),
                    None => tl!("not known on this system"),
                };
                rows.push((tl!("Resident memory"), resident));
                rows.push((
                    tl!("Thumbnails in memory"),
                    tl!("{used} of {budget}", used = size_or(&m["thumbnailBytes"], false), budget = size_or(&m["thumbnailBudgetBytes"], false)),
                ));
                rows.push((
                    tl!("Thumbnail textures"),
                    tl!("{n} ({size}), at most 600", n = number(r.thumb_textures), size = file_size(r.thumb_texture_bytes)),
                ));
                rows.push((tl!("All textures"), tl!("{n} ({size})", n = number(r.textures), size = file_size(r.texture_bytes))));
            }
            "thumbnailCache" => {
                let c = &data["thumbnailCache"];
                let mut place = text(&c["path"]);
                if place.is_empty() {
                    place = tl!("none (thumbnails are off)");
                } else if c["temporary"] == Value::Bool(true) {
                    place = tl!("{path} (temporary, for this session only)", path = place);
                }
                rows.push((tl!("Location"), place));
                if let Some(why) = c["reason"].as_str() {
                    rows.push((tl!("Why temporary"), why.to_string()));
                }
                rows.push((
                    tl!("Size on disk"),
                    tl!(
                        "{size} of {limit} (settings.json: cache.thumbnailBytes)",
                        size = size_or(&c["diskBytes"], measuring),
                        limit = size_or(&c["limitBytes"], false)
                    ),
                ));
                rows.push((tl!("Entries"), if c["entries"].is_null() && measuring { tl!("measuring…") } else { count(&c["entries"]) }));
                for s in c["sizes"].as_array().into_iter().flatten() {
                    let name = text(&s["size"]);
                    let label = if name.bytes().all(|c| c.is_ascii_digit()) { tl!("{size} px", size = name) } else { tl!("Screen-size previews") };
                    rows.push((label, tl!("{n} entries, {size}", n = count(&s["entries"]), size = size_or(&s["bytes"], false))));
                }
                let (loads, decodes) = (u(&c["loads"]).unwrap_or(0), u(&c["decodes"]).unwrap_or(0));
                let rate = c["hitRate"].as_f64().map_or_else(|| tl!("nothing asked yet"), |h| format!("{} %", decimal(h * 100.0, 1)));
                rows.push((
                    tl!("Hit rate this session"),
                    tl!("{rate} ({loads} read from the cache, {decodes} decoded)", rate = rate, loads = number(loads), decodes = number(decodes)),
                ));
                if c["clearing"] == Value::Bool(true) {
                    rows.push((tl!("Clearing"), tl!("the cache is being cleared…")));
                }
            }
            "libraryFolder" => {
                let l = &data["libraryFolder"];
                rows.push((tl!("Location"), text(&l["path"])));
                match l["files"].as_array() {
                    Some(files) => {
                        for f in files {
                            rows.push((text(&f["name"]), size_or(&f["bytes"], false)));
                        }
                        rows.push((tl!("Total"), size_or(&l["totalBytes"], false)));
                    }
                    None => rows.push((tl!("Files"), tl!("measuring…"))),
                }
                let corrupt = l["corrupt"].as_array().map_or(0, Vec::len);
                rows.push((tl!("Damaged files kept aside"), number(u64::try_from(corrupt).unwrap_or(u64::MAX))));
                for f in l["corrupt"].as_array().into_iter().flatten() {
                    rows.push((text(&f["name"]), size_or(&f["bytes"], false)));
                }
                rows.push((tl!("Folders with a damaged sidecar"), count(&l["damagedFolders"])));
            }
            "photoFolders" => {
                let p = &data["photoFolders"];
                let counting = p["counting"] == Value::Bool(true);
                if p["counted"] == Value::Bool(true) {
                    let b = &p["backups"];
                    rows.push((
                        tl!("Backups (.dalica-originals)"),
                        tl!(
                            "{files} files in {folders} folders, {size}",
                            files = count(&b["files"]),
                            folders = count(&b["folders"]),
                            size = size_or(&b["bytes"], false)
                        ),
                    ));
                    rows.push((tl!("Folders with a .dalica.json"), tl!("{n} of {total}", n = count(&p["sidecars"]), total = count(&p["folders"]))));
                } else {
                    rows.push((tl!("Backups (.dalica-originals)"), if counting { tl!("counting…") } else { tl!("not counted yet") }));
                }
                if counting && p["counted"] == Value::Bool(true) {
                    rows.push((tl!("Counting"), tl!("counting again…")));
                }
                note = Some(tl!("These are your originals from before Save. Delete them only if you're sure you won't want Undo Save."));
            }
            "library" => {
                let l = &data["library"];
                rows.push((
                    tl!("Folders"),
                    tl!(
                        "{watched} watched, {once} scanned once, {excluded} excluded, {offline} offline",
                        watched = count(&l["watched"]),
                        once = count(&l["scanOnce"]),
                        excluded = count(&l["excluded"]),
                        offline = count(&l["offline"])
                    ),
                ));
                rows.push((tl!("Photos and videos"), count(&l["photos"])));
                rows.push((tl!("Videos"), count(&l["videos"])));
                rows.push((tl!("Hidden"), count(&l["hidden"])));
                rows.push((tl!("Faces"), count(&l["faces"])));
                rows.push((tl!("People"), count(&l["people"])));
                rows.push((tl!("Albums"), count(&l["albums"])));
                rows.push((tl!("Photos with unsaved edits"), count(&l["unsavedEdits"])));
                rows.push((tl!("Files that cannot be read"), count(&l["unreadable"])));
                rows.push((tl!("Thumbnails that failed"), count(&l["thumbnailsFailed"])));
                let scan = &l["scanning"];
                let now = if scan["running"] == Value::Bool(true) {
                    tl!("yes: {indexed} of {found} found", indexed = count(&scan["indexed"]), found = count(&scan["found"]))
                } else {
                    tl!("no")
                };
                rows.push((tl!("Scanning now"), now));
                let last = &l["lastScan"];
                let last = if last.is_null() {
                    tl!("none this session")
                } else {
                    let secs = last["durationMs"].as_u64().unwrap_or(0) as f64 / 1000.0;
                    let rate = last["filesPerSecond"].as_f64().map_or_else(String::new, |r| tl!(", {rate} files a second", rate = decimal(r, 0)));
                    tl!("{files} files in {secs} s{rate}", files = count(&last["files"]), secs = decimal(secs, 1), rate = rate)
                };
                rows.push((tl!("Last scan"), last));
            }
            "work" => {
                let w = &data["work"];
                let jobs = w["jobs"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                rows.push((tl!("Jobs running"), number(u64::try_from(jobs.len()).unwrap_or(u64::MAX))));
                for j in jobs {
                    let progress = match (u(&j["done"]), u(&j["total"])) {
                        (Some(done), Some(total)) if total > 0 => tl!("{done} of {total}", done = number(done), total = number(total)),
                        _ => String::new(),
                    };
                    rows.push((text(&j["label"]), progress));
                }
                rows.push((
                    tl!("Thumbnail jobs"),
                    tl!("{n} queued or running, {prefetch} ahead of the view", n = count(&w["thumbnailJobs"]), prefetch = count(&w["prefetching"])),
                ));
                let pending = &w["pendingWrites"];
                let pending = if pending.is_null() {
                    tl!("none")
                } else {
                    tl!("{sidecars} sidecars, {metadata} photo files", sidecars = count(&pending["sidecars"]), metadata = count(&pending["metadata"]))
                };
                rows.push((tl!("Writes waiting"), pending));
            }
            "rendering" => {
                let ms = |v: Option<f64>| v.map_or_else(|| tl!("unknown"), |x| tl!("{ms} ms", ms = decimal(x, 1)));
                rows.push((tl!("Frames per second"), r.fps.map_or_else(|| tl!("unknown"), |f| decimal(f, 0))));
                rows.push((tl!("Frame interval (95 %)"), ms(r.interval_p95_ms)));
                rows.push((tl!("Longest frame interval"), ms(r.interval_max_ms)));
                rows.push((tl!("Frame budget"), tl!("8 ms of work a frame (ui.inspect perf: frameP95Ms)")));
                rows.push((
                    tl!("Window"),
                    tl!(
                        "{w} × {h} points at {ppp} pixels per point",
                        w = decimal(f64::from(r.width), 0),
                        h = decimal(f64::from(r.height), 0),
                        ppp = decimal(f64::from(r.pixels_per_point), 2)
                    ),
                ));
            }
            _ => {
                let b = &data["build"];
                let version = dalica_core::build::describe(b["version"].as_str().unwrap_or(""), b["commit"].as_str(), b["date"].as_str());
                rows.push((tl!("Version"), version));
                let features: Vec<String> = b["features"].as_array().into_iter().flatten().map(text).collect();
                rows.push((tl!("Features"), if features.is_empty() { tl!("none") } else { features.join(", ") }));
                rows.push((tl!("System"), format!("{} {}", text(&b["os"]), text(&b["arch"]))));
            }
        }
        out.push(Group { id, title: title(id), rows, note });
    }
    out
}

/// The page as plain text: the title, then each group's title and its rows `label: value`, its
/// note last. Every group is in it, open or closed.
pub fn as_text(groups: &[Group]) -> String {
    let mut out = tl!("Dalica: Stats for Nerds");
    out.push('\n');
    for g in groups {
        let _ = write!(out, "\n{}\n", g.title);
        for (label, value) in &g.rows {
            if value.is_empty() {
                let _ = writeln!(out, "  {label}");
            } else {
                let _ = writeln!(out, "  {label}: {value}");
            }
        }
        if let Some(note) = &g.note {
            let _ = writeln!(out, "  {note}");
        }
    }
    out
}

/// What the window keeps between frames (egui's memory: it is a cache of the engine's answer and
/// of the frame times, not view state).
#[derive(Clone, Default)]
struct Kept {
    /// When the engine was last asked (egui's time), and its answer.
    asked: Option<f64>,
    data: Value,
    /// The last frames' intervals (seconds), oldest first.
    intervals: std::collections::VecDeque<f32>,
    last_frame: Option<f64>,
}

fn kept_id() -> Id {
    Id::new("dialog:statsForNerds/kept")
}

/// The frame intervals' rate, 95th percentile and longest (ms).
fn frame_numbers(intervals: &std::collections::VecDeque<f32>) -> (Option<f64>, Option<f64>, Option<f64>) {
    let mut v: Vec<f64> = intervals.iter().map(|s| f64::from(*s)).filter(|s| s.is_finite() && *s > 0.0).collect();
    if v.is_empty() {
        return (None, None, None);
    }
    v.sort_by(f64::total_cmp);
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let at = ((v.len() as f64 * 0.95).ceil() as usize).saturating_sub(1).min(v.len().saturating_sub(1));
    let p95 = v.get(at).copied();
    let max = v.last().copied();
    ((mean > 0.0).then(|| 1.0 / mean), p95.map(|s| s * 1000.0), max.map(|s| s * 1000.0))
}

/// The engine's answer, asked again when [`REFRESH`] has passed, and the frame numbers brought up
/// to date. Returns the answer and the window's [`Rendering`].
fn refresh(app: &mut App, ui: &Ui, count_backups: bool) -> (Value, Rendering) {
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let mut kept: Kept = ctx.data_mut(|m| m.get_temp(kept_id())).unwrap_or_default();
    if let Some(last) = kept.last_frame
        && now > last
    {
        kept.intervals.push_back((now - last) as f32);
        while kept.intervals.len() > FRAMES {
            kept.intervals.pop_front();
        }
    }
    kept.last_frame = Some(now);
    let due = kept.asked.is_none_or(|t| now - t >= REFRESH || now < t);
    if due || count_backups {
        let params = if count_backups { json!({ "countBackups": true }) } else { json!({}) };
        match app.session_mut().map(|s| s.execute("app.diagnostics", &params)) {
            Some(Ok(v)) => kept.data = v,
            Some(Err(e)) => app.state_mut().status = e.to_string(),
            None => {}
        }
        kept.asked = Some(now);
    }
    let (fps, p95, max) = frame_numbers(&kept.intervals);
    let data = kept.data.clone();
    ctx.data_mut(|m| m.insert_temp(kept_id(), kept));
    let (mut textures, mut texture_bytes) = (0u64, 0u64);
    for (_, meta) in ctx.tex_manager().read().allocated() {
        textures = textures.saturating_add(1);
        texture_bytes = texture_bytes.saturating_add(u64::try_from(meta.bytes_used()).unwrap_or(u64::MAX));
    }
    let thumbs = app.panels().map(|p| p.thumbnails.counts()).unwrap_or_default();
    let screen = ctx.content_rect();
    let r = Rendering {
        textures,
        texture_bytes,
        thumb_textures: thumbs.thumb_textures,
        thumb_texture_bytes: thumbs.thumb_texture_bytes,
        fps,
        interval_p95_ms: p95,
        interval_max_ms: max,
        width: screen.width(),
        height: screen.height(),
        pixels_per_point: ctx.pixels_per_point(),
    };
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(REFRESH));
    (data, r)
}

/// The groups open when the window opens: Memory only, so the window starts short and every
/// other group is a click away.
pub const OPEN_AT_FIRST: [&str; 1] = ["memory"];

/// The groups open in the draft (`open`; [`OPEN_AT_FIRST`] until one is toggled).
fn open_list(d: &Dialog) -> Vec<Value> {
    d.fields.get("open").and_then(Value::as_array).cloned().unwrap_or_else(|| OPEN_AT_FIRST.iter().map(|g| json!(g)).collect())
}

/// Whether group `id` is open.
fn is_open(d: &Dialog, id: &str) -> bool {
    open_list(d).iter().any(|v| v.as_str() == Some(id))
}

fn toggle(d: &mut Dialog, id: &str) {
    let mut list = open_list(d);
    if let Some(at) = list.iter().position(|v| v.as_str() == Some(id)) {
        list.remove(at);
    } else {
        list.push(json!(id));
    }
    d.fields.insert("open".into(), Value::Array(list));
}

/// What a click in the body asked for, done after the body is drawn.
enum Ask {
    Copy,
    Clear,
    Count,
    Open(String),
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let (data, rendering) = refresh(app, ui, false);
    let list = groups(&data, &rendering);
    let mut ask: Option<Ask> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if widgets::text_button(ui, &t, COPY_ID, &tl!("Copy as Text"), false).clicked() {
            ask = Some(Ask::Copy);
        }
        ui.label(
            RichText::new(tl!("Refreshed every second. Everything here stays on this computer.")).font(fonts::regular(t.body)).color(t.text_dim),
        );
    });
    ui.add_space(8.0);
    let height = groups_height(d, ui.ctx().content_rect().height());
    let confirming = d.fields.get("confirmClear") == Some(&Value::Bool(true));
    let mut toggled: Option<&'static str> = None;
    let mut confirm: Option<bool> = None;
    egui::ScrollArea::vertical()
        .id_salt("dialog:statsForNerds/groups")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .scroll_source(egui::scroll_area::ScrollSource::MOUSE_WHEEL)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for g in &list {
                let open = is_open(d, g.id);
                if widgets::section_header(ui, &t, &format!("header:statsForNerds.{}", g.id), &g.title, open).clicked() {
                    toggled = Some(g.id);
                }
                if !open {
                    continue;
                }
                egui::Grid::new(format!("dialog:statsForNerds/{}", g.id)).num_columns(2).min_col_width(LABEL_W).spacing(vec2(16.0, 3.0)).show(
                    ui,
                    |ui| {
                        for (label, value) in &g.rows {
                            ui.add(Label::new(RichText::new(label).font(fonts::regular(t.body)).color(t.text_label)));
                            ui.add(Label::new(RichText::new(value).font(fonts::regular(t.body)).color(t.text)).wrap());
                            ui.end_row();
                        }
                    },
                );
                if let Some(note) = &g.note {
                    ui.add_space(4.0);
                    ui.add(Label::new(RichText::new(note).font(fonts::regular(t.body)).color(t.text_dim)).wrap());
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    match g.id {
                        "thumbnailCache" => {
                            let path = data["thumbnailCache"]["path"].as_str().map(str::to_string);
                            let has = path.is_some();
                            if ui.add_enabled_ui(has, |ui| widgets::text_button(ui, &t, OPEN_CACHE_ID, &tl!("Open Folder"), false)).inner.clicked() {
                                ask = path.map(Ask::Open);
                            }
                            let busy = data["thumbnailCache"]["clearing"] == Value::Bool(true);
                            if !confirming {
                                let clear = ui
                                    .add_enabled_ui(has && !busy, |ui| widgets::text_button(ui, &t, CLEAR_ID, &tl!("Clear Thumbnail Cache…"), false));
                                if clear.inner.clicked() {
                                    confirm = Some(true);
                                }
                            }
                        }
                        "libraryFolder" => {
                            let path = data["libraryFolder"]["path"].as_str().map(str::to_string);
                            let open =
                                ui.add_enabled_ui(path.is_some(), |ui| widgets::text_button(ui, &t, OPEN_LIBRARY_ID, &tl!("Open Folder"), false));
                            if open.inner.clicked() {
                                ask = path.map(Ask::Open);
                            }
                        }
                        "photoFolders" => {
                            let counting = data["photoFolders"]["counting"] == Value::Bool(true);
                            let label = if data["photoFolders"]["counted"] == Value::Bool(true) { tl!("Count Again") } else { tl!("Count Backups") };
                            if ui.add_enabled_ui(!counting, |ui| widgets::text_button(ui, &t, COUNT_ID, &label, false)).inner.clicked() {
                                ask = Some(Ask::Count);
                            }
                        }
                        _ => {}
                    }
                });
                if g.id == "thumbnailCache" && confirming {
                    ui.add_space(4.0);
                    ui.add(
                        Label::new(
                            RichText::new(tl!("Clear the thumbnail cache? It is rebuilt as you browse; nothing about your photos is lost."))
                                .font(fonts::regular(t.body))
                                .color(t.text),
                        )
                        .wrap(),
                    );
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if widgets::text_button(ui, &t, CLEAR_CONFIRM_ID, &tl!("Clear"), false).clicked() {
                            ask = Some(Ask::Clear);
                        }
                        if widgets::text_button(ui, &t, CLEAR_CANCEL_ID, &tl!("Cancel"), false).clicked() {
                            confirm = Some(false);
                        }
                    });
                }
                ui.add_space(8.0);
            }
        });
    grip(ui, &t, d);
    if let Some(id) = toggled {
        toggle(d, id);
    }
    if let Some(on) = confirm {
        d.fields.insert("confirmClear".into(), Value::Bool(on));
    }
    match ask {
        Some(Ask::Copy) => {
            let text = as_text(&list);
            ui.ctx().copy_text(text.clone());
            d.fields.insert("copied".into(), Value::String(text));
            app.state_mut().status = tl!("Copied Stats for Nerds as text.");
        }
        Some(Ask::Clear) => {
            d.fields.insert("confirmClear".into(), Value::Bool(false));
            if let Err(e) = app.run("app.clearThumbnailCache", &json!({})) {
                app.state_mut().status = e.to_string();
            }
            // Asked again at once, so the window says the cache is being cleared.
            forget_answer(ui);
        }
        Some(Ask::Count) => {
            let _ = refresh(app, ui, true);
        }
        Some(Ask::Open(path)) => {
            if let Err(e) = app.run("toast.showFolder", &json!({ "path": path })) {
                app.state_mut().status = e.to_string();
            }
        }
        None => {}
    }
}

/// Makes the next frame ask the engine again.
fn forget_answer(ui: &Ui) {
    ui.ctx().data_mut(|m| m.get_temp_mut_or_default::<Kept>(kept_id()).asked = None);
}

/// The resize grip at the bottom right: dragging it changes the window's width and the groups'
/// height (the draft's `width` and `height`).
fn grip(ui: &mut Ui, t: &dalica_ui_kit::Tokens, d: &mut Dialog) {
    let size = 14.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::hover());
    let grip = egui::Rect::from_min_size(egui::pos2(rect.max.x - size, rect.min.y), vec2(size, size));
    let id = Id::new(GRIP_ID);
    let resp = ui.interact(grip, id, Sense::drag());
    let color = if resp.hovered() || resp.dragged() { t.text } else { t.text_dim };
    let p = ui.painter();
    for k in 1..=3 {
        let o = k as f32 * 4.0;
        p.line_segment([egui::pos2(grip.max.x - o, grip.max.y), egui::pos2(grip.max.x, grip.max.y - o)], Stroke::new(1.0, color));
    }
    widgets::register(ui.ctx(), GRIP_ID, Role::Other, &tl!("Resize"), widgets::Target::Widget(&resp), resp.dragged());
    if resp.dragged() {
        let delta = resp.drag_delta();
        let w = (width(d) + delta.x * 2.0).clamp(MIN_WIDTH, MAX_WIDTH);
        let h = (groups_height(d, f32::MAX) + delta.y).clamp(MIN_HEIGHT, MAX_HEIGHT);
        d.fields.insert("width".into(), json!(w.round()));
        d.fields.insert("height".into(), json!(h.round()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "measuring": false,
            "memory": { "residentBytes": 300_000_000u64, "thumbnailBytes": 1_000_000, "thumbnailBudgetBytes": 201_326_592 },
            "thumbnailCache": { "path": "/c/thumbs", "temporary": false, "reason": null, "limitBytes": 2_147_483_648u64, "diskBytes": 5_000_000,
                "liveBytes": 4_000_000, "entries": 4, "sizes": [{ "size": "128", "entries": 2, "bytes": 1000 }], "loads": 3, "decodes": 1, "hitRate": 0.75, "clearing": false },
            "libraryFolder": { "path": "/l", "files": [{ "name": "index.log", "bytes": 2000 }], "totalBytes": 2000, "corrupt": [], "damagedFolders": 0 },
            "photoFolders": { "counting": false, "counted": false, "folders": null, "backups": null, "sidecars": null },
            "library": { "watched": 1, "scanOnce": 0, "excluded": 0, "offline": 0, "photos": 2, "videos": 0, "hidden": 0, "faces": 0, "people": 0,
                "albums": 0, "unsavedEdits": 0, "unreadable": 0, "thumbnailsFailed": 0, "scanning": { "running": false }, "lastScan": null },
            "work": { "jobs": [], "thumbnailJobs": 0, "prefetching": 0, "pendingWrites": null },
            "build": { "version": "1.0.0", "commit": null, "date": null, "features": [], "os": "linux", "arch": "x86_64" },
        })
    }

    #[test]
    fn every_group_in_order_and_the_text_holds_them_all() {
        let g = groups(&sample(), &Rendering::default());
        assert_eq!(g.iter().map(|g| g.id).collect::<Vec<_>>(), GROUPS);
        let text = as_text(&g);
        for needle in
            ["Memory", "Rendering", "Build", "Location: /c/thumbs", "75.0 %", "index.log: 2.0 KB", "Version: 1.0.0 (dev build)", BACKUPS_NOTE]
        {
            assert!(text.contains(needle), "{needle} in\n{text}");
        }
        // Before the first answer, nothing breaks and the sizes say they are being measured.
        let empty = as_text(&groups(&Value::Null, &Rendering::default()));
        assert!(empty.contains("measuring…"), "{empty}");
    }

    #[test]
    fn frame_numbers_take_rate_p95_and_longest() {
        let v: std::collections::VecDeque<f32> = std::iter::repeat_n(0.016, 19).chain([0.1]).collect();
        let (fps, p95, max) = frame_numbers(&v);
        assert!((fps.unwrap() - 1.0 / ((0.016 * 19.0 + 0.1) / 20.0)).abs() < 0.01);
        assert!((p95.unwrap() - 16.0).abs() < 0.01 && (max.unwrap() - 100.0).abs() < 0.01);
        assert_eq!(frame_numbers(&std::collections::VecDeque::new()), (None, None, None));
    }
}
