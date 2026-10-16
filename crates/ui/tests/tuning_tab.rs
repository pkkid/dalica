//! The Tuning tab and the Basic Fixes Fill Light row (`editing.md` §6.5, §7), driven as a user and
//! an agent would: a Fill Light drag is one "Tuning" step, previewed live through drafts that each
//! arrive within 16 ms, and further drags in the same tab visit join that step; the Basic Fixes row
//! shows the same value and its own drag is its own step; Auto lighting sets the three lighting
//! sliders in one step; the eyedropper picks the grey patch of a photo with a colour cast and the
//! cast is gone; hover tooltips, + / − nudges and double-click resets; the script path through the
//! Tuning tool and its graceful failures; the tab's fingerprints at 1400 × 900 and 900 × 600; after
//! a rotation the tab and the row still show the photo's Tuning and a drag updates it in place, and
//! after a retouch a drag adds a new entry (M8.28).
//!
//! `DALICA_SHOTS=<dir>` writes the tab's screenshots there too.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::{FRAMES, LARGE, SMALL, check_rect, new_driver};
use dalica_engine::Session;
use dalica_scenes::{Scene, fingerprint};
use dalica_ui::control::{Config, Connection, Control, Reply, Request, serve};
use dalica_ui::headless::{Click, Drag, KeyState, PointerEvent, PointerKind, Target};
use dalica_ui::panels::edit::canvas;
use dalica_ui::{App, Driver};
use dalica_ui_kit::WidgetInfo;
use egui::{Modifiers, PointerButton, pos2, vec2};
use serde_json::{Value, json};

/// A dull, dark photo (every level between 40 and 142): all three lighting wands have work.
const DULL: &str = "Dull.png";
/// A photo with a warm cast and a grey patch in its middle.
const CAST: &str = "Cast.png";
/// The cast: red, green and blue multiplied by these.
const TINT: [f64; 3] = [1.0, 0.86, 0.68];
/// The grey patch before the cast, and its share of each side around the centre.
const GREY: f64 = 150.0;
const PATCH: f64 = 0.3;
/// The longest a [`slow_drag`] move waits for its draft to arrive: only a guard against a hang,
/// as the move waits for the draft itself (however slow a shared CI runner's debug build is), not
/// for a time.
const DRAFT_WAIT: Duration = Duration::from_secs(60);

struct T {
    _dir: tempfile::TempDir,
    control: Control,
    conn: Connection,
    d: Driver,
    /// The demo photos by id.
    photos: Vec<u64>,
    dull: u64,
    cast: u64,
}

fn rgba(w: u32, h: u32, f: impl Fn(u32, u32, [u8; 3]) -> [u8; 3]) -> Vec<u8> {
    let scene = Scene::new(23, w, h).render().unwrap();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            let p = f(x, y, [scene.data[i], scene.data[i + 1], scene.data[i + 2]]);
            out.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
    }
    out
}

/// A library of one folder: `n` demo photos, the dull photo and the cast photo; the window at
/// `size`.
fn setup((w, h): (u32, u32), n: usize) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, n, 11).unwrap();
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    std::fs::create_dir_all(&folder).unwrap();
    for f in &files {
        std::fs::write(dir.path().join("photos").join(&f.path), &f.bytes).unwrap();
    }
    let (pw, ph) = (480, 320);
    let dull = rgba(pw, ph, |_, _, p| p.map(|v| (40.0 + f64::from(v) * 0.4).round() as u8));
    std::fs::write(folder.join(DULL), fingerprint::png_rgba(pw, ph, &dull)).unwrap();
    let cast = rgba(pw, ph, |x, y, p| {
        let inside = |v: u32, side: u32| (f64::from(v) / f64::from(side) - 0.5).abs() < PATCH / 2.0;
        let base = if inside(x, pw) && inside(y, ph) { [GREY; 3] } else { p.map(f64::from) };
        [0, 1, 2].map(|c| (base[c] * TINT[c]).round() as u8)
    });
    std::fs::write(folder.join(CAST), fingerprint::png_rgba(pw, ph, &cast)).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, u64)> = s.library().photos().map(|p| (p.name.clone(), p.id.get())).collect();
    ids.sort();
    let id_of = |name: &str| ids.iter().find(|(n, _)| n == name).unwrap().1;
    let (dull, cast) = (id_of(DULL), id_of(CAST));
    let photos: Vec<u64> = ids.iter().filter(|(n, _)| n != DULL && n != CAST).map(|p| p.1).collect();
    assert_eq!(photos.len(), n);
    let control = Control::new(Config::default());
    let mut d = new_driver(App::with_picker(Some(s), control.picker()), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, control, conn: Connection::new(), d, photos, dull, cast }
}

