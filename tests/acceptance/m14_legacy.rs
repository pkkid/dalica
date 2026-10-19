//! The previous organizer's data in the live app (`m14.rs` part 2; `data-model.md` §9 and §10.11):
//! `dalica --headless` on a library of folders that organizer managed, every legacy file written
//! here from the documented format (never a real one; the names from `dalica-sidecar`'s masked
//! constant), driven over its control channel:
//!
//! - `Lisbon` holds only the legacy sidecar: after the first scan its photos show their stars,
//!   captions and keywords, a recipe with an operation Dalica has no equivalent for kept as a
//!   `legacy:` entry, an album defined there, a person named there and a geotag; the first-run
//!   notice gives its folder count once (the whole window matches its fingerprint), and nothing is
//!   written. The first change there (a star) writes `.dalica.json` holding every photo's data
//!   while the legacy sidecar stays byte-identical, and the folder's source turns `both`;
//! - `Both` holds both files: only `.dalica.json` is read;
//! - `Lake` stays legacy-only: Options' "Read photo data left by the previous organizer" turned
//!   off takes its star away (the converted `Lisbon` keeps everything) and on brings it back,
//!   nothing written either way;
//! - `Saved` holds two photos the old program saved, their originals in its hidden originals
//!   folder: Undo Save brings one original back with its legacy recipe as unsaved edits, Revert
//!   the other with no edits, the saved files going to the trash and the originals folder never
//!   written;
//! - `Hostile` holds a legacy sidecar over the reader's line limit (refused whole) and `Odd` one
//!   of malformed values (each skipped, the good ones kept): no panic, their photos listed;
//! - after a restart the notice is not given again and the data is all there.

use std::path::{Path, PathBuf};
use std::time::Instant;

use dalica_sidecar::legacy::{folder_section, originals_dir_name, sidecar_names};
use dalica_ui::notices::{OK_ID, legacy_text};
use serde_json::{Value, json};

use crate::app::{self, click, flush_until, run, secs};
use crate::common::{self, LARGE, until};
use crate::live::{self, Client, Launch};

/// The album's token and the contact's id, as the legacy format writes them.
const ALBUM: &str = "5e1f0c2a9b3d4e6f708192a3b4c5d6e7";
const CONTACT: &str = "1a2b3c4d5e6f7081";
const PERSON: &str = "Marta Vieira";
const ALBUM_NAME: &str = "Summer House";
/// Praça do Comércio.
const LISBON: [f64; 2] = [38.7075, -9.1366];
const BREAKFAST: &str = "Breakfast on the terrace";
const GATE: &str = "The garden gate";
/// The recipe the first photo's legacy edits map to: black and white has an equivalent, the
/// red-eye fix does not.
const RECIPE: [&str; 2] = ["bw()", "legacy:redeye(1)"];
/// The option's key in the settings.
const OPTION: &str = "files.readLegacyData";

/// People's names reach automation only with the permission (`faces.md` §7).
fn launch() -> Launch {
    Launch::default().arg("--automation-allow-people")
}

fn jpeg(seed: u64) -> Vec<u8> {
    dalica_scenes::jpeg(&dalica_scenes::Scene::new(seed, 320, 240), 85, None).unwrap()
}

/// A legacy sidecar of `lines` (CRLF, as the old program writes it).
fn ini(lines: &[String]) -> Vec<u8> {
    (lines.join("\r\n") + "\r\n").into_bytes()
}

fn s(v: &str) -> String {
    v.to_string()
}

/// The library's folders, written: (`Saved`'s originals, its saved files, their legacy recipe).
struct Fixture {
    pictures: PathBuf,
    originals: Vec<Vec<u8>>,
}

