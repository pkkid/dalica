//! `ui.screenshot` (`control-protocol.md` §6.11): settle, capture, crop, then encode the PNG and
//! write it (or return it base64-encoded) on a worker, never on the UI thread.
//!
//! `headless: true` in a window draws the window's frame on the CPU, as `--headless` does, instead
//! of reading it back from the GPU: the window keeps a [`CpuMirror`] on its egui context (a CPU
//! copy of every texture it uploads), and the capture rasterizes one frame's shapes with it.

use std::path::{Path, PathBuf};

use dalica_ui_kit::raster::RasterError;
use dalica_ui_kit::{Image, Rasterizer};
use egui::{Color32, FullOutput, TexturesDelta};
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::{Host, Input, Played};
use super::{Call, Code, Error, MAX_REPLY_LINE, MAX_SCALE, MAX_SHOT_PIXELS, MAX_SHOT_SIDE, Next, Outcome, SCREENSHOT_SETTLE_MS, paths, view};
use crate::headless::DriverError;

/// Where a CPU capture of the window stands.
#[derive(Default)]
enum Capture {
    #[default]
    Idle,
    /// A capture has started (its settle may be running).
    Started,
    /// The next frame to end is painted, over `clear`.
    Armed {
        clear: Color32,
    },
    Painted(Result<Image, RasterError>),
}

/// The window's frames, kept for the CPU (`ui.screenshot {headless: true}` in a window, §6.11): an
/// egui plugin on the window's context that copies every texture the window uploads into a CPU
/// [`Rasterizer`], and paints the frame a capture asks for with it. Its cost between captures is
/// the textures' second copy and the uploads copied as they happen (nothing per frame otherwise).
///
/// A window that draws no frames (minimised, covered) runs no passes of its own; the control
/// channel then runs passes itself from the logic tick, outside every pass of the window
/// ([`CpuMirror::can_run_pass`]), and hands their output to the window's next frame
/// ([`CpuMirror::stash`]), so the GPU still gets every texture upload and release.
#[derive(Default)]
pub struct CpuMirror {
    raster: Rasterizer,
    /// The textures the last pass released, freed before the next pass's uploads (as the driver).
    to_free: TexturesDelta,
    /// A pass of the context is running now.
    in_pass: bool,
    /// A pass has begun since the mirror was added (so `in_pass` is known).
    begun: bool,
    /// The pass now running is one the control channel runs itself.
    own: bool,
    /// The output of passes the control channel ran, for the window's next frame.
    stashed: Option<FullOutput>,
    capture: Capture,
}

impl CpuMirror {
    /// Whether a CPU capture is under way (from its start until its picture is taken).
    pub fn capturing(&self) -> bool {
        !matches!(self.capture, Capture::Idle)
    }

    /// Whether the control channel may run a pass of the window's context now: outside every pass
    /// of the window (its logic tick while it is hidden), once the mirror has seen one begin.
    pub fn can_run_pass(&self) -> bool {
        self.begun && !self.in_pass
    }

    /// Marks the pass about to run (and then ended) as one the control channel runs itself.
    pub fn set_own(&mut self, own: bool) {
        self.own = own;
    }

    /// Keeps the output of a pass the control channel ran (its shapes dropped) for the window's
    /// next frame, which then uploads and frees its textures and obeys its commands.
    pub fn stash(&mut self, mut output: FullOutput) {
        output.shapes.clear();
        match &mut self.stashed {
            Some(older) => older.append(output),
            None => self.stashed = Some(output),
        }
    }

    /// Forgets a capture (a request that ended without taking its picture).
    pub fn reset(&mut self) {
        self.capture = Capture::Idle;
    }

    fn start(&mut self) {
        self.capture = Capture::Started;
    }

    fn arm(&mut self, clear: Color32) {
        self.capture = Capture::Armed { clear };
    }

    /// The painted frame, once there is one; the capture is then over.
    fn take(&mut self) -> Option<Result<Image, RasterError>> {
        match std::mem::take(&mut self.capture) {
            Capture::Painted(shot) => Some(shot),
            other => {
                self.capture = other;
                None
            }
        }
    }
}

