//! The Folder Manager (M5.18, `organizing.md` §2), driven headlessly by widget id on a library
//! whose standard places and volume are planted (`Session::set_places`): a home folder with
//! Pictures (watched: two folders of photos), Documents and Desktop, and a volume "card".
//!
//! - Expanding a root lists its children without blocking a frame: a spinner row first, the rows
//!   once the engine's worker has read them, and the UI thread touches no file.
//! - Every row shows its effective state; an inherited one is drawn at half opacity.
//! - Cancel changes nothing; OK applies the draft as one undo step, which undo takes back.
//! - Scan Always on a volume root asks first: Cancel reverts the radio, Watch Drive sets it.
//! - The library's selected folder is pre-selected and revealed; the watched list reveals too.
//! - Every widget has an id at 1400 × 900 and 900 × 600, and the dialog matches its fingerprints
//!   (`tests/fingerprints/dialog-folder-manager-<w>x<h>.json`; `DALICA_BLESS=1` stores new ones,
//!   renders go to `target/test-output/ui/`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dalica_core::FolderId;
use dalica_engine::Session;
use dalica_scan::{Places, Volume, VolumeKind};
use dalica_scenes::fingerprint;
use dalica_ui::dialogs::folder_manager::draft::Draft;
use dalica_ui::dialogs::folder_manager::{self, State, expand_id, mode_id, reveal_id, row_id};
use dalica_ui::state::DialogKind;
use dalica_ui::{App, Driver};
use dalica_ui_kit::registry::{self, Role};
use dalica_ui_kit::{Tokens, WidgetInfo};
use serde_json::json;

const FRAMES: usize = 60 * 60;
const SIZES: [(u32, u32); 2] = [(1400, 900), (900, 600)];

fn put_jpeg(path: &Path, seed: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let jpeg = dalica_scenes::encode::jpeg(&dalica_scenes::Scene::new(seed, 96, 64), 80, None).unwrap();
    std::fs::write(path, jpeg).unwrap();
}

struct Fx {
    _dir: tempfile::TempDir,
    home: PathBuf,
    pictures: PathBuf,
    card: PathBuf,
    d: Driver,
}

/// The library: `home/Pictures` watched (`2026-01 Trip` with 3 photos, `Old` with 2),
/// `home/Documents/Notes` and `home/Desktop` empty, the volume `card` with `DCIM` (1 photo).
fn fixture((w, h): (u32, u32)) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let pictures = home.join("Pictures");
    let card = dir.path().join("card");
    for i in 0..3 {
        put_jpeg(&pictures.join("2026-01 Trip").join(format!("IMG_{i:04}.jpg")), i);
    }
    for i in 0..2 {
        put_jpeg(&pictures.join("Old").join(format!("IMG_{i:04}.jpg")), 10 + i);
    }
    put_jpeg(&card.join("DCIM").join("IMG_0001.jpg"), 20);
    std::fs::create_dir_all(home.join("Documents").join("Notes")).unwrap();
    std::fs::create_dir_all(home.join("Desktop")).unwrap();
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.set_places(Places::under(&home), vec![Volume { name: "card".into(), path: card.clone(), kind: VolumeKind::Mounted }]);
    s.execute("folders.set", &json!({ "changes": [{ "path": pictures, "mode": "always" }] })).unwrap();
    s.wait_idle();
    assert_eq!(s.library().photos().count(), 5);
    let mut d = Driver::new(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    Fx { _dir: dir, home, pictures, card, d }
}

fn widget(d: &Driver, id: &str) -> Option<WidgetInfo> {
    d.widgets().into_iter().find(|w| w.id == id)
}

fn has(d: &Driver, id: &str) -> bool {
    widget(d, id).is_some()
}

