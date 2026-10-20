//! The export progress window ("Exporting 12 of 48…", `creating.md` §2): while the view state's
//! `exportProgress` is shown, a small window under the toolbar's right end, beside the activity
//! indicator's row, follows its job: a title, the job's progress line, a bar filled to the share
//! done and [Cancel]. It is not a dialog of the stack: no backdrop, and the window under it keeps
//! taking input (the export runs in the background).
//!
//! - **Following** ([`follow`], once a frame): an export job that starts is shown (whoever started
//!   it: the Export dialog, `export.run` over automation); a job is taken up once, so a window set
//!   aside (`ui.set {exportProgress: null}`, or `shown: false`) stays so while its job runs. When
//!   the job ends (done, cancelled or failed, or no longer listed) the window closes
//!   (`exportProgress` back to `null`); the job's toast says how it went (`crate::notices`). Another
//!   kind of job named over `ui.set` is followed the same way, but one that had already finished
//!   when it was named stays shown until it is set aside.
//! - **Cancel** runs what `jobs.cancel {job}` runs ([`dalica_engine::Session::cancel_jobs`]): the
//!   photos being written finish whole, no other starts. The line then says "Cancelling…" and the
//!   button is disabled until the job ends.
//! - **Ids** (`control-protocol.md` §7): the window `dialog:exportProgress`, its bar
//!   `dialog:exportProgress/bar` (labelled with the line), its button `button:exportProgress.cancel`
//!   (not `button:dialog.cancel`: the window is no dialog of the stack, and a dialog may be open
//!   over it).
//!
//! The same per-frame call also does what a toast's button asked for (`crate::notices::act`):
//! both follow the file jobs, and both run after the panels and the toasts are drawn.

use dalica_engine::{JobEntry, JobId, JobKind, Session};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, Align2, Area, CornerRadius, Frame, Id, Layout, Margin, RichText, Sense, Stroke, Vec2, vec2};

use crate::app::App;
use crate::state::ExportProgress;
use crate::tl;

/// The window's automation id.
pub const ID: &str = "dialog:exportProgress";
/// Its bar's and its Cancel button's.
pub const BAR_ID: &str = "dialog:exportProgress/bar";
pub const CANCEL_ID: &str = "button:exportProgress.cancel";
/// Its title.
pub const TITLE: &str = "Exporting";
/// The line once Cancel was pressed.
pub const CANCELLING: &str = "Cancelling…";
/// Its width, and its gap from the window's right edge and top (under the toolbar), in points.
pub const WIDTH: f32 = 280.0;
const RIGHT: f32 = 16.0;
const TOP: f32 = 88.0;
/// The window's padding, the bar's height and the gaps between its lines.
const PAD: i8 = 12;
const BAR_H: f32 = 4.0;
const GAP: f32 = 8.0;
/// The longest progress line kept, in characters.
const MAX_LINE: usize = 120;

/// What the window shows this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub job: JobId,
    /// "Exporting 12 of 48…", or "Cancelling…".
    pub line: String,
    /// The share done, 0–1; `None` before the job is listed or while its total is unknown.
    pub fraction: Option<f32>,
    /// Cancel was pressed: the button is disabled.
    pub cancelling: bool,
}

/// The window while it is shown; `cancelled` is the job whose Cancel was pressed, if any.
pub fn window(app: &App, cancelled: Option<JobId>) -> Option<Window> {
    let p = app.state().export_progress.filter(|p| p.shown)?;
    let entry = app.session().and_then(|s| s.job(p.job));
    let cancelling = cancelled == Some(p.job);
    let line = if cancelling { tl!("Cancelling…") } else { progress_line(entry.as_ref()) };
    let fraction =
        entry.as_ref().and_then(|e| (e.progress.total > 0).then(|| (e.progress.done.min(e.progress.total) as f64 / e.progress.total as f64) as f32));
    Some(Window { job: p.job, line, fraction, cancelling })
}

