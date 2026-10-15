//! Help ▸ About Dalica and Help ▸ Release Notes (M6.20; `contributors.md`, `menus-and-shortcuts.md`
//! §1.8), driven headlessly by widget id over fixtures planted with `credits::set_baked`, so the
//! pictures do not change with each refresh of the credits, release note or version:
//!
//! - each tab by its id, with the controls of `credits.rs`;
//! - every sort (from the dropdown and from the table's headers, and reversed) and the name toggle
//!   on contributors with missing display and real names: the `@username` fallback, and
//!   alphabetical order that ignores case and the `@`;
//! - the release notes leave HTML comments out;
//! - every widget has a valid, unique id (the driver checks every frame), and each tab and the release
//!   notes match their fingerprints (`tests/fingerprints/dialog-about-*.json`,
//!   `dialog-release-notes-*.json`; `DALICA_BLESS=1` stores new ones, renders go to
//!   `target/test-output/ui/`).

use std::sync::Arc;

use crate::common::{FRAMES, LARGE, SMALL, check_region, check_sizes, new_driver, try_widget};
use dalica_engine::Session;
use dalica_ui::credits::{self, Baked, Contributor, Model};
use dalica_ui::dialogs::release_notes::{self, Block};
use dalica_ui::state::{AboutTab, CreditsList, CreditsSort, NameMode};
use dalica_ui::{App, Driver};
use serde_json::json;

const fn person(
    login: &'static str,
    display_name: Option<&'static str>,
    real_name: Option<&'static str>,
    counts: [u64; 6],
    first_commit: &'static str,
    last_commit: &'static str,
) -> Contributor {
    let [prs, commits, lines_added, lines_deleted, binary_added, binary_deleted] = counts;
    Contributor { login, display_name, real_name, prs, commits, lines_added, lines_deleted, binary_added, binary_deleted, first_commit, last_commit }
}

/// Four contributors with a different value in every column (so each sort has one order), some
/// without a display or real name, and `al-x` credited through a pull request only.
static PEOPLE: [Contributor; 4] = [
    person("zoe", None, Some("Adam Zed"), [1, 10, 100, 10, 3, 2], "2026-03-01T10:00:00Z", "2026-09-01T10:00:00Z"),
    person("Bob", Some("bobby"), None, [5, 3, 50, 80, 4, 5], "2026-01-15T10:00:00Z", "2026-05-01T10:00:00Z"),
    person("carol", Some("Charlie"), Some("Carol Day"), [2, 30, 900, 100, 1, 1], "2026-02-01T10:00:00Z", "2026-10-01T10:00:00Z"),
    person("al-x", None, None, [3, 0, 0, 0, 0, 0], "", ""),
];

static MODELS: [Model; 2] = [
    Model { company: "Anthropic", model: "Claude Opus", version: "5.5", commits: 40, lines_added: 52_000, lines_deleted: 3_100 },
    Model { company: "OpenAI", model: "Codex", version: "1", commits: 2, lines_added: 120, lines_deleted: 8 },
];

const NOTES: &str = "# What's new in Dalica

<!--
Conventions for writers: never shown.
-->

## 0.1.0 — October 2026

### Library

- Browse the photos of your folders, newest first. <!-- a note for writers -->
- Star photos with **Picture ▸ Star** (`photo.star.toggle`).
  Stars are written to the photo's sidecar, never into the file.

### Reliability

- Dalica never changes a photo's pixels unless you save an edit.
  - Originals are kept in `.dalica-originals/`.

<!-- an unclosed comment
- never shown
";

fn fixture() -> Baked {
    Baked {
        version: "0.1.0",
        commit: Some("0123456789abcdef0123456789abcdef01234567"),
        build_date: Some("2026-10-01"),
        total_commits: 45,
        contributors: &PEOPLE,
        models: &MODELS,
        release_notes: NOTES,
    }
}

