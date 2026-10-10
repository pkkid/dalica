//! The headless driver (`architecture.md` §7): runs the real [`App`] against an egui `Context` with
//! no window, on a virtual clock, and paints with the CPU rasterizer. Tests, `dalica-cli snapshot`
//! and (later) the control channel's headless mode drive the app through it: by widget id, by key,
//! by command, never by OS input.
//!
//! Every input method queues real egui events and returns only after they have been consumed by a
//! frame and one more frame has been laid out (`control-protocol.md` §5.2), so the next call
//! observes the effect. Input is checked before anything is queued: a refused call runs no frame.
//! Each frame is measured into [`Perf`] (`ui.inspect` → `perf`).

use std::time::{Duration, Instant};

use dalica_engine::{CmdError, Key};
use dalica_ui_kit::{Image, RasterError, Rasterizer, WidgetInfo, registry};
use egui::epaint::ClippedPrimitive;
use egui::{Event, Modifiers, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, ViewportId};
use serde::Serialize;
use serde_json::Value;

use crate::app::App;
use crate::perf::{self, Perf, Sample};
use crate::state::UiState;

/// One frame of the virtual clock (60 Hz).
pub const FRAME_SECONDS: f64 = 1.0 / 60.0;
/// Consecutive frames with nothing visible pending before [`Driver::settle`] reports settled
/// (`control-protocol.md` §6.12).
pub const QUIET_FRAMES: usize = 12;
/// The most intermediate moves of a drag and the most events of one raw gesture.
pub const MAX_STEPS: usize = 10_000;
/// The longest [`Driver::wait`] and [`Driver::hover`] rest: ten minutes of virtual time.
pub const MAX_WAIT_MS: u64 = 600_000;
/// The farthest a pointer position or a scroll may reach, in points.
const MAX_COORD: f32 = 1.0e6;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("no widget `{0}` in the last frame")]
    NoWidget(String),
    #[error("widget `{0}` is disabled")]
    Disabled(String),
    #[error("the condition did not hold within {0} frames")]
    Timeout(usize),
    #[error("not settled after {frames} frames; still busy: {}", busy.join(", "))]
    NotSettled { frames: usize, busy: Vec<String> },
    #[error("cannot press `{0}`: {1}")]
    BadKey(String, String),
    #[error("bad input: {0}")]
    BadInput(String),
    #[error(transparent)]
    Command(#[from] CmdError),
    #[error(transparent)]
    Raster(#[from] RasterError),
}

/// Press and release, or only one half of it (hold gestures, `control-protocol.md` §6.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyState {
    #[default]
    Press,
    Down,
    Up,
}

/// How [`Driver::click_with`] clicks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    /// The point inside the widget, as fractions of its width and height (0..=1).
    pub fx: f32,
    pub fy: f32,
    /// 1 click, 2 double-click, 3 triple-click.
    pub count: u32,
    pub button: PointerButton,
    pub modifiers: Modifiers,
}

impl Default for Click {
    fn default() -> Click {
        Click { fx: 0.5, fy: 0.5, count: 1, button: PointerButton::Primary, modifiers: Modifiers::NONE }
    }
}

/// Where a drag ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target<'a> {
    /// The centre of another widget.
    Widget(&'a str),
    /// A screen point.
    Point(Pos2),
    /// An offset from where the drag starts.
    Delta(Vec2),
}

/// How [`Driver::drag`] drags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    /// Intermediate pointer moves, one frame each (1..=[`MAX_STEPS`]).
    pub steps: usize,
    pub button: PointerButton,
    pub modifiers: Modifiers,
}

impl Default for Drag {
    fn default() -> Drag {
        Drag { steps: 12, button: PointerButton::Primary, modifiers: Modifiers::NONE }
    }
}

/// One event of a raw gesture (`ui.pointer`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    Move,
    Down,
    Drag,
    Up,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerEvent {
    pub kind: PointerKind,
    /// Screen points.
    pub pos: Pos2,
}

/// What [`Driver::settle_for`] found (`ui.settle`'s reply).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settled {
    pub settled: bool,
    /// Frames run.
    pub frames: usize,
    /// Virtual time those frames took.
    pub ms: f64,
    /// What was still pending when `settled` is false; empty when settled.
    pub busy: Vec<String>,
}

/// The egui modifiers of a chord written the docs' way: `ctrl` is the command modifier (Command on
/// macOS); `mac_ctrl` is the physical Control key on macOS (Ctrl elsewhere).
pub fn modifiers(ctrl: bool, shift: bool, alt: bool, mac_ctrl: bool) -> Modifiers {
    let mac = cfg!(target_os = "macos");
    Modifiers { alt, ctrl: (ctrl && !mac) || mac_ctrl, shift, mac_cmd: ctrl && mac, command: ctrl || (mac_ctrl && !mac) }
}

pub struct Driver {
    ctx: egui::Context,
    app: App,
    raster: Rasterizer,
    width: u32,
    height: u32,
    pixels_per_point: f32,
    time: f64,
    frames: u64,
    events: Vec<Event>,
    /// Where the pointer is, if it is over the window.
    pointer: Option<Pos2>,
    /// Modifiers held down by [`KeyState::Down`].
    held: Modifiers,
    /// The last frame asked for another one at once (an animation is running).
    animating: bool,
    /// The last frame, tessellated, painted on demand by [`Driver::screenshot`].
    primitives: Vec<ClippedPrimitive>,
    /// Textures the last frame released, freed before the next frame's uploads.
    to_free: egui::TexturesDelta,
    perf: Perf,
    /// Test-only UI drawn over the app each frame (widgets M1's window does not have yet).
    #[cfg(test)]
    overlay: Option<Overlay>,
}