/// The window's progress line: the job's label ("Exporting 12 of 48") with "…" after it,
/// without control characters and at most [`MAX_LINE`] characters; "Exporting…" before the job
/// is listed.
pub fn progress_line(entry: Option<&JobEntry>) -> String {
    let label: String = entry.map(|e| e.label.chars().filter(|c| !c.is_control()).take(MAX_LINE).collect()).unwrap_or_default();
    let label = label.trim_end_matches(['…', '.']).trim_end();
    // The engine's label is looked up whole by its English (one carrying values stays English).
    if label.is_empty() { tl!("Exporting…") } else { tl!(ctx = "in progress", "{label}…", label = crate::i18n::tr(label)) }
}

/// The line under the title, while the window is shown.
pub fn text(app: &App) -> Option<String> {
    window(app, None).map(|w| w.line)
}

fn followed_id() -> Id {
    Id::new(ID).with("followed")
}

fn cancelled_id() -> Id {
    Id::new(ID).with("cancelled")
}

/// The newest running export job after `seen`, if any. Cheap while no file job runs.
fn newest_export(session: &Session, seen: JobId) -> Option<JobId> {
    if !session.file_jobs_running() {
        return None;
    }
    session.jobs().into_iter().filter(|e| e.kind == JobKind::Export && !e.state.is_finished() && e.job > seen).map(|e| e.job).max()
}

/// What following the jobs remembers from frame to frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Followed {
    /// The last export job taken up: each is taken up once.
    pub seen: JobId,
    /// The job followed that was seen running.
    pub ran: Option<JobId>,
}

/// Opens the window on an export job that started and closes it when its job has ended: an export
/// job once it has finished, any other job (one `ui.set` named) once it was seen running and has
/// finished, and a job no longer listed.
pub fn follow_jobs(app: &mut App, mut f: Followed) -> Followed {
    let Some(session) = app.session() else { return f };
    match app.state().export_progress {
        Some(p) => {
            f.seen = f.seen.max(p.job);
            let ended = match session.job(p.job) {
                None => true,
                Some(e) if !e.state.is_finished() => {
                    f.ran = Some(p.job);
                    false
                }
                Some(e) => e.kind == JobKind::Export || f.ran == Some(p.job),
            };
            if ended {
                app.state_mut().export_progress = None;
                f.ran = None;
            }
        }
        None => {
            f.ran = None;
            if let Some(job) = newest_export(session, f.seen) {
                app.state_mut().export_progress = Some(ExportProgress { job, shown: true });
                f = Followed { seen: job, ran: Some(job) };
            }
        }
    }
    f
}

/// [`follow_jobs`] with what it remembers kept in `ctx`, once a frame.
pub fn follow(app: &mut App, ctx: &egui::Context) {
    let f = ctx.data(|d| d.get_temp::<Followed>(followed_id())).unwrap_or_default();
    let f = follow_jobs(app, f);
    ctx.data_mut(|d| d.insert_temp(followed_id(), f));
    if app.state().export_progress.is_none() {
        ctx.data_mut(|d| d.remove::<JobId>(cancelled_id()));
    }
}