fn call(t: &mut T, method: &str, params: Value) -> Reply {
    serve(&t.control, &mut t.conn, &mut t.d, &Request::new(method, params))
}

fn ok(t: &mut T, method: &str, params: Value) -> Value {
    call(t, method, params.clone()).unwrap_or_else(|e| panic!("{method} {params}: {e}"))
}

fn state(t: &mut T) -> Value {
    ok(t, "ui.state", json!({}))
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = ok(t, "engine.execute", json!({ "command": id, "params": params }));
    t.d.settle(FRAMES).unwrap();
    v
}

/// The error code `id` answers with `params`.
fn refused(t: &mut T, id: &str, params: Value) -> String {
    match call(t, "engine.execute", json!({ "command": id, "params": params.clone() })) {
        Ok(v) => panic!("{id} {params} answered {v}"),
        Err(e) => e.code.as_str().to_owned(),
    }
}

fn click(t: &mut T, id: &str) {
    ok(t, "ui.clickWidget", json!({ "id": id }));
    t.d.settle(FRAMES).unwrap();
}

fn find(t: &T, id: &str) -> Option<WidgetInfo> {
    t.d.widgets().into_iter().find(|w| w.id == id)
}

fn widget(t: &T, id: &str) -> WidgetInfo {
    find(t, id).unwrap_or_else(|| panic!("no `{id}` in {:?}", t.d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>()))
}

fn history(t: &mut T, id: u64) -> Value {
    run(t, "photo.history.state", json!({ "id": id }))
}

fn edits(t: &mut T, id: u64) -> Vec<String> {
    serde_json::from_value(history(t, id)["edits"].clone()).unwrap()
}

/// The values of a `tuning(fill,highlights,shadows,temp,neutral)` entry.
fn tuning(entry: &str) -> (f64, f64, f64, f64, String) {
    let inner = entry.strip_prefix("tuning(").and_then(|e| e.strip_suffix(')')).unwrap_or_else(|| panic!("not a tuning entry: {entry}"));
    let parts: Vec<&str> = inner.split(',').collect();
    let n = |i: usize| parts[i].parse::<f64>().unwrap();
    (n(0), n(1), n(2), n(3), parts[4].to_owned())
}

fn last_tuning(t: &mut T, id: u64) -> (f64, f64, f64, f64, String) {
    tuning(edits(t, id).last().expect("an entry"))
}

/// Opens photo `id` in the edit view on the Tuning tab, its preview drawn.
fn open_tuning(t: &mut T, id: u64) {
    run(t, "view.edit", json!({ "id": id }));
    click(t, "tab:edit.tuning");
    wait_preview(t);
}

/// Waits for the open photo's preview to be final, and draws it.
fn wait_preview(t: &mut T) {
    t.d.app_mut().session_mut().unwrap().wait_preview(Duration::from_secs(20));
    t.d.settle(FRAMES).unwrap();
}

/// `v` as a Tuning slider's tooltip writes it (the slider holds an `f32`).
fn two_places(v: f64) -> String {
    format!("{:.2}", f64::from(v as f32))
}

/// The tooltip shown after resting the pointer on `id`.
fn tooltip(t: &mut T, id: &str) -> String {
    t.d.hover(id, 1000).unwrap();
    t.d.app().tooltip().unwrap_or_default().to_owned()
}