#[cfg(test)]
type Overlay = Box<dyn FnMut(&mut egui::Ui)>;

/// egui's name for one of our key codes.
fn egui_key(code: &str) -> Option<egui::Key> {
    let name = match code {
        "Esc" => "Escape",
        "Plus" => "+",
        other => other,
    };
    egui::Key::from_name(name)
}

fn check_point(p: Pos2) -> Result<Pos2, DriverError> {
    if p.x.is_finite() && p.y.is_finite() && p.x.abs() <= MAX_COORD && p.y.abs() <= MAX_COORD {
        Ok(p)
    } else {
        Err(DriverError::BadInput(format!("the point ({}, {}) is outside ±{MAX_COORD} points", p.x, p.y)))
    }
}

fn frames_for(ms: u64) -> Result<usize, DriverError> {
    if ms > MAX_WAIT_MS {
        return Err(DriverError::BadInput(format!("at most {MAX_WAIT_MS} ms")));
    }
    // Whole frames, rounded up: 16 ms is one frame, 17 ms two.
    Ok(usize::try_from((ms * 60).div_ceil(1000)).unwrap_or(usize::MAX))
}

impl Driver {
    /// A `width × height` pixel window at `pixels_per_point`, showing `app`; runs the first frame
    /// (fonts install) so the widget list is ready. `DALICA_PROFILE=1` logs slow frames.
    pub fn new(app: App, width: u32, height: u32, pixels_per_point: f32) -> Driver {
        let ctx = egui::Context::default();
        app.install(&ctx);
        let mut d = Driver {
            ctx,
            app,
            raster: Rasterizer::new(),
            width,
            height,
            pixels_per_point,
            time: 0.0,
            frames: 0,
            events: Vec::new(),
            pointer: None,
            held: Modifiers::NONE,
            animating: false,
            primitives: Vec::new(),
            to_free: egui::TexturesDelta::default(),
            perf: Perf::new(perf::profile_from_env()),
            #[cfg(test)]
            overlay: None,
        };
        d.step();
        d.step();
        d
    }

    /// Runs one frame: queued input, the app, texture uploads, tessellation. Advances the clock by
    /// one frame and records the frame's [`Sample`].
    pub fn step(&mut self) {
        let started = Instant::now();
        let mut input = dalica_ui_kit::raw_input(self.width, self.height, self.pixels_per_point);
        input.time = Some(self.time);
        input.events = std::mem::take(&mut self.events);
        let app = &mut self.app;
        #[cfg(test)]
        let overlay = &mut self.overlay;
        let out = self.ctx.run_ui(input, |ui| {
            app.frame(ui);
            #[cfg(test)]
            if let Some(o) = overlay {
                o(ui);
            }
        });
        let layout = started.elapsed();
        self.animating = out.viewport_output.get(&ViewportId::ROOT).is_some_and(|v| v.repaint_delay.is_zero());
        let (uploaded, freed) = self.perf.observe_textures(&out.textures_delta);
        self.raster.free_textures(&mut self.to_free);
        let mut delta = out.textures_delta;
        self.raster.set_textures(&mut delta);
        self.to_free = delta;
        let tessellating = Instant::now();
        self.primitives = self.ctx.tessellate(out.shapes, self.pixels_per_point);
        let tessellation = tessellating.elapsed();
        let mut sample = Sample {
            at: self.time,
            frame: layout + tessellation,
            update: started.elapsed(),
            textures_uploaded: uploaded,
            textures_freed: freed,
            ..Sample::default()
        };
        sample.count(&perf::reported(&self.ctx), &registry::widgets(&self.ctx));
        let _ = self.perf.record(sample);
        self.time += FRAME_SECONDS;
        self.frames += 1;
    }

    /// Steps until `done(app)` holds, at most `max_frames` frames; returns the frames it took.
    pub fn step_until(&mut self, mut done: impl FnMut(&App) -> bool, max_frames: usize) -> Result<usize, DriverError> {
        for n in 0..=max_frames {
            if done(&self.app) {
                return Ok(n);
            }
            if n < max_frames {
                self.step();
            }
        }
        Err(DriverError::Timeout(max_frames))
    }

    /// What is still pending, as `ui.settle` lists it: queued input, a library opening or
    /// scanning, visible thumbnails on their way, an open file dialog, a running animation. `jobs`
    /// also counts long jobs and background generation; none exist before M4, and scans always
    /// count.
    pub fn busy(&self, jobs: bool) -> Vec<String> {
        let _ = jobs;
        let mut busy = Vec::new();
        if !self.events.is_empty() {
            busy.push(format!("queued input: {}", self.events.len()));
        }
        match self.app.session() {
            None => busy.push("library: opening".into()),
            Some(s) if s.busy() => busy.push("library: scanning".into()),
            Some(_) => {}
        }
        if let Some(p) = self.app.panels() {
            let pending = p.thumbnails.pending();
            if pending > 0 {
                busy.push(format!("visible thumbnails: {pending}"));
            }
            if p.dialog.is_some() {
                busy.push("file dialog: waiting for an answer".into());
            }
        }
        if self.animating {
            busy.push("animation running".into());
        }
        busy
    }

