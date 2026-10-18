//! A seeded monkey: random clicks (single, double and right) on the widgets the last frame
//! registered, keys, drags between widgets and by offsets, scrolls, typed text, waits and window
//! sizes from 600 to 2000 pixels wide, played on a library of two folders and an album. After every
//! action: no panic, the view state passes its own checks and is already within its limits, every
//! interactive widget of every frame has a valid, unique automation id, and nothing touched the
//! disk from the UI thread (the test thread is marked as it). 200 frames quick, 2 000 at full
//! strength (`DALICA_FULL_TESTS=1`); `DALICA_MONKEY_SEED=<n>` replays another seed. A failure names
//! the seed and the actions that led to it.
//!
//! The monkey never sends photos to the trash and never adds folders from the user's disk: it
//! leaves alone the widgets and keys that delete, trash, import, open the Folder Manager or open
//! something outside the app, and answers their questions with Esc.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use crate::common::{FRAMES, demo_library, full, new_driver};
use dalica_engine::Session;
use dalica_ui::headless::{Click, Drag, Target};
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver, ScriptedPicker};
use egui::{PointerButton, Vec2, vec2};
use serde_json::json;

const QUICK_FRAMES: u64 = 200;
const FULL_FRAMES: u64 = 2_000;
const SEED: u64 = 0x00D4_11CA_1036;

/// Widgets and keys the monkey leaves alone, by a part of their id or label (lowercase): what
/// would send files to the system's trash, add folders from the user's disk or open something
/// outside the app.
const BLOCKED: [&str; 8] = ["delete", "trash", "foldermanager", "folders.manager", "import", "external", "locate", "watch"];
/// The dialogs the monkey answers with Esc, for the same reasons.
const ESCAPED: [DialogKind; 5] =
    [DialogKind::DeletePhotos, DialogKind::DeleteFolder, DialogKind::FolderManager, DialogKind::FirstRun, DialogKind::WatchDrive];
/// The keys it presses: navigation, editing, the view's shortcuts.
const KEYS: [&str; 34] = [
    "Esc",
    "Enter",
    "Tab",
    "Up",
    "Down",
    "Left",
    "Right",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Space",
    "Backspace",
    "F2",
    "F4",
    "F11",
    "Ctrl+A",
    "Ctrl+Z",
    "Ctrl+Y",
    "Ctrl+C",
    "Ctrl+X",
    "Ctrl+V",
    "Ctrl+H",
    "Ctrl+T",
    "Ctrl+F",
    "Ctrl+I",
    "Ctrl+0",
    "Ctrl+1",
    "Ctrl+2",
    "Ctrl+8",
    "Ctrl+S",
    "Shift+Right",
    "Alt+Enter",
    "Ctrl+Shift+S",
];
const TEXTS: [&str; 5] = ["Tram 28", "é漢字 ✓", "  ", "a\tb", "1234567890"];

/// SplitMix64: small, seeded, the same on every system.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `0 .. n`.
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// `lo ..= hi`.
    fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % u64::from(hi - lo + 1)) as u32
    }

    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