/// Steps frames (letting workers run) until `done` holds; panics after 20 s.
fn until(t: &mut T, what: &str, mut done: impl FnMut(&mut T) -> bool) {
    let start = Instant::now();
    while !done(t) {
        assert!(start.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
        t.d.step();
    }
    t.d.settle(FRAMES).unwrap();
}

/// What a [`slow_drag`] saw: every frame's time and every draft's time (request to shown) from
/// the press to the last move, gathered as they happen (the perf window holds only the last
/// frames), and how many moves left their draft still rendering on the preview's workers when
/// their frames were done.
struct Dragged {
    frames: Vec<Duration>,
    drafts: Vec<Duration>,
    rendering: usize,
}

/// The frames drawn since `seen` (a perf sample's clock time), gathered into `d`; `seen` moves on.
fn gather(t: &T, seen: &mut f64, d: &mut Dragged) {
    let from = *seen;
    for s in t.d.perf().samples().filter(|s| s.at > from) {
        d.frames.push(s.update);
        d.drafts.extend(s.preview_draft);
        *seen = s.at;
    }
}

/// Drags slider `id` from its centre by `dx` in `steps` moves, each move's draft arriving before the
/// next move. Waits for each draft itself, so the drag proves the same on a fast machine and on a
/// loaded one: a draft that lands within the move's own frames is counted, and a draft that never
/// arrives (dropped, or cancelled by the frames of the slider held still) fails the drag.
fn slow_drag(t: &mut T, id: &str, dx: f32, steps: usize) -> Dragged {
    let w = widget(t, id);
    let start = pos2(w.rect[0] + w.rect[2] / 2.0, w.rect[1] + w.rect[3] / 2.0);
    let ev = |kind, pos| PointerEvent { kind, pos };
    t.d.pointer(&[ev(PointerKind::Move, start), ev(PointerKind::Down, start)], PointerButton::Primary, Modifiers::NONE).unwrap();
    let mut seen = t.d.perf().last().map_or(0.0, |s| s.at);
    let mut dragged = Dragged { frames: Vec::new(), drafts: Vec::new(), rendering: 0 };
    let requests = |t: &T| t.d.app().session().unwrap().preview_stats().requests;
    for i in 1..=steps {
        let at = start + vec2(dx * i as f32 / steps as f32, 0.0);
        let drafts_before = dragged.drafts.len();
        t.d.pointer(&[ev(PointerKind::Drag, at)], PointerButton::Primary, Modifiers::NONE).unwrap();
        // The frames are done and the move's draft is still on the workers: no frame waited for it.
        if t.d.app().session().unwrap().preview_pending() > 0 {
            dragged.rendering += 1;
        }
        gather(t, &mut seen, &mut dragged);
        let before = requests(t);
        // Frames go on while the slider is held still (as the window's would).
        for _ in 0..3 {
            t.d.step();
        }
        gather(t, &mut seen, &mut dragged);
        // Wait for the move's draft (on the session's own wait, a frame drawn after each slice of
        // 100 ms), whenever it lands: within the move's frames, the held-still frames or later.
        let since = Instant::now();
        while dragged.drafts.len() == drafts_before {
            assert!(since.elapsed() < DRAFT_WAIT, "move {i}'s draft did not arrive within {DRAFT_WAIT:?}");
            let idle = t.d.app().session().unwrap().preview_pending() == 0;
            t.d.app_mut().session_mut().unwrap().wait_preview(Duration::from_millis(100));
            t.d.step();
            gather(t, &mut seen, &mut dragged);
            assert!(!idle || dragged.drafts.len() > drafts_before, "move {i}'s draft was dropped: no preview job is left and no frame showed it");
        }
        // The slider held still: its frames left the move's draft alone (asking for the same
        // preview again would cancel it, and a draft slower than a frame would never arrive).
        assert_eq!(requests(t), before, "frames of a slider held still asked again for move {i}'s preview, cancelling its draft");
    }
    let end = start + vec2(dx, 0.0);
    t.d.pointer(&[ev(PointerKind::Up, end), ev(PointerKind::Move, pos2(5.0, 5.0))], PointerButton::Primary, Modifiers::NONE).unwrap();
    t.d.settle(FRAMES).unwrap();
    dragged
}

/// The nearest-rank `q` quantile of `v` (sorted here).
fn quantile(mut v: Vec<Duration>, q: f64) -> Duration {
    v.sort();
    let i = ((v.len() as f64 * q).ceil() as usize).clamp(1, v.len().max(1)) - 1;
    v.get(i).copied().unwrap_or_default()
}

/// Checks the pixels of `rect` against `tests/fingerprints/<name>.json` (`DALICA_BLESS=1` writes
/// it), and writes them to the test output (and to `$DALICA_SHOTS`).
fn check_region(t: &T, name: &str, rect: [f32; 4]) {
    check_rect(&t.d, name, rect);
}

/// A Fill Light drag is one "Tuning" step, previewed through drafts that each arrive within
/// 16 ms; a Highlights drag in the same tab visit joins it; the Basic Fixes row shows the same
/// value and its own drag is its own step, undone on its own.
#[test]
fn a_fill_light_drag_is_one_tuning_step_and_basic_fixes_shows_it() {
    let mut t = setup((1400, 900), 1);
    let p = t.dull;
    open_tuning(&mut t, p);
    let before = edits(&mut t, p);
    assert!(!before.iter().any(|e| e.starts_with("tuning(")), "{before:?}");

    // What keeps every frame of a drag within 16 ms, checked structurally (this debug build's
    // renderer is unoptimised and CI runners are shared, so wall-clock times say little here): a
    // move's frame hands its draft to the preview's workers and ends while it renders, never
    // waiting for it, and the drafts arrive during the drag. With `DALICA_ACCEPT_RELEASE=1` in a
    // release build, the times themselves: the drag's frames and the drafts within 16 ms (p95).
    let steps = 8;
    let dragged = slow_drag(&mut t, "slider:tuning.fill_light", 60.0, steps);
    assert!(dragged.rendering >= steps - 1, "the moves' frames ended while their drafts rendered: {} of {steps}", dragged.rendering);
    let Dragged { frames, drafts, .. } = dragged;
    assert!(drafts.len() >= steps, "every move's draft arrived while dragging: {drafts:?}");
    let (frames_p95, drafts_p95) = (quantile(frames.clone(), 0.95), quantile(drafts.clone(), 0.95));
    eprintln!("drag: frames p95 {frames_p95:?}, drafts p95 {drafts_p95:?} ({drafts:?})");
    if std::env::var_os("DALICA_ACCEPT_RELEASE").is_some() {
        let release = !cfg!(debug_assertions);
        assert!(release, "DALICA_ACCEPT_RELEASE=1 checks times in a release build (`cargo test --release`)");
        assert!(frames_p95 <= Duration::from_millis(16), "the drag's frames: {frames:?}");
        assert!(drafts_p95 <= Duration::from_millis(16), "the drafts: {drafts:?}");
    }
    let s = state(&mut t);
    assert_eq!(s["edit"]["tool"], Value::Null, "letting go closes the session's tool");
    assert_eq!(s["edit"]["tuning"], Value::Null);
    let after = edits(&mut t, p);
    assert_eq!(after.len(), before.len() + 1, "{after:?}");
    let (fill, ..) = last_tuning(&mut t, p);
    assert!(fill > 0.5, "the drag went right of the middle: {fill}");
    assert_eq!(history(&mut t, p)["undoLabel"], "Undo Tuning");
    assert_eq!(widget(&t, "button:edit.undo").label, "Undo Tuning");
    // The preview shows the photo's own recipe again, with the Tuning in it.
    let shown = t.d.app().session().unwrap().preview().unwrap().recipe.clone();
    assert_eq!(shown, after);

    // Another drag in the same visit: still one step.
    slow_drag(&mut t, "slider:tuning.highlights", 40.0, 4);
    let joined = edits(&mut t, p);
    assert_eq!(joined.len(), after.len());
    let (fill2, highlights, ..) = last_tuning(&mut t, p);
    assert_eq!(fill2, fill);
    assert!(highlights > 0.25, "{highlights}");

    // The Basic Fixes row shows the same value.
    let tab_value = tooltip(&mut t, "slider:tuning.fill_light");
    assert_eq!(tab_value, two_places(fill));
    click(&mut t, "tab:edit.basic");
    assert_eq!(widget(&t, "slider:basic.fill_light").label, "Fill Light");
    assert_eq!(tooltip(&mut t, "slider:basic.fill_light"), tab_value);

    // Its own drag is a step of its own on the same entry.
    slow_drag(&mut t, "slider:basic.fill_light", -50.0, 4);
    let (fill3, highlights3, ..) = last_tuning(&mut t, p);
    assert!(fill3 < fill, "{fill3} < {fill}");
    assert_eq!(highlights3, highlights);
    assert_eq!(edits(&mut t, p).len(), joined.len(), "the Tuning entry is updated in place");
    click(&mut t, "tab:edit.tuning");
    assert_eq!(tooltip(&mut t, "slider:tuning.fill_light"), two_places(fill3));
    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, p), joined, "one undo takes back the Basic Fixes drag");
    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, p), before, "one more takes back the tab's whole session");
}