/// Runs frames (yielding to the workers) until widget `id` is drawn; the frames it took.
fn wait_for(d: &mut Driver, id: &str) -> usize {
    for n in 0..4000 {
        if has(d, id) {
            return n;
        }
        d.step();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("`{id}` never appeared; widgets: {:?}", d.widgets().iter().map(|w| w.id.clone()).collect::<Vec<_>>());
}

/// Opens the Folder Manager and waits for its roots.
fn open(fx: &mut Fx) {
    assert_eq!(fx.d.run("folders.manager", &json!({})).unwrap(), json!({ "dialog": "folderManager" }));
    let pictures = row_id(&fx.pictures, &fx.pictures);
    wait_for(&mut fx.d, &pictures);
}

fn draft(d: &Driver) -> Draft {
    let fm = d.state().dialogs.iter().find(|d| d.kind == DialogKind::FolderManager).expect("the Folder Manager is open");
    Draft::read(&fm.fields)
}

fn rules(d: &Driver, path: &Path) -> Option<(Option<&'static str>, Option<bool>)> {
    d.app().session().unwrap().folder_rules().get(path).map(|r| (r.mode.map(|m| m.id()), r.faces))
}

fn undo_steps(d: &Driver) -> usize {
    d.app().session().unwrap().undo_stack().len()
}

fn photos(d: &Driver) -> usize {
    d.app().session().unwrap().library().photos().count()
}

fn active(d: &Driver, id: &str) -> bool {
    widget(d, id).unwrap_or_else(|| panic!("no `{id}`")).active
}

/// Expanding a root lists its children without blocking a frame: the frame after the click shows a
/// spinner row, the rows come once a worker has read the folder, and the UI thread never touches
/// the disk meanwhile.
#[test]
fn expanding_a_root_lists_its_children_without_blocking_a_frame() {
    let mut fx = fixture(SIZES[0]);
    open(&mut fx);
    let home = fx.home.clone();
    // The roots: the places that exist, then the volume, each with its name.
    let roots: Vec<String> =
        fx.d.widgets().iter().filter(|w| w.id.starts_with("choice:dialog.folder/") && w.role == Role::TreeRow).map(|w| w.label.clone()).collect();
    assert_eq!(roots, ["Pictures", "Documents", "Desktop", "Home", "card"]);
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    fx.d.click(&expand_id(&home, &home)).unwrap();
    let loading = format!("{}/loading", row_id(&home, &home));
    let child = row_id(&home, &home.join("Pictures"));
    assert!(has(&fx.d, &loading) && !has(&fx.d, &child), "a spinner row while the worker reads");
    assert!(draft(&fx.d).is_expanded(&home));
    let frames = wait_for(&mut fx.d, &child);
    assert!(!has(&fx.d, &loading));
    let children: Vec<String> = ["Desktop", "Documents", "Pictures"].iter().map(|n| row_id(&home, &home.join(n))).collect();
    let ys: Vec<f32> = children.iter().map(|id| widget(&fx.d, id).unwrap().rect[1]).collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]), "sorted by name: {ys:?}");
    assert_eq!(dalica_fs::guard::refused(), refused, "the UI thread touched the disk");
    drop(ui);
    println!("children listed after {frames} frames");
    // Collapsing hides them; the triangle of a folder known to be empty goes away.
    fx.d.click(&expand_id(&home, &home)).unwrap();
    assert!(!has(&fx.d, &child));
    let desktop = fx.home.join("Desktop");
    fx.d.click(&expand_id(&desktop, &desktop)).unwrap();
    for _ in 0..4000 {
        if !has(&fx.d, &expand_id(&desktop, &desktop)) {
            break;
        }
        fx.d.step();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(!has(&fx.d, &expand_id(&desktop, &desktop)) && has(&fx.d, &row_id(&desktop, &desktop)), "an empty folder has no triangle");
}

