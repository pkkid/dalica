//! The activity popover under the toolbar's activity indicator (`ui-layout.md` §3 item 8,
//! `organizing.md` §1): the running and recent jobs, each a `progress_row` with its Cancel.
//!
//! - **Open** while [`UiState::activity`](crate::state::UiState::activity) is set: a click on
//!   `indicator:activity` toggles it, `ui.set {activity}` sets it; a press outside the popover and
//!   the indicator, or Esc, closes it.
//! - **Rows** come from [`Session::jobs`](dalica_engine::Session::jobs), the same entries as
//!   `jobs.list` and `ui.inspect` → `jobs` ([`rows`]): the running jobs first (oldest first), then
//!   the finished ones (newest first), at most [`MAX_ROWS`], and a "N more" line for the rest.
//!   Each row is ui-kit's `progress_row` (`design-system.md` §6): the job's label, its bar
//!   ([`fraction`]: `done / total`, the sliding third while a running job's total is unknown) and,
//!   while it runs and can be cancelled, its × (Cancel), which runs what `jobs.cancel {job}` runs,
//!   [`Session::cancel_jobs`](dalica_engine::Session::cancel_jobs). With no job it says "No
//!   background work".
//! - **Ids** (`control-protocol.md` §7): the popover `popover:activity`, a row `job:<n>`, its Cancel
//!   `job:<n>/cancel`.
//! - **Look:** the popup frame (`chrome`, `button_border`, radius 6, the popup shadow), [`WIDTH`]
//!   wide (less in a narrow window), its right edge under the indicator's, [`GAP`] below it.
//!
//! It never blocks a frame: the registry is in memory, and it is read only while the popover is
//! open.

use dalica_engine::{JobEntry, JobId, JobState};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align2, Area, CornerRadius, Id, Order, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

use crate::app::App;

/// The popover's automation id.
pub const POPOVER_ID: &str = "popover:activity";
/// The popover's width (points), when the window has room.
pub const WIDTH: f32 = 360.0;
/// The padding inside the frame, and the margin kept from the window's edges.
pub const PAD: f32 = 8.0;
/// The space between the indicator and the popover.
pub const GAP: f32 = 4.0;
/// The most jobs listed; the rest are counted on a "N more" line.
pub const MAX_ROWS: usize = 8;
/// Shown when there is no job.
pub const NO_WORK: &str = "No background work";

/// The automation id of job `job`'s row.
pub fn row_id(job: JobId) -> String {
    format!("job:{job}")
}

/// The automation id of job `job`'s Cancel button.
pub fn cancel_id(job: JobId) -> String {
    format!("job:{job}/cancel")
}

/// The jobs the popover lists, in order, and how many more there are: the running (and queued)
/// ones first, oldest first, then the finished ones, newest first; at most [`MAX_ROWS`].
pub fn rows(jobs: Vec<JobEntry>) -> (Vec<JobEntry>, usize) {
    let (mut list, finished): (Vec<JobEntry>, Vec<JobEntry>) = jobs.into_iter().partition(|j| !j.state.is_finished());
    list.extend(finished.into_iter().rev());
    let more = list.len().saturating_sub(MAX_ROWS);
    list.truncate(MAX_ROWS);
    (list, more)
}

/// How full job `e`'s bar is: `done / total`; a finished job's is full when it is done; `None`
/// (the sliding third) while a running job's total is unknown.
pub fn fraction(e: &JobEntry) -> Option<f32> {
    let share = (e.progress.total > 0).then(|| (e.progress.done.min(e.progress.total) as f64 / e.progress.total as f64) as f32);
    match e.state {
        JobState::Done => Some(1.0),
        JobState::Failed | JobState::Cancelled => Some(share.unwrap_or(0.0)),
        JobState::Queued | JobState::Running => share,
    }
}

/// Whether job `e`'s row shows a Cancel button.
pub fn can_cancel(e: &JobEntry) -> bool {
    e.cancellable && !e.state.is_finished()
}