/// Auto lighting sets Fill Light, Highlights and Shadows to the wand's answer in one step.
#[test]
fn auto_lighting_sets_three_sliders_in_one_step() {
    let mut t = setup((1400, 900), 1);
    let p = t.dull;
    open_tuning(&mut t, p);
    let before = edits(&mut t, p);
    // What the wand answers when asked directly.
    let job = run(&mut t, "photo.tuning.auto", json!({ "id": p, "what": "lighting" }))["job"].as_u64().unwrap();
    let answer = t.d.app_mut().session_mut().unwrap().wait_job(job, Duration::from_secs(20)).unwrap().result.unwrap();
    let want = ["fill", "highlights", "shadows"].map(|k| answer[k].as_f64().unwrap());
    assert!(want.iter().all(|v| *v > 0.0), "a dull, dark photo needs all three: {answer}");

    assert_eq!(widget(&t, "button:tuning.autoLighting").label, "Auto lighting");
    click(&mut t, "button:tuning.autoLighting");
    until(&mut t, "the wand's answer", |t| edits(t, p).len() > before.len());
    let after = edits(&mut t, p);
    assert_eq!(after.len(), before.len() + 1);
    let (fill, highlights, shadows, temp, neutral) = last_tuning(&mut t, p);
    assert_eq!([fill, highlights, shadows], want);
    assert_eq!((temp, neutral.as_str()), (0.0, "none"));
    for (id, v) in [("slider:tuning.fill_light", fill), ("slider:tuning.highlights", highlights), ("slider:tuning.shadows", shadows)] {
        assert_eq!(tooltip(&mut t, id), two_places(v), "{id}");
    }
    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, p), before, "one step");
}

