//! The Import room's importing and finishing (`organizing.md` §3, its flow): what the destination
//! strip shows of an import ([`Shown`]) and what follows when one ends.
//!
//! - **While it copies** the strip's hint line gives way to the progress ("Copying 45 of 340 at
//!   38 MB/s", the engine's `import.status` line, and a bar filled to the share copied:
//!   [`PROGRESS_ID`]); the same line stands in the status line (as its transient message, renewed
//!   while it changes), and the activity indicator lists the job as it lists every job. Cancel
//!   stops it after the current file.
//! - **When it ends** ([`poll`], once a frame the room is drawn; an import that ended while another
//!   tab was in front is taken up when the room is next drawn): with "Delete copied files from the
//!   card" (a card or a drive, something copied) the question
//!   (`crate::dialogs::remove_from_card`) first; then the room closes and the library selects and
//!   scrolls to the first folder it copied into (`crate::commands::import::finish`); the toast
//!   ("Imported 340 pictures into 2026-06-04", "into 3 folders") is the job's end
//!   (`crate::notices`). An import cancelled, or with files that failed, leaves the room open on
//!   its result instead: the line says what was copied ([`RESULT_ID`]), and each failed file is
//!   listed with its reason above it ([`failure_id`], at most [`MAX_FAILURES`], then "and n
//!   more"), so the user can see them and import again.
//!
//! Each import is taken up once: one that had already ended when the room was first drawn is left
//! alone. `import.status` is asked only while an import copies or one not yet taken up has ended;
//! otherwise a frame reads nothing but the job registry.

use std::path::PathBuf;

use dalica_engine::JobId;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Context, Id, Rect, Ui, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;
use crate::commands::import::{Ending, copying, finish, finished};
use crate::dialogs::remove_from_card;
use crate::state::AfterCopy;

/// The progress line's automation id (labelled with the line).
pub const PROGRESS_ID: &str = "progress:import";
/// The result line's (an import cancelled or with failures).
pub const RESULT_ID: &str = "text:import.result";
/// The most failed files listed; more are counted on one line.
pub const MAX_FAILURES: usize = 5;

/// Failed file `n`'s line (from 0; the "and n more" line is `<MAX_FAILURES>`).
pub fn failure_id(n: usize) -> String {
    format!("text:import.failure/{n}")
}

/// What the strip shows of the import.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Shown {
    /// Nothing: the hint line.
    #[default]
    Idle,
    /// Copying: the line and the share copied (0 … 1).
    Copying { line: String, fraction: Option<f32> },
    /// Ended cancelled or with failures: the result line and the failures' lines.
    Result { line: String, failures: Vec<String> },
}

impl Shown {
    /// The extra lines the strip makes room for (the failures listed).
    pub fn lines(&self) -> usize {
        match self {
            Shown::Result { failures, .. } => failures.len(),
            _ => 0,
        }
    }
}

/// What the room remembers of the imports it follows.
#[derive(Debug, Clone, Default)]
struct Watch {
    /// The first frame has looked: an import ended before it is not taken up.
    started: bool,
    /// The last import taken up.
    handled: Option<JobId>,
    /// The question to ask (its count, opened yet), and the folder to select once it is answered.
    asking: Option<(u64, bool, Option<PathBuf>)>,
    /// The status-line text last set.
    line: String,
    /// The result kept while the room stays open on it.
    result: Shown,
}

fn watch_id() -> Id {
    Id::new("import.progress")
}

/// `n failed files` as the result's lines: each file's name and its reason, at most
/// [`MAX_FAILURES`], then "and n more".
pub fn failure_lines(status: &Value) -> Vec<String> {
    let Some(list) = status["failed"].as_array() else { return Vec::new() };
    let mut lines: Vec<String> = list
        .iter()
        .take(MAX_FAILURES)
        .map(|f| {
            let file = f["file"].as_str().unwrap_or_default();
            let name = std::path::Path::new(file).file_name().map_or_else(|| file.to_string(), |n| n.to_string_lossy().into_owned());
            format!("{name}: {}", f["reason"].as_str().unwrap_or_default())
        })
        .collect();
    if list.len() > MAX_FAILURES {
        lines.push(format!("and {} more", list.len() - MAX_FAILURES));
    }
    lines
}

/// The result line of an import that ended cancelled or with failures.
pub fn result_line(status: &Value) -> String {
    let imported = status["imported"].as_u64().unwrap_or(0);
    let total = status["total"].as_u64().unwrap_or(0);
    let failed = status["failed"].as_array().map_or(0, Vec::len);
    let pictures = |n: u64| if n == 1 { "1 picture".to_string() } else { format!("{n} pictures") };
    let mut line = if status["state"] == "cancelled" {
        format!("Import cancelled: {imported} of {} copied", pictures(total))
    } else {
        format!("Imported {}", pictures(imported))
    };
    if failed > 0 {
        line.push_str(&format!("; {failed} could not be imported:"));
    }
    line
}

/// The share of `status` copied (0 … 1), when it knows its total.
fn fraction(status: &Value) -> Option<f32> {
    let total = status["total"].as_u64().filter(|t| *t > 0)?;
    let done = status["imported"].as_u64().unwrap_or(0).min(total);
    Some(done as f32 / total as f32)
}