impl Fixture {
    fn write(home: &Path) -> Fixture {
        let pictures = home.join("Pictures");
        let legacy_name = &sidecar_names()[0];
        let put = |dir: &str, name: &str, bytes: &[u8], i: u64| app::put(&pictures.join(dir).join(name), bytes, i);

        // Lisbon: only the legacy sidecar.
        for n in 1..=4 {
            put("Lisbon", &format!("IMG_{n:04}.jpg"), &jpeg(1400 + n), n);
        }
        let lisbon = ini(&[
            s("[encoding]"),
            s("utf8=1"),
            format!("[{}]", folder_section()),
            s("description=A weekend by the river"),
            s("location=Lisbon"),
            s("[IMG_0001.jpg]"),
            s("star=yes"),
            format!("caption={BREAKFAST}"),
            s("keywords=tram,river"),
            s("filters=bw=1;redeye=1;"),
            format!("faces=rect64(4000400080008000),{CONTACT};"),
            format!("albums={ALBUM}"),
            format!("geotag={},{}", LISBON[0], LISBON[1]),
            s("[IMG_0002.jpg]"),
            format!("caption={GATE}"),
            format!("albums={ALBUM}"),
            s("[IMG_0003.jpg]"),
            s("star=yes"),
            s("[Contacts2]"),
            format!("{CONTACT}={PERSON};;"),
            format!("[.album:{ALBUM}]"),
            format!("name={ALBUM_NAME}"),
            format!("token={ALBUM}"),
        ]);
        put("Lisbon", legacy_name, &lisbon, 10);

        // Lake: only the legacy sidecar, left unconverted.
        put("Lake", "a.jpg", &jpeg(1450), 11);
        put("Lake", legacy_name, &ini(&[s("[a.jpg]"), s("star=yes")]), 12);

        // Both: the legacy sidecar and Dalica's; only Dalica's is read.
        put("Both", "a.jpg", &jpeg(1460), 13);
        put("Both", legacy_name, &ini(&[s("[a.jpg]"), s("star=yes"), s("caption=From the old program")]), 14);
        let own = json!({ "dalica": { "version": 1 }, "photos": { "a.jpg": { "caption": "Dalica's own" } } });
        put("Both", ".dalica.json", (serde_json::to_string_pretty(&own).unwrap() + "\n").as_bytes(), 15);

        // Saved: two photos the old program saved, their originals and recipes kept.
        let originals: Vec<Vec<u8>> = (0..2).map(|i| jpeg(1470 + i)).collect();
        let kept = format!("{}/{}", "Saved", originals_dir_name());
        for (i, original) in originals.iter().enumerate() {
            let name = format!("IMG_{:04}.jpg", i + 1);
            put("Saved", &name, &jpeg(1480 + i as u64), 20 + i as u64);
            put(&kept, &name, original, 22 + i as u64);
        }
        put(&kept, legacy_name, &ini(&[s("[IMG_0001.jpg]"), s("filters=sepia=1;redeye=1;"), s("[IMG_0002.jpg]"), s("filters=bw=1;")]), 24);

        // Hostile: a line over the reader's limit (1 MiB), so the file is refused whole.
        put("Hostile", "a.jpg", &jpeg(1490), 25);
        let mut hostile = b"[a.jpg]\r\nstar=yes\r\ncaption=".to_vec();
        hostile.extend(std::iter::repeat_n(b'x', (1 << 20) + 16));
        hostile.extend_from_slice(b"\r\n");
        put("Hostile", legacy_name, &hostile, 26);

        // Odd: malformed values (each skipped) beside good ones (kept).
        put("Odd", "a.jpg", &jpeg(1491), 27);
        put("Odd", "b.jpg", &jpeg(1492), 28);
        let mut odd = ini(&[
            s("[(null)]"),
            s("star=yes"),
            s("[]"),
            s("[a.jpg]"),
            s("star=maybe"),
            s("rotate=rotate(99)"),
            s("crop=rect64(zzzz)"),
            s("faces=rect64(nothex),xyz;rect64(1),;"),
            s("geotag=nan,inf"),
            s("albums=!!!,"),
            s("filters=;;=;bw=;crop64=1,;"),
            s("[b.jpg]"),
            s("star=yes"),
        ]);
        odd.extend_from_slice(b"[a.jpg]\r\ncaption=\xff\xfe\x00bad\r\n");
        put("Odd", legacy_name, &odd, 29);
        Fixture { pictures, originals }
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.pictures.join(name)
    }

    fn legacy(&self, folder: &str) -> PathBuf {
        self.dir(folder).join(&sidecar_names()[0])
    }
}

