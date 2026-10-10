//! What the handlers run over: a [`Host`] (the app, its egui context, its perf counters) and, for
//! input, a [`Player`] that plays an [`Input`] over real frames. The headless [`Driver`] is both;
//! the window plays the same inputs over its own frames.

use dalica_ui_kit::{Image, registry};
use egui::{Modifiers, PointerButton, Pos2, Vec2};

use crate::app::App;
use crate::headless::{Click, Drag, Driver, DriverError, KeyState, PointerEvent, PointerKind, Settled, Target};
use crate::perf::Perf;

/// The window as `ui.inspect` reports it (§6.6).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// Points.
    pub width: f32,
    pub height: f32,
    pub pixels_per_point: f32,
    pub focused: bool,
    /// False while the compositor delivers no frames.
    pub drawing: bool,
    /// Drawn by the CPU rasterizer, with no window.
    pub headless: bool,
}

/// The app the handlers read and change, between frames, on the UI thread.
pub trait Host {
    fn app(&self) -> &App;
    fn app_mut(&mut self) -> &mut App;
    fn ctx(&self) -> &egui::Context;
    fn perf(&self) -> &Perf;
    fn perf_mut(&mut self) -> &mut Perf;
    fn window(&self) -> Window;
}

/// Input, checked by a handler, for the host to play over frames (§5.2). Coordinates are screen
/// points.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// `count` clicks (1–8) at a point of the widget; the pointer then leaves the window.
    Click {
        id: String,
        click: Click,
    },
    /// A drag from a point of widget `from` (fractions `at` of its size).
    Drag {
        from: String,
        at: (f32, f32),
        to: DragTo,
        drag: Drag,
    },
    /// The pointer rests at a point of the widget, then `ms` pass.
    Hover {
        id: String,
        at: (f32, f32),
        ms: u64,
    },
    /// A raw gesture, one frame per event.
    Pointer {
        events: Vec<PointerEvent>,
        button: PointerButton,
        modifiers: Modifiers,
    },
    /// A wheel scroll over a widget (or where the pointer is).
    Scroll {
        at: Option<String>,
        delta: Vec2,
    },
    Key {
        key: egui::Key,
        modifiers: Modifiers,
        state: KeyState,
    },
    Text(String),
    /// The window's new size in points.
    Resize {
        width: f32,
        height: f32,
    },
    /// `ms` of time pass.
    Wait {
        ms: u64,
    },
    /// Frames until nothing visible is pending, at most `timeout_ms` (§6.12).
    Settle {
        timeout_ms: u64,
        jobs: bool,
    },
    /// Let `n` frames run (after a command, so widgets show its effect).
    Frames(usize),
    /// A settle (or, without `settle_ms`, the next frame as is), then the window captured at
    /// `scale` pixels per point.
    Screenshot {
        settle_ms: Option<u64>,
        scale: f32,
    },
}

/// Where a drag ends.
#[derive(Debug, Clone, PartialEq)]
pub enum DragTo {
    Widget(String),
    Point(Pos2),
    /// From where the drag starts.
    Delta(Vec2),
}

/// What playing an input gave.
#[derive(Debug, Clone, PartialEq)]
pub enum Played {
    Done,
    Frames(usize),
    Settled(Settled),
    /// `settled` is `None` when no settle was asked for.
    Shot {
        image: Image,
        settled: Option<bool>,
    },
}

/// Plays input synchronously (the headless driver).
pub trait Player: Host {
    fn play(&mut self, input: &Input) -> Result<Played, DriverError>;
    fn as_host(&mut self) -> &mut dyn Host;
}

/// Frames of the 60 Hz virtual clock in `ms`, rounded up.
pub fn frames_for(ms: u64) -> usize {
    usize::try_from(ms.saturating_mul(60).div_ceil(1000)).unwrap_or(usize::MAX)
}

impl Host for Driver {
    fn app(&self) -> &App {
        Driver::app(self)
    }

    fn app_mut(&mut self) -> &mut App {
        Driver::app_mut(self)
    }

    fn ctx(&self) -> &egui::Context {
        Driver::ctx(self)
    }

    fn perf(&self) -> &Perf {
        Driver::perf(self)
    }

    fn perf_mut(&mut self) -> &mut Perf {
        Driver::perf_mut(self)
    }

    fn window(&self) -> Window {
        let (w, h, ppp) = self.size();
        Window { width: w as f32 / ppp, height: h as f32 / ppp, pixels_per_point: ppp, focused: true, drawing: true, headless: true }
    }
}

fn point_in(d: &Driver, id: &str, at: (f32, f32)) -> Result<Pos2, DriverError> {
    let w = registry::find(d.ctx(), id).ok_or_else(|| DriverError::NoWidget(id.into()))?;
    Ok(Pos2::new(w.rect[0] + w.rect[2] * at.0, w.rect[1] + w.rect[3] * at.1))
}

