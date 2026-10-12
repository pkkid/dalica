//! Help ▸ Release Notes shows the release this build is (M6.35; `docs/releasing.md` → Cutting a
//! release: the version is bumped together with its section of `docs/whats-new.md`).
//!
//! Through M6.20's dialog, with the notes and version compiled into this binary (no planted
//! fixture): the Help menu's row opens `dialog:releaseNotes`, which draws the compiled
//! `whats-new.md`, and that opens on the section of the build's own version, `## <version> — …`,
//! with its areas, the data-safety notes and what the release does not do yet. Renders of the
//! dialog go to `target/test-output/ui/release-notes-0.1-<w>x<h>.png` for looking at; they are not
//! fingerprinted, since the notes change with every release.

use std::path::PathBuf;
use std::sync::Arc;

use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::credits::{self, Baked};
use dalica_ui::dialogs::release_notes::{self, Block};
use dalica_ui::{App, Driver};
use serde_json::json;

const FRAMES: usize = 60 * 60;

/// The build's version without a pre-release part: `0.2.0-rc.1` is described by `## 0.2.0 — …`.
fn release() -> &'static str {
    let v = dalica_core::build::VERSION;
    v.split('-').next().unwrap_or(v)
}

/// The blocks of the section of `version`: from its `## <version> — ` heading up to the next `##`.
fn section_of<'a>(blocks: &'a [Block], version: &str) -> &'a [Block] {
    let heading = format!("{version} — ");
    let start = blocks
        .iter()
        .position(|b| matches!(b, Block::Heading { level: 2, text } if text.starts_with(&heading)))
        .unwrap_or_else(|| panic!("a `## {version} — <Month Year>` section"));
    let len = blocks[start + 1..].iter().position(|b| matches!(b, Block::Heading { level: 2, .. })).unwrap_or(blocks.len() - start - 1);
    &blocks[start..=start + len]
}

fn areas(section: &[Block]) -> Vec<&str> {
    section
        .iter()
        .filter_map(|b| match b {
            Block::Heading { level: 3, text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Every bullet under the area `area` of `section`, joined.
fn bullets_of(section: &[Block], area: &str) -> String {
    let mut inside = false;
    let mut out = String::new();
    for b in section {
        match b {
            Block::Heading { text, .. } => inside = text == area,
            Block::Bullet { text, .. } if inside => {
                out.push_str(text);
                out.push('\n');
            }
            _ => {}
        }
    }
    out
}

#[test]
fn the_notes_open_on_the_section_of_this_version() {
    assert_eq!(Baked::compiled().version, dalica_core::build::VERSION);
    let blocks = release_notes::blocks(release_notes::NOTES);
    let Some(Block::Heading { level: 2, text }) = blocks.first() else {
        panic!("the notes start with a `##` section: {:?}", blocks.first());
    };
    assert!(text.starts_with(&format!("{} — ", release())), "the newest section is this build's version: {text}");
}

#[test]
fn the_first_release_says_what_it_writes_and_what_it_lacks() {
    let blocks = release_notes::blocks(release_notes::NOTES);
    let section = section_of(&blocks, "0.1.0");
    assert_eq!(areas(section), ["Installing", "Folders", "Library", "Tags", "Your photos and their files", "Agents", "Not in this release yet"]);
    // The data-safety notes: what is written into photo files, when, and the write options with
    // their defaults; nothing about Save's originals folder before Save exists.
    let files = bullets_of(section, "Your photos and their files");
    for needle in [
        "never changes a photo's pixels",
        ".dalica-originals",
        ".dalica.json",
        "JPEG (XMP, IPTC and EXIF)",
        "about a second after your last change",
        "Write captions, tags and places into photo files: on.",
        "Write date changes into photo files: on.",
        "Write stars as ratings: off.",
        "Update the file's modification date when its metadata changes: off.",
        "Write XMP sidecars for RAW files: off.",
        "settings.json",
    ] {
        assert!(files.contains(needle), "the data-safety notes say `{needle}`:\n{files}");
    }
    // Honest about what this release cannot do.
    let missing = bullets_of(section, "Not in this release yet");
    for needle in ["No editing", "HEIC", "Open With", "dalica.exe --version"] {
        assert!(missing.contains(needle), "the missing list names `{needle}`:\n{missing}");
    }
}

#[test]
fn help_release_notes_shows_the_new_section() {
    for (w, h) in [(1400, 900), (900, 600)] {
        let dir = tempfile::tempdir().unwrap();
        let s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
        let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
        d.settle(FRAMES).unwrap();
        // The dialog draws what this build carries, not a planted fixture.
        assert_eq!(credits::baked(d.ctx()).release_notes, release_notes::NOTES);
        assert_eq!(credits::baked(d.ctx()).version, dalica_core::build::VERSION);

        // As a click on Help ▸ Release Notes does.
        let control = Control::new(Config::default());
        let mut conn = Connection::new();
        let reply = serve(&control, &mut conn, &mut d, &Request::new("ui.menu.invoke", json!({ "path": "Help/Release Notes" })))
            .unwrap_or_else(|e| panic!("Help/Release Notes: {e}"));
        assert_eq!(reply["dialog"], json!("releaseNotes"), "{reply}");
        d.settle(FRAMES).unwrap();
        let dialog = d.widgets().into_iter().find(|wi| wi.id == "dialog:releaseNotes").expect("the dialog is open");
        let r = dialog.rect;
        assert!(r[1] >= 0.0 && r[1] + r[3] <= h as f32, "{w}×{h}: inside the window, {r:?}");

        let shot = d.screenshot().unwrap();
        let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join(format!("release-notes-0.1-{w}x{h}.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();

        // Esc closes it.
        d.key("Escape").unwrap();
        d.settle(FRAMES).unwrap();
        assert!(!d.widgets().iter().any(|wi| wi.id == "dialog:releaseNotes"));
    }
}