/// An empty library's window at `w × h` showing the fixture (the directory lives as long as it).
fn driver(w: u32, h: u32) -> (tempfile::TempDir, Driver) {
    let dir = tempfile::tempdir().unwrap();
    let s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    credits::set_baked(d.ctx(), Some(fixture()));
    d.settle(FRAMES).unwrap();
    (dir, d)
}

fn has(d: &Driver, id: &str) -> bool {
    try_widget(d, id).is_some()
}

fn click(d: &mut Driver, id: &str) {
    d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    d.settle(FRAMES).unwrap();
}

fn open_about(d: &mut Driver) {
    assert_eq!(d.run("app.about", &json!({})).unwrap(), json!({ "dialog": "about" }));
    d.settle(FRAMES).unwrap();
    assert!(has(d, "dialog:about"));
}

/// The names About ▸ Contributors lists now, in its order.
fn listed(d: &Driver) -> Vec<String> {
    credits::listed(&PEOPLE, &d.state().about)
}

fn at(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| format!("@{n}")).collect()
}

#[test]
fn each_tab_by_its_id_with_its_controls() {
    let (_dir, mut d) = driver(1400, 900);
    open_about(&mut d);
    for (tab, id, _) in dalica_ui::dialogs::about::TABS {
        click(&mut d, id);
        assert_eq!(d.state().about.tab, tab);
        let w = try_widget(&d, id).unwrap();
        assert!(w.active, "{id} is the active tab");
        for (_, other, _) in dalica_ui::dialogs::about::TABS.iter().filter(|t| t.1 != id) {
            assert!(!try_widget(&d, other).unwrap().active, "{other}");
        }
        let controls = has(&d, "choice:dialog.sort") && has(&d, "button:dialog.reverse") && has(&d, "choice:dialog.names/realName");
        assert_eq!(controls, tab == AboutTab::Contributors, "{id}: the credit controls are the Contributors tab's");
    }
    // The About tab names the build.
    let facts = dalica_ui::dialogs::about::facts(&fixture());
    assert_eq!(facts.map(|f| f.1), ["0.1.0", "012345678", "2026-10-01", "MIT OR Apache-2.0"].map(String::from));
    // The table's headers sort; the grab bag has none.
    click(&mut d, "tab:about.contributors");
    assert!(!has(&d, "button:dialog.sort/commits"));
    click(&mut d, "choice:dialog.list/table");
    assert_eq!(d.state().about.list, CreditsList::Table);
    for (_, part, _, _) in credits::SORTS {
        assert!(has(&d, &format!("button:dialog.sort/{part}")), "{part}");
    }
    assert!(try_widget(&d, "button:dialog.sort/firstCommit").unwrap().active, "the default sort");
    // Close keeps the view for the next opening.
    click(&mut d, "button:dialog.cancel");
    assert!(d.state().dialogs.is_empty());
    open_about(&mut d);
    assert_eq!((d.state().about.tab, d.state().about.list), (AboutTab::Contributors, CreditsList::Table));
}

