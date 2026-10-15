//! Help ▸ About Dalica and Help ▸ Release Notes (M6.20; `contributors.md`, `menus-and-shortcuts.md`
//! §1.8), driven headlessly by widget id over fixtures planted with `credits::set_baked`, so the
//! pictures do not change with each refresh of the credits, release note or version:
//!
//! - each tab by its id, with the controls of `credits.rs`;
//! - every sort (from the dropdown and from the table's headers, and reversed) and the name toggle
//!   on contributors with missing display and real names: the `@username` fallback, and
//!   alphabetical order that ignores case and the `@`;
//! - `build.rs` (included here) takes names only from verified `people.toml` entries and drops
//!   anything shaped like an email address; nothing in the compiled tables looks like one;
//! - the release notes leave HTML comments out;
//! - every widget has a valid, unique id at 1400 × 900 and 900 × 600, and each tab and the release
//!   notes match their fingerprints (`tests/fingerprints/dialog-about-*.json`,
//!   `dialog-release-notes-*.json`; `DALICA_BLESS=1` stores new ones, renders go to
//!   `target/test-output/ui/`).

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::common::{FRAMES, new_driver, out_dir, try_widget};
use dalica_engine::Session;
use dalica_scenes::fingerprint;
use dalica_ui::credits::{self, Baked, Contributor, Model};
use dalica_ui::dialogs::release_notes::{self, Block};
use dalica_ui::state::{AboutTab, CreditsList, CreditsSort, NameMode};
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry;
use serde_json::json;

#[path = "../build.rs"]
#[allow(dead_code)]
mod build_script;

