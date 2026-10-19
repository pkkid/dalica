//! The face scan's pace follows the window's input (M13.51, `faces.md` §4, *Pace follows the
//! user*): the window reports each frame's input to the session (`dalica_ui::user_activity`), on a
//! session whose clock is virtual (16 ms a frame), with a synthetic detector and recogniser
//! planted in a models folder as `suggestions_ui.rs` plants its recogniser.
//!
//! - while a wheel, a key or a drag arrives every frame, the scan starts no photo and the session
//!   holds the input as just seen;
//! - a pointer move alone is reported as the pointer's;
//! - frames with no input for three seconds report `Idle`, and the scan finds the faces;
//! - with no input on the Unnamed grid, and while the window is minimised, `Away`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{Clock, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_engine::faces_scan::UserActivity;
use dalica_faces::graph::{Attr, Graph, Node, Weight};
use dalica_faces::known;
use dalica_faces::manifest::{InputSpec, ModelManifest, OutputSpec};
use dalica_faces::synthetic::{Planted, encode, yunet_shaped};
use dalica_fs::Expect;
use dalica_ui::headless::{KeyState, PointerEvent, PointerKind};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton, pos2, vec2};
use serde_json::json;

use crate::common::{FRAMES, LARGE, new_driver};

const WAIT: Duration = Duration::from_secs(120);
/// One frame of the virtual clock.
const FRAME: Duration = Duration::from_millis(16);
/// The photos; the detector finds one face on each.
const PHOTOS: usize = 3;
/// The detector's input side.
const SIDE: usize = 64;

/// Installs `bytes` as model `manifest.id` in `models` (its folder, `model.onnx` and
/// `face-model.json`, as an install leaves them).
fn install(models: &Path, mut manifest: ModelManifest, bytes: &[u8]) {
    manifest.sha256 = Some(dalica_faces::hash::sha256_hex(bytes));
    manifest.size_bytes = Some(bytes.len() as u64);
    let dir = models.join(&manifest.id);
    dalica_fs::create_dir_all(&dir).unwrap();
    dalica_fs::replace(&dir.join("model.onnx"), bytes, Expect::Absent).unwrap();
    dalica_fs::replace(&dir.join("face-model.json"), &serde_json::to_vec_pretty(&manifest).unwrap(), Expect::Absent).unwrap();
}

/// A models folder with a detector of the real one's shape reporting one face on every photo, and
/// a recogniser giving four numbers per face.
fn models(root: &Path) -> PathBuf {
    let models = root.join("models");
    let mut det = known::yunet();
    det.id = "test-detector".into();
    det.input.width = SIDE as u32;
    det.input.height = SIDE as u32;
    install(&models, det, &encode(&yunet_shaped(5, SIDE, &[Planted { row: 1, col: 1, side: 1.5 }]).graph));
    let nodes = vec![
        Node::new("Conv", &["data", "w", "b"], "c", &[("kernel_shape", Attr::Ints(vec![1, 1]))]),
        Node::new("GlobalAveragePool", &["c"], "p", &[]),
        Node::new("Flatten", &["p"], "emb", &[("axis", Attr::Int(1))]),
    ];
    let mix = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    let weights = [("w".to_string(), Weight::f32(&[4, 3, 1, 1], mix)), ("b".to_string(), Weight::f32(&[4], vec![0.0; 4]))];
    let bytes = encode(&Graph {
        nodes,
        weights: weights.into_iter().collect(),
        inputs: vec![("data".into(), vec![1, 3, 112, 112])],
        outputs: vec!["emb".into()],
    });
    let mut rec = known::sface();
    rec.id = "test-recogniser".into();
    rec.input = InputSpec::default();
    rec.output = OutputSpec::Embedding { dim: 4 };
    rec.thresholds.match_cosine = Some(0.5);
    rec.calibration = None;
    install(&models, rec, &bytes);
    models
}

fn session(d: &mut Driver) -> &mut Session {
    d.app_mut().session_mut().unwrap()
}

/// `n` frames with no input, the clock moving on a frame each.
fn idle(d: &mut Driver, clock: &VirtualClock, n: usize) {
    for _ in 0..n {
        clock.advance(FRAME);
        d.step();
    }
}