#[derive(Debug, Clone)]
enum Action {
    Click { id: String, fx: f32, fy: f32, count: u32, right: bool },
    Key(&'static str),
    DragTo { from: String, to: String },
    DragBy { from: String, by: Vec2 },
    Scroll { at: String, by: Vec2 },
    Type(&'static str),
    Resize(u32, u32),
    Wait(u32),
}

fn blocked(text: &str) -> bool {
    let text = text.to_lowercase();
    BLOCKED.iter().any(|b| text.contains(b))
}

/// The widgets an action may aim at: enabled, with an area, not blocked.
fn targets(d: &Driver) -> Vec<String> {
    d.widgets()
        .into_iter()
        .filter(|w| w.enabled && w.rect[2] > 0.0 && w.rect[3] > 0.0 && !blocked(&w.id) && !blocked(&w.label))
        .map(|w| w.id)
        .collect()
}

fn pick(rng: &mut Rng, d: &Driver) -> Action {
    if d.state().dialogs.last().is_some_and(|top| ESCAPED.contains(&top.kind)) {
        return Action::Key("Esc");
    }
    let ids = targets(d);
    let any = |rng: &mut Rng| ids.get(rng.below(ids.len())).cloned();
    let roll = rng.below(100);
    let action = match roll {
        0..45 => any(rng).map(|id| {
            let (fx, fy) = (0.1 + 0.8 * rng.unit(), 0.1 + 0.8 * rng.unit());
            let count = if rng.below(8) == 0 { 2 } else { 1 };
            Action::Click { id, fx, fy, count, right: rng.below(6) == 0 }
        }),
        45..65 => Some(Action::Key(KEYS[rng.below(KEYS.len())])),
        65..72 => any(rng).zip(any(rng)).map(|(from, to)| Action::DragTo { from, to }),
        72..77 => any(rng).map(|from| Action::DragBy { from, by: vec2(rng.unit() * 600.0 - 300.0, rng.unit() * 400.0 - 200.0) }),
        77..85 => any(rng).map(|at| Action::Scroll { at, by: vec2(0.0, rng.unit() * 1200.0 - 600.0) }),
        85..90 => Some(Action::Type(TEXTS[rng.below(TEXTS.len())])),
        90..95 => Some(Action::Resize(rng.range(600, 2000), rng.range(400, 1400))),
        _ => Some(Action::Wait(rng.range(1, 30))),
    };
    action.unwrap_or(Action::Wait(1))
}

/// Plays `a`. The driver refusing it (the widget went away, a disabled one) is not a failure: a
/// user's click can miss too.
fn play(d: &mut Driver, a: &Action) {
    let _ = match a {
        Action::Click { id, fx, fy, count, right } => {
            let button = if *right { PointerButton::Secondary } else { PointerButton::Primary };
            d.click_with(id, &Click { fx: *fx, fy: *fy, count: *count, button, ..Click::default() })
        }
        Action::Key(chord) => d.key(chord),
        Action::DragTo { from, to } => d.drag(from, Target::Widget(to), &Drag { steps: 6, ..Drag::default() }),
        Action::DragBy { from, by } => d.drag(from, Target::Delta(*by), &Drag { steps: 6, ..Drag::default() }),
        Action::Scroll { at, by } => d.scroll(Some(at), *by),
        Action::Type(text) => d.type_text(text),
        Action::Resize(w, h) => {
            d.resize(*w, *h);
            Ok(())
        }
        Action::Wait(frames) => {
            for _ in 0..*frames {
                d.step();
            }
            Ok(())
        }
    };
}

/// Why the app is not in order after an action, if it is not.
fn broken(d: &mut Driver, refused_before: u64) -> Option<String> {
    if let Some(breach) = d.id_breach() {
        return Some(format!("automation ids: {breach}"));
    }
    let s = d.state();
    for check in [s.check_m6(), s.check_m7(), s.check_m8(), s.check_m9(), s.check_m10()] {
        if let Err(e) = check {
            return Some(format!("the view state fails its check: {e}"));
        }
    }
    if s.clone().sanitized() != *s {
        return Some("the view state is outside its limits (sanitizing changes it)".into());
    }
    let refused = dalica_fs::guard::refused();
    (refused != refused_before).then(|| format!("{} file operations were attempted on the UI thread", refused - refused_before))
}

/// A library of two folders of scenes photos and an album holding two of them, in a 1400 × 900
/// window; exports go to the test's own folder.
fn setup(root: &Path) -> Driver {
    let mut folders = std::collections::BTreeSet::new();
    for f in demo_library(2, 12, 36).unwrap() {
        let path = root.join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
        folders.insert(path.parent().unwrap().to_path_buf());
    }
    let mut s = Session::open(&root.join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_pictures_dir(Some(root.join("Pictures")));
    // Map tiles and place searches the monkey allows go to an in-memory server, never the network.
    s.set_network_transport(Some(Arc::new(dalica_fetch::MemoryTransport::new())));
    for folder in &folders {
        s.execute("folders.add", &json!({ "path": folder })).unwrap();
    }
    s.wait_idle();
    let two: Vec<u64> = s.library().photos().take(2).map(|p| p.id.get()).collect();
    s.execute("album.create", &json!({ "name": "Trip", "ids": two })).unwrap();
    let mut d = new_driver(App::with_picker(Some(s), Arc::new(ScriptedPicker::new())), 1400, 900, 1.0);
    d.settle(FRAMES).unwrap();
    d
}

#[test]
fn a_seeded_monkey_breaks_nothing() {
    let seed = std::env::var("DALICA_MONKEY_SEED").ok().and_then(|s| s.parse().ok()).unwrap_or(SEED);
    let frames = if full() { FULL_FRAMES } else { QUICK_FRAMES };
    let dir = tempfile::tempdir().unwrap();
    let mut d = setup(dir.path());
    let mut rng = Rng(seed);
    let mut played: Vec<Action> = Vec::new();
    // From here this thread is the UI thread: the guard refuses (and counts) any file access.
    let ui_thread = dalica_fs::mark_ui_thread();
    let refused_before = dalica_fs::guard::refused();
    let start = d.frames();
    while d.frames() - start < frames {
        let action = pick(&mut rng, &d);
        played.push(action.clone());
        let tail = |played: &[Action]| played.iter().rev().take(40).rev().map(|a| format!("{a:?}")).collect::<Vec<_>>().join("\n  ");
        if catch_unwind(AssertUnwindSafe(|| play(&mut d, &action))).is_err() {
            panic!("seed {seed}: the app panicked on {action:?}; the last actions:\n  {}", tail(&played));
        }
        if let Some(why) = broken(&mut d, refused_before) {
            panic!("seed {seed}, frame {}: {why}; the last actions:\n  {}", d.frames() - start, tail(&played));
        }
    }
    drop(ui_thread);
    // Still a working app: a command runs and a frame follows.
    assert!(d.run("select.none", &json!({})).is_ok(), "seed {seed}: the app still runs commands after {} actions", played.len());
}