/// The eyedropper arms the picker (the Tuning tool opens, its button shows active); a click on the
/// grey patch of a photo with a warm cast picks it, and the photo shows the patch grey.
#[test]
fn picking_the_grey_patch_removes_the_cast() {
    let mut t = setup((1400, 900), 1);
    let p = t.cast;
    open_tuning(&mut t, p);
    let centre = |t: &T| {
        let s = t.d.app().session().unwrap().preview().unwrap();
        let (w, h) = (s.image.width, s.image.height);
        let i = ((h / 2 * w + w / 2) * 4) as usize;
        [s.image.data[i], s.image.data[i + 1], s.image.data[i + 2]]
    };
    let spread = |c: [u8; 3]| c.iter().max().unwrap() - c.iter().min().unwrap();
    assert!(spread(centre(&t)) > 30, "the cast shows: {:?}", centre(&t));
    assert_eq!(widget(&t, "button:tuning.neutralPicker").label, "Pick a grey or white area of the photo to remove a color cast");
    let before = edits(&mut t, p);

    click(&mut t, "button:tuning.neutralPicker");
    assert_eq!(state(&mut t)["edit"]["tool"], "tuning");
    assert!(widget(&t, "button:tuning.neutralPicker").active);
    // A second click disarms it, a third arms it again.
    click(&mut t, "button:tuning.neutralPicker");
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);
    assert!(!widget(&t, "button:tuning.neutralPicker").active);
    click(&mut t, "button:tuning.neutralPicker");

    let v = canvas::view(t.d.ctx()).unwrap();
    let at = v.image.center();
    let ev = |kind| PointerEvent { kind, pos: at };
    t.d.pointer(&[ev(PointerKind::Move), ev(PointerKind::Down), ev(PointerKind::Up)], PointerButton::Primary, Modifiers::NONE).unwrap();
    until(&mut t, "the picked colour", |t| edits(t, p).len() > before.len());
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null, "a pick disarms the eyedropper");
    assert_eq!(state(&mut t)["view"], "edit", "the click did not leave the edit view");
    let (.., neutral) = last_tuning(&mut t, p);
    assert!(neutral.starts_with('#'), "{neutral}");
    wait_preview(&mut t);
    let c = centre(&t);
    assert!(spread(c) <= 4, "the patch is grey now: {c:?}");
    assert_eq!(history(&mut t, p)["undoLabel"], "Undo Tuning");
}