/// The most colourful pixel of row `id`'s state icon (a row at `depth`), as RGB.
fn state_ink(d: &Driver, id: &str, depth: usize) -> [u8; 3] {
    let w = widget(d, id).unwrap_or_else(|| panic!("no `{id}`"));
    let row = egui::Rect::from_min_size(egui::pos2(w.rect[0], w.rect[1]), egui::vec2(w.rect[2], w.rect[3]));
    let r = folder_manager::tree::state_rect(&Tokens::default(), row, depth);
    let shot = d.screenshot().unwrap();
    let mut best = ([0u8; 3], 0i32);
    for y in r.min.y as u32..r.max.y as u32 {
        for x in r.min.x as u32..r.max.x as u32 {
            let p = shot.pixel(x, y).unwrap();
            let sat = i32::from(*p[..3].iter().max().unwrap()) - i32::from(*p[..3].iter().min().unwrap());
            if sat > best.1 {
                best = ([p[0], p[1], p[2]], sat);
            }
        }
    }
    best.0
}

/// Every row shows its folder's effective state: Pictures sets Scan Always (`accent`, full), its
/// subfolders inherit it (half opacity); Documents inherits Remove (`danger`, half) until one of
/// its folders sets it (full).
#[test]
fn rows_show_their_state_and_inherited_states_at_half_opacity() {
    let mut fx = fixture(SIZES[0]);
    open(&mut fx);
    let (pics, docs) = (fx.pictures.clone(), fx.home.join("Documents"));
    fx.d.click(&expand_id(&pics, &pics)).unwrap();
    let old = row_id(&pics, &pics.join("Old"));
    wait_for(&mut fx.d, &old);
    fx.d.click(&expand_id(&docs, &docs)).unwrap();
    let notes = row_id(&docs, &docs.join("Notes"));
    wait_for(&mut fx.d, &notes);
    let t = Tokens::default();
    let accent = [t.accent.r(), t.accent.g(), t.accent.b()];
    let own = state_ink(&fx.d, &row_id(&pics, &pics), 0);
    let inherited = state_ink(&fx.d, &old, 1);
    assert!(own[2] >= 200 && own.iter().zip(accent).all(|(a, b)| a.abs_diff(b) <= 24), "Scan Always set here: {own:?} vs {accent:?}");
    assert!((90..=160).contains(&inherited[2]) && inherited[0] < 60, "Scan Always inherited: {inherited:?}");
    let removed = state_ink(&fx.d, &row_id(&docs, &docs), 0);
    assert!((100..=160).contains(&removed[0]) && removed[1] < 80, "Remove inherited (nothing set): {removed:?}");
    // Setting Notes to Remove itself draws its icon in full.
    fx.d.click(&notes).unwrap();
    assert!(active(&fx.d, &mode_id(State::Remove)), "Remove, inherited");
    fx.d.click(&mode_id(State::Remove)).unwrap();
    let set = state_ink(&fx.d, &notes, 1);
    assert!(set[0] >= 200 && set[1] < 110, "Remove set here: {set:?}");
    assert_eq!(draft(&fx.d).changes.get(docs.join("Notes").to_string_lossy().as_ref()).and_then(|c| c.mode.as_deref()), Some("remove"));
}