/// The popover's rect for `lines` rows of `row_h` under `anchor` (the indicator) in `screen`: its
/// right edge under the indicator's, [`GAP`] below it, [`WIDTH`] wide or as wide as the window
/// allows, never outside it on the left.
pub fn rect(anchor: Rect, screen: Rect, lines: usize, row_h: f32) -> Rect {
    let (anchor, screen) = (finite(anchor), finite(screen));
    let width = WIDTH.min((screen.width() - 2.0 * PAD).max(0.0));
    let right = anchor.max.x.min(screen.max.x - PAD);
    let left = (right - width).max(screen.min.x + PAD);
    let height = 2.0 * PAD + lines.max(1) as f32 * row_h;
    Rect::from_min_size(pos2(left, anchor.max.y + GAP), vec2((right - left).max(0.0), height))
}

fn finite(r: Rect) -> Rect {
    if r.is_finite() { r } else { Rect::ZERO }
}

/// Draws the popover below `anchor` (the indicator's rect) while it is open, runs a Cancel clicked
/// in it, and closes it on a press outside it and the indicator, or on Esc.
pub fn show(app: &mut App, ui: &mut Ui, anchor: Rect) {
    if !app.state().activity {
        return;
    }
    let ctx = ui.ctx().clone();
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        app.state_mut().activity = false;
        return;
    }
    let t = *app.tokens();
    let jobs = app.session().map(dalica_engine::Session::jobs).unwrap_or_default();
    let (list, more) = rows(jobs);
    let row_h = t.metrics.status_h;
    let lines = list.len() + usize::from(more > 0);
    let frame = rect(anchor, ctx.content_rect(), lines, row_h);
    let mut cancel: Option<JobId> = None;
    Area::new(Id::new(POPOVER_ID)).order(Order::Foreground).fixed_pos(frame.min).constrain(false).show(&ctx, |ui| {
        let (r, _) = ui.allocate_exact_size(frame.size(), Sense::hover());
        paint_frame(ui, &t, r);
        widgets::register(ui.ctx(), POPOVER_ID, Role::Other, "Background activity", Target::Painted { rect: r, clip: ui.clip_rect() }, false);
        let inner = r.shrink(PAD);
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
        child.spacing_mut().item_spacing = egui::Vec2::ZERO;
        if list.is_empty() {
            let line = Rect::from_min_size(inner.min, vec2(inner.width(), row_h));
            widgets::label_in(child.painter(), line, Align2::LEFT_CENTER, NO_WORK, fonts::regular(t.small), t.text_dim);
        }
        for e in &list {
            let cancel_widget = can_cancel(e).then(|| cancel_id(e.job));
            let out = widgets::progress_row(&mut child, &t, &row_id(e.job), &e.label, fraction(e), cancel_widget.as_deref(), inner.width());
            if out.cancel {
                cancel = Some(e.job);
            }
        }
        if more > 0 {
            let top = inner.min.y + list.len() as f32 * row_h;
            let line = Rect::from_min_size(pos2(inner.min.x, top), vec2(inner.width(), row_h));
            widgets::label_in(child.painter(), line, Align2::LEFT_CENTER, &format!("{more} more"), fonts::regular(t.small), t.text_dim);
        }
    });
    if let Some(job) = cancel
        && let Some(session) = app.session_mut()
    {
        let _ = session.cancel_jobs(Some(job));
    }
    // A press anywhere but the popover and the indicator closes it.
    let pressed = ctx.input(|i| if i.pointer.any_pressed() { i.pointer.press_origin() } else { None });
    if pressed.is_some_and(|p| !frame.contains(p) && !anchor.contains(p)) {
        app.state_mut().activity = false;
    }
}

/// The popup frame of `design-system.md` §6 in `r`: the popup shadow, `chrome` fill, a
/// `button_border` line, radius 6.
fn paint_frame(ui: &Ui, t: &Tokens, r: Rect) {
    let radius = CornerRadius::same(6);
    let p = ui.painter();
    p.add(t.popup_shadow.as_shape(r, radius));
    p.rect(r, radius, t.chrome, Stroke::new(1.0, t.button_border), StrokeKind::Inside);
}

