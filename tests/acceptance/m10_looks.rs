//! M10's looks in the live app (`m10.rs` part 2): the Options dialog's four tabs opened with
//! Ctrl+, (General, File Types, Export, and Keyboard Shortcuts, the shortcut editor), and the main
//! window at UI scale 150 % with font size Large, each a whole-window screenshot at 1400 × 900
//! pixels matched against `fingerprints/m10-1400x900-*.json`; at 900 × 600 the dialog fits the
//! window (`design-system.md` §11.3; `roadmap.md` M10.10–M10.12).
//!
//! The shortcut editor's keys show as glyphs on macOS (`⌘8`), so there only its fit is checked, as
//! the editor's own test does (`crates/ui/tests/shortcut_editor.rs`).

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::{Value, json};

use crate::app::{self, click, dialog, idle, key, quiet, rect_of, resize, run, secs};
use crate::live::{self, Client};

const LARGE: (u32, u32) = (1400, 900);
const SMALL: (u32, u32) = (900, 600);
const DIALOG: &str = "dialog:options";

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
    fits_small(c, name);
    println!(
        "  Options ▸ {tab}: {}",
        if match_it { "matches its fingerprint, fits 900 × 600" } else { "taken (keys show as glyphs here), fits 900 × 600" }
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
    tab(c, home, "general", "options-general", true);
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
