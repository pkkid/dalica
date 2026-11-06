//! Help ▸ Stats for Nerds… (`roadmap.md` M16.36, issue #756): the item opens the window, its
//! groups fill from `app.diagnostics` without the UI thread touching the disk, a group header
//! opens and closes its group, Copy as Text copies every group, and Clear Thumbnail Cache… asks inside the
//! window before it starts its job. Screenshots at 1400 × 900 and 900 × 600 go to the test output.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{LARGE, SMALL, new_driver, put_jpeg, shot, try_widget};
use dalica_engine::Session;
use dalica_engine::cmd::diagnostics::CLEAR_LABEL;
use dalica_ui::dialogs::stats_for_nerds::{self, CLEAR_CONFIRM_ID, CLEAR_ID, COPY_ID};
use dalica_ui::menu_catalog::{self, Entry};
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use serde_json::{Value, json};

fn copied(d: &Driver) -> String {
    let dialog = d.state().dialogs.iter().find(|x| x.kind == DialogKind::StatsForNerds).expect("the window is open");
    dialog.fields.get("copied").and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Copies the page until the disk's facts are in it.
fn copy_measured(d: &mut Driver) -> String {
    for _ in 0..200 {
        d.wait(1100).unwrap();
        d.click(COPY_ID).unwrap();
        let text = copied(d);
        if !text.is_empty() && !text.contains("measuring…") {
            return text;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("never measured: {}", copied(d));
}

#[test]
fn the_window_opens_from_help_fills_and_copies_as_text() {
    let in_help = menu_catalog::HELP.iter().any(|e| matches!(e, Entry::Item(i) if i.id == "help.statsForNerds" && i.label == "Stats for Nerds…"));
    assert!(in_help, "Help ▸ Stats for Nerds…");
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("photos").join("Lisbon");
    put_jpeg(&folder.join("IMG_0001.jpg"), 1);
    put_jpeg(&folder.join("IMG_0002.jpg"), 2);
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut d = new_driver(App::new(Some(s)), LARGE.0, LARGE.1, 1.0);
    d.settle(600).unwrap();
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    assert_eq!(d.run("help.statsForNerds", &json!({})).unwrap(), json!({ "dialog": "statsForNerds" }));
    let text = copy_measured(&mut d);
    for g in stats_for_nerds::GROUPS {
        let title = match g {
            "memory" => "Memory",
            "thumbnailCache" => "Disk: the thumbnail cache",
            "libraryFolder" => "Disk: the library folder",
            "photoFolders" => "Disk: in your photo folders",
            "library" => "Library",
            "work" => "Work in progress",
            "rendering" => "Rendering",
            _ => "Build",
        };
        assert!(text.lines().any(|l| l == title), "{title} in\n{text}");
    }
    let library = dir.path().join("library");
    assert!(text.contains(&format!("Location: {}", library.display())), "{text}");
    assert!(text.contains("Photos and videos: 2"), "{text}");
    // Only Memory is open at first; a header opens its group, and the copy holds every group.
    assert!(try_widget(&d, CLEAR_ID).is_none(), "the cache's group starts closed");
    d.click("header:statsForNerds.memory").unwrap();
    d.click("header:statsForNerds.thumbnailCache").unwrap();
    d.click(COPY_ID).unwrap();
    assert!(copied(&d).lines().any(|l| l == "Memory"));
    let open = d.state().dialogs.iter().find(|x| x.kind == DialogKind::StatsForNerds).unwrap().fields.get("open").cloned();
    assert_eq!(open, Some(json!(["thumbnailCache"])));
    // Clear Thumbnail Cache… asks first, inside the window.
    d.click(CLEAR_ID).unwrap();
    assert!(try_widget(&d, CLEAR_CONFIRM_ID).is_some(), "the question");
    shot(&d, "stats_for_nerds_1400x900");
    d.click(CLEAR_CONFIRM_ID).unwrap();
    let jobs = d.app().session().unwrap().jobs();
    assert!(jobs.iter().any(|j| j.label == CLEAR_LABEL || j.label.contains("thumbnail cache")), "{jobs:?}");
    assert!(try_widget(&d, CLEAR_CONFIRM_ID).is_none(), "asked once");
    d.resize(SMALL.0, SMALL.1);
    d.wait(1100).unwrap();
    shot(&d, "stats_for_nerds_900x600");
    assert_eq!(dalica_fs::guard::refused(), refused, "the UI thread touched the disk");
    drop(ui);
}
