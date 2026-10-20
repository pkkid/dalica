//! The video player's scrubber (`editing.md` §11, `viewing.md` §4): a track with the part played,
//! the playhead dragged or clicked to a time, the trim ruler under it with its start and end
//! handles (never crossing), the parts trimmed away dimmed, and the readout under them, "00:00:09 /
//! 00:00:41 (trimmed to 00:00:21)".
//!
//! Every time is whole milliseconds; the caller gives the clip's length, the playhead and the
//! trim, and gets back what the pointer or the keys changed. The playhead goes anywhere in the clip
//! (the parts trimmed away can still be looked at); the trim keeps at least one step (a frame)
//! between its handles. The readout and the handles' limits are pure functions the player and the
//! tests share.

use egui::{Align2, Color32, CornerRadius, Id, Key, Modifiers, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};

use super::{Target, access_range, focus_ring, register};
use crate::registry::Role;
use crate::text::{self, Text};
use crate::{Tokens, fonts};

/// The height of the playhead's row, in points.
pub const SCRUB_TRACK_ROW: f32 = 18.0;
/// The height of the trim ruler's row, in points.
pub const SCRUB_RULER_ROW: f32 = 14.0;
/// The thickness of the track, in points.
pub const SCRUB_TRACK: f32 = 4.0;
/// The playhead's radius, in points.
pub const PLAYHEAD_R: f32 = 6.0;
/// How wide a trim handle's grab is, in points (it reaches inward, into the kept part).
pub const TRIM_GRAB: f32 = 14.0;
/// The length of a trim handle's tabs (a bracket), in points.
pub const TRIM_TAB: f32 = 5.0;
/// The narrowest track, in points.
pub const SCRUB_MIN_W: f32 = 40.0;
/// The step the keys move by when the caller gives none: a frame at 30 frames a second.
pub const SCRUB_DEFAULT_STEP: u64 = 33;

/// A clip as the scrubber shows it, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrubber {
    /// The clip's length.
    pub duration_ms: u64,
    /// The playhead.
    pub position_ms: u64,
    /// The trim's start (0 untrimmed).
    pub start_ms: u64,
    /// The trim's end (`duration_ms` untrimmed).
    pub end_ms: u64,
    /// What one arrow key moves (a frame); also the least the trim keeps. 0 means
    /// [`SCRUB_DEFAULT_STEP`].
    pub step_ms: u64,
}

impl Scrubber {
    /// An untrimmed clip of `duration_ms` with the playhead at its start, stepping a frame at 30
    /// frames a second.
    pub fn new(duration_ms: u64) -> Scrubber {
        Scrubber { duration_ms, position_ms: 0, start_ms: 0, end_ms: duration_ms, step_ms: SCRUB_DEFAULT_STEP }
    }

    /// The same clip with every time put in its place: the playhead and the trim inside the clip,
    /// the end not before the start, the step at least 1 ms.
    pub fn normalized(self) -> Scrubber {
        let d = self.duration_ms;
        let step = if self.step_ms == 0 { SCRUB_DEFAULT_STEP } else { self.step_ms };
        let start = self.start_ms.min(d);
        let end = self.end_ms.clamp(start, d);
        Scrubber { duration_ms: d, position_ms: self.position_ms.min(d), start_ms: start, end_ms: end, step_ms: step }
    }

    /// Whether the trim cuts anything away.
    pub fn trimmed(&self) -> bool {
        let s = self.normalized();
        s.start_ms > 0 || s.end_ms < s.duration_ms
    }

    /// The least the trim keeps: one step, or the whole clip when it is shorter.
    fn gap(&self) -> u64 {
        self.step_ms.max(1).min(self.duration_ms)
    }
}