#[test]
fn every_sort_and_the_name_toggle_on_missing_names() {
    let (_dir, mut d) = driver(1400, 900);
    open_about(&mut d);
    click(&mut d, "tab:about.contributors");
    // The default: first commit, oldest first, someone without a commit last.
    assert_eq!(listed(&d), at(&["Bob", "carol", "zoe", "al-x"]));
    let orders: [(CreditsSort, &str, [&str; 4]); 10] = [
        (CreditsSort::Name, "name", ["al-x", "Bob", "carol", "zoe"]),
        (CreditsSort::Prs, "prs", ["Bob", "al-x", "carol", "zoe"]),
        (CreditsSort::Commits, "commits", ["carol", "zoe", "Bob", "al-x"]),
        (CreditsSort::LinesAdded, "linesAdded", ["carol", "zoe", "Bob", "al-x"]),
        (CreditsSort::LinesDeleted, "linesDeleted", ["carol", "Bob", "zoe", "al-x"]),
        (CreditsSort::LinesDelta, "linesDelta", ["carol", "zoe", "al-x", "Bob"]),
        (CreditsSort::BinaryAdded, "binaryAdded", ["Bob", "zoe", "carol", "al-x"]),
        (CreditsSort::BinaryDeleted, "binaryDeleted", ["Bob", "zoe", "carol", "al-x"]),
        (CreditsSort::FirstCommit, "firstCommit", ["Bob", "carol", "zoe", "al-x"]),
        (CreditsSort::LastCommit, "lastCommit", ["Bob", "zoe", "carol", "al-x"]),
    ];
    // From the dropdown in the grab bag: each starts in its natural order; the arrow reverses it.
    for (sort, part, order) in orders {
        click(&mut d, "choice:dialog.sort");
        click(&mut d, &format!("choice:dialog.sort/{part}"));
        let about = d.state().about;
        assert_eq!((about.sort, about.ascending), (sort, credits::default_ascending(sort)), "{part}");
        assert_eq!(listed(&d), at(&order), "{part}");
        click(&mut d, "button:dialog.reverse");
        let mut reversed = at(&order);
        reversed.reverse();
        assert_eq!(listed(&d), reversed, "{part} reversed");
    }
    // From the table's headers: a click sorts by the column, a second one reverses it.
    click(&mut d, "choice:dialog.list/table");
    for (sort, part, order) in orders {
        let header = format!("button:dialog.sort/{part}");
        click(&mut d, &header);
        assert_eq!((d.state().about.sort, d.state().about.ascending), (sort, credits::default_ascending(sort)), "{part}");
        assert_eq!(listed(&d), at(&order), "{part} by its header");
        assert!(try_widget(&d, &header).unwrap().active);
        click(&mut d, &header);
        let mut reversed = at(&order);
        reversed.reverse();
        assert_eq!(listed(&d), reversed, "{part} reversed by its header");
    }
    // The name toggle: a missing name falls back to @username, and A–Z ignores case and the @.
    click(&mut d, "choice:dialog.sort");
    click(&mut d, "choice:dialog.sort/name");
    assert_eq!(listed(&d), at(&["al-x", "Bob", "carol", "zoe"]));
    click(&mut d, "choice:dialog.names/displayName");
    assert_eq!(d.state().about.names, NameMode::DisplayName);
    assert!(try_widget(&d, "choice:dialog.names/displayName").unwrap().active);
    assert_eq!(listed(&d), ["@al-x", "bobby", "Charlie", "@zoe"]);
    click(&mut d, "choice:dialog.names/realName");
    assert_eq!(listed(&d), ["Adam Zed", "@al-x", "@Bob", "Carol Day"]);
    click(&mut d, "button:dialog.reverse");
    assert_eq!(listed(&d), ["Carol Day", "@Bob", "@al-x", "Adam Zed"]);
    click(&mut d, "choice:dialog.names/username");
    click(&mut d, "choice:dialog.list/grabBag");
    assert_eq!(listed(&d), at(&["zoe", "carol", "Bob", "al-x"]));
    let a = d.state().about;
    assert_eq!((a.names, a.list, a.sort, a.ascending), (NameMode::Username, CreditsList::GrabBag, CreditsSort::Name, false));
}

#[test]
fn the_release_notes_leave_comments_out() {
    let shown = release_notes::blocks(NOTES);
    assert_eq!(
        shown,
        [
            Block::Heading { level: 2, text: "0.1.0 — October 2026".into() },
            Block::Heading { level: 3, text: "Library".into() },
            Block::Bullet { depth: 0, text: "Browse the photos of your folders, newest first.".into() },
            Block::Bullet {
                depth: 0,
                text: "Star photos with Picture ▸ Star (photo.star.toggle). Stars are written to the photo's sidecar, never into the file.".into()
            },
            Block::Heading { level: 3, text: "Reliability".into() },
            Block::Bullet { depth: 0, text: "Dalica never changes a photo's pixels unless you save an edit.".into() },
            Block::Bullet { depth: 1, text: "Originals are kept in .dalica-originals/.".into() },
        ]
    );
    // The dialog opens on them; the compiled notes show no comment either.
    let (_dir, mut d) = driver(1400, 900);
    assert_eq!(d.run("help.releaseNotes", &json!({})).unwrap(), json!({ "dialog": "releaseNotes" }));
    d.settle(FRAMES).unwrap();
    assert!(has(&d, "dialog:releaseNotes") && has(&d, "button:dialog.cancel"));
    let compiled = format!("{:?}", release_notes::blocks(release_notes::NOTES));
    assert!(!compiled.contains("<!--") && !compiled.contains("Conventions:"), "{compiled}");
    assert_eq!(Baked::compiled().release_notes, release_notes::NOTES);
}