const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

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
fn build_rs_takes_names_only_from_verified_entries_and_never_an_email() {
    let toml = r#"
# A comment, and the template's own lines commented out:
#   [people.your-github-username]
[people.alice]
real_name = "Alice Liddell"   # her own entry
display_name = 'Al'
consent_source = "PR #1"

[people.bob]
display_name = "bob@example.org"
real_name = "Robert \"Bob\" Tables"

[people.carol]
sync_github_name = true

[people.dave]
real_name = "Dave"

[people.erin]
real_name = "Erin <erin@mail.example.com>"

[other]
what = "x"
"#;
    let people = build_script::parse_people(toml);
    assert_eq!(people.entries.keys().map(String::as_str).collect::<Vec<_>>(), ["alice", "bob", "carol", "dave", "erin"]);
    assert_eq!(people.entries["alice"].display_name.as_deref(), Some("Al"));
    assert_eq!(people.entries["bob"].real_name.as_deref(), Some("Robert \"Bob\" Tables"));
    assert!(people.entries["carol"].sync_github_name);
    assert_eq!(people.problems.iter().map(|p| p.0).collect::<Vec<_>>(), [22, 23], "the unknown table and its key are reported");
    let json = json!({
        "schema": 1,
        "commits": 9,
        "contributors": [
            { "login": "alice", "people_entry": true, "github_name": null, "prs": 2, "commits": 4, "lines_added": 10, "lines_deleted": 1, "binary_added": 0, "binary_deleted": 0, "first_commit": "2026-10-01T00:00:00Z", "last_commit": "2026-10-02T00:00:00Z" },
            { "login": "bob", "people_entry": true, "prs": 1, "commits": 1 },
            { "login": "carol", "people_entry": true, "github_name": "Carol from GitHub", "commits": 1 },
            { "login": "dave", "people_entry": false, "commits": 1 },
            { "login": "erin", "people_entry": true, "commits": 1, "first_commit": "erin@mail.example.com" },
            { "login": "not a@login.example.com", "commits": 1 },
            { "login": "frank", "commits": "lots", "prs": -1 },
        ],
        "models": [
            { "company": "Anthropic", "model": "Claude Opus", "version": "5.5", "commits": 8, "lines_added": 9, "lines_deleted": 1 },
            { "company": "noreply@anthropic.com", "model": "X", "version": "1", "commits": 1 },
        ],
    });
    let (code, dropped) = build_script::generate(&json, &people.entries);
    let line = |login: &str| code.lines().find(|l| l.contains(&format!("login: {login:?}"))).unwrap_or_else(|| panic!("{login}: {code}")).to_string();
    assert!(line("alice").contains(r#"display_name: Some("Al"), real_name: Some("Alice Liddell"), prs: 2, commits: 4"#), "{code}");
    assert!(line("bob").contains(r#"display_name: None, real_name: Some("Robert \"Bob\" Tables")"#), "the email display name is dropped: {code}");
    assert!(line("carol").contains(r#"display_name: Some("Carol from GitHub")"#), "synced from GitHub: {code}");
    assert!(line("dave").contains("display_name: None, real_name: None"), "an unverified entry shows no name: {code}");
    assert!(line("erin").contains("real_name: None") && line("erin").contains(r#"first_commit: """#), "{code}");
    assert!(line("frank").contains("prs: 0, commits: 0"), "bad numbers count as 0: {code}");
    assert!(!code.contains("not a@login"), "an invalid username is skipped: {code}");
    assert!(code.contains("pub static TOTAL_COMMITS: u64 = 9;") && code.contains(r#"company: "", model: "X""#), "{code}");
    assert_eq!(dropped.len(), 4, "{dropped:?}");
    assert!(!code.lines().any(build_script::looks_like_email), "{code}");
    // Nothing breaks the build: missing files and garbage give empty tables.
    let (empty, _) = build_script::generate(&serde_json::Value::Null, &BTreeMap::new());
    assert!(empty.contains("TOTAL_COMMITS: u64 = 0;") && empty.contains("CONTRIBUTORS: &[Contributor] = &[\n];"));
    for odd in ["[", "[people.]", "x", "real_name = \"unclosed", "[people.a]\nreal_name = \"\\u12\"", "= =", "[people.a]\nsync_github_name = maybe"] {
        let _ = build_script::parse_people(odd);
    }
    for (s, email) in [("a@b.co", true), ("<x@y.org>", true), ("@pkkid", false), ("Bob @ home", false), ("x@localhost", false), ("Ann", false)] {
        assert_eq!(build_script::looks_like_email(s), email, "{s}");
    }
}

#[test]
fn no_email_reaches_the_binary() {
    let b = Baked::compiled();
    let mut strings: Vec<&str> = vec![credits::compiled::SOURCE];
    for c in b.contributors {
        strings.extend([c.login, c.first_commit, c.last_commit]);
        strings.extend(c.display_name);
        strings.extend(c.real_name);
        assert!(build_script::valid_login(c.login), "{}", c.login);
    }
    for m in b.models {
        strings.extend([m.company, m.model, m.version]);
    }
    for s in strings {
        assert!(!build_script::looks_like_email(s) && !s.contains("noreply"), "{s}");
    }
    // The compiled tables are the committed file's, at least its contributors and models.
    let json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contributors/contributors.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(b.contributors.len(), json["contributors"].as_array().unwrap().len());
    assert_eq!(b.models.len(), json["models"].as_array().unwrap().len());
    assert_eq!(b.total_commits, json["commits"].as_u64().unwrap());
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
fn check_shot(d: &Driver, dialog: &str, name: &str) {
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{name}");
    let mut seen = HashSet::new();
    for wi in d.widgets() {
        assert!(registry::is_valid_id(&wi.id) && seen.insert(wi.id.clone()), "{name}: `{}`", wi.id);
    }
    let r = try_widget(d, dialog).unwrap().rect;
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let (x0, y0, cw, ch) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y0..y0 + ch {
        for x in x0..x0 + cw {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(cw, ch, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(cw, ch, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap_or_else(|e| panic!("{}: {e}", stored.display()))).unwrap();
    if let Err(m) = fingerprint::check(cw, ch, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

#[test]
fn every_widget_has_an_id_and_the_dialogs_match_their_fingerprints_at_both_sizes() {
    for (w, h) in SIZES {
        let (_dir, mut d) = driver(w, h);
        open_about(&mut d);
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
            check_shot(&d, "dialog:about", &format!("dialog-about-{shot}-{w}x{h}"));
        }
        click(&mut d, "button:dialog.cancel");
        assert_eq!(d.run("help.releaseNotes", &json!({})).unwrap(), json!({ "dialog": "releaseNotes" }));
        d.settle(FRAMES).unwrap();
        let r = try_widget(&d, "dialog:releaseNotes").unwrap().rect;
        assert!(r[1] >= 0.0 && r[1] + r[3] <= h as f32, "{w}×{h}: inside the window, {r:?}");
        check_shot(&d, "dialog:releaseNotes", &format!("dialog-release-notes-{w}x{h}"));
    }
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