impl egui::Plugin for CpuMirror {
    fn debug_name(&self) -> &'static str {
        "dalica-cpu-mirror"
    }

    fn on_begin_pass(&mut self, _ui: &mut egui::Ui) {
        self.in_pass = true;
        self.begun = true;
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut FullOutput) {
        self.in_pass = false;
        self.raster.free_textures(&mut self.to_free);
        let mut delta = output.textures_delta.clone();
        self.raster.set_textures(&mut delta);
        self.to_free = delta;
        // The last pass of a frame (a discarded one is laid out again).
        if let Capture::Armed { clear } = self.capture
            && !output.platform_output.requested_discard()
        {
            let ppp = output.pixels_per_point;
            let rect = ctx.viewport_rect();
            let px = |v: f32| (v * ppp).round().max(0.0) as u32;
            let primitives = ctx.tessellate(output.shapes.clone(), ppp);
            self.capture = Capture::Painted(self.raster.render(&primitives, ppp, px(rect.width()), px(rect.height()), clear));
        }
        if !self.own
            && let Some(mut older) = self.stashed.take()
        {
            older.append(std::mem::take(output));
            *output = older;
        }
    }
}

impl Drop for CpuMirror {
    fn drop(&mut self) {
        // Nothing is left to upload or free: the context goes with the mirror.
        self.to_free.clear();
        if let Some(s) = &mut self.stashed {
            s.textures_delta.clear();
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    path: Option<PathBuf>,
    headless: Option<bool>,
    widget: Option<String>,
    region: Option<Value>,
    scale: Option<f64>,
    settle: Option<bool>,
}

/// What to crop, in points.
#[derive(Clone)]
enum Crop {
    Window,
    Widget(String),
    Rect([f32; 4]),
}

fn limit(why: String) -> Error {
    Error::new(Code::Limit, why)
}

/// The size in pixels of `[w, h]` points at `scale`, within §4's screenshot limits.
fn check_size(w: f64, h: f64, scale: f64) -> Result<(), Error> {
    let (pw, ph) = ((w * scale).round(), (h * scale).round());
    if !(pw >= 1.0 && ph >= 1.0) {
        return Err(limit(format!("a screenshot of {pw} × {ph} pixels: at least 1 pixel per side")));
    }
    if pw > MAX_SHOT_SIDE as f64 || ph > MAX_SHOT_SIDE as f64 || pw * ph > MAX_SHOT_PIXELS as f64 {
        return Err(limit(format!("a screenshot of {pw} × {ph} pixels: at most {MAX_SHOT_SIDE} per side and {MAX_SHOT_PIXELS} in all")));
    }
    Ok(())
}

/// `ui.screenshot {path?, headless?, widget?, region?, scale?, settle?}`.
pub(super) fn screenshot(call: &mut Call<'_>, params: &Value) -> Result<Outcome, Error> {
    let p: Params = call.params(params)?;
    let window = call.host.window();
    let scale = p.scale.unwrap_or(f64::from(window.pixels_per_point));
    if !(scale.is_finite() && scale > 0.0 && scale <= MAX_SCALE) {
        return Err(limit(format!("`scale` is more than 0 and at most {MAX_SCALE}, not {scale}")));
    }
    if p.headless == Some(false) && window.headless {
        return Err(Error::new(Code::Unsupported, "this instance has no window: its screenshots are headless"));
    }
    // A window drawn on the CPU (§6.11): at the window's own scale, from its mirror.
    let cpu = p.headless == Some(true) && !window.headless;
    if cpu && (scale - f64::from(window.pixels_per_point)).abs() > 1e-3 {
        return Err(call.bad(format!(
            "a window draws its CPU capture at its own scale, {} pixels per point; leave out `scale`, or use `--headless` for another one",
            window.pixels_per_point
        )));
    }
    if cpu && call.host.ctx().plugin_opt::<CpuMirror>().is_none() {
        return Err(Error::new(Code::Unsupported, "this window keeps no CPU copy of its frames (it was started without --control)"));
    }
    let crop = match (&p.widget, &p.region) {
        (Some(_), Some(_)) => return Err(call.bad("give `widget` or `region`, not both")),
        (Some(id), None) => {
            let w = dalica_ui_kit::registry::find(call.host.ctx(), id)
                .ok_or_else(|| Error::new(Code::NotFound, format!("no widget `{id}` on screen (see ui.widgets)")))?;
            check_size(f64::from(w.rect[2]), f64::from(w.rect[3]), scale)?;
            Crop::Widget(id.clone())
        }
        (None, Some(Value::String(name))) => {
            let w = view::region(call, name)?;
            check_size(f64::from(w.rect[2]), f64::from(w.rect[3]), scale)?;
            Crop::Widget(w.id)
        }
        (None, Some(r)) => {
            let rect: [f64; 4] = serde_json::from_value(r.clone()).map_err(|_| call.bad("`region` is a region name or [x, y, w, h] in points"))?;
            if rect.iter().any(|v| !v.is_finite()) {
                return Err(call.bad("`region` has a number that is not finite"));
            }
            check_size(rect[2], rect[3], scale)?;
            Crop::Rect(rect.map(|v| v as f32))
        }
        (None, None) => {
            check_size(f64::from(window.width), f64::from(window.height), scale)?;
            Crop::Window
        }
    };
    if let Some(path) = &p.path {
        paths::lexical(path, "path")?;
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
            return Err(call.bad(format!("`path` {} must end in .png", path.display())));
        }
    }
    let write_roots = call.control.config().effective_write_roots();
    let watched = call.host.app().session().map(|s| s.roots().to_vec()).unwrap_or_default();
    let settle_ms = p.settle.unwrap_or(true).then_some(SCREENSHOT_SETTLE_MS);
    let deliver = Deliver { crop, scale: scale as f32, path: p.path, write_roots, watched };
    if cpu {
        call.host.ctx().with_plugin(|m: &mut CpuMirror| m.start());
        return Ok(match settle_ms {
            Some(timeout_ms) => Outcome::Play(
                Input::Settle { timeout_ms, jobs: false },
                Box::new(move |host, played| match played {
                    Ok(Played::Settled(s)) => paint_on_cpu(host, Some(s.settled), deliver),
                    Ok(other) => cpu_failed(host, Error::new(Code::Internal, format!("a settle played as {other:?}"))),
                    Err(e) => cpu_failed(host, e.into()),
                }),
            ),
            None => paint_on_cpu(call.host, None, deliver),
        });
    }
    let input = Input::Screenshot { settle_ms, scale: scale as f32 };
    Ok(Outcome::Play(
        input,
        Box::new(move |host, played| match played {
            Ok(Played::Shot { image, settled }) => {
                let headless = host.window().headless;
                deliver.run(host, image, settled, headless)
            }
            Ok(other) => Error::new(Code::Internal, format!("a screenshot played as {other:?}")).into(),
            Err(e) => Error::from(e).into(),
        }),
    ))
}

/// Ends a CPU capture that failed.
fn cpu_failed(host: &mut dyn Host, e: Error) -> Outcome {
    host.ctx().with_plugin(|m: &mut CpuMirror| m.reset());
    e.into()
}

/// Asks the mirror to paint the next frame, lets that frame run, then delivers its picture.
fn paint_on_cpu(host: &mut dyn Host, settled: Option<bool>, deliver: Deliver) -> Outcome {
    let clear = host.app().tokens().chrome;
    host.ctx().with_plugin(|m: &mut CpuMirror| m.arm(clear));
    Outcome::Play(
        Input::Frames(1),
        Box::new(move |host, played| {
            if let Err(e) = played {
                return cpu_failed(host, e.into());
            }
            match host.ctx().with_plugin(|m: &mut CpuMirror| m.take()).flatten() {
                Some(Ok(image)) => deliver.run(host, image, settled, true),
                Some(Err(e)) => cpu_failed(host, Error::from(DriverError::Raster(e))),
                None => cpu_failed(host, Error::new(Code::Internal, "the window's frame was not painted on the CPU")),
            }
        }),
    )
}

/// What a capture does with its picture: crop it, then encode and write it off the UI thread.
struct Deliver {
    crop: Crop,
    scale: f32,
    path: Option<PathBuf>,
    write_roots: Vec<PathBuf>,
    watched: Vec<PathBuf>,
}

impl Deliver {
    fn run(self, host: &mut dyn Host, image: Image, settled: Option<bool>, headless: bool) -> Outcome {
        let rect = match self.crop {
            Crop::Window => None,
            Crop::Widget(id) => match dalica_ui_kit::registry::find(host.ctx(), &id) {
                Some(w) => Some(w.rect),
                None => return Error::new(Code::NotFound, format!("widget `{id}` was gone when the screenshot was taken")).into(),
            },
            Crop::Rect(r) => Some(r),
        };
        let image = match rect {
            None => image,
            Some(r) => match crop_image(&image, r, self.scale) {
                Some(i) => i,
                None => return Error::new(Code::Limit, format!("the region {r:?} is outside the window")).into(),
            },
        };
        let Deliver { path, write_roots, watched, .. } = self;
        Outcome::Work(Box::new(move || Next::Reply(finish(image, path.as_deref(), &write_roots, &watched, headless, settled))))
    }
}

/// The part of `image` under `rect` (points at `scale` pixels per point), clipped to the image;
/// `None` if nothing is left.
pub fn crop_image(image: &Image, rect: [f32; 4], scale: f32) -> Option<Image> {
    let x0 = (rect[0] * scale).round().max(0.0) as u32;
    let y0 = (rect[1] * scale).round().max(0.0) as u32;
    let x1 = (((rect[0] + rect[2]) * scale).round().max(0.0) as u32).min(image.width);
    let y1 = (((rect[1] + rect[3]) * scale).round().max(0.0) as u32).min(image.height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
    for y in y0..y1 {
        let start = (y as usize * image.width as usize + x0 as usize) * 4;
        rgba.extend_from_slice(image.rgba.get(start..start + w as usize * 4)?);
    }
    Some(Image { width: w, height: h, rgba })
}

/// Encodes RGBA as PNG.
pub fn encode_png(image: &Image) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut enc = png::Encoder::new(&mut bytes, image.width, image.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&image.rgba).and_then(|()| w.finish()))
        .map_err(|e| Error::new(Code::Internal, format!("cannot encode the PNG: {e}")))?;
    Ok(bytes)
}