/// Hovering a slider shows its value; + and − nudge the hovered slider by 2 % of its range, each
/// press joining the tab's session; a double-click resets to the default; the wands and the
/// eyedropper have their tooltips.
#[test]
fn tooltips_nudges_and_double_click_resets() {
    let mut t = setup((1400, 900), 1);
    let p = t.photos[0];
    open_tuning(&mut t, p);
    let before = edits(&mut t, p).len();
    assert_eq!(tooltip(&mut t, "slider:tuning.highlights"), "0.00");
    // The pointer stays on the slider: + and − reach it.
    t.d.key_raw(egui::Key::Plus, Modifiers::NONE, KeyState::Press);
    t.d.key_raw(egui::Key::Plus, Modifiers::NONE, KeyState::Press);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(last_tuning(&mut t, p).1, 0.02, "two nudges of 2 % of 0 … 0.5");
    assert_eq!(tooltip(&mut t, "slider:tuning.highlights"), "0.02");
    t.d.key_raw(egui::Key::Minus, Modifiers::NONE, KeyState::Press);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(last_tuning(&mut t, p).1, 0.01);
    assert_eq!(tooltip(&mut t, "slider:tuning.temperature"), "0.00");
    t.d.key_raw(egui::Key::Plus, Modifiers::NONE, KeyState::Press);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(last_tuning(&mut t, p).3, 0.04, "2 % of −1 … 1");
    assert_eq!(edits(&mut t, p).len(), before + 1, "every nudge joined one step");

    let double = Click { count: 2, ..Click::default() };
    t.d.click_with("slider:tuning.temperature", &double).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(last_tuning(&mut t, p).3, 0.0, "reset to the default");
    assert_eq!(tooltip(&mut t, "slider:tuning.temperature"), "0.00");

    for (id, tip) in [
        ("button:tuning.autoLighting", "Auto lighting"),
        ("button:tuning.autoColor", "Auto color"),
        ("button:tuning.neutralPicker", "Pick a grey or white area of the photo to remove a color cast"),
        ("button:tuning.reset", "Put every Tuning control back to its default"),
    ] {
        assert_eq!(tooltip(&mut t, id), tip, "{id}");
    }
    // Reset: every control back to its default, and then it is greyed.
    assert!(widget(&t, "button:tuning.reset").enabled);
    click(&mut t, "button:tuning.reset");
    assert_eq!(last_tuning(&mut t, p), (0.0, 0.0, 0.0, 0.0, "none".into()));
    assert!(!widget(&t, "button:tuning.reset").enabled);
    assert_eq!(edits(&mut t, p).len(), before + 1, "still the one session");
}

/// Scripts drive the tab through the Tuning tool: `set` with `preview` shows the draft without a
/// step, `set` and `apply` make one; the wands by action; refusals leave everything as it was.
/// Esc while the eyedropper is armed disarms it and the photo shows its own recipe.
#[test]
fn the_tool_drives_the_tab_and_refuses_bad_actions() {
    let mut t = setup((1400, 900), 1);
    let p = t.photos[0];
    open_tuning(&mut t, p);
    let before = edits(&mut t, p);
    run(&mut t, "edit.tool.open", json!({ "tool": "tuning" }));
    let v = run(&mut t, "edit.tool.action", json!({ "name": "set", "params": { "fill": 0.3, "temperature": -0.5, "preview": true } }));
    assert_eq!(v["preview"], true);
    assert_eq!(edits(&mut t, p), before, "a preview takes no step");
    let draft = state(&mut t)["edit"]["tuning"].clone();
    assert_eq!((draft["fill"].as_f64().unwrap(), draft["temperature"].as_f64().unwrap()), (0.3_f32 as f64, -0.5));
    let shown = t.d.app().session().unwrap().preview().unwrap().recipe.clone();
    assert_eq!(shown.last().unwrap(), "tuning(0.3,0,0,-0.5,none)", "the photo shows the draft");
    run(&mut t, "edit.tool.action", json!({ "name": "set", "params": { "neutral": [0.6, 0.5, 0.4] } }));
    assert_eq!(edits(&mut t, p).len(), before.len() + 1);
    assert_eq!(last_tuning(&mut t, p), (0.3, 0.0, 0.0, -0.5, "#998066".into()));
    let applied = run(&mut t, "edit.tool.apply", json!({}));
    assert_eq!(applied["changed"], false, "nothing left to apply");
    assert_eq!(state(&mut t)["edit"]["tool"], Value::Null);

    // Refusals change nothing.
    run(&mut t, "edit.tool.open", json!({ "tool": "tuning" }));
    let kept = edits(&mut t, p);
    for (params, code) in [
        (json!({ "name": "set", "params": { "fill": 1.5 } }), "bad_params"),
        (json!({ "name": "set", "params": { "highlights": -0.1 } }), "bad_params"),
        (json!({ "name": "set", "params": { "temperature": "warm" } }), "bad_params"),
        (json!({ "name": "set", "params": { "neutral": [1, 2, 3] } }), "bad_params"),
        (json!({ "name": "set", "params": { "temp": 0.1 } }), "bad_params"),
        (json!({ "name": "auto", "params": { "what": "everything" } }), "bad_params"),
        (json!({ "name": "auto", "params": {} }), "bad_params"),
        (json!({ "name": "pick", "params": { "x": 1.5, "y": 0.5 } }), "bad_params"),
        (json!({ "name": "pick", "params": { "x": 0.5 } }), "bad_params"),
        (json!({ "name": "picker", "params": { "on": "yes" } }), "bad_params"),
        (json!({ "name": "reset", "params": { "all": true } }), "bad_params"),
        (json!({ "name": "nope" }), "bad_params"),
        (json!({ "name": "set", "params": [1] }), "bad_params"),
    ] {
        assert_eq!(refused(&mut t, "edit.tool.action", params.clone()), code, "{params}");
    }
    assert_eq!(edits(&mut t, p), kept);

    // The colour wand by action, then a pick by action: each answer is one change.
    let wand = json!({ "command": "edit.tool.action", "params": { "name": "auto", "params": { "what": "color" } }, "wait": false });
    let job = ok(&mut t, "engine.execute", wand);
    t.d.settle(FRAMES).unwrap();
    assert_eq!(job["pending"], true);
    run(&mut t, "edit.tool.action", json!({ "name": "pick", "params": { "x": 0.5, "y": 0.5 } }));
    until(&mut t, "the wands' answers", |t| t.d.app().session().unwrap().jobs().iter().all(|j| j.state.is_finished()));
    let (.., neutral) = last_tuning(&mut t, p);
    assert!(neutral.starts_with('#') && neutral != "#998066", "{neutral}");

    // Reset by action.
    run(&mut t, "edit.tool.action", json!({ "name": "reset" }));
    assert_eq!(last_tuning(&mut t, p), (0.0, 0.0, 0.0, 0.0, "none".into()));
    run(&mut t, "edit.tool.apply", json!({}));

    // Esc disarms the eyedropper and drops a draft shown.
    click(&mut t, "button:tuning.neutralPicker");
    run(&mut t, "edit.tool.action", json!({ "name": "set", "params": { "shadows": 0.4, "preview": true } }));
    assert!(t.d.app().session().unwrap().preview().unwrap().recipe.last().unwrap().contains("0.4"));
    t.d.key("Esc").unwrap();
    t.d.settle(FRAMES).unwrap();
    let s = state(&mut t);
    assert_eq!((s["view"].clone(), s["edit"]["tool"].clone()), (json!("edit"), Value::Null), "Esc closed the tool, not the view");
    assert!(!widget(&t, "button:tuning.neutralPicker").active);
    wait_preview(&mut t);
    let shown = t.d.app().session().unwrap().preview().unwrap().recipe.clone();
    assert_eq!(shown, edits(&mut t, p), "the photo shows its own recipe again");
    assert_eq!(last_tuning(&mut t, p).2, 0.0, "the draft was not applied");
}

