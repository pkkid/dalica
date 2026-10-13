//! Control requests in the window (`control-protocol.md` §5.1, §5.2): requests are taken from the
//! transport in the app's logic tick and handled by the same handlers as `--headless`
//! (`dalica_ui::control`). Their input is played over the window's real frames: each [`Step`]
//! injects its events into one frame's raw input (`raw_input_hook`), and the continuation runs in
//! the logic tick after the last step's frame has been laid out. Work (path checks, PNG encoding,
//! writes) runs on a worker thread, never on the UI thread.
//!
//! The steps mirror the headless driver's (`dalica_ui::headless`) frame by frame, so a script
//! behaves the same in the window and headless.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use dalica_ui::App;
use dalica_ui::control::{Code, Connection, Control, DragTo, Error, Host, Input, Next, Outcome, Played, Then, Window, guard};
use dalica_ui::headless::{Click, Drag, DriverError, KeyState, MAX_STEPS, PointerEvent, PointerKind, QUIET_FRAMES, Settled};
use dalica_ui::perf::Perf;
use dalica_ui_kit::{Image, registry};
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, ViewportCommand};
use serde_json::{Value, json};

use super::transport::{Hub, Msg, Pending, QUIT, RUN_LIMIT};

/// The farthest a pointer position or a scroll may reach, in points (as the driver).
const MAX_COORD: f32 = 1.0e6;
/// How long a resize may take to reach the window, and a screenshot to come back (§6.11).
const WINDOW_WAIT: Duration = Duration::from_secs(2);
/// Without a frame for this long, the window is not drawing (`ui.inspect` → `window.drawing`).
const NOT_DRAWING: Duration = Duration::from_secs(1);
/// Why a play in a window that draws no frames ends.
pub const STALLED: &str = "the window drew no frames for 2 s (minimised or covered), so nothing can be played or captured; Dalica never raises its window: use `--headless` or `dalica-cli snapshot` to draw without one";

/// Whether a play that started at `started` has seen no frame for [`WINDOW_WAIT`] at `now`: since
/// it started, or since the last frame if one came after.
fn stalled(started: Instant, last_frame: Option<Instant>, now: Instant) -> bool {
    let since = last_frame.map_or(started, |f| f.max(started));
    now.saturating_duration_since(since) > WINDOW_WAIT
}

/// What one frame of a play does.
#[derive(Debug)]
enum Step {
    /// Inject these events into one frame (possibly none: just let a frame run).
    Events { events: Vec<Event>, injected: bool },
    /// Let `n` frames run.
    Frames(usize),
    /// Frames until `until` (real time); counts them.
    Wait { until: Instant },
    /// Frames until nothing visible is pending for [`QUIET_FRAMES`] frames, at most until `until`.
    Settle { until: Instant, started: Instant, quiet: usize, frames: usize, jobs: bool },
    /// Ask the window for a new inner size, then frames until it has it (at most until `until`).
    Resize { size: Vec2, sent: bool, until: Instant },
    /// Ask the window for a screenshot; it comes back in a later frame's raw input.
    Capture { sent: bool, until: Instant },
}

/// What a play gives when its steps are done.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Done,
    Frames,
    Settled,
    Shot,
}

/// Input being played over frames.
struct Play {
    steps: VecDeque<Step>,
    kind: Kind,
    /// Frames run by `Wait` and `Frames` steps.
    frames: usize,
    settled: Option<Settled>,
    image: Option<Image>,
    error: Option<DriverError>,
}

impl Play {
    fn new(kind: Kind) -> Play {
        Play { steps: VecDeque::new(), kind, frames: 0, settled: None, image: None, error: None }
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.steps.push_back(Step::Events { events, injected: false });
    }

    fn done(&self) -> bool {
        self.steps.is_empty() || self.error.is_some()
    }

    fn result(self) -> Result<Played, DriverError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        Ok(match self.kind {
            Kind::Done => Played::Done,
            Kind::Frames => Played::Frames(self.frames),
            Kind::Settled => Played::Settled(self.settled.unwrap_or(Settled { settled: false, frames: 0, ms: 0.0, busy: vec![] })),
            Kind::Shot => match self.image {
                Some(image) => Played::Shot { image, settled: self.settled.map(|s| s.settled) },
                None => return Err(DriverError::BadInput("the window gave no screenshot".into())),
            },
        })
    }
}

/// Where a request stands.
enum Stage {
    Playing(Play, Then),
    /// The play is over: run the continuation in the next logic tick.
    Played(Result<Played, DriverError>, Then),
    Working(Receiver<Next>),
}

struct Active {
    /// `None` for a play no client waits for (key releases of a closed connection).
    pending: Option<Pending>,
    started: Instant,
    stage: Stage,
}

