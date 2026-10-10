//! The headless driver (`architecture.md` §7): runs the real [`App`] against an egui `Context` with
//! no window, on a virtual clock, and paints with the CPU rasterizer. Tests, `dalica-cli snapshot`
//! and (later) the control channel's headless mode drive the app through it: by widget id, by key,
//! by command, never by OS input.

use dalica_engine::{CmdError, Key};
use dalica_ui_kit::{Image, RasterError, Rasterizer, WidgetInfo, registry};
use egui::{Event, Modifiers, PointerButton, TexturesDelta};
use serde_json::Value;

use crate::app::App;
use crate::state::UiState;

/// One frame of the virtual clock (60 Hz).
pub const FRAME_SECONDS: f64 = 1.0 / 60.0;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("no widget `{0}` in the last frame")]
    NoWidget(String),
    #[error("widget `{0}` is disabled")]
    Disabled(String),
    #[error("the condition did not hold within {0} frames")]
    Timeout(usize),
    #[error("cannot press `{0}`: {1}")]
    BadKey(String, String),
    #[error(transparent)]
    Command(#[from] CmdError),
    #[error(transparent)]
    Raster(#[from] RasterError),
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
    /// The last frame's shapes, painted on demand by [`Driver::screenshot`].
    shapes: Vec<egui::epaint::ClippedShape>,
    /// Textures the last frame released, freed before the next frame's uploads.
    to_free: TexturesDelta,
}

/// egui's name for one of our key codes.
fn egui_key(code: &str) -> Option<egui::Key> {
    let name = match code {
        "Esc" => "Escape",
        "Plus" => "+",
        other => other,
    };
    egui::Key::from_name(name)
}

impl Driver {
    /// A `width × height` pixel window at `pixels_per_point`, showing `app`; runs the first frame
    /// (fonts install) so the widget list is ready.
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
            shapes: Vec::new(),
            to_free: TexturesDelta::default(),
        };
        d.step();
        d.step();
        d
    }

    /// Runs one frame: queued input, the app, texture uploads. Advances the clock by one frame.
    pub fn step(&mut self) {
        let mut input = dalica_ui_kit::raw_input(self.width, self.height, self.pixels_per_point);
        input.time = Some(self.time);
        input.events = std::mem::take(&mut self.events);
        let app = &mut self.app;
        let out = self.ctx.run_ui(input, |ui| app.frame(ui));
        self.raster.free_textures(&mut self.to_free);
        let mut delta = out.textures_delta;
        self.raster.set_textures(&mut delta);
        self.to_free = delta;
        self.shapes = out.shapes;
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

    /// Steps until the app is idle (no background work), then two more frames so the layout has
    /// caught up with the last results.
    pub fn settle(&mut self, max_frames: usize) -> Result<usize, DriverError> {
        let n = self.step_until(App::idle, max_frames)?;
        self.step();
        self.step();
        Ok(n + 2)
    }

    /// Clicks the centre of the widget with automation id `id` (press and release, one frame each),
    /// then moves the pointer away so no hover is left behind.
    pub fn click(&mut self, id: &str) -> Result<(), DriverError> {
        let w = registry::find(&self.ctx, id).ok_or_else(|| DriverError::NoWidget(id.into()))?;
        if !w.enabled {
            return Err(DriverError::Disabled(id.into()));
        }
        let pos = w.center();
        let modifiers = Modifiers::NONE;
        self.events.push(Event::PointerMoved(pos));
        self.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers });
        self.step();
        self.events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers });
        self.step();
        self.events.push(Event::PointerGone);
        self.step();
        Ok(())
    }

    /// Presses and releases a key chord written as the keymap writes it (`Ctrl+8`, `F4`, `Esc`).
    /// `Ctrl` is the primary modifier: Command on macOS.
    pub fn key(&mut self, chord: &str) -> Result<(), DriverError> {
        let k = Key::parse(chord).map_err(|e| DriverError::BadKey(chord.into(), e))?;
        let key = egui_key(&k.code).ok_or_else(|| DriverError::BadKey(chord.into(), format!("egui has no key `{}`", k.code)))?;
        let modifiers = Modifiers {
            alt: k.alt,
            ctrl: k.ctrl && !cfg!(target_os = "macos"),
            shift: k.shift,
            mac_cmd: k.ctrl && cfg!(target_os = "macos"),
            command: k.ctrl,
        };
        self.events.push(Event::ModifiersChanged(modifiers));
        self.events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
        self.step();
        self.events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
        self.events.push(Event::ModifiersChanged(Modifiers::NONE));
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

    /// Frames run so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Changes the window size; the next frame lays out for it.
    pub fn resize(&mut self, width: u32, height: u32) {
        (self.width, self.height) = (width, height);
        self.step();
    }

    /// The last frame, painted.
    pub fn screenshot(&self) -> Result<Image, DriverError> {
        let primitives = self.ctx.tessellate(self.shapes.clone(), self.pixels_per_point);
        Ok(self.raster.render(&primitives, self.pixels_per_point, self.width, self.height, self.app.tokens().chrome)?)
    }

    /// Ends the drive and hands back the app (to flush and close its session).
    pub fn into_app(mut self) -> App {
        std::mem::replace(&mut self.app, App::new(None))
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
    use std::sync::Arc;

    fn count(img: &Image, f: impl Fn([u8; 4]) -> bool) -> usize {
        img.rgba.as_chunks::<4>().0.iter().filter(|&&p| f(p)).count()
    }

    /// The M1.17 done-when: an empty app, driven headlessly, gives a screenshot of the size asked.
    #[test]
    fn an_empty_app_is_driven_headlessly_and_screenshotted_at_the_requested_size() {
        let dir = tempfile::tempdir().unwrap();
        let session = dalica_engine::Session::open(dir.path(), Arc::new(dalica_core::SystemClock)).unwrap();
        let mut d = Driver::new(App::new(Some(session)), 900, 600, 1.0);
        assert!(d.settle(60).unwrap() >= 2);
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
}
