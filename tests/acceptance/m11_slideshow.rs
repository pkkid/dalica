//! M11's *Slideshow and quick preview* (`m11.rs` part 3).
//!
//! **In the live app** ([`keys_wheel_and_bar`]): `dalica --headless` on a folder of six `scenes`
//! photos, driven over its control channel with the keys, the wheel and the bar of `viewing.md`
//! §1.3–§1.4: Ctrl+4 with the second photo selected plays the folder from it, full screen; → ←
//! PgDn PgUp End Home step; Space pauses and plays; + zooms (pausing) and − fits; the wheel steps
//! back and forth; Ctrl+8 stars and Ctrl+R rotates the photo shown; a mouse move shows the bar,
//! which matches `fingerprints/m11-1400x900-slideshow-bar.json`; its Next, Previous (greyed on the
//! first photo without Loop), Play, the Transition drop-up, the Captions toggle and the Display
//! time stepper each do their part (the settings kept through `options.set`); Esc goes back to the
//! library with the last photo shown selected. The display time is 60 s there, so the show,
//! which runs on the binary's system clock, never moves on by itself while the test steps it.
//!
//! **In the app on a virtual clock** ([`transitions_and_a_slowed_decoder`]): the same `App` in the
//! headless driver (`dalica_ui::Driver`), where time is the test's to give and the engine's
//! slowed-decoder hook (`Session::plant_decode_delay`) can be planted, neither of which the binary
//! offers: each of the seven transitions at the midpoint of its change matches
//! `fingerprints/m11-1400x900-slideshow-<transition>.json`; with every decode slowed to 1.5 s on
//! the default 3 s display time, a show started with Ctrl+4 makes 30 changes and none waits for its
//! photo's preview (transitions never wait on disk).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, Timestamp, VirtualClock};
use dalica_engine::Session;
use dalica_ui::commands::slideshow as cmd;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

use crate::app::{self, click, idle, key, rect_of, resize, run, secs};
use crate::live::{self, Client};

const LARGE: (u32, u32) = (1400, 900);
const TRANSITIONS: [&str; 7] = ["cut", "panAndZoom", "dissolve", "wipe", "circle", "push", "rectangle"];

fn show(c: &mut Client) -> Value {
    let st = c.state();
    assert_eq!(st["view"], "slideshow", "a slideshow plays: {st}");
    st["slideshow"].clone()
}

fn index(c: &mut Client) -> u64 {
    show(c)["index"].as_u64().unwrap()
}

fn option(c: &mut Client, key: &str) -> Value {
    let r = c.exec("options.list", json!({ "tab": "slideshow" }));
    r["options"].as_array().unwrap().iter().find(|o| o["key"] == key).map(|o| o["value"].clone()).unwrap_or_else(|| panic!("no {key}: {r}"))
}

/// Presses the key named `name` (egui's name: `Plus`, `Minus`).
fn press(c: &mut Client, name: &str) {
    c.ok("ui.key", json!({ "key": name }));
    idle(c);
}

/// Rests the pointer on the bar's left padding: the bar shows, and stays while the pointer is on it.
fn on_bar(c: &mut Client) {
    c.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": 300.0, "y": 300.0 }] }));
    idle(c);
    let bar = rect_of(c, "controls:slideshow").unwrap_or_else(|| panic!("a mouse move shows the bar: {}", show(c)));
    c.ok("ui.pointer", json!({ "events": [{ "kind": "move", "x": bar[0] + 4.0, "y": bar[1] + 4.0 }] }));
    idle(c);
    assert_eq!(show(c)["bar"], true);
}