/// The scan is paused and has started no photo, the session holding input as seen within the last
/// second (an unchanged activity is reported once a second).
fn held(d: &mut Driver, clock: &VirtualClock, what: &str) {
    let s = session(d);
    let input = s.face_scan().activity().input;
    assert!(input.is_some_and(|t| clock.now().since(t) < Duration::from_secs(1)), "{what}: reported as input: {input:?}");
    let status = s.execute("faces.index", &json!({})).unwrap();
    assert_eq!((status["pace"].clone(), status["stats"]["started"].clone()), (json!("pause"), json!(0)), "{what}: no photo starts");
}

#[test]
fn the_scan_holds_back_while_input_arrives_and_runs_when_the_window_is_idle() {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, PHOTOS, 51).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let models = models(dir.path());
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_780_000_000_000)));
    let mut s = Session::open(&dir.path().join("library"), clock.clone()).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), PHOTOS);
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(FRAMES).unwrap();
    // Below the photos: an empty part of the light table.
    let empty = pos2(700.0, 700.0);

    // The wheel turning while the models load: nothing starts.
    d.scroll(None, vec2(0.0, -40.0)).unwrap();
    session(&mut d).set_face_models_dir(Some(models));
    let until = Instant::now() + WAIT;
    while session(&mut d).faces_index().model().is_none() {
        assert!(Instant::now() < until, "the models loaded");
        clock.advance(FRAME);
        d.scroll(None, vec2(0.0, -40.0)).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    held(&mut d, &clock, "the wheel");
    // Two seconds of input of each kind, a frame of the clock for each event.
    for _ in 0..40 {
        clock.advance(FRAME);
        d.scroll(None, vec2(0.0, -40.0)).unwrap();
        held(&mut d, &clock, "the wheel");
        clock.advance(FRAME);
        d.key_raw(egui::Key::F20, Modifiers::NONE, KeyState::Press);
        held(&mut d, &clock, "a key");
        clock.advance(FRAME);
        let drag = [
            PointerEvent { kind: PointerKind::Down, pos: empty },
            PointerEvent { kind: PointerKind::Drag, pos: empty + vec2(20.0, 0.0) },
            PointerEvent { kind: PointerKind::Up, pos: empty + vec2(40.0, 0.0) },
        ];
        d.pointer(&drag, PointerButton::Primary, Modifiers::NONE).unwrap();
        held(&mut d, &clock, "a drag");
    }

    // The pointer moving alone.
    clock.advance(FRAME);
    d.pointer(&[PointerEvent { kind: PointerKind::Move, pos: empty + vec2(0.0, 30.0) }], PointerButton::Primary, Modifiers::NONE).unwrap();
    let seen = session(&mut d).face_scan().activity();
    assert_eq!(seen.pointer, Some(clock.now()), "a pointer move is the pointer's");
    assert!(seen.input < seen.pointer, "and not input");

    // Three seconds of the clock with no input: idle, and the scan finds the faces.
    idle(&mut d, &clock, 200);
    let last = session(&mut d).face_scan().activity().last.map(|(a, _)| a);
    assert_eq!(last, Some(UserActivity::Idle));
    let until = Instant::now() + WAIT;
    while session(&mut d).face_scan().stats().found < PHOTOS as u64 {
        assert!(Instant::now() < until, "the scan found the faces: {:?}", session(&mut d).face_scan().stats());
        idle(&mut d, &clock, 1);
        std::thread::sleep(Duration::from_millis(2));
    }

    // No input on the Unnamed grid: away.
    d.app_mut().state_mut().unnamed.open = true;
    idle(&mut d, &clock, 2);
    let last = session(&mut d).face_scan().activity().last.map(|(a, _)| a);
    assert_eq!(last, Some(UserActivity::Away), "idle on the People view");
}

#[test]
fn a_minimised_window_is_away() {
    let ctx = egui::Context::default();
    let read = |minimized: bool| {
        let mut raw = egui::RawInput::default();
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().minimized = Some(minimized);
        let mut got = None;
        let mut out = ctx.run_ui(raw, |ui| got = Some(ui.input(|i| dalica_ui::user_activity::read(i, false))));
        out.textures_delta.clear();
        got.unwrap()
    };
    assert_eq!(read(true), UserActivity::Away);
    assert_eq!(read(false), UserActivity::Idle);
}