/// Every widget has a valid, unique id and none is left unregistered; the dialog matches the
/// fingerprint `name`.
/// Each tab and the release notes match their fingerprints in the large window; in both windows
/// every tab is as wide as specified, as tall as the others and inside the window.
#[test]
fn the_dialogs_match_their_fingerprints_and_fit_the_small_window() {
    let (_dir, mut d) = driver(LARGE.0, LARGE.1);
    open_about(&mut d);
    for (w, h) in [LARGE, SMALL] {
        d.resize(w, h);
        d.settle(FRAMES).unwrap();
        let mut height = None;
        for (shot, clicks) in [
            ("about", &["tab:about.about"][..]),
            ("contributors", &["tab:about.contributors", "choice:dialog.list/grabBag"][..]),
            ("contributors-table", &["choice:dialog.list/table"][..]),
            ("models", &["tab:about.models"][..]),
        ] {
            for id in clicks {
                click(&mut d, id);
            }
            let r = try_widget(&d, "dialog:about").unwrap().rect;
            assert!((r[2] - dalica_ui::dialogs::about::WIDTH).abs() <= 2.0, "{w}×{h} {shot}: {r:?}");
            assert!(r[1] >= 0.0 && r[1] + r[3] <= h as f32, "{w}×{h} {shot}: inside the window, {r:?}");
            assert_eq!(*height.get_or_insert(r[3]), r[3], "{w}×{h} {shot}: every tab is as tall");
            if (w, h) == LARGE {
                check_region(&d, "dialog:about", &format!("dialog-about-{shot}-{w}x{h}"));
            }
        }
    }
    click(&mut d, "button:dialog.cancel");
    d.resize(LARGE.0, LARGE.1);
    assert_eq!(d.run("help.releaseNotes", &json!({})).unwrap(), json!({ "dialog": "releaseNotes" }));
    d.settle(FRAMES).unwrap();
    check_sizes(&mut d, "dialog:releaseNotes", "dialog-release-notes");
}

/// The pixels of `d`'s window inside the widget `id` that are the app icon's orange (`#d65d0e`).
fn orange_in(d: &Driver, id: &str) -> usize {
    let r = try_widget(d, id).unwrap().rect;
    let shot = d.screenshot().unwrap();
    let (x0, y0, x1, y1) = (r[0].round() as u32, r[1].round() as u32, (r[0] + r[2]).round() as u32, (r[1] + r[3]).round() as u32);
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|&(x, y)| shot.pixel(x, y).is_some_and(|p| p[..3].iter().zip([0xd6u8, 0x5d, 0x0e]).all(|(&g, o)| g.abs_diff(o) <= 8)))
        .count()
}

/// The About tab shows the app icon (`assets/app-icon/`): its orange tile fills much of the 96 pt
/// square; the Contributors tab, which has no icon, has none of it.
#[test]
fn the_about_tab_shows_the_app_icon() {
    let (_dir, mut d) = driver(1400, 900);
    open_about(&mut d);
    let about = orange_in(&d, "dialog:about");
    assert!(about >= 96 * 96 / 4, "the About tab shows the orange app icon ({about} orange pixels)");
    click(&mut d, "tab:about.contributors");
    assert_eq!(orange_in(&d, "dialog:about"), 0, "no icon on the Contributors tab");
}