/// Cancel changes nothing; OK applies every change of the draft as one undo step (folders removed,
/// modes and the faces flag set), and one undo takes it all back.
#[test]
fn cancel_changes_nothing_and_ok_applies_one_undo_step() {
    let mut fx = fixture(SIZES[0]);
    let pics = fx.pictures.clone();
    let (old, trip, docs) = (pics.join("Old"), pics.join("2026-01 Trip"), fx.home.join("Documents"));
    let steps = undo_steps(&fx.d);
    open(&mut fx);
    assert!(!widget(&fx.d, &mode_id(State::Always)).unwrap().enabled, "the radios wait for a selection");
    assert!(!widget(&fx.d, folder_manager::FACES_ID).unwrap().enabled);
    fx.d.click(&expand_id(&pics, &pics)).unwrap();
    wait_for(&mut fx.d, &row_id(&pics, &old));
    fx.d.click(&row_id(&pics, &old)).unwrap();
    assert!(active(&fx.d, &mode_id(State::Always)), "Old inherits Scan Always");
    fx.d.click(&mode_id(State::Remove)).unwrap();
    assert!(active(&fx.d, &mode_id(State::Remove)));
    fx.d.click("button:dialog.cancel").unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!((rules(&fx.d, &old), undo_steps(&fx.d), photos(&fx.d)), (None, steps, 5), "Cancel changed something");

    // Again, with four changes, then OK.
    open(&mut fx);
    assert_eq!(draft(&fx.d).changes.len(), 0, "a new draft each time");
    fx.d.click(&expand_id(&pics, &pics)).unwrap();
    wait_for(&mut fx.d, &row_id(&pics, &old));
    fx.d.click(&row_id(&pics, &old)).unwrap();
    fx.d.click(&mode_id(State::Remove)).unwrap();
    fx.d.click(&row_id(&pics, &trip)).unwrap();
    fx.d.click(&mode_id(State::Once)).unwrap();
    fx.d.click(&row_id(&docs, &docs)).unwrap();
    fx.d.click(&mode_id(State::Always)).unwrap();
    fx.d.click(&row_id(&pics, &pics)).unwrap();
    assert!(active(&fx.d, folder_manager::FACES_ID), "faces are found unless a folder says not");
    fx.d.click(folder_manager::FACES_ID).unwrap();
    assert!(!active(&fx.d, folder_manager::FACES_ID));
    assert_eq!(draft(&fx.d).changes.len(), 4);
    // The watched list follows the draft: Documents joins Pictures.
    let watched: Vec<String> = fx.d.widgets().iter().filter(|w| w.id.starts_with("button:dialog.reveal/")).map(|w| w.label.clone()).collect();
    assert_eq!(watched, ["Documents", "Pictures"]);
    fx.d.click("button:dialog.ok").unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!(undo_steps(&fx.d), steps + 1, "one undo step");
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(rules(&fx.d, &old), Some((Some("remove"), None)));
    assert_eq!(rules(&fx.d, &trip), Some((Some("once"), None)));
    assert_eq!(rules(&fx.d, &docs), Some((Some("always"), None)));
    assert_eq!(rules(&fx.d, &pics), Some((Some("always"), Some(false))));
    assert_eq!(photos(&fx.d), 3, "Old's photos left the library");
    assert!(old.join("IMG_0000.jpg").is_file(), "the files stay on disk");
    // One undo takes everything back.
    fx.d.run("edit.undo", &json!({})).unwrap();
    fx.d.settle(FRAMES).unwrap();
    assert_eq!((rules(&fx.d, &old), rules(&fx.d, &trip), rules(&fx.d, &docs)), (None, None, None));
    assert_eq!(rules(&fx.d, &pics), Some((Some("always"), None)));
    assert_eq!(photos(&fx.d), 5);
}

