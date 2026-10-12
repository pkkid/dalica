//! The status line (`ui-layout.md` §6.1): 20 high, full width, between the light table and the
//! bottom area, the text centred in Small `text_dim` on `chrome`. It says what the Photo Tray
//! holds (the text itself is [`crate::status_text`]'s): "No selection"; one photo's name, date,
//! size in pixels and on disk; or how many pictures the tray holds (a selection, held photos, a
//! section selected as a unit), their date range and their size on disk. A transient message (the
//! result of the last action, or why it was refused) replaces the text for 4 s of the frame clock.
//! While a scan runs, "Scanning: 1,204 pictures found" stands in place of "No selection", and is
//! the last field when something is selected ([`status_text::with_activity`], `organizing.md` §1).
//! Text too long for the line is fitted by [`Line::fit`]: the scan field goes first, then the
//! prefix, then the name's middle.
//!
//! The line is rebuilt only when the library, the selection or the held photos change, and fitted
//! only when it, the scan's count or the width changes (the scan's count is read from the
//! session's in-memory progress each frame); the text drawn is written to
//! [`UiState::status_line`](crate::state::UiState::status_line), which `ui.state` reports as
//! `status`.

use dalica_core::PhotoId;
use dalica_ui_kit::fonts;
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::{Align2, Id, Ui};

use super::Panels;
use crate::app::App;
use crate::status_text::{self, Line};

// The formatters' first home was here; callers still reach them through this module.
pub use crate::status_text::{MESSAGE_SECONDS, NO_SELECTION, SEP, date_range, date_time, elide_middle, pictures, size};

/// Padding kept free at each end of the line when the text is fitted (points).
pub const PAD: f32 = 8.0;

/// The line and what it was built from: the library revision, the selection, the held photos. Kept
/// in the context's data and compared in place, so an unchanged frame copies no list.
#[derive(Clone, Default)]
struct Built {
    revision: Option<u64>,
    selection: Vec<PhotoId>,
    held: Vec<PhotoId>,
    line: Line,
}

/// The text fitted last: the whole text, the width and font size (bits), the text that fitted.
#[derive(Clone, Default)]
struct Fitted {
    text: String,
    key: (u32, u32),
    fitted: String,
}

/// The line for the tray in `app`, rebuilt only when the library, the selection or the held photos
/// changed; `None` before the library is open.
fn cached_line(app: &App, ctx: &egui::Context) -> Option<Line> {
    let session = app.session()?;
    let state = app.state();
    let revision = session.library().revision();
    ctx.data_mut(|d| {
        let b = d.get_temp_mut_or_default::<Built>(Id::new("status:line"));
        if b.revision != Some(revision) || b.selection != state.selection.ids || b.held != state.tray.held {
            b.revision = Some(revision);
            b.selection.clone_from(&state.selection.ids);
            b.held.clone_from(&state.tray.held);
            b.line = status_text::line(session.library(), state);
        }
        Some(b.line.clone())
    })
}

/// `line` fitted to `max` points in a font `size` points high, remembered while none of them
/// changes.
fn fitted(ctx: &egui::Context, line: &Line, max: f32, size: f32, width: impl Fn(&str) -> f32) -> String {
    let text = line.text();
    let key = (max.to_bits(), size.to_bits());
    let id = Id::new("status:fitted");
    let hit = ctx.data_mut(|d| {
        let f = d.get_temp_mut_or_default::<Fitted>(id);
        (f.text == text && f.key == key).then(|| f.fitted.clone())
    });
    if let Some(f) = hit {
        return f;
    }
    let fitted = line.fit(max, width);
    ctx.data_mut(|d| d.insert_temp(id, Fitted { text, key, fitted: fitted.clone() }));
    fitted
}

/// Clears the transient message once it has shown for [`MESSAGE_SECONDS`], and asks for the frame
/// that will clear it.
fn expire_message(app: &mut App, ctx: &egui::Context) {
    let id = Id::new("status:message");
    if app.state().status.is_empty() {
        ctx.data_mut(|d| d.remove::<(String, f64)>(id));
        return;
    }
    let now = ctx.input(|i| i.time);
    let since = match ctx.data(|d| d.get_temp::<(String, f64)>(id)) {
        Some((m, since)) if m == app.state().status => since,
        _ => {
            let message = app.state().status.clone();
            ctx.data_mut(|d| d.insert_temp(id, (message, now)));
            now
        }
    };
    let left = MESSAGE_SECONDS - (now - since);
    if left.is_finite() && left > 0.0 {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
    } else {
        app.state_mut().status.clear();
        ctx.data_mut(|d| d.remove::<(String, f64)>(id));
    }
}

/// The scan's field while one runs: "Scanning: 1,204 pictures found".
fn scan_field(app: &App) -> Option<String> {
    let progress = app.session()?.scan_progress();
    progress.running.then(|| status_text::scanning(progress.found))
}

/// Paints the status line into `ui`'s rect (the status-line region of [`super::layout`]) and
/// records the text drawn in the view state.
pub fn show(_p: &Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let ctx = ui.ctx().clone();
    expire_message(app, &ctx);
    let line = match app.state().status.as_str() {
        "" => status_text::with_activity(cached_line(app, &ctx).unwrap_or_default(), scan_field(app)),
        message => Line::plain(message),
    };
    let font = fonts::regular(t.small);
    let painter = ui.painter();
    let max = (rect.width() - 2.0 * PAD).max(0.0);
    let text = fitted(&ctx, &line, max, t.small, |s| painter.layout_no_wrap(s.to_owned(), font.clone(), t.text_dim).size().x);
    let drawn = widgets::label_in(painter, rect, Align2::CENTER_CENTER, &text, font, t.text_dim);
    widgets::register(&ctx, "text:status", Role::Other, &text, Target::Painted { rect: drawn, clip: rect }, false);
    app.state_mut().status_line = text;
}