/// `legacy.inspect`'s entry for the folder at `path`.
fn source(c: &mut Client, path: &Path) -> String {
    let r = c.exec("legacy.inspect", json!({}));
    r["folders"]
        .as_array()
        .and_then(|f| f.iter().find(|f| f["path"].as_str().map(Path::new) == Some(path)))
        .and_then(|f| f["source"].as_str())
        .map_or_else(|| format!("(not listed: {r})"), str::to_string)
}

/// The photo at `rel` (`"Lisbon/IMG_0001.jpg"`) as `photo.inspect` gives it.
fn photo(c: &mut Client, pictures: &Path, rel: &str) -> Value {
    let all = app::photos(c, pictures);
    all.get(rel).unwrap_or_else(|| panic!("{rel} is in the library: {:?}", all.keys().collect::<Vec<_>>())).inspected.clone()
}

fn star(c: &mut Client, pictures: &Path, rel: &str) -> bool {
    photo(c, pictures, rel)["star"] == true
}

/// A recipe's entries as `photo.inspect` gives them.
fn edits(p: &Value) -> Vec<String> {
    let r = &p["recipe"];
    let list = r["edits"].as_array().or_else(|| r.as_array()).or_else(|| r["list"].as_array());
    list.map(|l| l.iter().map(|e| e.as_str().map_or_else(|| e.to_string(), str::to_string)).collect()).unwrap_or_default()
}

/// The notice's toast id while it shows the count `n`.
fn notice(c: &mut Client, n: u64) -> Option<String> {
    let text = legacy_text(n as usize);
    c.widgets("toast:").into_iter().find(|w| w["label"] == text.as_str()).and_then(|w| w["id"].as_str().map(str::to_string))
}

fn any_notice(c: &mut Client) -> bool {
    c.widgets("toast:").iter().any(|w| w["label"].as_str().is_some_and(|l| l.starts_with("Found photo data from your previous photo program")))
}