/// A time as the readout writes it: hours, minutes and whole seconds, `00:00:09` (the seconds
/// passed: 59 999 ms is `00:00:59`); past 99 hours the hours take more digits.
pub fn clock_text(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// The readout: "00:00:09 / 00:00:41", and " (trimmed to 00:00:21)" with the length kept when the
/// trim cuts anything away.
pub fn scrubber_readout(s: &Scrubber) -> String {
    readout_in(&text::Texts::english(), s)
}

/// [`scrubber_readout`] in the texts of `texts` ([`Text::Trimmed`]).
pub fn readout_in(texts: &text::Texts, s: &Scrubber) -> String {
    let s = s.normalized();
    let base = format!("{} / {}", clock_text(s.position_ms), clock_text(s.duration_ms));
    if s.trimmed() { text::fill(texts.get(Text::Trimmed), &[("time", &base), ("length", &clock_text(s.end_ms - s.start_ms))]) } else { base }
}

/// Where the trim's start goes when asked for `want`: inside the clip, at least a step before the
/// end. It never passes the end.
pub fn trim_start_to(s: &Scrubber, want: u64) -> u64 {
    let s = s.normalized();
    want.min(s.end_ms.saturating_sub(s.gap()))
}

/// Where the trim's end goes when asked for `want`: inside the clip, at least a step after the
/// start. It never passes the start.
pub fn trim_end_to(s: &Scrubber, want: u64) -> u64 {
    let s = s.normalized();
    want.max(s.start_ms.saturating_add(s.gap())).min(s.duration_ms)
}

/// What a [`scrubber`] did this pass.
#[derive(Debug, Clone)]
pub struct ScrubberOut {
    /// The new playhead, when it moved.
    pub position: Option<u64>,
    /// The trim's new start, when it moved.
    pub start: Option<u64>,
    /// The trim's new end, when it moved.
    pub end: Option<u64>,
    /// A drag of the playhead or a handle began (a trim drag is one undo step).
    pub drag_started: bool,
    /// It ended.
    pub drag_stopped: bool,
    /// The readout as painted.
    pub readout: String,
    /// The track's response.
    pub response: Response,
}

/// The x of time `ms` on a track from `x0` to `x1` for a clip of `duration` ms.
fn time_x(ms: u64, duration: u64, x0: f32, x1: f32) -> f32 {
    if duration == 0 { x0 } else { x0 + (ms as f64 / duration as f64) as f32 * (x1 - x0) }
}

/// The time at `x` on the track, clamped to the clip.
fn x_time(x: f32, duration: u64, x0: f32, x1: f32) -> u64 {
    if !(x.is_finite() && x1 > x0) {
        return 0;
    }
    let f = f64::from(((x - x0) / (x1 - x0)).clamp(0.0, 1.0));
    ((f * duration as f64).round() as u64).min(duration)
}

/// The arrow keys pressed with the focus on `r`: steps left (negative) or right.
fn arrow_steps(ui: &Ui, r: &Response) -> i64 {
    if !r.has_focus() {
        return 0;
    }
    ui.input_mut(|i| {
        let left = i.consume_key(Modifiers::NONE, Key::ArrowLeft);
        let right = i.consume_key(Modifiers::NONE, Key::ArrowRight);
        i64::from(right) - i64::from(left)
    })
}

/// `ms` moved by `steps` steps of `step`, never below 0.
fn stepped(ms: u64, steps: i64, step: u64) -> u64 {
    let by = step.saturating_mul(steps.unsigned_abs());
    if steps < 0 { ms.saturating_sub(by) } else { ms.saturating_add(by) }
}

/// One trim handle as dragged: where the pointer took it, in the handle's own terms.
fn handle_drag(ui: &Ui, r: &Response, at_x: f32) -> Option<f32> {
    let key = r.id.with("grab");
    let pointer = r.interact_pointer_pos()?;
    if r.drag_started() {
        // From where the button went down: the drag starts only once the pointer has moved.
        let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(pointer);
        ui.ctx().data_mut(|d| d.insert_temp(key, at_x - origin.x));
    }
    if !r.dragged() {
        return None;
    }
    let offset = ui.ctx().data(|d| d.get_temp::<f32>(key)).unwrap_or(0.0);
    Some(pointer.x + offset)
}

/// The colour of a handle or the playhead in its state.
fn grip_color(t: &Tokens, r: &Response, enabled: bool) -> Color32 {
    if !enabled {
        t.text_disabled
    } else if r.dragged() || r.is_pointer_button_down_on() {
        t.pick
    } else if r.hovered() {
        t.text
    } else {
        t.thumb_hover
    }
}

/// The scrubber, `width` wide (at least [`SCRUB_MIN_W`] plus the playhead): the playhead's row
/// ([`SCRUB_TRACK_ROW`]), the trim ruler's ([`SCRUB_RULER_ROW`]) and the readout's line under
/// them.
///
/// The track is a [`SCRUB_TRACK`] bar in `track`, the part played (from the clip's start to the
/// playhead) `accent`; the parts the trim cuts away are drawn at a third of their strength. The
/// playhead is a [`PLAYHEAD_R`] disc: `thumb_hover`, `text` hovered, `pick` held or dragged. A
/// click on the row jumps the playhead there and a drag moves it with the pointer, clamped to the
/// clip; ← / → with the focus step it by `step_ms`.
///
/// The ruler: a 1 pt `button_border` line the length of the track, the kept part 2 pt `text_label`,
/// the start handle an opening bracket and the end a closing one, the height of the row (colours
/// as the playhead's), each grabbed in a [`TRIM_GRAB`] wide strip reaching into the kept part and
/// dragged from where it was grabbed; ← / → with the focus step it. A handle stops a step short
/// of the other ([`trim_start_to`], [`trim_end_to`]). Disabled, everything is `text_disabled` and
/// takes no input.
///
/// The readout ([`scrubber_readout`]) in `detail` `text_label`, at the track's start.
///
/// Registers `id` on the playhead's row (role `other`, active while dragged, labelled
/// "Position"), `<id>/start` and `<id>/end` (role `other`, active while dragged) and
/// `<id>/readout` (role `other`, painted, labelled with the readout).
pub fn scrubber(ui: &mut Ui, t: &Tokens, id: &str, clip: &Scrubber, width: f32) -> ScrubberOut {
    let s = clip.normalized();
    let enabled = ui.is_enabled();
    let texts = text::texts(ui.ctx());
    let first_readout = readout_in(&texts, &s);
    let galley = ui.painter().layout_no_wrap(first_readout.clone(), fonts::regular(t.detail), if enabled { t.text_label } else { t.text_disabled });
    let min_w = SCRUB_MIN_W + 2.0 * (PLAYHEAD_R + 1.0);
    let width = if width.is_finite() { width.max(min_w) } else { min_w };
    let height = SCRUB_TRACK_ROW + SCRUB_RULER_ROW + galley.size().y.max(16.0);
    let (_, rect) = ui.allocate_space(vec2(width, height));
    let (x0, x1) = (rect.min.x + PLAYHEAD_R + 1.0, rect.max.x - PLAYHEAD_R - 1.0);
    let track_row = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + SCRUB_TRACK_ROW));
    let ruler = Rect::from_min_max(pos2(rect.min.x, track_row.max.y), pos2(rect.max.x, track_row.max.y + SCRUB_RULER_ROW));
    let x_of = |ms: u64| time_x(ms, s.duration_ms, x0, x1);
    let live = enabled && s.duration_ms > 0;
    let sense = if live { Sense::click_and_drag() } else { Sense::hover() };

    // The handles' grabs on the ruler's row, the playhead's on the track's.
    let (xs, xe) = (x_of(s.start_ms), x_of(s.end_ms));
    let start_grab = Rect::from_min_max(pos2(xs - 4.0, ruler.min.y), pos2(xs - 4.0 + TRIM_GRAB, ruler.max.y));
    let end_grab = Rect::from_min_max(pos2(xe + 4.0 - TRIM_GRAB, ruler.min.y), pos2(xe + 4.0, ruler.max.y));
    let start_id = format!("{id}/start");
    let end_id = format!("{id}/end");
    let rs = ui.interact(start_grab, Id::new(&start_id), sense);
    let re = ui.interact(end_grab, Id::new(&end_id), sense);
    let mut r = ui.interact(track_row, Id::new(id), sense);

    let mut out = ScrubberOut {
        position: None,
        start: None,
        end: None,
        drag_started: false,
        drag_stopped: false,
        readout: first_readout.clone(),
        response: r.clone(),
    };
    let mut now = s;
    if live {
        if (r.dragged() || r.clicked())
            && let Some(Pos2 { x, .. }) = r.interact_pointer_pos()
        {
            now.position_ms = x_time(x, s.duration_ms, x0, x1);
        }
        now.position_ms = stepped(now.position_ms, arrow_steps(ui, &r), s.step_ms).min(s.duration_ms);
        let start_steps = arrow_steps(ui, &rs);
        if let Some(x) = handle_drag(ui, &rs, xs) {
            now.start_ms = trim_start_to(&s, x_time(x, s.duration_ms, x0, x1));
        } else if start_steps != 0 {
            now.start_ms = trim_start_to(&s, stepped(s.start_ms, start_steps, s.step_ms));
        }
        let end_steps = arrow_steps(ui, &re);
        if let Some(x) = handle_drag(ui, &re, xe) {
            now.end_ms = trim_end_to(&s, x_time(x, s.duration_ms, x0, x1));
        } else if end_steps != 0 {
            now.end_ms = trim_end_to(&s, stepped(s.end_ms, end_steps, s.step_ms));
        }
        out.drag_started = r.drag_started() || rs.drag_started() || re.drag_started();
        out.drag_stopped = r.drag_stopped() || rs.drag_stopped() || re.drag_stopped();
    }
    if now.position_ms != s.position_ms {
        out.position = Some(now.position_ms);
        r.mark_changed();
    }
    if now.start_ms != s.start_ms {
        out.start = Some(now.start_ms);
        r.mark_changed();
    }
    if now.end_ms != s.end_ms {
        out.end = Some(now.end_ms);
        r.mark_changed();
    }
    if out.position.is_some() || out.start.is_some() || out.end.is_some() {
        out.readout = readout_in(&texts, &now);
    }

    let p = ui.painter();
    let (xs, xe, xp) = (x_of(now.start_ms), x_of(now.end_ms), x_of(now.position_ms));
    let y = track_row.center().y;
    let (track, played) = if enabled { (t.track, t.accent) } else { (t.text_disabled.gamma_multiply(0.6), t.text_disabled) };
    let half = SCRUB_TRACK / 2.0;
    // Each part of the track in its colour, a third as strong outside the trim.
    let bar = |a: f32, b: f32, color: Color32| {
        if b > a {
            p.rect_filled(Rect::from_min_max(pos2(a, y - half), pos2(b, y + half)), CornerRadius::ZERO, color);
        }
    };
    let dim = |c: Color32| c.gamma_multiply(0.35);
    bar(x0, xs, dim(track));
    bar(xs, xe, track);
    bar(xe, x1, dim(track));
    bar(x0, xp.min(xs), dim(played));
    bar(xs, xp.min(xe), played);
    bar(xe, xp, dim(played));
    p.circle_filled(pos2(xp, y), PLAYHEAD_R, grip_color(t, &r, enabled));
    focus_ring(p, t, &r, Rect::from_center_size(pos2(xp, y), Vec2::splat(2.0 * PLAYHEAD_R)), PLAYHEAD_R as u8);

    // The ruler: its line, the kept part, the two brackets.
    let ry = ruler.center().y;
    let line = if enabled { t.button_border } else { t.text_disabled.gamma_multiply(0.6) };
    p.rect_filled(Rect::from_min_max(pos2(x0, ry - 0.5), pos2(x1, ry + 0.5)), CornerRadius::ZERO, line);
    let kept = if enabled { t.text_label } else { t.text_disabled };
    p.rect_filled(Rect::from_min_max(pos2(xs, ry - 1.0), pos2(xe, ry + 1.0)), CornerRadius::ZERO, kept);
    let (top, bottom) = (ruler.min.y + 1.0, ruler.max.y - 1.0);
    for (x, inward, resp) in [(xs, 1.0, &rs), (xe, -1.0, &re)] {
        let c = grip_color(t, resp, enabled);
        p.rect_filled(Rect::from_min_max(pos2(x - 1.0, top), pos2(x + 1.0, bottom)), CornerRadius::ZERO, c);
        for ty in [top, bottom - 2.0] {
            let tab = Rect::from_two_pos(pos2(x, ty), pos2(x + inward * TRIM_TAB, ty + 2.0));
            p.rect_filled(tab, CornerRadius::ZERO, c);
        }
        focus_ring(p, t, resp, Rect::from_min_max(pos2(x - 2.0, top), pos2(x + 2.0, bottom)), 1);
    }

    // The readout under them.
    let galley = if out.readout == first_readout {
        galley
    } else {
        p.layout_no_wrap(out.readout.clone(), fonts::regular(t.detail), if enabled { t.text_label } else { t.text_disabled })
    };
    let text_pos = Align2::LEFT_TOP.align_size_within_rect(galley.size(), Rect::from_min_max(pos2(x0, ruler.max.y), rect.max)).min;
    let readout_rect = Rect::from_min_size(text_pos, galley.size());
    p.galley(text_pos.round(), galley, t.text_label);

    let ctx = ui.ctx();
    let texts = text::texts(ctx);
    let (duration, step) = (s.duration_ms as f64, s.step_ms as f64);
    for (resp, rid, name, ms) in [
        (&r, id, Text::Position, now.position_ms),
        (&rs, start_id.as_str(), Text::TrimStart, now.start_ms),
        (&re, end_id.as_str(), Text::TrimEnd, now.end_ms),
    ] {
        let name = texts.get(name);
        resp.widget_info(|| egui::WidgetInfo::slider(enabled, ms as f64, name));
        access_range(resp, ms as f64, 0.0, duration, step, &clock_text(ms));
        register(ctx, rid, Role::Other, name, Target::Widget(resp), resp.dragged());
    }
    register(ctx, &format!("{id}/readout"), Role::Other, &out.readout, Target::Painted { rect: readout_rect, clip: ui.clip_rect() }, false);
    out.response = r;
    out
}