/// The window's host: the app, its egui context and its counters, between frames.
struct WinHost<'a> {
    app: &'a mut App,
    ctx: &'a egui::Context,
    perf: &'a mut Perf,
    window: Window,
}

impl Host for WinHost<'_> {
    fn app(&self) -> &App {
        self.app
    }

    fn app_mut(&mut self) -> &mut App {
        self.app
    }

    fn ctx(&self) -> &egui::Context {
        self.ctx
    }

    fn perf(&self) -> &Perf {
        self.perf
    }

    fn perf_mut(&mut self) -> &mut Perf {
        self.perf
    }

    fn window(&self) -> Window {
        self.window
    }
}

/// The control channel's UI side in the window.
pub struct WindowControl {
    hub: Hub,
    control: Control,
    conns: HashMap<u64, Connection>,
    active: Option<Active>,
    /// Plays waiting for no client: key releases of closed connections.
    orphans: VecDeque<Input>,
    /// Modifiers held down by `ui.key {state: down}`.
    held: Modifiers,
    /// Where synthetic input left the pointer.
    pointer: Option<Pos2>,
    last_frame: Option<Instant>,
}

/// What the window looks like now (§6.6).
fn window_info(ctx: &egui::Context, last_frame: Option<Instant>) -> Window {
    let rect = ctx.content_rect();
    let focused = ctx.input(|i| i.focused);
    let drawing = last_frame.is_some_and(|t| t.elapsed() < NOT_DRAWING);
    Window { width: rect.width(), height: rect.height(), pixels_per_point: ctx.pixels_per_point(), focused, drawing, headless: false }
}

impl WindowControl {
    pub fn new(hub: Hub, control: Control) -> WindowControl {
        WindowControl {
            hub,
            control,
            conns: HashMap::new(),
            active: None,
            orphans: VecDeque::new(),
            held: Modifiers::NONE,
            pointer: None,
            last_frame: None,
        }
    }

    /// The logic tick: takes requests off the queue, runs continuations and finished work, until
    /// a request is waiting for frames or work, or nothing is queued.
    pub fn logic(&mut self, app: &mut App, ctx: &egui::Context, perf: &mut Perf) {
        let window = window_info(ctx, self.last_frame);
        for _ in 0..64 {
            let next = match self.active.take() {
                None => match self.next_job() {
                    Some(job) => job,
                    None => return,
                },
                Some(mut a) => match a.stage {
                    Stage::Played(played, then) => {
                        let mut host = WinHost { app, ctx, perf, window };
                        let outcome = guard(move || then(&mut host, played));
                        Job::Continue(a.pending.take(), a.started, outcome)
                    }
                    Stage::Working(rx) => match rx.try_recv() {
                        Ok(Next::Reply(r)) => Job::Continue(a.pending.take(), a.started, Outcome::Reply(r)),
                        Ok(Next::Resume(k)) => {
                            let mut host = WinHost { app, ctx, perf, window };
                            let outcome = guard(move || k(&mut host));
                            Job::Continue(a.pending.take(), a.started, outcome)
                        }
                        Err(TryRecvError::Empty) => {
                            self.active = Some(Active { stage: Stage::Working(rx), ..a });
                            return;
                        }
                        Err(TryRecvError::Disconnected) => {
                            let e = Error::new(Code::Internal, "a control worker stopped without an answer");
                            Job::Continue(a.pending.take(), a.started, Outcome::Reply(Err(e)))
                        }
                    },
                    Stage::Playing(play, then) => {
                        if stalled(a.started, self.last_frame, Instant::now()) {
                            // Minimised or covered: the window draws no frames, so input cannot play
                            // and a screenshot cannot be read back (§6.11). Dalica never raises the
                            // window for it; `--headless` and `dalica-cli snapshot` draw without one.
                            let e = Error::new(Code::Timeout, STALLED);
                            Job::Continue(a.pending.take(), a.started, Outcome::Reply(Err(e)))
                        } else if a.started.elapsed() > RUN_LIMIT {
                            // The window stopped drawing (hidden, minimised): give up on this one so
                            // later requests are not stuck behind it.
                            let e = Error::new(Code::Timeout, "the window drew no frames to play the input; it may be hidden or minimised");
                            Job::Continue(a.pending.take(), a.started, Outcome::Reply(Err(e)))
                        } else {
                            self.active = Some(Active { stage: Stage::Playing(play, then), ..a });
                            return;
                        }
                    }
                },
            };
            match next {
                Job::Start(pending) => {
                    if pending.req.method == QUIT {
                        // The session is flushed when the window closes (`on_exit`).
                        pending.reply(Ok(json!({})));
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                        return;
                    }
                    let conn = self.conns.entry(pending.conn).or_default();
                    let mut host = WinHost { app, ctx, perf, window };
                    let outcome = conn.handle(&self.control, &mut host, &pending.req);
                    self.proceed(Some(pending), Instant::now(), outcome, ctx);
                }
                Job::Release(input) => {
                    let then: Then = Box::new(|_, _| Outcome::Reply(Ok(Value::Null)));
                    self.proceed(None, Instant::now(), Outcome::Play(input, then), ctx);
                }
                Job::Continue(pending, started, outcome) => self.proceed(pending, started, outcome, ctx),
            }
        }
        // Many quick replies in one tick: let the frame run, and come back.
        ctx.request_repaint();
    }