/// Scan Always on a volume root asks first: the Watch Drive question opens over the Folder
/// Manager, Cancel (Esc) leaves the radio as it was, Watch Drive sets it, and OK watches the drive.
#[test]
fn the_drive_confirmation_reverts_on_cancel_and_watch_drive_sets_the_radio() {
    let mut fx = fixture(SIZES[0]);
    let card = fx.card.clone();
    open(&mut fx);
    fx.d.click(&row_id(&card, &card)).unwrap();
    assert!(active(&fx.d, &mode_id(State::Remove)), "a volume nothing watches");
    fx.d.click(&mode_id(State::Always)).unwrap();
    let top = fx.d.state().dialogs.last().unwrap().clone();
    assert_eq!(top.kind, DialogKind::WatchDrive);
    for id in ["dialog:watchDrive", "button:dialog.watchDrive", "button:dialog.cancel"] {
        assert!(has(&fx.d, id), "{id}");
    }
    // Esc on the question's first frame closes the question, not the dialog under it.
    fx.d.key("Escape").unwrap();
    assert_eq!(fx.d.state().dialogs.len(), 1, "only Watch Drive closed");
    assert!(active(&fx.d, &mode_id(State::Remove)) && !active(&fx.d, &mode_id(State::Always)), "Cancel reverts the radio");
    assert!(draft(&fx.d).changes.is_empty());
    // Watch Drive sets it.
    fx.d.click(&mode_id(State::Always)).unwrap();
    fx.d.click("button:dialog.watchDrive").unwrap();
    assert_eq!(fx.d.state().dialogs.len(), 1);
    assert!(active(&fx.d, &mode_id(State::Always)));
    let dr = draft(&fx.d);
    assert!(dr.is_confirmed(&card) && dr.changes.len() == 1, "{dr:?}");
    // Clicking Always again asks nothing: it is already set.
    fx.d.click(&mode_id(State::Once)).unwrap();
    fx.d.click(&mode_id(State::Always)).unwrap();
    assert_eq!(fx.d.state().dialogs.len(), 1, "confirmed once for this dialog");
    fx.d.click("button:dialog.ok").unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    fx.d.settle(FRAMES).unwrap();
    assert_eq!(rules(&fx.d, &card), Some((Some("always"), None)));
    assert_eq!(photos(&fx.d), 6);
}

/// With the Folder Manager open (its roots name the drives), `folders.set` watching the home folder
/// asks through the host, and confirming runs it with `"confirm": true`; over `engine.execute` the
/// same question is `needs_confirm`.
#[test]
fn folders_set_on_a_drive_asks_through_the_host() {
    let mut fx = fixture(SIZES[0]);
    let home = fx.home.clone();
    open(&mut fx);
    let params = json!({ "changes": [{ "path": home, "mode": "always" }] });
    assert_eq!(fx.d.run("folders.set", &params).unwrap(), json!({ "dialog": "watchDrive" }));
    let top = fx.d.state().dialogs.last().unwrap().clone();
    assert_eq!((top.kind, top.command.as_ref().map(|c| c.id.as_str())), (DialogKind::WatchDrive, Some("folders.set")));
    assert_eq!(rules(&fx.d, &home), None, "nothing yet");
    let r = dalica_ui::dialogs::answer(fx.d.app_mut(), None).unwrap();
    assert_eq!(r["changed"], 1, "{r}");
    assert_eq!(rules(&fx.d, &home), Some((Some("always"), None)));
}

