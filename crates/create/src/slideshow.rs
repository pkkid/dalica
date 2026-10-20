//! Slideshow sequencing (`viewing.md` §1, M11.4): the show's state machine on a [`Clock`], and
//! (in [`transition`] and [`geometry`]) where its photos are drawn, and (in [`faces`]) the face
//! Pan and Zoom moves toward. Pure: no I/O, no egui; the engine owns a [`Show`] and the UI only
//! paints what [`Show::tick`] and [`transition::frame`] answer.
//!
//! The timing: a photo's display time starts the moment its change begins, so with a 3 s display
//! time and Dissolve a photo fades in over 0.8 s and stays alone for the other 2.2 s. Pausing
//! freezes the display time (and Pan and Zoom's motion) but lets a change already under way finish.

pub mod faces;
pub mod geometry;
pub mod transition;

use std::sync::Arc;
use std::time::Duration;

use dalica_core::{Clock, Timestamp};

pub use faces::{FaceBox, face_focus, focus_face};
pub use geometry::{Point, Rect, Size, caption_plate, caption_wrap_width, fit};
pub use transition::{Frame, Layer, Slide, TRANSITION_TIME, Transition, frame, pan_zoom_amount};

/// Display time limits and default, in whole seconds (`viewing.md` §1.3).
pub const DISPLAY_MIN: u32 = 1;
pub const DISPLAY_MAX: u32 = 60;
pub const DISPLAY_DEFAULT: u32 = 3;
/// How long the control bar shows when the show starts.
pub const BAR_AT_START: Duration = Duration::from_millis(2_000);
/// How long the control bar stays after a mouse move or Space.
pub const BAR_AFTER_INPUT: Duration = Duration::from_millis(2_500);

/// The persisted slideshow settings the show reads (Options → Slideshow mirrors them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    /// Seconds each photo shows, clamped to [`DISPLAY_MIN`]..=[`DISPLAY_MAX`] by the show.
    pub display_secs: u32,
    pub transition: Transition,
    /// Loop slideshow: wrap around at the end instead of stopping.
    pub looped: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { display_secs: DISPLAY_DEFAULT, transition: Transition::default(), looped: false }
    }
}

/// Where the show is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Playing,
    Paused,
    /// The photo is zoomed in, and the show therefore paused until it is zoomed out.
    Zoomed,
    /// The last photo has had its display time without Loop: it stays, the bar shows, and Play
    /// restarts from the first photo. A show without photos is at its end from the start.
    Ended,
}

/// A change from one photo to the next that is under way.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Change {
    from: usize,
    at: Timestamp,
    transition: Transition,
    /// How far through its display time the outgoing photo was when it left.
    from_motion: f32,
}

/// What the UI needs to draw the show at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// The photo shown (or coming in), by its place in the show; `None` when the show is empty.
    pub index: Option<usize>,
    pub state: State,
    /// Whether the control bar shows.
    pub bar: bool,
    /// A change under way: the outgoing photo's place and the progress (0..1) through it.
    pub change: Option<(usize, f32)>,
    /// The transition of that change (or the setting, when none is under way).
    pub transition: Transition,
    /// How far through its display time the current photo is (0..1).
    pub motion: f32,
    /// How far through its display time the outgoing photo was when the change began.
    pub outgoing_motion: f32,
    /// Whether Previous does anything (it is disabled on the first photo without Loop).
    pub can_previous: bool,
}

/// One slideshow over `len` photos, driven by a clock. Every input method answers whether it
/// changed anything; [`Show::tick`] brings the show up to the clock's time and answers a [`View`].
pub struct Show {
    clock: Arc<dyn Clock>,
    len: usize,
    index: usize,
    settings: Settings,
    state: State,
    /// The state Zoomed returns to (Playing, Paused or Ended).
    resume: State,
    /// When the current photo's display time began, shifted by the time spent paused.
    shown_at: Timestamp,
    /// The display time already spent while the show is not playing.
    frozen: Option<Duration>,
    change: Option<Change>,
    bar_until: Timestamp,
    /// The photo shown is held ([`Show::set_held`]): its display time does not count.
    held: bool,
}

impl std::fmt::Debug for Show {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Show")
            .field("len", &self.len)
            .field("index", &self.index)
            .field("state", &self.state)
            .field("settings", &self.settings)
            .finish_non_exhaustive()
    }
}