    /// Runs frames until nothing visible has been pending for [`QUIET_FRAMES`] frames in a row, at
    /// most `max_frames` (`control-protocol.md` §6.12). While only workers are busy, each frame
    /// yields a millisecond to them.
    pub fn settle_for(&mut self, max_frames: usize, jobs: bool) -> Settled {
        let mut quiet = 0;
        let mut busy = Vec::new();
        for n in 1..=max_frames {
            self.step();
            busy = self.busy(jobs);
            if busy.is_empty() {
                quiet += 1;
                if quiet >= QUIET_FRAMES {
                    return Settled { settled: true, frames: n, ms: n as f64 * FRAME_SECONDS * 1000.0, busy };
                }
            } else {
                quiet = 0;
                if !self.animating && self.events.is_empty() {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
        }
        Settled { settled: false, frames: max_frames, ms: max_frames as f64 * FRAME_SECONDS * 1000.0, busy }
    }

    /// [`Driver::settle_for`] without long jobs; the frames it took, or what was still busy.
    pub fn settle(&mut self, max_frames: usize) -> Result<usize, DriverError> {
        let s = self.settle_for(max_frames, false);
        if s.settled { Ok(s.frames) } else { Err(DriverError::NotSettled { frames: s.frames, busy: s.busy }) }
    }

    /// Lets `ms` of virtual time pass (`ui.wait`): tooltips, auto-repeat, slideshow timing.
    /// Returns the frames run.
    pub fn wait(&mut self, ms: u64) -> Result<usize, DriverError> {
        let n = frames_for(ms)?;
        for _ in 0..n {
            self.step();
        }
        Ok(n)
    }

    fn widget(&self, id: &str) -> Result<WidgetInfo, DriverError> {
        registry::find(&self.ctx, id).ok_or_else(|| DriverError::NoWidget(id.into()))
    }

    fn enabled_widget(&self, id: &str) -> Result<WidgetInfo, DriverError> {
        let w = self.widget(id)?;
        if !w.enabled {
            return Err(DriverError::Disabled(id.into()));
        }
        Ok(w)
    }

    fn move_to(&mut self, pos: Pos2) {
        self.events.push(Event::PointerMoved(pos));
        self.pointer = Some(pos);
    }

    fn button(&mut self, pos: Pos2, button: PointerButton, pressed: bool, modifiers: Modifiers) {
        self.events.push(Event::PointerButton { pos, button, pressed, modifiers });
    }

    fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.events.push(Event::ModifiersChanged(modifiers));
    }

    fn pointer_gone(&mut self) {
        self.events.push(Event::PointerGone);
        self.pointer = None;
    }

    /// Clicks the centre of the widget with automation id `id` (press and release, one frame each),
    /// then moves the pointer away so no hover is left behind.
    pub fn click(&mut self, id: &str) -> Result<(), DriverError> {
        self.click_with(id, &Click::default())
    }

    /// Clicks widget `id` at a point inside it, `count` times, with a button and modifiers
    /// (`ui.clickWidget`).
    pub fn click_with(&mut self, id: &str, c: &Click) -> Result<(), DriverError> {
        let w = self.enabled_widget(id)?;
        if !(0.0..=1.0).contains(&c.fx) || !(0.0..=1.0).contains(&c.fy) {
            return Err(DriverError::BadInput(format!("fx and fy are fractions 0..1, not {} and {}", c.fx, c.fy)));
        }
        if !(1..=3).contains(&c.count) {
            return Err(DriverError::BadInput(format!("count is 1, 2 or 3, not {}", c.count)));
        }
        let pos = Pos2::new(w.rect[0] + w.rect[2] * c.fx, w.rect[1] + w.rect[3] * c.fy);
        let mods = c.modifiers | self.held;
        self.set_modifiers(mods);
        self.move_to(pos);
        for _ in 0..c.count {
            self.button(pos, c.button, true, mods);
            self.step();
            self.button(pos, c.button, false, mods);
            self.step();
        }
        self.set_modifiers(self.held);
        self.pointer_gone();
        self.step();
        Ok(())
    }

    /// Drags from the centre of widget `from` to `to` in `steps` moves, one frame each, then drops
    /// and moves the pointer away (`ui.dragWidget`).
    pub fn drag(&mut self, from: &str, to: Target<'_>, d: &Drag) -> Result<(), DriverError> {
        let start = self.enabled_widget(from)?.center();
        let end = match to {
            Target::Widget(id) => self.widget(id)?.center(),
            Target::Point(p) => p,
            Target::Delta(v) => start + v,
        };
        check_point(end)?;
        if !(1..=MAX_STEPS).contains(&d.steps) {
            return Err(DriverError::BadInput(format!("steps is 1..={MAX_STEPS}, not {}", d.steps)));
        }
        let mods = d.modifiers | self.held;
        self.set_modifiers(mods);
        self.move_to(start);
        self.step();
        self.button(start, d.button, true, mods);
        self.step();
        for i in 1..=d.steps {
            let t = i as f32 / d.steps as f32;
            self.move_to(start.lerp(end, t));
            self.step();
        }
        self.button(end, d.button, false, mods);
        self.step();
        self.set_modifiers(self.held);
        self.pointer_gone();
        self.step();
        Ok(())
    }

    /// Rests the pointer on the centre of widget `id` (enabled or not: disabled widgets have
    /// tooltips too), then lets `ms` pass (`ui.hoverWidget`). The pointer stays there.
    pub fn hover(&mut self, id: &str, ms: u64) -> Result<(), DriverError> {
        let pos = self.widget(id)?.center();
        let n = frames_for(ms)?;
        self.move_to(pos);
        self.step();
        for _ in 0..n.max(1) {
            self.step();
        }
        Ok(())
    }

    /// Plays a raw gesture (`ui.pointer`, screen space): one frame per event, then one more. The
    /// pointer stays where the last event left it.
    pub fn pointer(&mut self, events: &[PointerEvent], button: PointerButton, modifiers: Modifiers) -> Result<(), DriverError> {
        if events.len() > MAX_STEPS {
            return Err(DriverError::BadInput(format!("at most {MAX_STEPS} events")));
        }
        for e in events {
            check_point(e.pos)?;
        }
        let mods = modifiers | self.held;
        self.set_modifiers(mods);
        for e in events {
            self.move_to(e.pos);
            match e.kind {
                PointerKind::Move | PointerKind::Drag => {}
                PointerKind::Down => self.button(e.pos, button, true, mods),
                PointerKind::Up => self.button(e.pos, button, false, mods),
            }
            self.step();
        }
        self.set_modifiers(self.held);
        self.step();
        Ok(())
    }

    /// Scrolls by `delta` points over widget `at` (or wherever the pointer is, or the middle of the
    /// window), as a trackpad does: the whole delta in one frame, no smoothing (`ui.scroll`).
    /// Positive `y` moves the content down, revealing what is above.
    pub fn scroll(&mut self, at: Option<&str>, delta: Vec2) -> Result<(), DriverError> {
        let pos = match at {
            Some(id) => self.widget(id)?.center(),
            None => self.pointer.unwrap_or_else(|| self.ctx.content_rect().center()),
        };
        if !(delta.x.is_finite() && delta.y.is_finite() && delta.x.abs() <= MAX_COORD && delta.y.abs() <= MAX_COORD) {
            return Err(DriverError::BadInput(format!("the scroll ({}, {}) is outside ±{MAX_COORD} points", delta.x, delta.y)));
        }
        let modifiers = self.held;
        self.move_to(pos);
        let wheel = |delta, phase| Event::MouseWheel { unit: MouseWheelUnit::Point, delta, phase, modifiers };
        self.events.push(wheel(Vec2::ZERO, TouchPhase::Start));
        self.events.push(wheel(delta, TouchPhase::Move));
        self.step();
        self.events.push(wheel(Vec2::ZERO, TouchPhase::End));
        self.step();
        Ok(())
    }

    /// Presses and releases a key chord written as the keymap writes it (`Ctrl+8`, `F4`, `Esc`).
    /// `Ctrl` is the primary modifier: Command on macOS.
    pub fn key(&mut self, chord: &str) -> Result<(), DriverError> {
        self.key_state(chord, KeyState::Press)
    }

    /// A chord pressed, or only pressed down or released (`ui.key {combo, state}`).
    pub fn key_state(&mut self, chord: &str, state: KeyState) -> Result<(), DriverError> {
        let k = Key::parse(chord).map_err(|e| DriverError::BadKey(chord.into(), e))?;
        let key = egui_key(&k.code).ok_or_else(|| DriverError::BadKey(chord.into(), format!("egui has no key `{}`", k.code)))?;
        self.key_raw(key, modifiers(k.ctrl, k.shift, k.alt, false), state);
        Ok(())
    }

    /// An egui key with egui modifiers (`ui.key {key, ctrl?, …}`). [`KeyState::Down`] keeps the
    /// modifiers held for later input until the matching [`KeyState::Up`].
    pub fn key_raw(&mut self, key: egui::Key, modifiers: Modifiers, state: KeyState) {
        let press = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        match state {
            KeyState::Press => {
                self.set_modifiers(modifiers);
                self.events.push(press(true));
                self.step();
                self.events.push(press(false));
                self.set_modifiers(self.held);
                self.step();
            }
            KeyState::Down => {
                self.held = modifiers;
                self.set_modifiers(modifiers);
                self.events.push(press(true));
                self.step();
                self.step();
            }
            KeyState::Up => {
                self.held = Modifiers::NONE;
                self.events.push(press(false));
                self.set_modifiers(Modifiers::NONE);
                self.step();
                self.step();
            }
        }
    }

    /// Types `text` into the focused widget (`ui.type`). Control characters other than newline and
    /// tab are refused: press their keys instead.
    pub fn type_text(&mut self, text: &str) -> Result<(), DriverError> {
        if text.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            return Err(DriverError::BadInput("text with control characters other than newline and tab; press keys for those".into()));
        }
        self.events.push(Event::Text(text.into()));
        self.step();
        self.step();
        Ok(())
    }