    /// The next request (or orphan play) to start.
    fn next_job(&mut self) -> Option<Job> {
        if let Some(input) = self.orphans.pop_front() {
            return Some(Job::Release(input));
        }
        loop {
            match self.hub.rx.try_recv() {
                Ok(Msg::Request(p)) => {
                    if p.dispatch() {
                        return Some(Job::Start(p));
                    }
                }
                Ok(Msg::Closed(id)) => {
                    if let Some(mut conn) = self.conns.remove(&id) {
                        self.orphans.extend(conn.release());
                        if let Some(input) = self.orphans.pop_front() {
                            return Some(Job::Release(input));
                        }
                    }
                }
                Err(_) => return None,
            }
        }
    }

    /// Acts on an outcome: replies, starts a play or starts work.
    fn proceed(&mut self, pending: Option<Pending>, started: Instant, outcome: Outcome, ctx: &egui::Context) {
        match outcome {
            Outcome::Reply(r) => {
                if let Some(p) = pending {
                    p.reply(r);
                }
            }
            Outcome::Play(input, then) => match self.plan(ctx, &input) {
                Ok(play) => {
                    self.active = Some(Active { pending, started, stage: Stage::Playing(play, then) });
                    ctx.request_repaint();
                }
                // Refused before any frame: the continuation answers in this same tick.
                Err(e) => self.active = Some(Active { pending, started, stage: Stage::Played(Err(e), then) }),
            },
            Outcome::Work(work) => {
                let (tx, rx) = mpsc::channel();
                let wake = ctx.clone();
                let spawned = std::thread::Builder::new().name("dalica-control-work".into()).spawn(move || {
                    let next = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
                        Ok(n) => n,
                        Err(_) => Next::Reply(Err(Error::new(Code::Internal, "a control worker panicked (logged; the app keeps running)"))),
                    };
                    let _ = tx.send(next);
                    wake.request_repaint();
                });
                match spawned {
                    Ok(_) => self.active = Some(Active { pending, started, stage: Stage::Working(rx) }),
                    Err(e) => {
                        if let Some(p) = pending {
                            p.reply(Err(Error::new(Code::Internal, format!("cannot start a worker: {e}"))));
                        }
                    }
                }
            }
        }
    }

    /// Before a frame: injects the current step's events; picks up a screenshot that came back.
    pub fn raw_input(&mut self, raw: &mut egui::RawInput) {
        let Some(Active { stage: Stage::Playing(play, _), .. }) = &mut self.active else { return };
        if let Some(Step::Capture { sent: true, .. }) = play.steps.front() {
            let shot = raw.events.iter().find_map(|e| match e {
                Event::Screenshot { image, user_data, .. } if is_ours(user_data) => Some(image.clone()),
                _ => None,
            });
            if let Some(image) = shot {
                let [w, h] = image.size;
                let rgba = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                play.image = Some(Image { width: u32::try_from(w).unwrap_or(0), height: u32::try_from(h).unwrap_or(0), rgba });
                play.steps.pop_front();
            }
        }
        if let Some(Step::Events { events, injected }) = play.steps.front_mut()
            && !*injected
        {
            raw.events.append(events);
            *injected = true;
        }
    }

    /// After a frame has been laid out: advances the play.
    pub fn after_frame(&mut self, app: &App, ctx: &egui::Context) {
        self.last_frame = Some(Instant::now());
        let Some(active) = &mut self.active else { return };
        let Stage::Playing(play, _) = &mut active.stage else { return };
        advance(play, app, ctx);
        if play.done()
            && let Some(Active { pending, started, stage: Stage::Playing(play, then) }) = self.active.take()
        {
            self.active = Some(Active { pending, started, stage: Stage::Played(play.result(), then) });
        }
        // Keep frames coming while input plays, and run the continuation next tick.
        ctx.request_repaint();
    }

    /// Turns checked input into frame steps, as the headless driver plays it.
    fn plan(&mut self, ctx: &egui::Context, input: &Input) -> Result<Play, DriverError> {
        let mut p = Play::new(Kind::Done);
        let held = self.held;
        match input {
            Input::Click { id, click } if click.count > 3 => {
                let pos = point_in(ctx, id, (click.fx, click.fy), true)?;
                let mut events = vec![PointerEvent { kind: PointerKind::Move, pos }];
                for _ in 0..click.count {
                    events.push(PointerEvent { kind: PointerKind::Down, pos });
                    events.push(PointerEvent { kind: PointerKind::Up, pos });
                }
                self.pointer_steps(&mut p, &events, click.button, click.modifiers)?;
            }
            Input::Click { id, click } => self.click(&mut p, ctx, id, click)?,
            Input::Drag { from, at, to, drag } if *at == (0.5, 0.5) => self.drag(&mut p, ctx, from, to, drag)?,
            Input::Drag { from, at, to, drag } => {
                let start = point_in(ctx, from, *at, true)?;
                let end = match to {
                    DragTo::Widget(id) => point_in(ctx, id, (0.5, 0.5), false)?,
                    DragTo::Point(pt) => *pt,
                    DragTo::Delta(v) => start + *v,
                };
                let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: start }, PointerEvent { kind: PointerKind::Down, pos: start }];
                for i in 1..=drag.steps {
                    events.push(PointerEvent { kind: PointerKind::Drag, pos: start.lerp(end, i as f32 / drag.steps as f32) });
                }
                events.push(PointerEvent { kind: PointerKind::Up, pos: end });
                self.pointer_steps(&mut p, &events, drag.button, drag.modifiers)?;
            }
            Input::Hover { id, at, ms } => {
                let pos = point_in(ctx, id, *at, false)?;
                self.pointer = Some(pos);
                p.frame(vec![Event::PointerMoved(pos)]);
                p.steps.push_back(Step::Wait { until: Instant::now() + Duration::from_millis(*ms) });
            }
            Input::Pointer { events, button, modifiers } => self.pointer_steps(&mut p, events, *button, *modifiers)?,
            Input::Scroll { at, delta } => {
                let pos = match at {
                    Some(id) => point_in(ctx, id, (0.5, 0.5), false)?,
                    None => self.pointer.unwrap_or_else(|| ctx.content_rect().center()),
                };
                if !(delta.x.is_finite() && delta.y.is_finite() && delta.x.abs() <= MAX_COORD && delta.y.abs() <= MAX_COORD) {
                    return Err(DriverError::BadInput(format!("the scroll ({}, {}) is outside ±{MAX_COORD} points", delta.x, delta.y)));
                }
                self.pointer = Some(pos);
                let wheel = |delta, phase| Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers: held };
                p.frame(vec![Event::PointerMoved(pos), wheel(Vec2::ZERO, TouchPhase::Start), wheel(*delta, TouchPhase::Move)]);
                p.frame(vec![wheel(Vec2::ZERO, TouchPhase::End)]);
            }
            Input::Key { key, modifiers, state } => {
                let press = |pressed| Event::Key { key: *key, physical_key: None, pressed, repeat: false, modifiers: *modifiers };
                match state {
                    KeyState::Press => {
                        p.frame(vec![Event::ModifiersChanged(*modifiers), press(true)]);
                        p.frame(vec![press(false), Event::ModifiersChanged(held)]);
                    }
                    KeyState::Down => {
                        self.held = *modifiers;
                        p.frame(vec![Event::ModifiersChanged(*modifiers), press(true)]);
                        p.frame(vec![]);
                    }
                    KeyState::Up => {
                        self.held = Modifiers::NONE;
                        p.frame(vec![press(false), Event::ModifiersChanged(Modifiers::NONE)]);
                        p.frame(vec![]);
                    }
                }
            }
            Input::Text(text) => {
                if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
                    return Err(DriverError::BadInput("text with control characters other than newline and tab; press keys for those".into()));
                }
                p.frame(vec![Event::Text(text.clone())]);
                p.frame(vec![]);
            }
            Input::Resize { width, height } => {
                p.steps.push_back(Step::Resize { size: Vec2::new(*width, *height), sent: false, until: Instant::now() + WINDOW_WAIT });
                p.steps.push_back(Step::Frames(1));
            }
            Input::Wait { ms } => {
                p.kind = Kind::Frames;
                p.steps.push_back(Step::Wait { until: Instant::now() + Duration::from_millis(*ms) });
            }
            Input::Settle { timeout_ms, jobs } => {
                p.kind = Kind::Settled;
                p.steps.push_back(settle(*timeout_ms, *jobs));
            }
            Input::Frames(n) => {
                p.kind = Kind::Frames;
                p.frames = *n;
                p.steps.push_back(Step::Frames(*n));
            }
            Input::Screenshot { settle_ms, scale } => {
                let ppp = ctx.pixels_per_point();
                if (scale - ppp).abs() > 1e-3 {
                    return Err(DriverError::BadInput(format!(
                        "the window captures at its own scale, {ppp} pixels per point; leave out `scale`, or use `--headless` for another one"
                    )));
                }
                p.kind = Kind::Shot;
                match settle_ms {
                    Some(ms) => p.steps.push_back(settle(*ms, false)),
                    None => p.steps.push_back(Step::Frames(1)),
                }
                p.steps.push_back(Step::Capture { sent: false, until: Instant::now() + WINDOW_WAIT });
            }
        }
        Ok(p)
    }

    /// The driver's `click_with`: press and release `count` times, one frame each, then the
    /// pointer leaves.
    fn click(&mut self, p: &mut Play, ctx: &egui::Context, id: &str, c: &Click) -> Result<(), DriverError> {
        if !(0.0..=1.0).contains(&c.fx) || !(0.0..=1.0).contains(&c.fy) {
            return Err(DriverError::BadInput(format!("fx and fy are fractions 0..1, not {} and {}", c.fx, c.fy)));
        }
        if !(1..=3).contains(&c.count) {
            return Err(DriverError::BadInput(format!("count is 1, 2 or 3, not {}", c.count)));
        }
        let pos = point_in(ctx, id, (c.fx, c.fy), true)?;
        let mods = c.modifiers | self.held;
        let button = |pressed| Event::PointerButton { pos, button: c.button, pressed, modifiers: mods };
        for i in 0..c.count {
            let mut down = vec![button(true)];
            if i == 0 {
                down.splice(0..0, [Event::ModifiersChanged(mods), Event::PointerMoved(pos)]);
            }
            p.frame(down);
            p.frame(vec![button(false)]);
        }
        p.frame(vec![Event::ModifiersChanged(self.held), Event::PointerGone]);
        self.pointer = None;
        Ok(())
    }

    /// The driver's `drag`: move to the start, press, `steps` moves, release, leave.
    fn drag(&mut self, p: &mut Play, ctx: &egui::Context, from: &str, to: &DragTo, d: &Drag) -> Result<(), DriverError> {
        let start = point_in(ctx, from, (0.5, 0.5), true)?;
        let end = match to {
            DragTo::Widget(id) => point_in(ctx, id, (0.5, 0.5), false)?,
            DragTo::Point(pt) => *pt,
            DragTo::Delta(v) => start + *v,
        };
        check_point(end)?;
        if !(1..=MAX_STEPS).contains(&d.steps) {
            return Err(DriverError::BadInput(format!("steps is 1..={MAX_STEPS}, not {}", d.steps)));
        }
        let mods = d.modifiers | self.held;
        p.frame(vec![Event::ModifiersChanged(mods), Event::PointerMoved(start)]);
        p.frame(vec![Event::PointerButton { pos: start, button: d.button, pressed: true, modifiers: mods }]);
        for i in 1..=d.steps {
            p.frame(vec![Event::PointerMoved(start.lerp(end, i as f32 / d.steps as f32))]);
        }
        p.frame(vec![Event::PointerButton { pos: end, button: d.button, pressed: false, modifiers: mods }]);
        p.frame(vec![Event::ModifiersChanged(self.held), Event::PointerGone]);
        self.pointer = None;
        Ok(())
    }

    /// The driver's `pointer`: one frame per event, then one more; the pointer stays.
    fn pointer_steps(&mut self, p: &mut Play, events: &[PointerEvent], button: PointerButton, modifiers: Modifiers) -> Result<(), DriverError> {
        if events.len() > MAX_STEPS {
            return Err(DriverError::BadInput(format!("at most {MAX_STEPS} events")));
        }
        for e in events {
            check_point(e.pos)?;
        }
        let mods = modifiers | self.held;
        for (i, e) in events.iter().enumerate() {
            let mut frame = if i == 0 { vec![Event::ModifiersChanged(mods)] } else { vec![] };
            frame.push(Event::PointerMoved(e.pos));
            match e.kind {
                PointerKind::Move | PointerKind::Drag => {}
                PointerKind::Down => frame.push(Event::PointerButton { pos: e.pos, button, pressed: true, modifiers: mods }),
                PointerKind::Up => frame.push(Event::PointerButton { pos: e.pos, button, pressed: false, modifiers: mods }),
            }
            p.frame(frame);
            self.pointer = Some(e.pos);
        }
        p.frame(vec![Event::ModifiersChanged(self.held)]);
        Ok(())
    }
}