/// Folder ▸ Remove from Dalica… (`folder.remove`) asks through the host, naming the folder: Esc
/// keeps it, Remove Folder removes it from the library (one undo step, the files stay); an unknown
/// or bad id asks nothing and is the command's to refuse. Over automation the question is the
/// `needs_confirm` prompt.
#[test]
fn remove_folder_asks_through_the_host() {
    let mut fx = fixture(SIZES[0]);
    let old = fx.pictures.join("Old");
    let id = fx.d.app().session().unwrap().library().folders().find(|f| f.path == old).unwrap().id.get();
    let steps = undo_steps(&fx.d);
    assert_eq!(fx.d.run("folder.remove", &json!({ "id": id })).unwrap(), json!({ "dialog": "removeFolder" }));
    fx.d.settle(FRAMES).unwrap();
    for w in ["dialog:removeFolder", "button:dialog.removeFolder", "button:dialog.cancel"] {
        assert!(has(&fx.d, w), "{w}");
    }
    let v = dalica_ui::dialogs::inspect(fx.d.app());
    assert_eq!((v["title"].as_str(), v["default"].as_str()), (Some("Remove Folder"), Some("button:dialog.removeFolder")));
    let top = fx.d.state().dialogs.last().unwrap().clone();
    assert_eq!(
        dalica_ui::dialogs::confirm::question(fx.d.app(), &top),
        "Remove the folder \"Old\" and its subfolders from Dalica? The files stay on your disk."
    );
    let ask = dalica_ui::dialogs::asking(fx.d.app(), "folder.remove", &json!({ "id": id })).unwrap();
    assert_eq!(
        dalica_ui::dialogs::confirm::prompt(ask),
        json!({ "title": "Remove Folder", "text": dalica_ui::dialogs::confirm::REMOVE_QUESTION, "buttons": ["Remove Folder", "Cancel"], "dialog": "removeFolder" })
    );
    fx.d.key("Escape").unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!((photos(&fx.d), undo_steps(&fx.d)), (5, steps), "Cancel keeps the folder");
    fx.d.run("folder.remove", &json!({ "id": id })).unwrap();
    fx.d.settle(FRAMES).unwrap();
    fx.d.click("button:dialog.removeFolder").unwrap();
    assert!(fx.d.state().dialogs.is_empty());
    assert_eq!((photos(&fx.d), undo_steps(&fx.d)), (3, steps + 1));
    assert_eq!(rules(&fx.d, &old), Some((Some("remove"), None)));
    assert!(old.join("IMG_0001.jpg").is_file(), "the files stay on disk");
    // Nothing to ask about: the command refuses, no dialog opens.
    for bad in [json!({ "id": 9999 }), json!({ "id": -1 }), json!({ "id": "x" }), json!({})] {
        assert!(dalica_ui::dialogs::asking(fx.d.app(), "folder.remove", &bad).is_none(), "{bad}");
        assert!(fx.d.run("folder.remove", &bad).is_err(), "{bad}");
        assert!(fx.d.state().dialogs.is_empty(), "{bad}");
    }
}

/// The library's selected folder is pre-selected and its parents expanded; a click on a watched
/// folder selects and reveals it.
#[test]
fn the_selected_library_folder_is_preselected_and_the_watched_list_reveals() {
    let mut fx = fixture(SIZES[0]);
    let (pics, old) = (fx.pictures.clone(), fx.pictures.join("Old"));
    let id: FolderId = fx.d.app().session().unwrap().library().folders().find(|f| f.path == old).unwrap().id;
    fx.d.app_mut().state_mut().folder = Some(id);
    open(&mut fx);
    wait_for(&mut fx.d, &row_id(&pics, &old));
    let dr = draft(&fx.d);
    assert_eq!(dr.selected.as_deref(), Some(old.to_string_lossy().as_ref()));
    assert!(dr.is_expanded(&pics) && dr.reveal.is_none(), "{dr:?}");
    assert!(active(&fx.d, &row_id(&pics, &old)), "its row is selected");
    assert!(widget(&fx.d, &mode_id(State::Always)).unwrap().enabled && active(&fx.d, &mode_id(State::Always)));
    // The watched list: Pictures, by its name; a click selects it in the tree.
    let w = widget(&fx.d, &reveal_id(&pics)).unwrap();
    assert_eq!(w.label, "Pictures");
    fx.d.click(&reveal_id(&pics)).unwrap();
    fx.d.step();
    assert_eq!(draft(&fx.d).selected.as_deref(), Some(pics.to_string_lossy().as_ref()));
    assert!(active(&fx.d, &row_id(&pics, &pics)));
}

