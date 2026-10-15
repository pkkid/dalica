//! What the ui-kit's integration tests share: the test output folder, the fingerprint check, the
//! quick or full switch, and a canvas that paints a frame only when a test asks for its pixels.

#![allow(dead_code)]

use std::path::PathBuf;

use dalica_scenes::fingerprint;
use dalica_ui_kit::{Image, Rasterizer};
use egui::epaint::ClippedShape;
use egui::{Color32, Context, FullOutput, TexturesDelta};

/// Full strength (`DALICA_FULL_TESTS=1`, the review gate) or quick (a task's gate).
pub fn full() -> bool {
    std::env::var_os("DALICA_FULL_TESTS").is_some_and(|v| v == "1")
}

/// Where renders go: `target/test-output/ui-kit/`.
pub fn out_dir() -> PathBuf {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui-kit");
    std::fs::create_dir_all(&out).unwrap();
    out
}

/// Writes `img` as `<name>.png` to the test output and compares it with
/// `tests/fingerprints/<name>.json` (`DALICA_BLESS=1` stores a new one); a mismatch or a missing
/// fingerprint is pushed onto `failures` (with `<name>-actual.png` written next to the render).
pub fn check(name: &str, img: &Image, failures: &mut Vec<String>) {
    let out = out_dir();
    std::fs::write(out.join(format!("{name}.png")), fingerprint::png_rgba(img.width, img.height, &img.rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(img.width, img.height, &img.rgba)) + "\n").unwrap();
    }
    match std::fs::read_to_string(&stored).map_err(|e| e.to_string()).and_then(|s| fingerprint::from_json(&s)) {
        Ok(expected) => {
            if let Err(m) = fingerprint::check(img.width, img.height, &img.rgba, &expected, 2.0) {
                std::fs::write(out.join(format!("{name}-actual.png")), &m.actual_png).unwrap();
                failures.push(format!("{name}: {} (render in {})", m.message, out.display()));
            }
        }
        Err(e) => failures.push(format!("{name}: no fingerprint ({e}); run with DALICA_BLESS=1 and look at the PNG")),
    }
}

/// [`check`] for one image: panics on a mismatch.
pub fn check_one(name: &str, img: &Image) {
    let mut failures = Vec::new();
    check(name, img, &mut failures);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A rig's last frame, painted by the CPU rasterizer only when a test asks for its pixels: every
/// frame's textures are taken in as they come, its shapes are kept until [`Canvas::image`].
pub struct Canvas {
    raster: Rasterizer,
    size: (u32, u32),
    clear: Color32,
    shapes: Vec<ClippedShape>,
    pixels_per_point: f32,
    /// The last frame's textures to free, once a later frame replaces it.
    to_free: TexturesDelta,
    painted: Option<Image>,
}

impl Canvas {
    pub fn new(size: (u32, u32), clear: Color32) -> Canvas {
        Canvas { raster: Rasterizer::new(), size, clear, shapes: Vec::new(), pixels_per_point: 1.0, to_free: TexturesDelta::default(), painted: None }
    }

    /// Takes in a frame's output: its textures now, its shapes for [`Canvas::image`].
    pub fn keep(&mut self, out: FullOutput) {
        self.raster.free_textures(&mut self.to_free);
        let mut delta = out.textures_delta;
        self.raster.set_textures(&mut delta);
        self.to_free = delta;
        self.shapes = out.shapes;
        self.pixels_per_point = out.pixels_per_point;
        self.painted = None;
    }

    /// The last frame, painted (once).
    pub fn image(&mut self, ctx: &Context) -> &Image {
        if self.painted.is_none() {
            let primitives = ctx.tessellate(self.shapes.clone(), self.pixels_per_point);
            let (w, h) = self.size;
            self.painted = Some(self.raster.render(&primitives, self.pixels_per_point, w, h, self.clear).unwrap());
        }
        self.painted.as_ref().unwrap()
    }
}
