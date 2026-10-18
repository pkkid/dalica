//! What M12's live parts share (`m12.rs`): the effect catalogue as `effect.list` gives it, a
//! photo's recipe and history label, the photo's place in the edit view found in a screenshot, and
//! the pixel measures the parts judge pictures by.

use std::path::Path;

use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata};
use serde_json::{Value, json};

use crate::app::rect_of;
use crate::live::{self, Client};

pub const LARGE: (u32, u32) = (1400, 900);

/// `effect.list`'s effects, in the grids' order (tab 3, 4, 5, each in place order).
pub fn effect_list(c: &mut Client) -> Vec<Value> {
    let r = c.exec("effect.list", json!({}));
    r["effects"].as_array().unwrap_or_else(|| panic!("effect.list: {r}")).clone()
}

/// The `edit.tab` of effect tab `n` (3, 4 or 5).
pub fn tab(n: u64) -> &'static str {
    match n {
        3 => "effects",
        4 => "effects2",
        5 => "effects3",
        _ => panic!("no effect tab {n}"),
    }
}

/// The photo's recipe (`photo.inspect` → `recipe.edits`).
pub fn edits(c: &mut Client, id: &Value) -> Vec<String> {
    let p = c.inspect(id);
    p["recipe"]["edits"]
        .as_array()
        .unwrap_or_else(|| panic!("photo.inspect names the recipe: {p}"))
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect()
}

/// What the photo's history offers to undo (`photo.history.state` → `undoLabel`).
pub fn undo_label(c: &mut Client, id: &Value) -> String {
    let r = c.exec("photo.history.state", json!({ "id": id }));
    r["undoLabel"].as_str().unwrap_or_else(|| panic!("photo.history.state: {r}")).to_string()
}

/// The open effect panel (`ui.state` → `edit.effect`), `null` when none is open.
pub fn panel(c: &mut Client) -> Value {
    c.state()["edit"]["effect"].clone()
}

/// The pixel of `rgba` (a `width`-wide window) at `(x, y)`.
pub fn px(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

fn apart(a: [u8; 3], b: [u8; 3]) -> u8 {
    (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap_or(0)
}

/// Where the photo shows in the edit view, in screen points `[x, y, w, h]`: the bounding box of
/// the canvas's pixels that differ from its background (the canvas's top-left corner) by more than
/// the photo's drop shadow does, found in a screenshot (`label`) at [`LARGE`].
pub fn photo_rect(c: &mut Client, home: &Path, label: &str) -> [f64; 4] {
    let canvas = rect_of(c, "canvas:edit").expect("the edit view's canvas");
    let shot = live::screenshot(c, home, None, label, LARGE.0, LARGE.1);
    let w = shot.width;
    let (x0, y0) = (canvas[0] as u32 + 2, canvas[1] as u32 + 2);
    let (x1, y1) = ((canvas[0] + canvas[2]) as u32 - 2, (canvas[1] + canvas[3]) as u32 - 2);
    let bg = px(&shot.rgba, w, x0, y0);
    let (mut lo_x, mut lo_y, mut hi_x, mut hi_y) = (u32::MAX, u32::MAX, 0, 0);
    for y in y0..y1 {
        for x in x0..x1 {
            if apart(px(&shot.rgba, w, x, y), bg) > 40 {
                lo_x = lo_x.min(x);
                lo_y = lo_y.min(y);
                hi_x = hi_x.max(x);
                hi_y = hi_y.max(y);
            }
        }
    }
    assert!(lo_x < hi_x && lo_y < hi_y, "{label}: no photo on the canvas {canvas:?} (background {bg:?})");
    [f64::from(lo_x), f64::from(lo_y), f64::from(hi_x + 1 - lo_x), f64::from(hi_y + 1 - lo_y)]
}

/// The screen point of the shown point `at` (normalised to the picture as it shows) of `rect`.
pub fn screen(rect: [f64; 4], at: [f64; 2]) -> (f64, f64) {
    (rect[0] + at[0] * rect[2], rect[1] + at[1] * rect[3])
}

/// The stored point under the shown point `s` of a photo turned a quarter clockwise (its stored
/// left edge shows along the top).
pub fn stored_of_turned(s: [f64; 2]) -> [f64; 2] {
    [s[1], 1.0 - s[0]]
}

/// A PNG of `w × h`: a red checker of `square`-pixel squares (2: any blur flattens it) over green
/// and blue ramps.
pub fn textured_png(w: u32, h: u32, square: u32) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let v = if ((x / square) + (y / square)).is_multiple_of(2) { 50 } else { 210 };
            data.extend_from_slice(&[v, (x * 255 / w) as u8, (y * 255 / h) as u8, 255]);
        }
    }
    encode::png(&Image::rgba8(&Rgba8 { width: w, height: h, data }), &Metadata::default()).unwrap()
}

/// How much of the red checker shows round the point `at` (normalised) of `img`: the mean step
/// between horizontal neighbours in a 9 × 9 window (about 80 where sharp, near 0 where blurred).
pub fn checker(img: &Rgba8, at: [f64; 2]) -> f64 {
    let (cx, cy) = ((at[0] * f64::from(img.width)) as i64, (at[1] * f64::from(img.height)) as i64);
    let red = |x: i64, y: i64| f64::from(img.data[((y as u32 * img.width + x as u32) * 4) as usize]);
    let (mut sum, mut n) = (0.0, 0.0);
    for y in cy - 4..=cy + 4 {
        for x in cx - 4..cx + 4 {
            sum += (red(x + 1, y) - red(x, y)).abs();
            n += 1.0;
        }
    }
    sum / n
}

/// A decoded picture file, as RGBA.
pub fn decode(bytes: &[u8]) -> Rgba8 {
    dalica_image::decode(bytes, &Default::default()).unwrap()
}

/// The mean difference of one channel (RGB) between the `w × h` region of `a` at `(ax, ay)` and
/// that of `b` at `(bx, by)`.
pub fn mean_difference(a: &Rgba8, (ax, ay): (u32, u32), b: &Rgba8, (bx, by): (u32, u32), (w, h): (u32, u32)) -> f64 {
    let mut sum = 0u64;
    for y in 0..h {
        for x in 0..w {
            let p = px(&a.data, a.width, ax + x, ay + y);
            let q = px(&b.data, b.width, bx + x, by + y);
            sum += (0..3).map(|i| u64::from(p[i].abs_diff(q[i]))).sum::<u64>();
        }
    }
    sum as f64 / f64::from(w * h * 3)
}