/// Standard base64 with padding.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Off the UI thread: encode, then write within the roots or return the PNG inline.
fn finish(
    image: Image,
    path: Option<&Path>,
    write_roots: &[PathBuf],
    watched: &[PathBuf],
    headless: bool,
    settled: Option<bool>,
) -> Result<Value, Error> {
    let png = encode_png(&image)?;
    let (width, height) = (image.width, image.height);
    match path {
        None => {
            let encoded = base64(&png);
            if encoded.len() >= MAX_REPLY_LINE {
                return Err(Error::new(
                    Code::Limit,
                    format!(
                        "the PNG is {} bytes base64-encoded, over the {MAX_REPLY_LINE}-byte reply limit; write it to a `path` instead",
                        encoded.len()
                    ),
                ));
            }
            Ok(json!({ "path": null, "png": encoded, "width": width, "height": height, "headless": headless, "settled": settled }))
        }
        Some(path) => {
            let target = paths::check_write(path, write_roots, watched)?;
            let expect = match dalica_fs::identity(&target) {
                Ok(Some(id)) => dalica_fs::Expect::Unchanged(id),
                Ok(None) => dalica_fs::Expect::Absent,
                Err(e) => return Err(Error::new(Code::PathDenied, format!("cannot look at {}: {e}", target.display()))),
            };
            dalica_fs::replace(&target, &png, expect).map_err(|e| Error::new(Code::PathDenied, format!("cannot write {}: {e}", target.display())))?;
            Ok(json!({ "path": path, "width": width, "height": height, "headless": headless, "settled": settled }))
        }
    }
}