/// Follows the export jobs, does what a toast's button asked for, and draws the window while it
/// is shown. Once a frame, after the panels and the toasts.
pub fn show(app: &mut App, ctx: &egui::Context) {
    crate::notices::act(app);
    follow(app, ctx);
    let cancelled = ctx.data(|d| d.get_temp::<JobId>(cancelled_id()));
    let Some(w) = window(app, cancelled) else { return };
    let t: Tokens = *app.tokens();
    let mut cancel = false;
    let shown = Area::new(Id::new(ID)).anchor(Align2::RIGHT_TOP, Vec2::new(-RIGHT, TOP)).order(egui::Order::Middle).show(ctx, |ui| {
        Frame::new()
            .fill(t.chrome)
            .stroke(Stroke::new(1.0, t.button_border))
            .corner_radius(t.window_radius)
            .shadow(t.window_shadow)
            .inner_margin(Margin::same(PAD))
            .show(ui, |ui| {
                // The 1 pt stroke each side counts in the window's width.
                let inner = WIDTH - 2.0 * f32::from(PAD) - 2.0;
                ui.set_width(inner);
                ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                ui.label(RichText::new(tl!("Exporting")).font(fonts::semibold(t.title)).color(t.text));
                ui.label(RichText::new(&w.line).font(fonts::regular(t.body)).color(t.text_label));
                ui.add_space(GAP - 4.0);
                let (bar, _) = ui.allocate_exact_size(vec2(inner, BAR_H), Sense::hover());
                let p = ui.painter();
                p.rect_filled(bar, CornerRadius::same(2), t.track);
                if let Some(f) = w.fraction.filter(|f| f.is_finite()) {
                    let fill = bar.width() * f.clamp(0.0, 1.0);
                    if fill > 0.0 {
                        p.rect_filled(egui::Rect::from_min_size(bar.min, vec2(fill, BAR_H)), CornerRadius::same(2), t.accent);
                    }
                }
                widgets::register(ui.ctx(), BAR_ID, Role::Other, &w.line, Target::Painted { rect: bar, clip: ui.clip_rect() }, false);
                ui.add_space(GAP);
                ui.allocate_ui_with_layout(vec2(inner, t.metrics.button_h), Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(!w.cancelling, |ui| {
                        cancel = widgets::text_button(ui, &t, CANCEL_ID, &tl!("Cancel"), false).clicked();
                    });
                });
            })
    });
    let rect = shown.response.rect;
    widgets::register(ctx, ID, Role::Other, &tl!("Exporting"), Target::Painted { rect, clip: ctx.content_rect() }, false);
    if cancel && !w.cancelling {
        if let Some(session) = app.session_mut() {
            // A job that has just ended has nothing left to cancel; the window closes next frame.
            if let Err(e) = session.cancel_jobs(Some(w.job)) {
                log::debug!("export {}: not cancelled: {e}", w.job);
            }
        }
        ctx.data_mut(|d| d.insert_temp(cancelled_id(), w.job));
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_engine::{JobProgress, JobState};

    #[test]
    fn the_window_shows_only_while_shown() {
        let mut app = App::new(None);
        assert_eq!(text(&app), None);
        app.state_mut().export_progress = Some(ExportProgress { job: 3, shown: false });
        assert_eq!(text(&app), None, "hidden");
        app.state_mut().export_progress = Some(ExportProgress { job: 3, shown: true });
        assert_eq!(text(&app).as_deref(), Some("Exporting…"), "a job not known yet");
        let w = window(&app, Some(3)).unwrap();
        assert_eq!((w.line.as_str(), w.cancelling, w.fraction), (CANCELLING, true, None));
        assert!(!window(&app, Some(4)).unwrap().cancelling, "another job's Cancel");
        // Without a session nothing is followed and nothing changes.
        let f = Followed { seen: 7, ran: None };
        assert_eq!(follow_jobs(&mut app, f), f);
        assert_eq!(app.state().export_progress, Some(ExportProgress { job: 3, shown: true }));
    }

    #[test]
    fn the_line_is_the_jobs_label_cleaned() {
        let entry = |label: &str| JobEntry {
            job: 1,
            kind: JobKind::Export,
            label: label.into(),
            state: JobState::Running,
            progress: JobProgress { done: 12, total: 48 },
            cancellable: true,
            started_ms: 0,
            finished_ms: None,
            result: None,
            error: None,
        };
        assert_eq!(progress_line(Some(&entry("Exporting 12 of 48"))), "Exporting 12 of 48…");
        assert_eq!(progress_line(Some(&entry("Exporting 1 of 2…"))), "Exporting 1 of 2…", "never two");
        assert_eq!(progress_line(Some(&entry("Ex\u{7}porting"))), "Exporting…");
        assert_eq!(progress_line(Some(&entry(""))), "Exporting…");
        assert_eq!(progress_line(None), "Exporting…");
        assert_eq!(progress_line(Some(&entry(&"x".repeat(1000)))).chars().count(), MAX_LINE + 1);
    }
}