pub fn legacy(exe: &Path, home: &Path) {
    let started = Instant::now();
    let fx = Fixture::write(home);
    let all_before = common::tree(&fx.pictures);
    let legacy_files: Vec<(PathBuf, Vec<u8>)> =
        ["Lisbon", "Lake", "Both", "Hostile", "Odd"].iter().map(|f| (fx.legacy(f), std::fs::read(fx.legacy(f)).unwrap())).collect();
    let kept_dir = fx.dir("Saved").join(originals_dir_name());
    let kept_before = common::tree(&kept_dir);

    let (a, mut cl) = live::start_with(exe, home, launch());
    let c = &mut cl;
    app::resize(c, LARGE);
    // One folder added, so one scan finds every legacy folder (the notice counts them all).
    run(c, "folders.add", json!({ "path": fx.pictures }));
    let pictures = fx.pictures.clone();

    // The first scan shows the legacy data and writes nothing.
    let p1 = photo(c, &pictures, "Lisbon/IMG_0001.jpg");
    assert_eq!((&p1["star"], &p1["caption"]), (&json!(true), &json!(BREAKFAST)), "{p1}");
    assert_eq!(edits(&p1), RECIPE, "bw has an equivalent, the red-eye fix is kept as legacy: {p1}");
    let tags = p1["tags"].to_string();
    assert!(tags.contains("tram") && tags.contains("river"), "the keywords: {p1}");
    let album = p1["albums"].as_array().and_then(|a| a.first()).cloned().unwrap_or_else(|| panic!("in the album: {p1}"));
    assert_eq!(album["name"], ALBUM_NAME, "{p1}");
    let place = [p1["place"][0].as_f64().unwrap_or(f64::NAN), p1["place"][1].as_f64().unwrap_or(f64::NAN)];
    assert!((place[0] - LISBON[0]).abs() < 1e-6 && (place[1] - LISBON[1]).abs() < 1e-6, "the geotag: {p1}");
    let p2 = photo(c, &pictures, "Lisbon/IMG_0002.jpg");
    assert_eq!((&p2["star"], &p2["caption"], &p2["albums"][0]["name"]), (&json!(false), &json!(GATE), &json!(ALBUM_NAME)), "{p2}");
    assert!(star(c, &pictures, "Lisbon/IMG_0003.jpg"), "IMG_0003's star");
    let people = c.exec("people.list", json!({}));
    let marta = people["people"].as_array().and_then(|p| p.iter().find(|p| p["name"] == PERSON)).cloned();
    assert_eq!(marta.as_ref().map(|m| m["count"].clone()), Some(json!(1)), "the person named in the sidecar, on one photo: {people}");
    let members = app::section(c, &pictures, &format!("album:{}", album["id"].as_str().unwrap()));
    assert_eq!(members, ["Lisbon/IMG_0001.jpg", "Lisbon/IMG_0002.jpg"], "the album holds its two photos");
    assert_eq!(source(c, &fx.dir("Lisbon")), "legacy");
    println!("  Lisbon after the first scan: stars, captions, keywords, the recipe {RECIPE:?}, the album, {PERSON} and the geotag");

    // Both: only Dalica's file.
    let both = photo(c, &pictures, "Both/a.jpg");
    assert_eq!((&both["star"], &both["caption"]), (&json!(false), &json!("Dalica's own")), "only .dalica.json is read: {both}");
    assert_eq!(source(c, &fx.dir("Both")), "both");
    // Hostile and Odd: refused or skipped, without a panic; their photos listed.
    assert!(!star(c, &pictures, "Hostile/a.jpg"), "the refused file says nothing");
    let odd_a = photo(c, &pictures, "Odd/a.jpg");
    assert_eq!((&odd_a["star"], &odd_a["place"]), (&json!(false), &Value::Null), "malformed values skipped: {odd_a}");
    assert!(star(c, &pictures, "Odd/b.jpg"), "the good value beside them kept");
    println!(
        "  Both reads only .dalica.json; Hostile's file (a line over 1 MiB) refused whole, Odd's malformed values skipped and its good one kept"
    );

    // The notice, once, with its count; the whole window.
    let inspected = c.exec("legacy.inspect", json!({}));
    let count = inspected["notice"].as_u64().unwrap_or_else(|| panic!("the notice waits to give a count: {inspected}"));
    assert!(count >= 3, "{inspected}");
    until(c, "the notice", |c| notice(c, count).is_some());
    run(c, "select.set", json!({ "ids": [] }));
    common::steady_shot(c, home, "m14-1400x900-legacy-notice", &[]);
    let after_scan = common::tree(&fx.pictures);
    assert!(after_scan == all_before, "the scan wrote nothing");
    click(c, OK_ID);
    assert!(notice(c, count).is_none(), "OK closes it");
    let inspected = c.exec("legacy.inspect", json!({}));
    assert_eq!((&inspected["noticeShown"], &inspected["notice"]), (&json!(true), &Value::Null), "{inspected}");
    println!("  the notice gave \"{count} folders\" once and closed on OK; nothing written");

    // The first change in Lisbon: .dalica.json with all of it; the legacy file byte-identical.
    let lisbon = fx.dir("Lisbon");
    let p2_id = p2["id"].clone();
    run(c, "photo.star.set", json!({ "ids": [p2_id], "on": true }));
    let entries =
        |dir: &Path| -> Value { serde_json::from_slice(&std::fs::read(dir.join(".dalica.json")).unwrap_or_default()).unwrap_or(Value::Null) };
    flush_until(c, || (1..=3).all(|n| entries(&lisbon)["photos"][format!("IMG_{n:04}.jpg")]["star"] == true));
    let written = entries(&lisbon);
    let e1 = &written["photos"]["IMG_0001.jpg"];
    assert_eq!((&e1["star"], &e1["caption"], &e1["edits"]), (&json!(true), &json!(BREAKFAST), &json!(RECIPE)), "{written:#}");
    assert!(e1["faces"].as_array().is_some_and(|f| f.len() == 1), "the face: {written:#}");
    assert!(e1.to_string().contains("tram") && e1.to_string().contains("38.70"), "the keywords and the geotag: {e1}");
    assert_eq!((&written["photos"]["IMG_0002.jpg"]["star"], &written["photos"]["IMG_0002.jpg"]["caption"]), (&json!(true), &json!(GATE)));
    assert!(written.to_string().contains(ALBUM_NAME) && written.to_string().contains(PERSON), "the album and the person: {written:#}");
    for (path, bytes) in &legacy_files {
        assert!(std::fs::read(path).unwrap() == *bytes, "{}: byte-identical", path.display());
    }
    let mut now = common::tree(&fx.pictures);
    now.remove(&PathBuf::from("Lisbon").join(".dalica.json"));
    assert!(now == all_before, "only Lisbon's .dalica.json was written");
    until(c, "Lisbon's source turns both", |c| source(c, &lisbon) == "both");
    println!("  the first change wrote Lisbon's .dalica.json with every photo, the album and the person; every legacy file byte-identical");

    // The option off: the unconverted Lake loses its star, Lisbon keeps everything; on: back.
    run(c, "options.set", json!({ "values": { OPTION: false } }));
    until(c, "Lake read without legacy data", |c| !star(c, &pictures, "Lake/a.jpg") && !star(c, &pictures, "Odd/b.jpg"));
    let p1 = photo(c, &pictures, "Lisbon/IMG_0001.jpg");
    assert_eq!(
        (&p1["star"], &p1["caption"], edits(&p1)),
        (&json!(true), &json!(BREAKFAST), RECIPE.map(str::to_string).to_vec()),
        "Lisbon keeps its own file's data"
    );
    run(c, "options.set", json!({ "values": { OPTION: true } }));
    until(c, "Lake's star back", |c| star(c, &pictures, "Lake/a.jpg") && star(c, &pictures, "Odd/b.jpg"));
    assert!(!fx.dir("Lake").join(".dalica.json").exists() && !fx.dir("Odd").join(".dalica.json").exists(), "nothing written");
    println!("  the option off took Lake's legacy star away, Lisbon kept its data; on brought it back; nothing written");

    // Undo Save and Revert from the legacy originals.
    let s1 = photo(c, &pictures, "Saved/IMG_0001.jpg")["id"].clone();
    let s2 = photo(c, &pictures, "Saved/IMG_0002.jpg")["id"].clone();
    let saved: Vec<Vec<u8>> = (1..=2).map(|n| std::fs::read(fx.dir("Saved").join(format!("IMG_{n:04}.jpg"))).unwrap()).collect();
    run(c, "photo.undoSave", json!({ "ids": [s1] }));
    run(c, "photo.revert", json!({ "ids": [s2], "confirm": true }));
    let path = |n: u32| fx.dir("Saved").join(format!("IMG_{n:04}.jpg"));
    until(c, "both originals back", |_| {
        std::fs::read(path(1)).ok().as_ref() == Some(&fx.originals[0]) && std::fs::read(path(2)).ok().as_ref() == Some(&fx.originals[1])
    });
    assert_eq!(edits(&c.inspect(&s1)), ["sepia()", "legacy:redeye(1)"], "Undo Save brings the legacy recipe back as unsaved edits");
    assert!(edits(&c.inspect(&s2)).is_empty(), "Revert drops the edits");
    assert!(common::tree(&kept_dir) == kept_before, "the legacy originals folder is never written");
    let trashed = common::tree(&home.join("data"));
    for (n, bytes) in saved.iter().enumerate() {
        assert!(trashed.values().any(|b| b == bytes), "the saved IMG_{:04}.jpg went to the trash", n + 1);
    }
    println!("  Undo Save and Revert brought the legacy originals back (the saved files in the trash, the originals folder untouched)");
    live::quit(a, cl);

    // A restart: the notice is not given again; the data is all there.
    let (a, mut cl) = live::start_with(exe, home, launch());
    let c = &mut cl;
    app::resize(c, LARGE);
    app::idle(c);
    assert!(!any_notice(c), "the notice is given once");
    let p1 = photo(c, &pictures, "Lisbon/IMG_0001.jpg");
    assert_eq!((&p1["star"], &p1["caption"], edits(&p1)), (&json!(true), &json!(BREAKFAST), RECIPE.map(str::to_string).to_vec()), "{p1}");
    assert!(star(c, &pictures, "Lake/a.jpg"), "Lake's legacy star");
    for (path, bytes) in &legacy_files {
        assert!(std::fs::read(path).unwrap() == *bytes, "{}: byte-identical", path.display());
    }
    live::quit(a, cl);
    println!("  M14 legacy sidecars in {:.1} s", secs(started));
}