/// Whole milliseconds of a duration, saturated.
fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl Show {
    /// A show of `len` photos starting at `start` (clamped to the last photo), playing, with the
    /// bar shown for [`BAR_AT_START`].
    pub fn new(clock: Arc<dyn Clock>, len: usize, start: usize, settings: Settings) -> Show {
        let now = clock.now();
        let settings = Settings { display_secs: settings.display_secs.clamp(DISPLAY_MIN, DISPLAY_MAX), ..settings };
        let state = if len == 0 { State::Ended } else { State::Playing };
        Show {
            clock,
            len,
            index: start.min(len.saturating_sub(1)),
            settings,
            state,
            resume: State::Playing,
            shown_at: now,
            frozen: (len == 0).then_some(Duration::ZERO),
            change: None,
            bar_until: now.saturating_add(BAR_AT_START),
            held: false,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The display time, as set (1–60 s).
    pub fn display(&self) -> Duration {
        Duration::from_secs(u64::from(self.settings.display_secs))
    }

    /// The current photo's place, `None` when the show is empty.
    pub fn index(&self) -> Option<usize> {
        (self.len > 0).then_some(self.index)
    }

    fn last(&self) -> usize {
        self.len.saturating_sub(1)
    }

    /// The display time the current photo has had.
    fn elapsed(&self, now: Timestamp) -> Duration {
        self.frozen.unwrap_or_else(|| now.since(self.shown_at))
    }

    fn motion(&self, now: Timestamp) -> f32 {
        let display = ms(self.display()).max(1);
        (ms(self.elapsed(now)) as f64 / display as f64).clamp(0.0, 1.0) as f32
    }

    /// Brings the show up to the clock's time: advances past every photo whose display time is
    /// over (wrapping with Loop, else stopping at the end), drops a finished change, and answers
    /// what to draw.
    pub fn tick(&mut self) -> View {
        let now = self.clock.now();
        if self.state == State::Playing && !self.held && self.len > 0 {
            let display = ms(self.display()).max(1);
            let steps = ms(now.since(self.shown_at)) / display;
            if steps > 0 {
                self.advance(steps, display);
            }
        }
        if let Some(c) = self.change
            && now.since(c.at) >= c.transition.duration()
        {
            self.change = None;
        }
        self.view()
    }

    /// Moves `steps` photos on from the timer (only called while playing).
    fn advance(&mut self, steps: u64, display: u64) {
        let step_time = |k: u64| self.shown_at.saturating_add(Duration::from_millis(k.saturating_mul(display)));
        let len = self.len as u64;
        if self.settings.looped {
            if self.len < 2 {
                self.shown_at = step_time(steps);
                return;
            }
            let to = ((self.index as u64 + steps % len) % len) as usize;
            let at = step_time(steps);
            let from = (to + self.len - 1) % self.len;
            self.change = Some(Change { from, at, transition: self.settings.transition, from_motion: 1.0 });
            self.index = to;
            self.shown_at = at;
            return;
        }
        let remaining = (self.last() - self.index) as u64;
        if remaining > 0 {
            let k = steps.min(remaining);
            let at = step_time(k);
            self.index += k as usize;
            self.change = Some(Change { from: self.index - 1, at, transition: self.settings.transition, from_motion: 1.0 });
            self.shown_at = at;
        }
        if steps > remaining {
            self.state = State::Ended;
            self.frozen = Some(self.display());
        }
    }

    /// What to draw now, without moving the show on (call [`Show::tick`] for that).
    pub fn view(&self) -> View {
        let now = self.clock.now();
        let change = self.change.and_then(|c| {
            let total = ms(c.transition.duration());
            let progress = if total == 0 { 1.0 } else { (ms(now.since(c.at)) as f64 / total as f64).min(1.0) as f32 };
            (progress < 1.0).then_some((c, progress))
        });
        View {
            index: self.index(),
            state: self.state,
            bar: self.bar_visible(),
            change: change.map(|(c, p)| (c.from, p)),
            transition: change.map_or(self.settings.transition, |(c, _)| c.transition),
            motion: self.motion(now),
            outgoing_motion: change.map_or(1.0, |(c, _)| c.from_motion),
            can_previous: self.can_previous(),
        }
    }

    /// Whether the control bar shows: always at the end, else for a while after the start, a
    /// mouse move or Space.
    pub fn bar_visible(&self) -> bool {
        self.state == State::Ended || (self.state == State::Zoomed && self.resume == State::Ended) || self.clock.now() < self.bar_until
    }

    pub fn can_previous(&self) -> bool {
        self.index > 0 || (self.settings.looped && self.len > 1)
    }

    fn show_bar(&mut self) {
        let until = self.clock.now().saturating_add(BAR_AFTER_INPUT);
        self.bar_until = self.bar_until.max(until);
    }

    /// The show not playing: the display time frozen where it is.
    fn freeze(&mut self, now: Timestamp) {
        self.frozen = Some(self.elapsed(now));
    }

    /// The show playing again from where the display time was frozen.
    fn thaw(&mut self, now: Timestamp) {
        if self.held {
            return;
        }
        if let Some(spent) = self.frozen.take() {
            self.shown_at = now.saturating_sub(spent);
        }
    }

    /// Shows photo `to` at once, with the current transition, its display time from the start.
    /// Zoom ends; a paused show stays paused; at the end the show is paused on the new photo.
    fn go(&mut self, to: usize) -> bool {
        if self.len == 0 || to >= self.len {
            return false;
        }
        let now = self.clock.now();
        self.change =
            (to != self.index).then(|| Change { from: self.index, at: now, transition: self.settings.transition, from_motion: self.motion(now) });
        if self.state == State::Zoomed {
            self.state = self.resume;
        }
        if self.state == State::Ended {
            self.state = State::Paused;
        }
        self.index = to;
        self.shown_at = now;
        self.held = false;
        self.frozen = (self.state != State::Playing).then_some(Duration::ZERO);
        true
    }

    /// Next (→, ↓, PgDn, the bar's Next): the next photo; with Loop the first after the last;
    /// without Loop, Next on the last photo ends the show as its timer would.
    pub fn next_photo(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        if self.index < self.last() {
            return self.go(self.index + 1);
        }
        if self.settings.looped && self.len > 1 {
            return self.go(0);
        }
        if self.state == State::Ended || (self.state == State::Zoomed && self.resume == State::Ended) {
            return false;
        }
        self.state = State::Ended;
        self.frozen = Some(self.display());
        true
    }

    /// Previous (←, ↑, PgUp, the bar's Previous): none before the first photo without Loop.
    pub fn previous_photo(&mut self) -> bool {
        if self.index > 0 {
            self.go(self.index - 1)
        } else if self.can_previous() {
            self.go(self.last())
        } else {
            false
        }
    }

    /// Home.
    pub fn first_photo(&mut self) -> bool {
        self.index != 0 && self.go(0)
    }

    /// End.
    pub fn last_photo(&mut self) -> bool {
        self.index != self.last() && self.go(self.last())
    }

    /// The mouse wheel: up (a positive delta, as egui reports it) is previous, down is next.
    pub fn wheel(&mut self, delta: f32) -> bool {
        if delta > 0.0 {
            self.previous_photo()
        } else if delta < 0.0 {
            self.next_photo()
        } else {
            false
        }
    }

    /// A click on the photo: next, but only while the bar is hidden.
    pub fn click(&mut self) -> bool {
        !self.bar_visible() && self.next_photo()
    }

    /// Play / Pause (the bar's button): pause a playing show, resume a paused or zoomed one, and
    /// restart an ended one from the first photo.
    pub fn play_pause(&mut self) -> bool {
        let now = self.clock.now();
        match self.state {
            State::Playing => {
                self.freeze(now);
                self.state = State::Paused;
            }
            State::Paused => {
                self.thaw(now);
                self.state = State::Playing;
            }
            State::Zoomed if self.resume != State::Ended => {
                self.state = State::Playing;
                self.thaw(now);
            }
            State::Zoomed | State::Ended => {
                if self.len == 0 {
                    return false;
                }
                self.change =
                    (self.index != 0).then_some(Change { from: self.index, at: now, transition: self.settings.transition, from_motion: 1.0 });
                self.index = 0;
                self.state = State::Playing;
                self.shown_at = now;
                self.frozen = None;
                self.held = false;
            }
        }
        true
    }

    /// Space: Play / Pause, and the bar shows.
    pub fn space(&mut self) -> bool {
        self.show_bar();
        self.play_pause()
    }

    /// A mouse move: the bar shows, and the show keeps playing.
    pub fn mouse_moved(&mut self) {
        self.show_bar();
    }

    /// Zooming the photo in (`true`) pauses the show; zooming back out (`false`) returns it to
    /// what it was doing.
    pub fn set_zoomed(&mut self, zoomed: bool) -> bool {
        let now = self.clock.now();
        match (zoomed, self.state) {
            (true, State::Zoomed) | (false, State::Playing | State::Paused | State::Ended) => false,
            (true, state) => {
                if self.len == 0 {
                    return false;
                }
                if state == State::Playing {
                    self.freeze(now);
                }
                self.resume = state;
                self.state = State::Zoomed;
                true
            }
            (false, State::Zoomed) => {
                self.state = self.resume;
                if self.state == State::Playing {
                    self.thaw(now);
                }
                true
            }
        }
    }

    /// The display time, clamped to 1–60 s. The current photo's time counts toward the new value,
    /// and when it is already past it the show moves on by one photo, never skipping any.
    pub fn set_display_secs(&mut self, secs: u32) {
        self.settings.display_secs = secs.clamp(DISPLAY_MIN, DISPLAY_MAX);
        let now = self.clock.now();
        let display = self.display();
        match self.frozen {
            Some(spent) => self.frozen = Some(spent.min(display)),
            None if now.since(self.shown_at) > display => self.shown_at = now.saturating_sub(display),
            None => {}
        }
    }

    /// Holds the photo shown (`true`; a video playing to its end, M15.27) or lets it go: while held
    /// its display time does not count, as when paused, but the show's state is unchanged (Pause
    /// and Play still toggle it); letting go counts on from the time already spent. Moving to
    /// another photo lets go.
    pub fn set_held(&mut self, held: bool) {
        if held == self.held {
            return;
        }
        let now = self.clock.now();
        if held {
            if self.state == State::Playing {
                self.freeze(now);
            }
            self.held = true;
        } else {
            self.held = false;
            if self.state == State::Playing {
                self.thaw(now);
            }
        }
    }

    /// Whether the photo shown is held ([`Show::set_held`]).
    pub fn held(&self) -> bool {
        self.held
    }

    /// The transition for the next change.
    pub fn set_transition(&mut self, transition: Transition) {
        self.settings.transition = transition;
    }

    /// Loop slideshow. Turning it on at the end leaves the show there (Play restarts it).
    pub fn set_looped(&mut self, looped: bool) {
        self.settings.looped = looped;
    }
}