    /// Runs a command as a button or menu item would, then one frame.
    pub fn run(&mut self, id: &str, params: &Value) -> Result<Value, DriverError> {
        let result = self.app.run(id, params);
        self.step();
        Ok(result?)
    }

    pub fn state(&self) -> &UiState {
        self.app.state()
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// The widgets of the last frame.
    pub fn widgets(&self) -> Vec<WidgetInfo> {
        registry::widgets(&self.ctx)
    }

    /// The egui context, for what the widget list does not say (focus, hover).
    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Where the pointer rests, if it is over the window.
    pub fn pointer_pos(&self) -> Option<Pos2> {
        self.pointer
    }

    /// Frames run so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// The frame counters (`ui.inspect` → `perf`).
    pub fn perf(&self) -> &Perf {
        &self.perf
    }

    pub fn perf_mut(&mut self) -> &mut Perf {
        &mut self.perf
    }

    /// Changes the window size; the next frame lays out for it.
    pub fn resize(&mut self, width: u32, height: u32) {
        (self.width, self.height) = (width, height);
        self.step();
    }

    /// The last frame, painted.
    pub fn screenshot(&self) -> Result<Image, DriverError> {
        Ok(self.raster.render(&self.primitives, self.pixels_per_point, self.width, self.height, self.app.tokens().chrome)?)
    }

    /// Ends the drive and hands back the app (to flush and close its session).
    pub fn into_app(mut self) -> App {
        std::mem::replace(&mut self.app, App::new(None))
    }

    /// Draws `ui` over the app every frame (tests of input on widgets the app does not have yet).
    #[cfg(test)]
    fn set_overlay(&mut self, ui: impl FnMut(&mut egui::Ui) + 'static) {
        self.overlay = Some(Box::new(ui));
        self.step();
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        // The textures die with the context; nothing is left to free.
        self.to_free.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_ui_kit::registry::Role;
    use egui::{Rect, Sense, vec2};
    use std::sync::{Arc, Mutex};

    fn count(img: &Image, f: impl Fn([u8; 4]) -> bool) -> usize {
        img.rgba.as_chunks::<4>().0.iter().filter(|&&p| f(p)).count()
    }

    /// A library of `folders` folders × `per_folder` photos, all copies of one small scenes JPEG (the
    /// light table only needs their headers), opened and scanned.
    fn library(folders: usize, per_folder: usize) -> (tempfile::TempDir, dalica_engine::Session) {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = dalica_scenes::encode::jpeg(&dalica_scenes::Scene::new(7, 48, 32), 80, None).unwrap();
        let mut s = dalica_engine::Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        for f in 0..folders {
            let folder = dir.path().join("photos").join(format!("2026-06-{:02} Trip", f + 1));
            std::fs::create_dir_all(&folder).unwrap();
            for i in 0..per_folder {
                std::fs::write(folder.join(format!("IMG_{i:04}.jpg")), &jpeg).unwrap();
            }
            s.execute("folders.add", &serde_json::json!({ "path": folder })).unwrap();
        }
        s.wait_idle();
        (dir, s)
    }

    /// The light-table cells of the last frame, in paint order. Looked up through the same rule
    /// the perf counters use, so it follows the widget-id grammar rather than one spelling.
    fn cells(d: &Driver) -> Vec<WidgetInfo> {
        d.widgets().into_iter().filter(|w| perf::visited_from_widgets(std::slice::from_ref(w)).0 == 1).collect()
    }

    fn find(d: &Driver, id: &str) -> WidgetInfo {
        d.widgets().into_iter().find(|w| w.id == id).unwrap()
    }

    /// The M1.17 done-when: an empty app, driven headlessly, gives a screenshot of the size asked.
    #[test]
    fn an_empty_app_is_driven_headlessly_and_screenshotted_at_the_requested_size() {
        let dir = tempfile::tempdir().unwrap();
        let session = dalica_engine::Session::open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
        let mut d = Driver::new(App::new(Some(session)), 900, 600, 1.0);
        assert!(d.settle(60).unwrap() >= QUIET_FRAMES);
        let shot = d.screenshot().unwrap();
        assert_eq!((shot.width, shot.height), (900, 600));
        let chrome = d.app().tokens().chrome.to_array();
        assert_eq!(shot.pixel(0, 0), Some(chrome));
        assert_eq!(shot.pixel(899, 599), Some(chrome));
        // The empty-state title is drawn, centred.
        let lit = count(&shot, |p| p[0] > 120);
        assert!(lit > 200, "{lit} text pixels");
        assert!(shot.pixel(450, 288).is_some());
        // Another size, another scale.
        d.resize(1400, 900);
        assert_eq!((d.screenshot().unwrap().width, d.screenshot().unwrap().height), (1400, 900));
        let mut big = Driver::new(App::new(None), 600, 400, 2.0);
        big.step();
        let s2 = big.screenshot().unwrap();
        assert_eq!((s2.width, s2.height), (600, 400));
        assert!(count(&s2, |p| p[0] > 120) > lit / 2, "text at 2× is drawn with 4× the pixels per glyph");
    }

    #[test]
    fn the_driver_is_deterministic_and_counts_frames() {
        let shot = || {
            let mut d = Driver::new(App::new(None), 320, 200, 1.0);
            d.step();
            (d.frames(), d.screenshot().unwrap())
        };
        let (fa, a) = shot();
        let (fb, b) = shot();
        assert_eq!(fa, 3);
        assert!(fa == fb && a == b, "same input, same pixels");
    }

    #[test]
    fn waiting_input_and_commands_fail_cleanly() {
        let mut d = Driver::new(App::new(None), 320, 200, 1.0);
        // Without a session the app is never idle: the wait times out after exactly its budget.
        let before = d.frames();
        assert!(matches!(d.step_until(App::idle, 5), Err(DriverError::Timeout(5))));
        assert_eq!(d.frames(), before + 5);
        assert_eq!(d.step_until(|_| true, 0).unwrap(), 0);
        assert!(matches!(d.click("no.such.widget"), Err(DriverError::NoWidget(_))));
        for bad in ["", "Hyper+X", "Ctrl+"] {
            assert!(matches!(d.key(bad), Err(DriverError::BadKey(..))), "{bad:?}");
        }
        for good in ["Ctrl+8", "F4", "Esc", "Ctrl+Plus", "Shift+Up", "Ctrl+,"] {
            d.key(good).unwrap();
        }
        assert!(matches!(d.run("photo.star.toggle", &serde_json::json!({ "ids": [1] })), Err(DriverError::Command(CmdError::Disabled(_)))));
        assert_eq!(d.run("select.none", &serde_json::json!({})).unwrap(), serde_json::json!({ "selection": [] }));
        assert_eq!(d.state().selection.ids.len(), 0);
        assert!(d.widgets().is_empty(), "the empty app registers no widgets");
        assert!(d.into_app().session().is_none());
    }

    /// A text field and a custom scroll bar (a track with a draggable thumb, as M4's speed-control
    /// `scroll:thumb` will be) drawn over the app, registered by id. Returns the field's text and the scroll
    /// offset.
    fn overlay(d: &mut Driver) -> (Arc<Mutex<String>>, Arc<Mutex<f32>>) {
        let text = Arc::new(Mutex::new(String::new()));
        let offset = Arc::new(Mutex::new(0.0f32));
        let (t, o) = (text.clone(), offset.clone());
        let mut grab: Option<f32> = None;
        d.set_overlay(move |ui| {
            egui::Area::new(egui::Id::new("test-overlay")).fixed_pos(Pos2::new(20.0, 40.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                let mut s = t.lock().unwrap();
                let field = ui.add(egui::TextEdit::singleline(&mut *s).desired_width(160.0));
                registry::register(ui.ctx(), WidgetInfo::new("field:test", Role::Other, "Test field", field.rect, true, false));
                // The track is 300 points tall, the thumb 40, moved by the pointer's travel since
                // the press, so no movement is lost to the drag threshold.
                let (track, _) = ui.allocate_exact_size(vec2(12.0, 300.0), Sense::hover());
                registry::register(ui.ctx(), WidgetInfo::new("box:test.track", Role::Other, "", track, true, false));
                let end = Rect::from_min_size(track.left_bottom() - vec2(0.0, 20.0), vec2(12.0, 20.0));
                registry::register(ui.ctx(), WidgetInfo::new("box:test.end", Role::Other, "", end, true, false));
                let mut off = o.lock().unwrap();
                let thumb = Rect::from_min_size(track.min + vec2(0.0, *off), vec2(12.0, 40.0));
                let r = ui.interact(thumb, egui::Id::new("box:test.thumb"), Sense::drag());
                if r.drag_started() {
                    grab = Some(*off);
                }
                if let (Some(start), Some(from), Some(now)) = (grab, ui.input(|i| i.pointer.press_origin()), r.interact_pointer_pos())
                    && r.dragged()
                {
                    *off = (start + now.y - from.y).clamp(0.0, 260.0);
                }
                if r.drag_stopped() {
                    grab = None;
                }
                registry::register(ui.ctx(), WidgetInfo::new("box:test.thumb", Role::Other, "", thumb, true, r.dragged()));
            });
        });
        (text, offset)
    }

    /// Drags by id to a delta, onto another widget, to a point, and the same as a raw gesture.
    #[test]
    fn a_custom_scroll_thumb_is_dragged_by_widget_id_to_a_point_a_delta_or_another_widget() {
        let mut d = Driver::new(App::new(None), 640, 480, 1.0);
        let (_, offset) = overlay(&mut d);
        let start = find(&d, "box:test.thumb");
        d.drag("box:test.thumb", Target::Delta(vec2(0.0, 120.0)), &Drag::default()).unwrap();
        assert_eq!(*offset.lock().unwrap(), 120.0);
        let moved = find(&d, "box:test.thumb");
        assert_eq!(moved.rect[1] - start.rect[1], 120.0, "the registry sees the thumb where it was dropped");
        assert!(!moved.active, "the drag is over");
        assert_eq!(d.pointer_pos(), None, "the pointer left after the drop");
        // Onto another widget: the thumb's centre follows the pointer onto the end marker (clamped).
        d.drag("box:test.thumb", Target::Widget("box:test.end"), &Drag { steps: 3, ..Drag::default() }).unwrap();
        assert_eq!(*offset.lock().unwrap(), 260.0);
        // To a point, in one step.
        let track = find(&d, "box:test.track");
        d.drag("box:test.thumb", Target::Point(Pos2::new(track.rect[0], track.rect[1] + 20.0)), &Drag { steps: 1, ..Drag::default() }).unwrap();
        assert_eq!(*offset.lock().unwrap(), 0.0);
        // The same gesture as raw pointer events.
        let c = find(&d, "box:test.thumb").center();
        let gesture: Vec<PointerEvent> =
            [(PointerKind::Move, 0.0), (PointerKind::Down, 0.0), (PointerKind::Drag, 30.0), (PointerKind::Drag, 60.0), (PointerKind::Up, 60.0)]
                .into_iter()
                .map(|(kind, dy)| PointerEvent { kind, pos: c + vec2(0.0, dy) })
                .collect();
        d.pointer(&gesture, PointerButton::Primary, Modifiers::NONE).unwrap();
        assert_eq!(*offset.lock().unwrap(), 60.0);
        assert_eq!(d.pointer_pos(), Some(c + vec2(0.0, 60.0)), "a raw gesture leaves the pointer where it ended");
    }

    /// The M2.2 done-when, part 1: drag the light table's scroll bar by widget id.
    #[test]
    fn the_light_table_scrolls_when_its_scroll_thumb_is_dragged_by_widget_id() {
        let (_dir, s) = library(1, 300);
        let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
        d.settle(3600).unwrap();
        let thumb = find(&d, "scroll:thumb");
        let low = cells(&d).into_iter().find(|w| w.rect[1] > 500.0).unwrap();
        d.drag("scroll:thumb", Target::Delta(vec2(0.0, 40.0)), &Drag::default()).unwrap();
        let moved = find(&d, "scroll:thumb");
        assert!((moved.rect[1] - thumb.rect[1] - 40.0).abs() < 0.5, "the handle followed the pointer: {:?} → {:?}", thumb.rect, moved.rect);
        let after = find(&d, &low.id);
        assert!(after.rect[1] < low.rect[1] - 40.0, "the content moved up faster than the handle: {:?} → {:?}", low.rect, after.rect);
        // And back to the top, onto the track's top edge.
        let track = find(&d, "scroll:track");
        d.drag("scroll:thumb", Target::Point(Pos2::new(track.rect[0] + track.rect[2] / 2.0, track.rect[1] - 100.0)), &Drag::default()).unwrap();
        assert_eq!(find(&d, &low.id).rect, low.rect, "back where it started");
    }

    /// The M2.2 done-when, part 2: type into a text field.
    #[test]
    fn text_is_typed_into_the_focused_field_and_keys_edit_it() {
        let dir = tempfile::tempdir().unwrap();
        let session = dalica_engine::Session::open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
        let mut d = Driver::new(App::new(Some(session)), 640, 480, 1.0);
        let (text, _) = overlay(&mut d);
        // Nothing is focused: typing goes nowhere.
        d.type_text("lost").unwrap();
        assert_eq!(*text.lock().unwrap(), "");
        d.click("field:test").unwrap();
        assert!(d.ctx().egui_wants_keyboard_input(), "the click focused the field");
        d.type_text("Lisbon 2026").unwrap();
        assert_eq!(*text.lock().unwrap(), "Lisbon 2026");
        d.key("Backspace").unwrap();
        d.type_text("7 ✓").unwrap();
        assert_eq!(*text.lock().unwrap(), "Lisbon 2027 ✓");
        // A shortcut key typed into a field is text, not a command.
        d.type_text("8").unwrap();
        assert!(text.lock().unwrap().ends_with('8'));
        assert!(d.state().status.is_empty(), "no command ran");
        let frames = d.frames();
        for bad in ["\u{7}", "a\u{1b}b"] {
            assert!(matches!(d.type_text(bad), Err(DriverError::BadInput(_))), "{bad:?}");
        }
        assert_eq!(d.frames(), frames, "refused text runs no frame");
        d.key("Esc").unwrap();
        assert!(!d.ctx().egui_wants_keyboard_input(), "Esc leaves the field");
    }

    #[test]
    fn clicks_take_a_point_a_count_a_button_and_modifiers_and_check_them_first() {
        let (_dir, s) = library(1, 6);
        let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
        d.settle(3600).unwrap();
        let cells = cells(&d);
        assert_eq!(cells.len(), 6);
        // A plain click replaces the selection, a Ctrl-click (anywhere in the cell) toggles.
        d.click(&cells[0].id).unwrap();
        d.click_with(&cells[2].id, &Click { modifiers: modifiers(true, false, false, false), fx: 0.1, fy: 0.9, ..Click::default() }).unwrap();
        assert_eq!(d.state().selection.ids.len(), 2);
        d.click_with(&cells[3].id, &Click { count: 2, ..Click::default() }).unwrap();
        assert_eq!(d.state().selection.ids.len(), 1, "a double click selects one cell");
        d.click_with(&cells[4].id, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
        assert_eq!(d.state().selection.ids.len(), 1, "a right click does not select (no context menu yet)");
        let frames = d.frames();
        for bad in [
            Click { fx: -0.1, ..Click::default() },
            Click { fy: f32::NAN, ..Click::default() },
            Click { count: 0, ..Click::default() },
            Click { count: 4, ..Click::default() },
        ] {
            assert!(matches!(d.click_with(&cells[0].id, &bad), Err(DriverError::BadInput(_))), "{bad:?}");
        }
        let id = &cells[0].id;
        assert!(matches!(d.drag(id, Target::Delta(vec2(f32::INFINITY, 0.0)), &Drag::default()), Err(DriverError::BadInput(_))));
        assert!(matches!(d.drag(id, Target::Point(Pos2::new(0.0, 2.0e6)), &Drag::default()), Err(DriverError::BadInput(_))));
        assert!(matches!(d.drag(id, Target::Delta(vec2(1.0, 1.0)), &Drag { steps: 0, ..Drag::default() }), Err(DriverError::BadInput(_))));
        assert!(matches!(d.drag(id, Target::Widget("nope"), &Drag::default()), Err(DriverError::NoWidget(_))));
        assert!(matches!(d.drag("nope", Target::Delta(Vec2::ZERO), &Drag::default()), Err(DriverError::NoWidget(_))));
        assert!(matches!(d.hover("nope", 0), Err(DriverError::NoWidget(_))));
        assert!(matches!(d.hover(id, MAX_WAIT_MS + 1), Err(DriverError::BadInput(_))));
        assert!(matches!(d.wait(u64::MAX), Err(DriverError::BadInput(_))));
        assert!(matches!(d.scroll(None, vec2(0.0, f32::NAN)), Err(DriverError::BadInput(_))));
        assert!(matches!(d.scroll(Some("nope"), vec2(0.0, 1.0)), Err(DriverError::NoWidget(_))));
        let too_many = vec![PointerEvent { kind: PointerKind::Move, pos: Pos2::ZERO }; MAX_STEPS + 1];
        assert!(matches!(d.pointer(&too_many, PointerButton::Primary, Modifiers::NONE), Err(DriverError::BadInput(_))));
        let late_bad =
            [PointerEvent { kind: PointerKind::Down, pos: Pos2::ZERO }, PointerEvent { kind: PointerKind::Up, pos: Pos2::new(f32::NAN, 0.0) }];
        assert!(matches!(d.pointer(&late_bad, PointerButton::Primary, Modifiers::NONE), Err(DriverError::BadInput(_))));
        assert_eq!(d.frames(), frames, "refused input runs no frame");
        assert_eq!(d.state().selection.ids.len(), 1, "and changes nothing");
    }

    #[test]
    fn hover_rests_the_pointer_and_wait_lets_virtual_time_pass() {
        let (_dir, s) = library(1, 3);
        let mut d = Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        let cell = cells(&d).remove(0);
        let before = d.frames();
        d.hover(&cell.id, 650).unwrap();
        assert_eq!(d.frames() - before, 1 + 39, "650 ms is 39 frames at 60 Hz, after the move");
        assert_eq!(d.pointer_pos(), Some(cell.center()));
        assert!(d.ctx().is_pointer_over_egui());
        assert_eq!(d.wait(16).unwrap(), 1);
        assert_eq!(d.wait(17).unwrap(), 2);
        assert_eq!(d.wait(0).unwrap(), 0);
    }

    #[test]
    fn held_keys_stay_down_until_released() {
        let mut d = Driver::new(App::new(None), 320, 200, 1.0);
        d.key_state("Ctrl+Alt+Space", KeyState::Down).unwrap();
        assert!(d.ctx().input(|i| i.key_down(egui::Key::Space) && i.modifiers.alt && i.modifiers.command));
        d.step();
        assert!(d.ctx().input(|i| i.key_down(egui::Key::Space) && i.modifiers.alt), "still held a frame later");
        d.key_state("Ctrl+Alt+Space", KeyState::Up).unwrap();
        assert!(d.ctx().input(|i| !i.key_down(egui::Key::Space) && i.modifiers.is_none()));
        assert!(matches!(d.key_state("Ctrl+", KeyState::Down), Err(DriverError::BadKey(..))));
        let m = modifiers(true, true, false, false);
        assert!(m.command && m.shift && !m.alt);
        assert_eq!(m.mac_cmd, cfg!(target_os = "macos"));
        let c = modifiers(false, false, false, true);
        assert!(c.ctrl && !c.mac_cmd);
    }

    #[test]
    fn the_wheel_scrolls_the_light_table_under_a_widget() {
        let (_dir, s) = library(1, 120);
        let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
        d.settle(3600).unwrap();
        // A cell low enough to stay on screen when the content moves up.
        let first = cells(&d).into_iter().find(|w| w.rect[1] > 500.0).unwrap();
        d.scroll(Some(&first.id), vec2(0.0, -300.0)).unwrap();
        let after = find(&d, &first.id);
        assert_eq!(first.rect[1] - after.rect[1], 300.0, "the content moved up by exactly the scroll");
        d.scroll(None, vec2(0.0, 300.0)).unwrap();
        assert_eq!(find(&d, &first.id).rect, first.rect, "and back, under the pointer");
    }

    #[test]
    fn settle_waits_for_quiet_frames_and_says_what_is_busy() {
        // No session: the library is opening, which never settles.
        let mut d = Driver::new(App::new(None), 320, 200, 1.0);
        let s = d.settle_for(20, false);
        assert!(!s.settled && s.frames == 20 && s.busy == ["library: opening"], "{s:?}");
        assert!((s.ms - 20.0 * 1000.0 / 60.0).abs() < 1e-9);
        assert!(matches!(d.settle(5), Err(DriverError::NotSettled { frames: 5, ref busy }) if busy.len() == 1));
        let msg = d.settle(1).unwrap_err().to_string();
        assert!(msg.contains("library: opening"), "{msg}");
        // A library with photos settles once its visible thumbnails have arrived.
        let (_dir, session) = library(2, 30);
        let mut d = Driver::new(App::new(Some(session)), 1400, 900, 1.0);
        let s = d.settle_for(3600, true);
        assert!(s.settled && s.busy.is_empty() && s.frames >= QUIET_FRAMES, "{s:?}");
        assert_eq!(d.app().panels().unwrap().thumbnails.pending(), 0);
        assert_eq!(serde_json::to_value(&s).unwrap()["settled"], true);
        // Already settled: exactly the quiet frames.
        assert_eq!(d.settle(QUIET_FRAMES).unwrap(), QUIET_FRAMES);
        assert!(d.settle(QUIET_FRAMES - 1).is_err(), "fewer frames than the quiet run cannot settle");
    }

    /// The M2.2 done-when, part 3: cells visited per frame stay flat between a 200- and a 2 000-photo
    /// library, and the other counters are measured.
    #[test]
    fn cells_visited_per_frame_stay_flat_from_200_to_2000_photos() {
        let measure = |per_folder: usize| {
            let (_dir, s) = library(4, per_folder);
            let mut d = Driver::new(App::new(Some(s)), 1400, 900, 1.0);
            d.settle(3600).unwrap();
            // Scroll down through the first screens; every frame is sampled.
            for _ in 0..4 {
                d.scroll(None, vec2(0.0, -400.0)).unwrap();
            }
            d.settle(3600).unwrap();
            let max_cells = d.perf().samples().map(|s| s.cells_visited).max().unwrap();
            (max_cells, d.perf().report())
        };
        let (small_max, small) = measure(50);
        let (big_max, big) = measure(500);
        eprintln!("200 photos: max cells {small_max}, {small:?}\n2000 photos: max cells {big_max}, {big:?}");
        assert!(small.cells_visited > 0);
        assert_eq!(small_max, big_max, "the most cells any frame laid out does not grow with the library");
        assert!(big.cells_visited <= big_max);
        assert_eq!((small.tree_rows_visited, big.tree_rows_visited), (4, 4));
        assert!(big_max < 200, "one screen of cells: {big_max}");
        // The other counters are live.
        assert!(big.frames > QUIET_FRAMES && big.frames <= perf::WINDOW);
        assert!(big.frame_ms > 0.0 && big.frame_p95_ms > 0.0 && big.max_frame_ms >= big.frame_p95_ms);
        assert!(big.update_ms >= big.frame_ms);
        assert!((big.fps - 60.0).abs() < 1e-6, "the virtual clock runs at 60 Hz: {}", big.fps);
        assert!(big.textures > 1 && big.texture_bytes > 0, "the font atlas and the thumbnails");
    }

    #[test]
    fn textures_uploaded_are_counted_when_thumbnails_arrive() {
        let (_dir, s) = library(1, 8);
        let mut d = Driver::new(App::new(Some(s)), 900, 600, 1.0);
        d.settle(3600).unwrap();
        let uploaded: u64 = d.perf().samples().map(|s| s.textures_uploaded).sum();
        assert!(uploaded >= 9, "eight thumbnails and the font atlas were uploaded: {uploaded}");
        assert!(d.perf().report().textures >= 9);
    }
}
