//! `ui.screenshot` (`control-protocol.md` §6.11): settle, capture, crop, then encode the PNG and
//! write it (or return it base64-encoded) on a worker, never on the UI thread.

use std::path::{Path, PathBuf};

use dalica_ui_kit::Image;
use serde::Deserialize;
use serde_json::{Value, json};

use super::host::{Input, Played};
use super::{Call, Code, Error, MAX_REPLY_LINE, MAX_SCALE, MAX_SHOT_PIXELS, MAX_SHOT_SIDE, Next, Outcome, SCREENSHOT_SETTLE_MS, paths, view};

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
    let input = Input::Screenshot { settle_ms, scale: scale as f32 };
    Ok(Outcome::Play(
        input,
        Box::new(move |host, played| {
            let (image, settled) = match played {
                Ok(Played::Shot { image, settled }) => (image, settled),
                Ok(other) => return Error::new(Code::Internal, format!("a screenshot played as {other:?}")).into(),
                Err(e) => return Error::from(e).into(),
            };
            let headless = host.window().headless;
            let rect = match crop {
                Crop::Window => None,
                Crop::Widget(id) => match dalica_ui_kit::registry::find(host.ctx(), &id) {
                    Some(w) => Some(w.rect),
                    None => return Error::new(Code::NotFound, format!("widget `{id}` was gone when the screenshot was taken")).into(),
                },
                Crop::Rect(r) => Some(r),
            };
            let image = match rect {
                None => image,
                Some(r) => match crop_image(&image, r, scale as f32) {
                    Some(i) => i,
                    None => return Error::new(Code::Limit, format!("the region {r:?} is outside the window")).into(),
                },
            };
            Outcome::Work(Box::new(move || Next::Reply(finish(image, p.path.as_deref(), &write_roots, &watched, headless, settled))))
        }),
    ))
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
