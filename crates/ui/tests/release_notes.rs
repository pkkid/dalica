//! Help ▸ Release Notes shows the release this build is (M6.35; `docs/releasing.md` → Cutting a
//! release: the version is bumped together with its section of `docs/whats-new.md`).
//!
//! Through M6.20's dialog, with the notes and version compiled into this binary (no planted
//! fixture): the Help menu's row opens `dialog:releaseNotes`, which draws the compiled
//! `whats-new.md`, and that opens on the section of the build's own version, `## <version> — …`,
//! and fits the small window. Renders of the dialog go to
//! `target/test-output/ui/release-notes-0.1-1400x900.png` for looking at; they are not
//! fingerprinted, since the notes change with every release.

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, check_small_and_back, new_driver, shot};
use dalica_engine::Session;
use dalica_ui::App;
use dalica_ui::control::{Config, Connection, Control, Request, serve};
use dalica_ui::credits::{self, Baked};
use dalica_ui::dialogs::release_notes::{self, Block};
use serde_json::json;

/// The build's version without a pre-release part: `0.2.0-rc.1` is described by `## 0.2.0 — …`.
fn release() -> &'static str {
    let v = dalica_core::build::VERSION;
    v.split('-').next().unwrap_or(v)
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
fn help_release_notes_shows_the_new_section() {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
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
    shot(&d, "release-notes-0.1-1400x900");
    // Inside the small window too.
    check_small_and_back(&mut d, "dialog:releaseNotes");

    // Esc closes it.
    d.key("Escape").unwrap();
    d.settle(FRAMES).unwrap();
    assert!(!d.widgets().iter().any(|wi| wi.id == "dialog:releaseNotes"));
}
