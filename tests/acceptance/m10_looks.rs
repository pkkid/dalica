//! M10's looks in the live app (`m10.rs` part 2): the Options dialog's four tabs opened with
//! Ctrl+, (General, File Types, Export, and Keyboard Shortcuts, the shortcut editor), and the main
//! window at UI scale 150 % with font size Large, each a whole-window screenshot at 1400 × 900
//! pixels matched against `fingerprints/m10-1400x900-*.json`; at 900 × 600 the dialog fits the
//! window (`design-system.md` §11.3; `roadmap.md` M10.10–M10.12).
//!
//! The shortcut editor's keys show as glyphs on macOS (`⌘8`), so there only its fit is checked, as
//! the editor's own test does (`crates/ui/tests/shortcut_editor.rs`). The General tab's fingerprint
//! is the Windows and Linux layout; macOS leaves out the options of the other platforms (Use system
//! title bar), so there the tab is checked by its rows and its fit instead, as the dialog's own test
//! does (`crates/ui/tests/options_dialog.rs`). On every platform each option `options.list` gives
//! for this platform has its control drawn, and no other option does.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, dialog, idle, key, quiet, rect_of, resize, run, secs};
use crate::live::{self, Client};

const LARGE: (u32, u32) = (1400, 900);
const SMALL: (u32, u32) = (900, 600);
const DIALOG: &str = "dialog:options";
const TITLE_BAR: &str = "interface.systemTitleBar";

/// This platform's name in `options.list` → `platforms`.
const HERE: &str = if cfg!(target_os = "macos") {
    "mac"
} else if cfg!(target_os = "windows") {
    "win"
} else {
    "linux"
};

/// `home/Pictures` with two folders of photos taken on fixed dates.
fn pictures(home: &Path) -> PathBuf {
    let pictures = home.join("Pictures");
    let photos = [
        ("Harbour/IMG_0001.jpg", "2024-05-01T09:00:00"),
        ("Harbour/IMG_0002.jpg", "2024-05-01T09:10:00"),
        ("Harbour/IMG_0003.jpg", "2024-05-01T09:20:00"),
        ("Market/IMG_0101.jpg", "2024-05-04T11:00:00"),
        ("Market/IMG_0102.jpg", "2024-05-04T11:05:00"),
    ];
    for (i, (rel, taken)) in photos.iter().enumerate() {
        let jpeg = app::jpeg(500 + i as u64, (480, 320), "Dalica", "Scenes 1", taken);
        app::put(&app::under(&pictures, rel), &jpeg, i as u64);
    }
    pictures
}

/// The window's pixels per point (`ui.inspect` → `window`), UI scale included.
fn pixels_per_point(c: &mut Client) -> f64 {
    let w = c.ok("ui.inspect", json!({}))["window"].clone();
    w["pixelsPerPoint"].as_f64().unwrap_or_else(|| panic!("ui.inspect names the window's pixels per point: {w}"))
}

/// A rect in points as pixels at `ppp`.
fn in_pixels(r: [f64; 4], ppp: f64) -> [f64; 4] {
    r.map(|v| v * ppp)
}

/// The dialog lies inside a 900 × 600 window, then the window is 1400 × 900 again.
fn fits_small(c: &mut Client, what: &str) {
    resize(c, SMALL);
    let r = rect_of(c, DIALOG).unwrap_or_else(|| panic!("{what}: the dialog is shown at 900 × 600"));
    let (w, h) = (f64::from(SMALL.0), f64::from(SMALL.1));
    assert!(r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= w + 0.5 && r[1] + r[3] <= h + 0.5, "{what}: the dialog {r:?} fits 900 × 600");
    resize(c, LARGE);
}

/// The parts of a tab that hold a temporary path: a folder option's field.
fn path_fields(c: &mut Client) -> Vec<[f64; 4]> {
    c.widgets("field:option.")
        .into_iter()
        .filter(|w| w["id"].as_str().is_some_and(|id| id.ends_with("location") || id.ends_with("folder")))
        .filter_map(|w| {
            let r = &w["rect"];
            Some([r[0].as_f64()?, r[1].as_f64()?, r[2].as_f64()?, r[3].as_f64()?])
        })
        .collect()
}

/// The automation id of the control of an option of kind `kind` (as the General tab draws it).
fn control_id(kind: &str, key: &str) -> String {
    match kind {
        "bool" => format!("check:option.{key}"),
        "folder" => format!("field:option.{key}"),
        _ => format!("choice:option.{key}"),
    }
}

/// Whether widget `id` is drawn, scrolling the dialog's body to it when it lies out of view; the
/// body is scrolled back to its top after.
fn drawn(c: &mut Client, id: &str) -> bool {
    let mut found = false;
    for _ in 0..40 {
        let body = rect_of(c, DIALOG).unwrap_or_else(|| panic!("the dialog is shown while looking for {id}"));
        let (top, bottom) = (body[1] + 80.0, body[1] + body[3] - 60.0);
        let dy = match rect_of(c, id) {
            Some(r) if r[1] >= top && r[1] + r[3] <= bottom => {
                found = true;
                break;
            }
            Some(r) if r[1] < top => 60.0,
            _ => -60.0,
        };
        c.ok("ui.scroll", json!({ "dy": dy, "widget": DIALOG }));
        idle(c);
    }
    c.ok("ui.scroll", json!({ "dy": 2000, "widget": DIALOG }));
    idle(c);
    found
}

