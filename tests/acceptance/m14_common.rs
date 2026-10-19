//! What M14's live parts share (`m14.rs`): the window size, painted photos whose colours tell
//! them apart on a canvas and in a created JPEG, the waits for a condition the app reaches in the
//! background, whole-window shots taken once the window stops changing, and the bytes of a
//! folder's files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dalica_image::Rgba8;
use dalica_image::encode::{self, Image, Metadata, Subsampling};
use serde_json::{Value, json};

use crate::app::{self, idle};
use crate::live::{Client, Shot};

pub const LARGE: (u32, u32) = (1400, 900);
pub const SMALL: (u32, u32) = (900, 600);
/// How long the app may take to reach a state it reaches in the background.
pub const PATIENCE: Duration = Duration::from_secs(300);

pub type Rgb = [u8; 3];

/// Eight colours far apart, one per painted photo.
pub const COLOURS: [Rgb; 8] =
    [[200, 40, 40], [40, 170, 60], [40, 70, 200], [230, 200, 40], [190, 50, 190], [40, 190, 200], [240, 130, 30], [120, 120, 120]];

/// A JPEG of `w × h` painted in `colour`, with a soft gradient over it so it is a photo and not a
/// flat card (at most ±12 per channel; procedural, deterministic).
pub fn painted_jpeg(w: u32, h: u32, colour: Rgb) -> Vec<u8> {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let d = (x * 24 / w.max(1)) as i32 + (y * 24 / h.max(1)) as i32 - 24;
            let px = colour.map(|c| (i32::from(c) + d / 2).clamp(0, 255) as u8);
            data.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    encode::jpeg(&Image::rgba8(&Rgba8 { width: w, height: h, data }), 92, Subsampling::S444, &Metadata::default()).unwrap()
}

/// Waits until `holds` (asking the app between tries), at most [`PATIENCE`]; panics naming `what`.
pub fn until(c: &mut Client, what: &str, mut holds: impl FnMut(&mut Client) -> bool) {
    let end = Instant::now() + PATIENCE;
    loop {
        idle(c);
        if holds(c) {
            return;
        }
        assert!(Instant::now() < end, "{what}: still not so after {PATIENCE:?}");
        c.ok("ui.wait", json!({ "ms": 100 }));
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Screenshots of the window until two in a row are the same picture (thumbnails and textures
/// in); the last, kept as `<label>.png` in the test output.
pub fn steady(c: &mut Client, home: &Path, label: &str) -> Shot {
    let mut last: Option<Shot> = None;
    for _ in 0..30 {
        idle(c);
        app::quiet(c, f64::from(LARGE.1));
        let shot = crate::live::screenshot(c, home, None, label, LARGE.0, LARGE.1);
        if last.as_ref().is_some_and(|l| l.rgba == shot.rgba) {
            return shot;
        }
        last = Some(shot);
        c.ok("ui.wait", json!({ "ms": 300 }));
    }
    panic!("{label}: the window never stopped changing");
}

/// [`steady`], then matched against `fingerprints/<name>.json` with the moving parts (the status
/// line, the activity indicator) masked, and any `masks` more.
pub fn steady_shot(c: &mut Client, home: &Path, name: &str, masks: &[[f64; 4]]) {
    steady(c, home, &format!("{name}-probe"));
    let mut all = app::moving_parts(c);
    all.extend_from_slice(masks);
    app::masked_shot(c, home, name, LARGE, &all);
}

/// Every file directly in `dir` with its bytes, by name (subfolders left out).
pub fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).unwrap()))
        .collect()
}

/// Every file under `dir`, recursively, with its bytes, by path relative to `dir`.
pub fn tree(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                todo.push(p);
            } else {
                out.insert(p.strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// `ui.state`'s room in front.
pub fn room(c: &mut Client) -> String {
    c.state()["room"].as_str().unwrap_or_default().to_string()
}

/// `collage.inspect`.
pub fn collage(c: &mut Client) -> Value {
    c.exec("collage.inspect", json!({}))
}