#[cfg(test)]
mod tests {
    use dalica_engine::{JobKind, JobProgress};

    use super::*;

    fn job(id: JobId, state: JobState, done: u64, total: u64) -> JobEntry {
        JobEntry {
            job: id,
            kind: JobKind::Scan,
            label: format!("job {id}"),
            state,
            progress: JobProgress { done, total },
            cancellable: true,
            started_ms: id * 10,
            finished_ms: None,
            result: None,
            error: None,
        }
    }

    #[test]
    fn running_jobs_come_first_then_the_newest_finished_and_the_rest_are_counted() {
        let jobs =
            vec![job(1, JobState::Done, 4, 4), job(2, JobState::Running, 1, 4), job(3, JobState::Cancelled, 2, 4), job(4, JobState::Queued, 0, 0)];
        let (list, more) = rows(jobs);
        assert_eq!(list.iter().map(|j| j.job).collect::<Vec<_>>(), [2, 4, 3, 1]);
        assert_eq!(more, 0);
        let many: Vec<JobEntry> = (1..=12).map(|i| job(i, JobState::Done, 1, 1)).collect();
        let (list, more) = rows(many);
        assert_eq!((list.len(), more, list[0].job), (MAX_ROWS, 4, 12));
        assert_eq!(rows(Vec::new()), (Vec::new(), 0));
    }

    #[test]
    fn the_bar_shows_done_of_total_and_slides_while_the_total_is_unknown() {
        assert_eq!(fraction(&job(1, JobState::Running, 1, 4)), Some(0.25));
        assert_eq!(fraction(&job(1, JobState::Running, 0, 0)), None);
        assert_eq!(fraction(&job(1, JobState::Running, 9, 4)), Some(1.0), "never past full");
        assert_eq!(fraction(&job(1, JobState::Done, 0, 0)), Some(1.0));
        assert_eq!(fraction(&job(1, JobState::Cancelled, 0, 0)), Some(0.0));
        assert_eq!(fraction(&job(1, JobState::Failed, 3, 4)), Some(0.75));
        assert_eq!(fraction(&job(1, JobState::Running, u64::MAX, u64::MAX)), Some(1.0));
        assert!(can_cancel(&job(1, JobState::Running, 0, 0)));
        assert!(!can_cancel(&job(1, JobState::Done, 0, 0)));
        assert!(!can_cancel(&JobEntry { cancellable: false, ..job(1, JobState::Running, 0, 0) }));
    }

    #[test]
    fn the_popover_hangs_under_the_indicator_inside_the_window() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 900.0));
        let anchor = Rect::from_min_size(pos2(1364.0, 30.0), vec2(28.0, 28.0));
        let r = rect(anchor, screen, 3, 20.0);
        assert_eq!((r.max.x, r.min.y, r.width(), r.height()), (1392.0, 62.0, WIDTH, 76.0));
        assert_eq!(rect(anchor, screen, 0, 20.0).height(), 36.0, "one line for 'No background work'");
        // A window narrower than the popover: as wide as it allows, inside it.
        let narrow = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 600.0));
        let r = rect(Rect::from_min_size(pos2(164.0, 30.0), vec2(28.0, 28.0)), narrow, 1, 20.0);
        assert!(r.min.x >= PAD && r.max.x <= 192.0, "{r:?}");
        for (a, s) in [(Rect::NAN, screen), (anchor, Rect::NAN), (Rect::ZERO, Rect::ZERO)] {
            let r = rect(a, s, 2, 20.0);
            assert!(r.is_finite() && r.width() >= 0.0, "{r:?}");
        }
        assert_eq!((row_id(7), cancel_id(7)), ("job:7".to_string(), "job:7/cancel".to_string()));
        assert!(dalica_ui_kit::registry::is_valid_id(POPOVER_ID) && dalica_ui_kit::registry::is_valid_id(&cancel_id(7)));
    }
}