/// The General tab's rows: every option `options.list` gives for this platform has its control
/// drawn, and an option of other platforms (Use system title bar on macOS) never is.
fn general_rows(c: &mut Client) -> usize {
    let listed = c.ok("engine.execute", json!({ "command": "options.list", "params": { "tab": "general" } }));
    let options = listed["options"].as_array().unwrap_or_else(|| panic!("options.list gives the tab's options: {listed}")).clone();
    let on_here = |o: &Value| o["platforms"].as_array().is_none_or(|p| p.iter().any(|p| p == HERE));
    let title_bar = options.iter().find(|o| o["key"] == TITLE_BAR).unwrap_or_else(|| panic!("options.list names {TITLE_BAR}"));
    assert_eq!(on_here(title_bar), HERE != "mac", "{TITLE_BAR} is a Windows and Linux option: {title_bar}");
    let mut shown = 0;
    for o in &options {
        let key = o["key"].as_str().unwrap_or_else(|| panic!("an option without a key: {o}"));
        let id = control_id(o["type"].as_str().unwrap_or_default(), key);
        let is_drawn = drawn(c, &id);
        if on_here(o) {
            assert!(is_drawn, "{key}: its control {id} is drawn on {HERE}");
            shown += 1;
        } else {
            assert!(!is_drawn, "{key}: {id} is not drawn on {HERE}");
        }
    }
    shown
}

/// One tab: its screenshot matched (unless `match_it` is false, then only taken) and its fit.
fn tab(c: &mut Client, home: &Path, tab: &str, name: &str, match_it: bool) {
    if tab != "general" {
        click(c, &format!("tab:options.{tab}"));
    }
    assert_eq!(c.ok("ui.state", json!({}))["options"]["tab"], tab);
    app::park(c, f64::from(LARGE.1));
    let mut masks = app::moving_parts(c);
    masks.extend(path_fields(c));
    let full = format!("m10-{}x{}-{name}", LARGE.0, LARGE.1);
    if match_it {
        app::masked_shot(c, home, &full, LARGE, &masks);
    } else {
        live::screenshot(c, home, None, &full, LARGE.0, LARGE.1);
    }
    let rows = (tab == "general").then(|| general_rows(c));
    fits_small(c, name);
    let rows = rows.map(|n| format!(", {n} options drawn")).unwrap_or_default();
    println!(
        "  Options ▸ {tab}: {}{rows}",
        if match_it { "matches its fingerprint, fits 900 × 600" } else { "taken (not the Windows and Linux layout here), fits 900 × 600" }
    );
}

pub fn looks(exe: &Path, home: &Path) {
    let started = Instant::now();
    let pictures = pictures(home);
    let (a, mut cl) = live::start(exe, home);
    let c = &mut cl;
    for folder in ["Harbour", "Market"] {
        run(c, "folders.add", json!({ "path": pictures.join(folder) }));
    }
    idle(c);
    resize(c, LARGE);
    quiet(c, f64::from(LARGE.1));

    // The Options dialog, Ctrl+, on its General tab, then each other tab.
    key(c, "Ctrl+,");
    assert_eq!(dialog(c).as_deref(), Some("options"), "Ctrl+, opens Options");
    tab(c, home, "general", "options-general", !cfg!(target_os = "macos"));
    tab(c, home, "fileTypes", "options-file-types", true);
    tab(c, home, "export", "options-export", true);
    tab(c, home, "shortcuts", "options-shortcuts", !cfg!(target_os = "macos"));
    click(c, "button:dialog.cancel");
    assert_eq!(dialog(c), None);

    // The window at 150 % with Large text: 1400 × 900 pixels lay out 933 × 600 points.
    run(c, "options.set", json!({ "values": { "interface.uiScale": 150, "interface.fontSize": "large" } }));
    idle(c);
    quiet(c, 600.0);
    let ppp = pixels_per_point(c);
    assert!((ppp - 1.5).abs() < 1e-6, "UI scale 150 % at a 100 % display: 1.5 pixels per point, not {ppp}");
    let masks: Vec<[f64; 4]> = app::moving_parts(c).into_iter().map(|r| in_pixels(r, ppp)).collect();
    let name = format!("m10-{}x{}-window-150-large", LARGE.0, LARGE.1);
    app::masked_shot(c, home, &name, LARGE, &masks);
    println!("  the window at 150 % and Large: matches its fingerprint ({ppp} pixels per point)");
    let reset: Value = json!({ "values": { "interface.uiScale": "auto", "interface.fontSize": "small" } });
    run(c, "options.set", reset);
    live::quit(a, cl);
    println!("  M10 looks in {:.1} s", secs(started));
}
