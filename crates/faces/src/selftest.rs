//! The self-test a model passes before it is installed (`faces.md` §3.1): the manifest is valid,
//! the file is the one it names, the model loads within the limits, its input and outputs have the
//! shapes the manifest says, and a fixed synthetic input gives sensible results.
//!
//! For a recogniser, a noise picture and a gradient picture (both fixed) must give the manifest's
//! number of finite values at unit length, the same picture the same vector and the two pictures
//! different vectors. For a detector, a fixed photo-like picture must run, twice with the same
//! faces, every box inside the picture. The report lists each check with whether it held, so the
//! install dialog can say why a model was refused; nothing here panics on a hostile file.
//!
//! Ported from LightCraft `crates/faces/src/runtime.rs` @ f34c7a8a (MIT OR Apache-2.0): its
//! `Embedder::self_test` (the two pictures and their checks); the file, load and shape checks and
//! the detector's run are Dalica's.

use std::path::Path;
use std::time::Instant;

use serde::Serialize;

use crate::align::Rgb;
use crate::detect::{self, Detector};
use crate::embed::Embedder;
use crate::manifest::{self, ModelManifest, Role};
use crate::runtime::Model;

/// One check and whether it held.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Check {
    pub what: String,
    pub ok: bool,
}

/// What the self-test found.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfTest {
    /// Every check held: the model may be installed.
    pub ok: bool,
    pub checks: Vec<Check>,
    /// Milliseconds to read and check the model.
    pub load_ms: f64,
    /// Milliseconds of the slowest run on the synthetic input.
    pub run_ms: f64,
}

impl SelfTest {
    /// The first check that failed, in words.
    pub fn failure(&self) -> Option<&str> {
        self.checks.iter().find(|c| !c.ok).map(|c| c.what.as_str())
    }
}

struct Report {
    checks: Vec<Check>,
    load_ms: f64,
    run_ms: f64,
}

impl Report {
    fn check(&mut self, what: impl Into<String>, ok: bool) -> bool {
        self.checks.push(Check { what: what.into(), ok });
        ok
    }

    fn done(self) -> SelfTest {
        SelfTest { ok: !self.checks.is_empty() && self.checks.iter().all(|c| c.ok), checks: self.checks, load_ms: self.load_ms, run_ms: self.run_ms }
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

/// The self-test of the model file at `path` as `manifest` describes it.
pub fn run_path(path: &Path, manifest: &ModelManifest) -> SelfTest {
    let read = std::fs::metadata(path).map_err(|e| e.to_string()).and_then(|m| {
        if m.len() > crate::limits::MAX_MODEL_BYTES { Err("the file exceeds 512 MiB".into()) } else { std::fs::read(path).map_err(|e| e.to_string()) }
    });
    match read {
        Ok(bytes) => run(&bytes, manifest),
        Err(e) => {
            let mut r = Report { checks: Vec::new(), load_ms: 0.0, run_ms: 0.0 };
            r.check(format!("the file can be read: {e}"), false);
            r.done()
        }
    }
}

/// The self-test of a model file's bytes as `manifest` describes it.
pub fn run(bytes: &[u8], manifest: &ModelManifest) -> SelfTest {
    let mut r = Report { checks: Vec::new(), load_ms: 0.0, run_ms: 0.0 };
    let valid = manifest::validate(manifest);
    if !r.check(valid.as_ref().map_or_else(|e| format!("the manifest is valid: {e}"), |_| "the manifest is valid".into()), valid.is_ok()) {
        return r.done();
    }
    if let Some(want) = &manifest.sha256 {
        let ok = crate::hash::sha256_hex(bytes) == *want;
        if !r.check(if ok { "the file is the one the manifest names" } else { "the file is not the one the manifest names (SHA-256)" }, ok) {
            return r.done();
        }
    }
    let started = Instant::now();
    let model = match Model::from_bytes(bytes) {
        Ok(m) => m,
        Err(e) => {
            r.check(format!("the model loads within the limits: {e}"), false);
            return r.done();
        }
    };
    r.check("the model loads within the limits", true);
    match manifest.role {
        Role::Embedder => recogniser(&mut r, model, manifest, started),
        Role::Detector => detector(&mut r, model, manifest, started),
    }
    r.done()
}

fn recogniser(r: &mut Report, model: Model, manifest: &ModelManifest, started: Instant) {
    let e = match Embedder::new(model, manifest) {
        Ok(e) => e,
        Err(err) => {
            r.check(format!("the model's shapes match the manifest: {err}"), false);
            return;
        }
    };
    r.load_ms = ms(started);
    r.check("the model's shapes match the manifest", true);
    let (w, h) = e.input_size();
    let n = w * h * 3;
    // two fixed, different pictures: xorshift noise, and a diagonal gradient with a colour cast
    let mut seed = 0x2545_f491u32;
    let noise: Vec<u8> = (0..n)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as u8
        })
        .collect();
    let gradient: Vec<u8> = (0..n).map(|i| (((i / 3) % w + (i / 3) / w) * 255 / (w + h) + (i % 3) * 40).min(255) as u8).collect();
    let mut run = |img: &[u8]| {
        let t = Instant::now();
        let v = e.embed(img);
        r.run_ms = r.run_ms.max(ms(t));
        v
    };
    let (a, a2, b) = (run(&noise), run(&noise), run(&gradient));
    let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| f64::from(*p) * f64::from(*q)).sum::<f64>();
    match (&a, &a2, &b) {
        (Ok(a), Ok(a2), Ok(b)) => {
            r.check(
                format!("it gives {} finite numbers per face at unit length", a.len()),
                a.len() == e.dimension() && (dot(a, a) - 1.0).abs() < 1e-3,
            );
            r.check("the same picture gives the same face", dot(a, a2) > 0.9999);
            r.check("different pictures give different faces", dot(a, b) < 0.9995);
        }
        (Err(err), _, _) | (_, Err(err), _) | (_, _, Err(err)) => {
            r.check(format!("it runs on a test face: {err}"), false);
        }
    }
}

fn detector(r: &mut Report, model: Model, manifest: &ModelManifest, started: Instant) {
    let d = match Detector::new(model, manifest) {
        Ok(d) => d,
        Err(err) => {
            r.check(format!("the model's shapes match the manifest: {err}"), false);
            return;
        }
    };
    r.load_ms = ms(started);
    r.check("the model's shapes match the manifest", true);
    let (w, h) = (640, 480);
    let pixels = detect::probe_picture(w, h);
    let img = Rgb { data: &pixels, width: w, height: h };
    let mut run = || {
        let t = Instant::now();
        let found = d.detect(&img, &d.options());
        r.run_ms = r.run_ms.max(ms(t));
        found
    };
    match (run(), run()) {
        (Ok(a), Ok(b)) => {
            let inside = a.iter().all(|f| f.rect.iter().all(|v| (0.0..=1.0).contains(v)) && f.rect[0] < f.rect[2] && f.rect[1] < f.rect[3]);
            r.check(format!("it runs on a test picture ({} faces), every box inside it", a.len()), inside);
            r.check("the same picture gives the same faces", a == b);
        }
        (Err(err), _) | (_, Err(err)) => {
            r.check(format!("it runs on a test picture: {err}"), false);
        }
    }
}