fn centred(at: (f32, f32)) -> bool {
    at == (0.5, 0.5)
}

impl Player for Driver {
    fn play(&mut self, input: &Input) -> Result<Played, DriverError> {
        match input {
            // The driver plays up to triple clicks; more presses are the same gesture, raw.
            Input::Click { id, click } if click.count > 3 => {
                let w = registry::find(self.ctx(), id).ok_or_else(|| DriverError::NoWidget(id.clone()))?;
                if !w.enabled {
                    return Err(DriverError::Disabled(id.clone()));
                }
                let pos = Pos2::new(w.rect[0] + w.rect[2] * click.fx, w.rect[1] + w.rect[3] * click.fy);
                let mut events = vec![PointerEvent { kind: PointerKind::Move, pos }];
                for _ in 0..click.count {
                    events.push(PointerEvent { kind: PointerKind::Down, pos });
                    events.push(PointerEvent { kind: PointerKind::Up, pos });
                }
                self.pointer(&events, click.button, click.modifiers)?;
            }
            Input::Click { id, click } => self.click_with(id, click)?,
            Input::Drag { from, at, to, drag } if centred(*at) => {
                let to = match to {
                    DragTo::Widget(id) => Target::Widget(id),
                    DragTo::Point(p) => Target::Point(*p),
                    DragTo::Delta(v) => Target::Delta(*v),
                };
                self.drag(from, to, drag)?;
            }
            Input::Drag { from, at, to, drag } => {
                let w = registry::find(self.ctx(), from).ok_or_else(|| DriverError::NoWidget(from.clone()))?;
                if !w.enabled {
                    return Err(DriverError::Disabled(from.clone()));
                }
                let start = point_in(self, from, *at)?;
                let end = match to {
                    DragTo::Widget(id) => point_in(self, id, (0.5, 0.5))?,
                    DragTo::Point(p) => *p,
                    DragTo::Delta(v) => start + *v,
                };
                let mut events = vec![PointerEvent { kind: PointerKind::Move, pos: start }, PointerEvent { kind: PointerKind::Down, pos: start }];
                for i in 1..=drag.steps {
                    events.push(PointerEvent { kind: PointerKind::Drag, pos: start.lerp(end, i as f32 / drag.steps as f32) });
                }
                events.push(PointerEvent { kind: PointerKind::Up, pos: end });
                self.pointer(&events, drag.button, drag.modifiers)?;
            }
            Input::Hover { id, at, ms } if centred(*at) => self.hover(id, *ms)?,
            Input::Hover { id, at, ms } => {
                let pos = point_in(self, id, *at)?;
                self.pointer(&[PointerEvent { kind: PointerKind::Move, pos }], PointerButton::Primary, Modifiers::NONE)?;
                self.wait(*ms)?;
            }
            Input::Pointer { events, button, modifiers } => self.pointer(events, *button, *modifiers)?,
            Input::Scroll { at, delta } => self.scroll(at.as_deref(), *delta)?,
            Input::Key { key, modifiers, state } => self.key_raw(*key, *modifiers, *state),
            Input::Text(text) => self.type_text(text)?,
            Input::Resize { width, height } => {
                let (_, _, ppp) = self.size();
                let px = |v: f32| (v * ppp).round().max(1.0) as u32;
                self.resize(px(*width), px(*height));
            }
            Input::Wait { ms } => return Ok(Played::Frames(self.wait(*ms)?)),
            Input::Settle { timeout_ms, jobs } => return Ok(Played::Settled(self.settle_for(frames_for(*timeout_ms), *jobs))),
            Input::Frames(n) => {
                for _ in 0..*n {
                    self.step();
                }
                return Ok(Played::Frames(*n));
            }
            Input::Screenshot { settle_ms, scale } => return screenshot(self, *settle_ms, *scale),
        }
        Ok(Played::Done)
    }

    fn as_host(&mut self) -> &mut dyn Host {
        self
    }
}

/// Settles (or runs one frame), captures at `scale`, and puts the window's own scale back.
fn screenshot(d: &mut Driver, settle_ms: Option<u64>, scale: f32) -> Result<Played, DriverError> {
    let (w, h, ppp) = d.size();
    let rescale = (scale - ppp).abs() > f32::EPSILON;
    if rescale {
        let px = |v: u32| ((v as f32 / ppp) * scale).round().max(1.0) as u32;
        d.set_size(px(w), px(h), scale);
    }
    let settled = match settle_ms {
        Some(ms) => Some(d.settle_for(frames_for(ms), false).settled),
        None => {
            d.step();
            None
        }
    };
    let image = d.screenshot();
    if rescale {
        d.set_size(w, h, ppp);
    }
    Ok(Played::Shot { image: image?, settled })
}