enum Job {
    Start(Pending),
    Release(Input),
    Continue(Option<Pending>, Instant, Outcome),
}

fn settle(timeout_ms: u64, jobs: bool) -> Step {
    let now = Instant::now();
    Step::Settle { until: now + Duration::from_millis(timeout_ms), started: now, quiet: 0, frames: 0, jobs }
}

fn check_point(p: Pos2) -> Result<Pos2, DriverError> {
    if p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD {
        Ok(p)
    } else {
        Err(DriverError::BadInput(format!("the point ({}, {}) is outside ±{MAX_COORD} points", p.x, p.y)))
    }
}

/// A point of widget `id` (fractions `at` of its size), from the last frame's widgets.
fn point_in(ctx: &egui::Context, id: &str, at: (f32, f32), enabled: bool) -> Result<Pos2, DriverError> {
    let w = registry::find(ctx, id).ok_or_else(|| DriverError::NoWidget(id.into()))?;
    if enabled && !w.enabled {
        return Err(DriverError::Disabled(id.into()));
    }
    Ok(Pos2::new(w.rect[0] + w.rect[2] * at.0, w.rect[1] + w.rect[3] * at.1))
}

/// The marker on our screenshot requests.
#[derive(Debug)]
struct Ours;

fn is_ours(data: &egui::UserData) -> bool {
    data.data.as_ref().is_some_and(|d| d.downcast_ref::<Ours>().is_some())
}