pub fn keys_wheel_and_bar(exe: &Path, home: &Path) {
    let started = Instant::now();
    let folder: PathBuf = home.join("Pictures").join("Show");
    for n in 0..6u64 {
        let jpeg = dalica_scenes::jpeg(&dalica_scenes::Scene::new(5000 + n, 900, 600), 85, None).unwrap();
        app::put(&folder.join(format!("IMG_{n:04}.jpg")), &jpeg, n);
    }
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    resize(c, LARGE);
    run(c, "folders.add", json!({ "path": folder }));
    run(c, "options.set", json!({ "values": { "slideshow.displayTime": 60, "slideshow.loop": false, "slideshow.transition": "dissolve" } }));
    let by_path = app::photos(c, &home.join("Pictures"));
    let o: Vec<Value> = (0..6).map(|n| by_path[&format!("Show/IMG_{n:04}.jpg")].id.clone()).collect();

    // Ctrl+4 plays the selected photo's folder from it, full screen.
    run(c, "select.set", json!({ "ids": [o[1]] }));
    key(c, "Ctrl+4");
    let s = show(c);
    assert_eq!((s["photos"].clone(), s["index"].clone(), s["playing"].clone()), (json!(o), json!(1), json!(true)), "{s}");
    assert_eq!(c.state()["fullScreen"], true, "full screen asked through the view state");

    // The keys of §1.4.
    for (k, want) in [("Right", 2), ("Left", 1), ("PageDown", 2), ("PageUp", 1), ("End", 5), ("Home", 0), ("Right", 1)] {
        key(c, k);
        assert_eq!(index(c), want, "{k}");
    }
    key(c, "Space");
    let s = show(c);
    assert_eq!((s["playing"].clone(), s["bar"].clone()), (json!(false), json!(true)), "Space pauses and shows the bar: {s}");
    key(c, "Space");
    assert_eq!(show(c)["playing"], true, "Space plays again");
    press(c, "Plus");
    let s = show(c);
    assert!(s["zoom"].as_f64().unwrap() > 1.0 && s["playing"] == false, "+ zooms in and pauses: {s}");
    press(c, "Minus");
    assert_eq!(show(c)["zoom"].as_f64(), Some(1.0), "− back to fitted");
    if show(c)["playing"] == false {
        key(c, "Space");
    }
    assert_eq!(show(c)["playing"], true);

    // The wheel steps.
    c.ok("ui.scroll", json!({ "dy": -120, "widget": "canvas:slideshow" }));
    idle(c);
    assert_eq!(index(c), 2, "the wheel turned down: next");
    c.ok("ui.scroll", json!({ "dy": 120, "widget": "canvas:slideshow" }));
    idle(c);
    assert_eq!(index(c), 1, "the wheel turned up: previous");

    // Ctrl+8 stars and Ctrl+R rotates the photo shown.
    key(c, "Ctrl+8");
    key(c, "Ctrl+R");
    let p = c.inspect(&o[1]);
    assert_eq!((p["star"].clone(), p["recipe"]["edits"].clone()), (json!(true), json!(["rotate(90)"])), "{p}");
    run(c, "edit.undo", json!({}));
    run(c, "edit.undo", json!({}));
    let p = c.inspect(&o[1]);
    assert_eq!((p["star"].clone(), p["recipe"]["edits"].clone()), (json!(false), json!([])), "one undo step each: {p}");
    println!("  Ctrl+4 played the folder from the selected photo; → ← PgDn PgUp End Home, Space, + −, the wheel, Ctrl+8 and Ctrl+R");

    // The bar, paused: shown by a mouse move, kept while the pointer rests on it.
    key(c, "Space");
    assert_eq!(show(c)["playing"], false, "paused");
    on_bar(c);
    let masks: Vec<[f64; 4]> = Vec::new();
    app::masked_shot(c, home, "m11-1400x900-slideshow-bar", LARGE, &masks);
    click(c, "button:slideshow.next");
    assert_eq!(index(c), 2, "Next");
    click(c, "button:slideshow.previous");
    assert_eq!(index(c), 1, "Previous");
    click(c, "button:slideshow.previous");
    assert_eq!(index(c), 0);
    let prev = app::widget(c, "button:slideshow.previous").unwrap();
    assert_eq!(prev["enabled"], false, "Previous is greyed on the first photo without Loop: {prev}");
    assert_eq!((show(c)["playing"].clone(), app::label(c, "button:slideshow.play")), (json!(false), "Play".to_string()), "still paused");
    click(c, "button:slideshow.play");
    assert_eq!((show(c)["playing"].clone(), app::label(c, "button:slideshow.play")), (json!(true), "Pause".to_string()), "Play");
    click(c, "button:slideshow.play");
    assert_eq!(show(c)["playing"], false, "Pause");
    click(c, "button:slideshow.transition");
    click(c, "menuitem:slideshow.transition/wipe");
    assert_eq!(option(c, "slideshow.transition"), "wipe", "the Transition drop-up keeps its choice");
    on_bar(c);
    click(c, "check:slideshow.captions");
    assert_eq!(option(c, "slideshow.captions"), false, "the Captions toggle");
    click(c, "check:slideshow.captions");
    assert_eq!(option(c, "slideshow.captions"), true);
    assert_eq!(app::widget(c, "stepper:slideshow.displayTime/plus").unwrap()["enabled"], false, "+ is greyed at 60 s");
    click(c, "stepper:slideshow.displayTime/minus");
    assert_eq!(option(c, "slideshow.displayTime"), 59, "the Display time stepper");
    assert_eq!(app::label(c, "stepper:slideshow.displayTime"), "59 seconds");
    click(c, "stepper:slideshow.displayTime/plus");
    assert_eq!(option(c, "slideshow.displayTime"), 60);
    println!(
        "  the bar on a mouse move (matches its fingerprint): Next, Previous greyed on the first photo, Play, Wipe, Captions, 59 and 60 seconds"
    );

    // Esc: back to the library with the last photo shown selected.
    key(c, "Right");
    key(c, "Escape");
    let st = c.state();
    assert_eq!((st["view"].clone(), st["slideshow"].clone(), st["fullScreen"].clone()), (json!("library"), Value::Null, json!(false)), "{st}");
    assert_eq!(st["selection"]["ids"], json!([o[1]]), "the last photo shown is selected: {st}");
    println!("  Esc: back to the library, the last photo shown selected, the window given back");
    live::quit(a, cl);
    println!("  M11 slideshow keys, wheel and bar in {:.1} s", secs(started));
}