/// Checks the last frame: no unregistered interactive widget, unique ids in the grammar, every
/// rect on screen.
fn check_ids(d: &Driver, (w, h): (u32, u32)) {
    assert_eq!(d.app().panels().unwrap().unregistered_widgets(), [], "{w}×{h}: interactive widgets without an id");
    let mut seen = HashSet::new();
    for wi in d.widgets() {
        assert!(registry::is_valid_id(&wi.id), "{w}×{h}: `{}` breaks the id grammar", wi.id);
        assert!(seen.insert(wi.id.clone()), "{w}×{h}: `{}` is registered twice", wi.id);
        let [x, y, rw, rh] = wi.rect;
        assert!(
            rw > 0.0 && rh > 0.0 && x >= 0.0 && y >= 0.0 && x + rw <= w as f32 + 0.01 && y + rh <= h as f32 + 0.01,
            "{w}×{h}: `{}` at {:?}",
            wi.id,
            wi.rect
        );
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-output/ui");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Checks the region `id` covers in the last frame against `tests/fingerprints/<name>.json`, and
/// writes it and the whole window to the test output.
pub fn check_region(d: &Driver, id: &str, name: &str) {
    let shot = d.screenshot().unwrap();
    std::fs::write(out_dir().join(format!("{name}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba)).unwrap();
    let r = widget(d, id).unwrap().rect;
    let (x0, y0, w, h) = (r[0].round() as u32, r[1].round() as u32, r[2].round() as u32, r[3].round() as u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            rgba.extend_from_slice(&shot.pixel(x, y).unwrap());
        }
    }
    std::fs::write(out_dir().join(format!("{name}.png")), fingerprint::png_rgba(w, h, &rgba)).unwrap();
    let stored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fingerprints/{name}.json"));
    if std::env::var_os("DALICA_BLESS").is_some() {
        std::fs::write(&stored, fingerprint::to_json(&fingerprint::fingerprint(w, h, &rgba)) + "\n").unwrap();
    }
    let expected = fingerprint::from_json(&std::fs::read_to_string(&stored).unwrap()).unwrap();
    if let Err(m) = fingerprint::check(w, h, &rgba, &expected, 2.0) {
        std::fs::write(out_dir().join(format!("{name}-actual.png")), &m.actual_png).unwrap();
        panic!("{name}: {}", m.message);
    }
}

/// Every widget of the dialog has an id at both window sizes (the tree expanded, a spinner row
/// gone, the Watch Drive question over it), and the dialog matches its fingerprint at both sizes:
/// Home expanded, Pictures/Old selected, Documents set to Scan Once in the draft.
#[test]
fn every_widget_has_an_id_and_the_dialog_matches_its_fingerprints_at_both_sizes() {
    for size in SIZES {
        let mut fx = fixture(size);
        let (home, pics, docs) = (fx.home.clone(), fx.pictures.clone(), fx.home.join("Documents"));
        open(&mut fx);
        check_ids(&fx.d, size);
        fx.d.click(&expand_id(&home, &home)).unwrap();
        check_ids(&fx.d, size);
        wait_for(&mut fx.d, &row_id(&home, &docs));
        fx.d.click(&expand_id(&pics, &pics)).unwrap();
        wait_for(&mut fx.d, &row_id(&pics, &pics.join("Old")));
        fx.d.click(&row_id(&home, &docs)).unwrap();
        fx.d.click(&mode_id(State::Once)).unwrap();
        fx.d.click(&row_id(&pics, &pics.join("Old"))).unwrap();
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d, size);
        let (w, h) = size;
        check_region(&fx.d, "dialog:folderManager", &format!("dialog-folder-manager-{w}x{h}"));
        // The Watch Drive question over it.
        fx.d.click(&row_id(&home, &home)).unwrap();
        fx.d.click(&mode_id(State::Always)).unwrap();
        assert!(has(&fx.d, "dialog:watchDrive"));
        // Faded in, over the Folder Manager.
        fx.d.settle(FRAMES).unwrap();
        check_ids(&fx.d, size);
        assert!(has(&fx.d, "dialog:folderManager/ok") && !has(&fx.d, "button:dialog.ok"), "the buttons under the question are its parts");
        let shot = fx.d.screenshot().unwrap();
        std::fs::write(out_dir().join(format!("dialog-watch-drive-{w}x{h}-window.png")), fingerprint::png_rgba(shot.width, shot.height, &shot.rgba))
            .unwrap();
    }
}