/// Follows the imports, once a frame the room is drawn: the progress while one copies (and the
/// status line's text), and what follows when one ends. Returns what the strip shows.
pub fn poll(app: &mut App, ctx: &Context) -> Shown {
    let mut w: Watch = ctx.data(|d| d.get_temp(watch_id())).unwrap_or_default();
    let shown = follow(app, &mut w);
    ctx.data_mut(|d| d.insert_temp(watch_id(), w));
    shown
}

fn follow(app: &mut App, w: &mut Watch) -> Shown {
    let Some(job) = app.session().and_then(|s| s.import_run().job()) else {
        w.started = true;
        return w.result.clone();
    };
    let running = copying(app).is_some();
    if !w.started {
        w.started = true;
        // The room's "After copying" starts from the last choice (`settings.json`).
        if let Some(s) = app.session()
            && s.settings().import().after == dalica_engine::settings::AfterImport::Delete
        {
            app.state_mut().import.destination.after = AfterCopy::Delete;
        }
        if !running {
            w.handled = Some(job);
        }
    }
    if running || w.handled != Some(job) {
        let status = app.session_mut().and_then(|s| s.execute("import.status", &json!({})).ok()).unwrap_or(Value::Null);
        if running {
            w.result = Shown::Idle;
            let line = status["status"].as_str().unwrap_or_default().to_string();
            if app.state().status != line {
                app.state_mut().status = line.clone();
            }
            w.line = line.clone();
            ask(app, w);
            return Shown::Copying { fraction: fraction(&status), line };
        }
        if status["state"] == "done" || status["state"] == "cancelled" {
            w.handled = Some(job);
            if !w.line.is_empty() && app.state().status == w.line {
                app.state_mut().status.clear();
            }
            w.line.clear();
            match finished(&status) {
                Ending::Stay => w.result = Shown::Result { line: result_line(&status), failures: failure_lines(&status) },
                Ending::Ask { count, folder } => {
                    w.result = Shown::Idle;
                    w.asking = Some((count, false, folder));
                }
                Ending::Finish(folder) => {
                    w.result = Shown::Idle;
                    finish(app, folder.as_deref());
                }
            }
        }
    }
    ask(app, w);
    w.result.clone()
}

/// The question asked while one is due: opened once (again on a later frame while another dialog
/// keeps it from opening); once answered, finishing.
fn ask(app: &mut App, w: &mut Watch) {
    let Some((count, opened, folder)) = w.asking.take() else { return };
    if !opened {
        let now = remove_from_card::open(app, count).is_ok();
        w.asking = Some((count, now, folder));
    } else if remove_from_card::is_open(app) {
        w.asking = Some((count, true, folder));
    } else {
        finish(app, folder.as_deref());
    }
}

/// Draws what `shown` says in `rect` (the hint line's place): the progress line and its bar, or
/// the result line.
pub fn show_line(ui: &mut Ui, t: &Tokens, rect: Rect, shown: &Shown) {
    match shown {
        Shown::Idle => {}
        Shown::Copying { line, fraction } => {
            let row = Rect::from_center_size(rect.center(), vec2(rect.width(), t.metrics.status_h));
            let mut child = ui.new_child(UiBuilder::new().max_rect(row));
            widgets::progress_row(&mut child, t, PROGRESS_ID, line, *fraction, None, row.width());
        }
        Shown::Result { line, .. } => text_line(ui, t, rect, RESULT_ID, line, t.text_label),
    }
}

/// Draws the failed files' lines, one per `line_h`, from the top of `rect`.
pub fn show_failures(ui: &mut Ui, t: &Tokens, rect: Rect, shown: &Shown, line_h: f32) {
    let Shown::Result { failures, .. } = shown else { return };
    for (n, text) in failures.iter().enumerate() {
        let top = rect.min.y + n as f32 * line_h;
        let line = Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, top + line_h));
        text_line(ui, t, line, &failure_id(n), text, t.text_dim);
    }
}

/// One line of text in `rect`, left and vertically centred, cut with "…"; registered as `id`.
fn text_line(ui: &mut Ui, t: &Tokens, rect: Rect, id: &str, text: &str, color: egui::Color32) {
    let p = ui.painter().clone();
    let g = super::one_line(&p, text, fonts::regular(t.body), color, rect.width());
    let at = pos2(rect.min.x, rect.center().y - g.size().y / 2.0).round();
    let shown = Rect::from_min_size(at, g.size());
    p.galley(at, g, color);
    widgets::register(ui.ctx(), id, Role::Other, text, Target::Painted { rect: shown, clip: ui.clip_rect() }, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_says_what_was_copied_and_lists_the_failures() {
        let cancelled = json!({ "state": "cancelled", "imported": 2, "total": 5, "failed": [] });
        assert_eq!(result_line(&cancelled), "Import cancelled: 2 of 5 pictures copied");
        assert_eq!(fraction(&cancelled), Some(0.4));
        let failed: Vec<Value> = (1..=7).map(|n| json!({ "file": format!("/card/DCIM/IMG_000{n}.JPG"), "reason": "unreadable" })).collect();
        let done = json!({ "state": "done", "imported": 1, "total": 8, "failed": failed });
        assert_eq!(result_line(&done), "Imported 1 picture; 7 could not be imported:");
        let lines = failure_lines(&done);
        assert_eq!(lines.len(), MAX_FAILURES + 1);
        assert_eq!(lines[0], "IMG_0001.JPG: unreadable");
        assert_eq!(lines[MAX_FAILURES], "and 2 more");
        // Hostile: nothing known.
        assert_eq!((failure_lines(&json!(null)), fraction(&json!({ "total": 0 }))), (Vec::<String>::new(), None));
    }
}