// --- on a virtual clock -------------------------------------------------------------------------

/// One step of the virtual clock.
const STEP: Duration = Duration::from_millis(100);
/// The default display time (Options ▸ Slideshow).
const DISPLAY_MS: u64 = 3_000;
const PATIENCE: Duration = Duration::from_secs(120);

struct Fx {
    _dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    d: Driver,
    order: Vec<PhotoId>,
    section: String,
}

/// A library holding one folder of `n` `scenes` photos of `w × h`, on a virtual clock, in the
/// headless driver at 1400 × 900.
fn setup(n: usize, (w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Harbour");
    std::fs::create_dir_all(&folder).unwrap();
    for k in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(6000 + k as u64, w, h), 85, None).unwrap();
        std::fs::write(folder.join(format!("IMG_{k:04}.jpg")), bytes).unwrap();
    }
    let clock = Arc::new(VirtualClock::new(Timestamp::from_millis(1_780_000_000_000)));
    let mut s = Session::open(&dir.path().join("library"), clock.clone()).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let any = s.execute("library.query", &json!({ "limit": 1 })).unwrap()["photos"][0]["id"].as_u64().unwrap();
    let section = format!("folder:{}", s.library().photo(PhotoId::new(any)).unwrap().folder);
    let list = s.execute("slideshow.list", &json!({ "section": section })).unwrap();
    let order: Vec<PhotoId> = serde_json::from_value(list["photos"].clone()).unwrap();
    assert_eq!(order.len(), n);
    let mut d = Driver::new(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(3600).unwrap();
    Fx { _dir: dir, clock, d, order, section }
}

fn session(f: &mut Fx) -> &mut Session {
    f.d.app_mut().session_mut().unwrap()
}

/// Lets `ms` pass on the session's clock, a frame every [`STEP`], each after the preview's work and
/// the conversions to textures the frames before started have landed (the show itself never waits
/// for either; without the wait the clock runs ahead of the workers on a slow machine).
fn pass(f: &mut Fx, ms: u64) {
    for _ in 0..ms / STEP.as_millis() as u64 {
        f.clock.advance(STEP);
        assert!(session(f).wait_preview(PATIENCE));
        assert!(dalica_ui::panels::slideshow::show::wait_prepared(PATIENCE));
        f.d.step();
    }
}

fn exec(f: &mut Fx, id: &str, params: Value) {
    f.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    f.d.step();
}

fn shown(f: &Fx) -> dalica_ui::state::Slideshow {
    f.d.state().slideshow.clone().expect("a slideshow plays")
}

pub fn transitions_and_a_slowed_decoder() {
    let started = Instant::now();
    // Each transition at the midpoint of the change from the first photo to the second.
    let mut f = setup(2, (640, 420));
    let o = f.order.clone();
    let play = json!({ "section": f.section, "ids": [o[0]] });
    for t in TRANSITIONS {
        session(&mut f).execute("options.set", &json!({ "values": { "slideshow.transition": t } })).unwrap();
        exec(&mut f, "section.slideshow", play.clone());
        pass(&mut f, DISPLAY_MS + 400);
        let s = shown(&f);
        let progress = s.transition.map(|r| (r.from, r.progress));
        if t == "cut" {
            assert_eq!((s.index, progress), (1, None), "Cut takes no time");
        } else {
            assert_eq!((s.index, progress), (1, Some((0, 0.5))), "{t}: half-way from the first photo to the second");
        }
        let shot = f.d.screenshot().unwrap();
        let name = format!("m11-1400x900-slideshow-{t}");
        let out = dalica_acceptance::workspace_root().join("target/test-output/acceptance");
        std::fs::create_dir_all(&out).unwrap();
        let kept = out.join(format!("{name}.png"));
        std::fs::write(&kept, dalica_scenes::fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
        live::check(&name, shot.width, shot.height, &shot.rgba, &kept);
        exec(&mut f, "slideshow.exit", json!({}));
    }
    println!("  each of the {} transitions at its midpoint matches its fingerprint", TRANSITIONS.len());

    // Every decode slowed to 1.5 s, the default 3 s display time: 30 changes, none waits.
    let mut f = setup(31, (96, 64));
    session(&mut f).plant_decode_delay(Some(Duration::from_millis(1_500)));
    let o = f.order.clone();
    exec(&mut f, "select.set", json!({ "ids": [o[0]] }));
    f.d.key("Ctrl+4").unwrap();
    f.d.step();
    pass(&mut f, 1_500);
    for _ in 0..2_000 {
        if cmd::stats().is_some_and(|s| s.changes >= 30) {
            break;
        }
        pass(&mut f, STEP.as_millis() as u64);
    }
    let stats = cmd::stats().unwrap();
    assert_eq!((stats.changes, stats.waits), (30, 0), "no change waits for its preview: {stats:?}");
    assert_eq!(shown(&f).index, 30);
    println!("  every decode slowed to 1.5 s: {} changes, {} waits", stats.changes, stats.waits);
    println!("  M11 transitions and the slowed decoder in {:.1} s", secs(started));
}