/// Everything the tab draws is enabled and inside the panel in both windows; the tab's
/// fingerprints, with values set and a neutral colour picked; a modal tool draws it disabled.
#[test]
fn the_tab_fits_matches_its_fingerprints_and_a_modal_tool_disables_it() {
    let mut t = setup(LARGE, 1);
    let p = t.photos[0];
    open_tuning(&mut t, p);
    for (w, h) in [SMALL, LARGE] {
        t.d.resize(w, h);
        wait_preview(&mut t);
        for id in [
            "slider:tuning.fill_light",
            "slider:tuning.highlights",
            "slider:tuning.shadows",
            "slider:tuning.temperature",
            "button:tuning.autoLighting",
            "button:tuning.neutralPicker",
            "button:tuning.autoColor",
        ] {
            let wi = widget(&t, id);
            assert!(wi.enabled, "{w}×{h}: {id}");
            let panel = widget(&t, "region:editPanel").rect;
            assert!(wi.rect[1] + wi.rect[3] <= panel[1] + panel[3], "{w}×{h}: {id} inside the panel");
        }
    }
    let (w, h) = LARGE;
    run(&mut t, "edit.tool.open", json!({ "tool": "tuning" }));
    run(
        &mut t,
        "edit.tool.action",
        json!({ "name": "set", "params": { "fill": 0.35, "highlights": 0.1, "temperature": 0.4, "neutral": [0.7, 0.6, 0.5] } }),
    );
    run(&mut t, "edit.tool.apply", json!({}));
    wait_preview(&mut t);
    let panel = widget(&t, "region:editPanel").rect;
    check_region(&t, &format!("edit-tuning-{w}x{h}"), panel);
    if let Some(dir) = std::env::var_os("DALICA_SHOTS") {
        let shot = t.d.screenshot().unwrap();
        std::fs::write(Path::new(&dir).join(format!("tuning-window-{w}x{h}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba))
            .unwrap();
    }
    click(&mut t, "tab:edit.basic");
    check_region(&t, &format!("edit-tuning-basic-{w}x{h}"), panel);
    click(&mut t, "tool:straighten");
    assert!(!widget(&t, "slider:basic.fill_light").enabled, "{w}×{h}: a modal tool draws the row disabled");
    click(&mut t, "tab:edit.tuning");
    assert!(!widget(&t, "slider:tuning.fill_light").enabled, "{w}×{h}: a modal tool draws the tab disabled");
}

/// The driver's quick drag (`ui.dragWidget`) on a Tuning slider is one step too.
#[test]
fn a_quick_drag_is_also_one_step() {
    let mut t = setup((900, 600), 1);
    let p = t.photos[0];
    open_tuning(&mut t, p);
    let before = edits(&mut t, p).len();
    t.d.drag("slider:tuning.shadows", Target::Delta(vec2(30.0, 0.0)), &Drag::default()).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(edits(&mut t, p).len(), before + 1);
    assert!(last_tuning(&mut t, p).2 > 0.25);
}

/// M8.28: Fill Light 0.3, then a rotation: the tab and the Basic Fixes row show 0.3; a Highlights
/// drag updates that entry in place as one step that one Undo takes back; after a retouch the
/// sliders still show it, and a drag adds a new entry holding only what it moved.
#[test]
fn the_sliders_show_the_photos_tuning_after_later_edits() {
    let mut t = setup((1400, 900), 1);
    let p = t.photos[0];
    run(&mut t, "view.edit", json!({ "id": p }));
    run(&mut t, "photo.history.tuning", json!({ "id": p, "session": "basic", "fill": 0.3 }));
    run(&mut t, "photo.history.apply", json!({ "id": p, "op": "rotate(90)" }));
    click(&mut t, "tab:edit.basic");
    assert_eq!(tooltip(&mut t, "slider:basic.fill_light"), "0.30", "the row after a rotation");
    click(&mut t, "tab:edit.tuning");
    wait_preview(&mut t);
    assert_eq!(tooltip(&mut t, "slider:tuning.fill_light"), "0.30", "the tab after a rotation");

    t.d.drag("slider:tuning.highlights", Target::Delta(vec2(30.0, 0.0)), &Drag::default()).unwrap();
    t.d.settle(FRAMES).unwrap();
    let dragged = edits(&mut t, p);
    assert_eq!(dragged.len(), 2, "{dragged:?}");
    assert_eq!(dragged[1], "rotate(90)");
    let (fill, highlights, ..) = tuning(&dragged[0]);
    assert_eq!(fill, 0.3);
    assert!(highlights > 0.25, "{highlights}");
    assert_eq!(widget(&t, "button:edit.undo").label, "Undo Tuning");
    // The preview shows the photo's own recipe, the entry updated where it sits.
    assert_eq!(t.d.app().session().unwrap().preview().unwrap().recipe, dragged);
    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, p), ["tuning(0.3,0,0,0,none)", "rotate(90)"], "one Undo");
    click(&mut t, "button:edit.redo");
    assert_eq!(edits(&mut t, p), dragged);

    // A retouch after it: the sliders still show it; a drag appends a new entry.
    run(&mut t, "photo.history.apply", json!({ "id": p, "op": "retouch(0.2,0.2,0.6,0.6,0.05)" }));
    assert_eq!(tooltip(&mut t, "slider:tuning.fill_light"), "0.30", "the tab after a retouch");
    t.d.drag("slider:tuning.shadows", Target::Delta(vec2(30.0, 0.0)), &Drag::default()).unwrap();
    t.d.settle(FRAMES).unwrap();
    let appended = edits(&mut t, p);
    assert_eq!(appended.len(), 4, "{appended:?}");
    assert_eq!(appended[..3], dragged.iter().cloned().chain(["retouch(0.2,0.2,0.6,0.6,0.05)".to_string()]).collect::<Vec<_>>()[..]);
    let (fill, highlights, shadows, ..) = tuning(&appended[3]);
    assert_eq!((fill, highlights), (0.0, 0.0), "only the moved control, so the earlier Tuning is not applied twice");
    assert!(shadows > 0.25, "{shadows}");
    assert_eq!(tooltip(&mut t, "slider:tuning.fill_light"), "0.00", "the sliders show the new entry");
    click(&mut t, "button:edit.undo");
    assert_eq!(edits(&mut t, p).len(), 3, "one Undo");
    assert_eq!(tooltip(&mut t, "slider:tuning.fill_light"), "0.30");
}