/// What is still pending, as `ui.settle` lists it (the headless driver's list, without its queue).
/// `jobs` also counts metadata writes into photo files still to land (their debounce included; not
/// while a root is offline, whose writes wait for it), as the headless driver does.
pub fn busy(app: &App, jobs: bool) -> Vec<String> {
    let mut busy = Vec::new();
    match app.session() {
        None => busy.push("library: opening".into()),
        Some(s) if s.busy() => busy.push("library: scanning".into()),
        Some(_) => {}
    }
    if let Some(s) = app.session() {
        let writes = s.pending_metadata_writes();
        if jobs && writes > 0 && s.offline_roots().is_empty() {
            busy.push(format!("metadata writes: {writes}"));
        }
    }
    if let Some(p) = app.panels() {
        let pending = p.thumbnails.pending();
        if pending > 0 {
            busy.push(format!("visible thumbnails: {pending}"));
        }
        if p.dialog.is_some() {
            busy.push("file dialog: waiting for an answer".into());
        }
    }
    busy
}

/// One frame has been laid out: moves the play on.
fn advance(play: &mut Play, app: &App, ctx: &egui::Context) {
    let Some(step) = play.steps.front_mut() else { return };
    let finished = match step {
        Step::Events { injected, .. } => *injected,
        Step::Frames(n) => {
            *n = n.saturating_sub(1);
            *n == 0
        }
        Step::Wait { until } => {
            play.frames += 1;
            Instant::now() >= *until
        }
        Step::Settle { until, started, quiet, frames, jobs } => {
            *frames += 1;
            let busy = busy(app, *jobs);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            if busy.is_empty() {
                *quiet += 1;
                if *quiet >= QUIET_FRAMES {
                    play.settled = Some(Settled { settled: true, frames: *frames, ms, busy });
                    true
                } else {
                    false
                }
            } else {
                *quiet = 0;
                if Instant::now() >= *until {
                    play.settled = Some(Settled { settled: false, frames: *frames, ms, busy });
                    true
                } else {
                    false
                }
            }
        }
        Step::Resize { size, sent, until } => {
            if !*sent {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(*size));
                *sent = true;
                false
            } else {
                let now = ctx.content_rect().size();
                (now - *size).length() < 1.0 || Instant::now() >= *until
            }
        }
        Step::Capture { sent, until } => {
            if !*sent {
                ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::new(Ours)));
                *sent = true;
            } else if Instant::now() >= *until {
                play.error = Some(DriverError::Timeout(0));
            }
            // The image arrives in a later frame's raw input (`raw_input`), which ends this step.
            false
        }
    };
    if finished {
        play.steps.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ctx_with(widgets: &[(&str, [f32; 4], bool)]) -> egui::Context {
        let ctx = egui::Context::default();
        // Dalica's fonts (egui has none of its own here).
        App::new(None).install(&ctx);
        let ws: Vec<(String, egui::Rect, bool)> = widgets
            .iter()
            .map(|(id, r, en)| (id.to_string(), egui::Rect::from_min_size(egui::pos2(r[0], r[1]), egui::vec2(r[2], r[3])), *en))
            .collect();
        // Two passes: the registry lists the last completed one.
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                for (id, rect, enabled) in &ws {
                    let info = registry::WidgetInfo::new(id, registry::Role::Button, id, *rect, *enabled, false);
                    registry::register(ui.ctx(), info);
                }
            });
            out.textures_delta.clear();
        }
        ctx
    }

    fn control() -> WindowControl {
        let l = super::super::transport::bind(0).unwrap();
        let token = super::super::token::Token::generate().unwrap();
        let hub = super::super::transport::start(l, token, Arc::new(|| {})).unwrap();
        WindowControl::new(hub, Control::new(Default::default()))
    }

    fn events(p: &Play) -> Vec<Vec<Event>> {
        p.steps
            .iter()
            .map(|s| match s {
                Step::Events { events, .. } => events.clone(),
                other => panic!("not an events step: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_click_plays_like_the_driver_one_frame_per_press_and_release() {
        let ctx = ctx_with(&[("button:star", [10.0, 20.0, 40.0, 20.0], true), ("button:off", [0.0, 0.0, 5.0, 5.0], false)]);
        let mut c = control();
        let click = Click { count: 2, ..Click::default() };
        let p = c.plan(&ctx, &Input::Click { id: "button:star".into(), click }).unwrap();
        let f = events(&p);
        assert_eq!(f.len(), 5);
        let pos = egui::pos2(30.0, 30.0);
        assert_eq!(f[0][1], Event::PointerMoved(pos));
        assert!(matches!(f[0][2], Event::PointerButton { pressed: true, .. }));
        assert!(matches!(f[1][0], Event::PointerButton { pressed: false, .. }));
        assert!(matches!(f[2][0], Event::PointerButton { pressed: true, .. }));
        assert_eq!(f[4][1], Event::PointerGone);
        assert!(matches!(c.plan(&ctx, &Input::Click { id: "button:off".into(), click: Click::default() }), Err(DriverError::Disabled(_))));
        assert!(matches!(c.plan(&ctx, &Input::Click { id: "button:nope".into(), click: Click::default() }), Err(DriverError::NoWidget(_))));
        let bad = Click { fx: 1.5, ..Click::default() };
        assert!(matches!(c.plan(&ctx, &Input::Click { id: "button:star".into(), click: bad }), Err(DriverError::BadInput(_))));
        // Many presses are a raw gesture: a move, then a frame per press and per release, then one.
        let many = Click { count: 5, ..Click::default() };
        assert_eq!(events(&c.plan(&ctx, &Input::Click { id: "button:star".into(), click: many }).unwrap()).len(), 12);
    }

    #[test]
    fn keys_hold_modifiers_until_released() {
        let ctx = ctx_with(&[]);
        let mut c = control();
        let ctrl = Modifiers::COMMAND;
        let p = c.plan(&ctx, &Input::Key { key: egui::Key::Num8, modifiers: ctrl, state: KeyState::Press }).unwrap();
        let f = events(&p);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0][0], Event::ModifiersChanged(ctrl));
        assert_eq!(f[1][1], Event::ModifiersChanged(Modifiers::NONE));
        c.plan(&ctx, &Input::Key { key: egui::Key::Space, modifiers: Modifiers::SHIFT, state: KeyState::Down }).unwrap();
        assert_eq!(c.held, Modifiers::SHIFT);
        c.plan(&ctx, &Input::Key { key: egui::Key::Space, modifiers: Modifiers::NONE, state: KeyState::Up }).unwrap();
        assert_eq!(c.held, Modifiers::NONE);
    }

    #[test]
    fn hostile_input_is_refused_before_any_frame() {
        let ctx = ctx_with(&[("tray", [0.0, 0.0, 10.0, 10.0], true)]);
        let mut c = control();
        let far = PointerEvent { kind: PointerKind::Move, pos: egui::pos2(f32::NAN, 0.0) };
        let none = Modifiers::NONE;
        assert!(c.plan(&ctx, &Input::Pointer { events: vec![far], button: PointerButton::Primary, modifiers: none }).is_err());
        let many = vec![PointerEvent { kind: PointerKind::Move, pos: egui::pos2(1.0, 1.0) }; MAX_STEPS + 1];
        assert!(c.plan(&ctx, &Input::Pointer { events: many, button: PointerButton::Primary, modifiers: none }).is_err());
        assert!(c.plan(&ctx, &Input::Scroll { at: None, delta: egui::vec2(f32::INFINITY, 0.0) }).is_err());
        assert!(c.plan(&ctx, &Input::Text("a\u{7}".into())).is_err());
        let drag = Drag { steps: 0, ..Drag::default() };
        assert!(c.plan(&ctx, &Input::Drag { from: "tray".into(), at: (0.5, 0.5), to: DragTo::Delta(egui::vec2(5.0, 5.0)), drag }).is_err());
        let far = DragTo::Point(egui::pos2(2.0e6, 0.0));
        assert!(c.plan(&ctx, &Input::Drag { from: "tray".into(), at: (0.5, 0.5), to: far, drag: Drag::default() }).is_err());
        // A screenshot at another scale than the window's is refused (the window cannot rescale).
        assert!(c.plan(&ctx, &Input::Screenshot { settle_ms: None, scale: 3.0 }).is_err());
        assert!(c.plan(&ctx, &Input::Screenshot { settle_ms: None, scale: ctx.pixels_per_point() }).is_ok());
    }

    #[test]
    fn frames_and_waits_advance_frame_by_frame() {
        let ctx = ctx_with(&[]);
        let app = App::new(None);
        let mut c = control();
        let mut p = c.plan(&ctx, &Input::Frames(3)).unwrap();
        for _ in 0..3 {
            assert!(!p.done());
            advance(&mut p, &app, &ctx);
        }
        assert!(p.done());
        assert_eq!(p.result().unwrap(), Played::Frames(3));
        let mut p = c.plan(&ctx, &Input::Wait { ms: 0 }).unwrap();
        advance(&mut p, &app, &ctx);
        assert_eq!(p.result().unwrap(), Played::Frames(1));
        // No library yet: a settle says so when it times out.
        let mut p = c.plan(&ctx, &Input::Settle { timeout_ms: 0, jobs: false }).unwrap();
        advance(&mut p, &app, &ctx);
        match p.result().unwrap() {
            Played::Settled(s) => assert!(!s.settled && s.busy == ["library: opening"]),
            other => panic!("{other:?}"),
        }
    }

    /// `ui.settle {jobs: true}` waits for metadata writes into photo files, as the headless
    /// driver's settle does; without `jobs` it does not.
    #[test]
    fn settle_with_jobs_counts_metadata_writes() {
        let dir = tempfile::tempdir().unwrap();
        let spec = dalica_engine::synthetic::Spec::new(2, 1, 7);
        let mut s = dalica_engine::synthetic::open_scanned(dir.path(), &spec, 64, Arc::new(dalica_core::SystemClock)).unwrap();
        s.wait_idle();
        let ids: Vec<_> = s.library().photos().map(|p| p.id).collect();
        s.execute("photo.caption.set", &serde_json::json!({ "ids": ids, "caption": "Harbour" })).unwrap();
        assert_eq!(s.pending_metadata_writes(), 2);
        let app = App::new(Some(s));
        assert_eq!(busy(&app, true), ["metadata writes: 2"]);
        assert!(busy(&app, false).is_empty(), "without jobs the writes do not count");
    }

    /// `tests/acceptance/m2.md`: a screenshot of a minimised or covered window answers `timeout`
    /// within about 2 s, without raising the window; a window that draws keeps playing.
    #[test]
    fn a_window_that_draws_no_frames_answers_timeout_within_2_s() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        assert!(!stalled(t0, None, at(1900)));
        assert!(stalled(t0, None, at(2100)), "no frame since the request started");
        assert!(!stalled(t0, Some(at(1500)), at(3000)), "a frame came after it started");
        assert!(stalled(t0, Some(at(1500)), at(3600)));
        assert!(!stalled(at(5000), Some(t0), at(6000)), "an old frame does not count against a new request");

        // The whole path: a screenshot play queued, no frame ever drawn.
        let ctx = ctx_with(&[]);
        let mut app = App::new(None);
        let mut c = control();
        let mut perf = Perf::new(false);
        let p = c.plan(&ctx, &Input::Screenshot { settle_ms: Some(5000), scale: ctx.pixels_per_point() }).unwrap();
        let then: Then = Box::new(|_, played| match played {
            Ok(_) => Outcome::Reply(Ok(Value::Null)),
            Err(e) => Outcome::Reply(Err(e.into())),
        });
        let started = Instant::now().checked_sub(Duration::from_millis(2500)).unwrap();
        c.active = Some(Active { pending: None, started, stage: Stage::Playing(p, then) });
        c.logic(&mut app, &ctx, &mut perf);
        assert!(c.active.is_none(), "answered (timeout) and done, not left waiting for frames");
    }

    #[test]
    fn events_are_injected_once_and_consumed_by_the_next_frame() {
        let ctx = ctx_with(&[]);
        let app = App::new(None);
        let mut c = control();
        let p = c.plan(&ctx, &Input::Text("hi".into())).unwrap();
        let then: Then = Box::new(|_, _| Outcome::Reply(Ok(Value::Null)));
        c.active = Some(Active { pending: None, started: Instant::now(), stage: Stage::Playing(p, then) });
        let mut raw = egui::RawInput::default();
        c.raw_input(&mut raw);
        assert_eq!(raw.events, vec![Event::Text("hi".into())]);
        let mut again = egui::RawInput::default();
        c.raw_input(&mut again);
        assert!(again.events.is_empty(), "injected once");
        c.after_frame(&app, &ctx);
        c.raw_input(&mut again);
        c.after_frame(&app, &ctx);
        assert!(matches!(c.active, Some(Active { stage: Stage::Played(Ok(Played::Done), _), .. })));
    }
}
