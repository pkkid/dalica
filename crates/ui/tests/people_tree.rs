//! The People group in the tree, person sections and the person menu (M13.25, `organizing.md`
//! §6.2–§6.3, `ui-layout.md` §4 and §5.2), driven headlessly by widget id on a library of six
//! photos: Ana's face on photos 1 and 2, Rui's on 3 to 5, two unnamed faces on photo 6.
//!
//! - the People group sits above Folders and lists Unnamed (2), then Ana (2) and Rui (3) by name,
//!   Rui first by count; Unnamed shows the Unnamed grid, a folder puts the light table back;
//! - dragging the two Unnamed faces onto Ana names them as one undo step;
//! - Ana's row selects her section, which shows her photos; Zoom to Faces shows her faces, whose
//!   menu removes one from her (back to Unnamed), as Delete does, and whose photo Set as Person
//!   Thumbnail takes;
//! - the person menu's Rename…, Merge Into ▸ and Delete Person each act through their dialog and
//!   undo in one step; a name another person has asks to merge instead;
//! - Play on Ana's section starts a slideshow of her photos;
//! - at 1400 × 900 the tree and Ana's section match `tests/fingerprints/people-tree-1400x900.json`
//!   and `person-section-1400x900.json` (`DALICA_BLESS=1` stores new ones; renders go to
//!   `target/test-output/ui/`); at 900 × 600 the tree fits.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{FRAMES, LARGE, check_region, check_sizes, new_driver, shot, try_widget, widget};
use dalica_core::{PersonId, PhotoId};
use dalica_engine::Session;
use dalica_ui::headless::{Click, Drag, Target};
use dalica_ui::state::{AlbumSection, DialogKind, FaceRef, PersonKey, Section, View};
use dalica_ui::{App, Driver};
use egui::{Modifiers, PointerButton};
use serde_json::{Value, json};

struct T {
    _dir: tempfile::TempDir,
    d: Driver,
    /// The photos in photo order.
    photos: Vec<PhotoId>,
    ana: PersonId,
    rui: PersonId,
    /// The two unnamed faces of photo 6.
    unnamed: Vec<FaceRef>,
}

/// Six photos, Ana on the first two, Rui on the next three, two unnamed faces on the last, at
/// `w × h`.
fn setup(w: u32, h: u32) -> T {
    let dir = tempfile::tempdir().unwrap();
    let files = crate::common::demo_library(1, 6, 43).unwrap();
    for f in &files {
        let path = dir.path().join("photos").join(&f.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &f.bytes).unwrap();
    }
    let folder = dir.path().join("photos").join(files[0].path.parent().unwrap());
    let mut s = Session::open(&dir.path().join("library"), Arc::new(dalica_core::SystemClock)).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let found = s.execute("library.query", &json!({ "limit": 1000 })).unwrap();
    let mut ids: Vec<u64> = found["photos"].as_array().unwrap().iter().map(|p| p["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    assert_eq!(ids.len(), 6);
    let person = |s: &mut Session, name: &str| -> PersonId {
        let r = s.execute("people.create", &json!({ "name": name })).unwrap();
        PersonId::parse(r["person"].as_str().unwrap()).unwrap()
    };
    let (ana, rui) = (person(&mut s, "Ana"), person(&mut s, "Rui"));
    for (i, id) in ids.iter().enumerate().take(5) {
        let who = if i < 2 { &ana } else { &rui };
        s.execute("faces.add", &json!({ "id": id, "rect": [0.3, 0.15, 0.65, 0.75], "person": who })).unwrap();
    }
    let mut unnamed = Vec::new();
    for x in [0.1, 0.55] {
        let r = s.execute("faces.add", &json!({ "id": ids[5], "rect": [x, 0.2, x + 0.3, 0.7] })).unwrap();
        unnamed.push(r["face"].as_str().unwrap().parse::<FaceRef>().unwrap());
    }
    let mut d = new_driver(App::new(Some(s)), w, h, 1.0);
    d.settle(FRAMES).unwrap();
    T { _dir: dir, d, photos: ids.into_iter().map(PhotoId::new).collect(), ana, rui, unnamed }
}

fn run(t: &mut T, id: &str, params: Value) -> Value {
    let v = t.d.run(id, &params).unwrap_or_else(|e| panic!("{id} {params}: {e}"));
    t.d.settle(FRAMES).unwrap();
    v
}

fn click(t: &mut T, id: &str) {
    t.d.click(id).unwrap_or_else(|e| panic!("{id}: {e}"));
    t.d.settle(FRAMES).unwrap();
}

fn right_click(t: &mut T, id: &str) {
    t.d.click_with(id, &Click { button: PointerButton::Secondary, ..Click::default() }).unwrap();
    t.d.settle(FRAMES).unwrap();
}

fn has(t: &T, id: &str) -> bool {
    try_widget(&t.d, id).is_some()
}

fn row_id(p: &PersonId) -> String {
    format!("tree:person:{p}")
}

fn section_of(p: &PersonId) -> Section {
    Section::Album(AlbumSection::Person(PersonKey::of(p)))
}

/// The People group's rows, top to bottom: (id, label).
fn people_rows(t: &T) -> Vec<(String, String)> {
    let mut rows: Vec<_> = t.d.widgets().into_iter().filter(|w| w.id.starts_with("tree:person:")).collect();
    rows.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]));
    rows.into_iter().map(|w| (w.id, w.label)).collect()
}

/// Who a face is: `Some(person)`, `None` while unnamed.
fn person_of(t: &T, f: FaceRef) -> Option<PersonId> {
    let lib = t.d.app().session().unwrap().library();
    lib.face(dalica_library::FaceRef { photo: f.photo, index: usize::from(f.index) }).unwrap().person.person().cloned()
}

/// The photos of `p`'s section as the light table shows them.
fn section_photos(t: &T, p: &PersonId) -> Option<Vec<PhotoId>> {
    t.d.app().panels().unwrap().sections.get(section_of(p)).map(|s| s.photos.clone())
}

fn name_of(t: &T, p: &PersonId) -> Option<String> {
    t.d.app().session().unwrap().library().person(p).map(|x| x.name.clone())
}

/// Ana's first face, on photo 1.
fn face_of(t: &T, i: usize) -> FaceRef {
    FaceRef { photo: t.photos[i], index: 0 }
}

#[test]
fn the_people_group_lists_unnamed_and_people_by_name_and_by_count() {
    let mut t = setup(LARGE.0, LARGE.1);
    assert_eq!(widget(&t.d, "tree:group:people").label, "People (2)");
    assert!(widget(&t.d, "tree:group:people").rect[1] < widget(&t.d, "tree:group:folders").rect[1], "People comes before Folders");
    let (ana, rui) = (row_id(&t.ana), row_id(&t.rui));
    let by_name =
        vec![("tree:person:unnamed".to_string(), "Unnamed (2)".to_string()), (ana.clone(), "Ana (2)".into()), (rui.clone(), "Rui (3)".into())];
    assert_eq!(people_rows(&t), by_name);
    // The person rows are 28 high, their faces 20.
    assert_eq!(widget(&t.d, &ana).rect[3], 28.0);
    run(&mut t, "view.peopleSort", json!({ "by": "count" }));
    let by_count: Vec<String> = people_rows(&t).into_iter().map(|(id, _)| id).collect();
    assert_eq!(by_count, ["tree:person:unnamed".to_string(), rui.clone(), ana.clone()]);
    // The sections follow the tree: Rui's before Ana's.
    let order: Vec<Section> = t.d.app().panels().unwrap().sections.list.iter().map(|s| s.id).filter(|s| s.person().is_some()).collect();
    assert_eq!(order, [section_of(&t.rui), section_of(&t.ana)]);
    // Unnamed shows the Unnamed grid; a folder's row puts the light table back.
    click(&mut t, "tree:person:unnamed");
    assert!(t.d.state().unnamed.open && has(&t, "button:faces.expandGroups"));
    assert!(widget(&t.d, "tree:person:unnamed").active);
    let folder = t.d.widgets().into_iter().find(|w| w.id.starts_with("tree:folder:")).unwrap().id;
    click(&mut t, &folder);
    assert!(!t.d.state().unnamed.open && !has(&t, "button:faces.expandGroups"));
}

#[test]
fn dragging_two_unnamed_faces_onto_ana_names_them_as_one_step() {
    let mut t = setup(LARGE.0, LARGE.1);
    click(&mut t, "tree:person:unnamed");
    let [a, b] = [t.unnamed[0], t.unnamed[1]];
    click(&mut t, &a.widget());
    t.d.click_with(&b.widget(), &Click { modifiers: Modifiers::COMMAND, ..Click::default() }).unwrap();
    assert_eq!(t.d.state().unnamed.selection.len(), 2);
    t.d.drag(&b.widget(), Target::Widget(&row_id(&t.ana)), &Drag::default()).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!((person_of(&t, a), person_of(&t, b)), (Some(t.ana.clone()), Some(t.ana.clone())), "both are Ana's");
    assert_eq!(widget(&t.d, &row_id(&t.ana)).label, "Ana (3)");
    assert_eq!(widget(&t.d, "tree:person:unnamed").label, "Unnamed (0)");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!((person_of(&t, a), person_of(&t, b)), (None, None), "one undo step");
    assert_eq!(widget(&t.d, "tree:person:unnamed").label, "Unnamed (2)");
}

#[test]
fn anas_section_shows_her_photos_and_zoom_to_faces_her_faces() {
    let mut t = setup(LARGE.0, LARGE.1);
    let ana = t.ana.clone();
    click(&mut t, &row_id(&ana));
    let sel = t.d.state().selection.clone();
    assert_eq!((sel.section, sel.whole), (Some(section_of(&ana)), true), "her section is selected as a unit");
    assert!(widget(&t.d, &row_id(&ana)).active);
    assert_eq!(section_photos(&t, &ana), Some(t.photos[..2].to_vec()));
    let header = format!("section:person:{ana}");
    assert_eq!(widget(&t.d, &header).label, "Ana");
    assert!(has(&t, &format!("thumb:{}/in/{ana}", t.photos[0].get())));
    assert!(!has(&t, &face_of(&t, 0).widget()), "whole photos");
    // Zoom to Faces: her faces in the photos' place.
    click(&mut t, &format!("{header}/zoomToFaces"));
    assert_eq!(t.d.state().people.zoom_to_faces, std::slice::from_ref(&ana));
    assert!(widget(&t.d, &format!("{header}/zoomToFaces")).active);
    assert!(has(&t, &face_of(&t, 0).widget()) && has(&t, &face_of(&t, 1).widget()));
    // A click on a face selects its photo; Set as Person Thumbnail takes that face.
    let second = face_of(&t, 1).widget();
    click(&mut t, &second);
    assert_eq!((t.d.state().selection.ids.clone(), t.d.state().selection.section), (vec![t.photos[1]], Some(section_of(&ana))));
    click(&mut t, &format!("{header}/thumbnail"));
    let thumb = t.d.app().session().unwrap().library().person_thumbnail(&ana).unwrap();
    assert_eq!(thumb.photo, t.photos[1]);
    // The face menu: Remove from Person sends the face back to Unnamed, one undo step.
    let first = face_of(&t, 0).widget();
    right_click(&mut t, &first);
    assert!(widget(&t.d, "menuitem:faces.removeFromPerson").enabled);
    assert!(has(&t, "menuitem:people.thumbnail"));
    click(&mut t, "menuitem:faces.removeFromPerson");
    assert_eq!(person_of(&t, face_of(&t, 0)), None);
    assert_eq!(section_photos(&t, &ana), Some(vec![t.photos[1]]));
    assert_eq!(widget(&t.d, "tree:person:unnamed").label, "Unnamed (3)");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!(person_of(&t, face_of(&t, 0)), Some(ana.clone()));
    // Delete in her section does the same for the photos selected, and deletes no file.
    click(&mut t, &first);
    t.d.key("Delete").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(t.d.state().dialogs.is_empty(), "no Delete from Disk question");
    assert_eq!(person_of(&t, face_of(&t, 0)), None);
    run(&mut t, "edit.undo", json!({}));
    assert_eq!(person_of(&t, face_of(&t, 0)), Some(ana.clone()));
    // Show Whole Photos again.
    click(&mut t, &format!("{header}/zoomToFaces"));
    assert!(t.d.state().people.zoom_to_faces.is_empty() && !has(&t, &face_of(&t, 0).widget()));
}

#[test]
fn rename_merge_and_delete_person_each_undo_in_one_step() {
    let mut t = setup(LARGE.0, LARGE.1);
    let (ana, rui) = (t.ana.clone(), t.rui.clone());
    // Rename…: the name and nickname start as they are.
    right_click(&mut t, &row_id(&ana));
    click(&mut t, "menuitem:people.rename");
    let d = t.d.state().dialogs.last().cloned().expect("Rename Person");
    assert_eq!((d.kind, d.fields["name"].clone()), (DialogKind::RenamePerson, json!("Ana")));
    t.d.app_mut().state_mut().dialogs.last_mut().unwrap().fields.insert("name".into(), json!("Ana Maria"));
    click(&mut t, "button:dialog.ok");
    assert_eq!(name_of(&t, &ana).as_deref(), Some("Ana Maria"));
    assert_eq!(widget(&t.d, &row_id(&ana)).label, "Ana Maria (2)");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!(name_of(&t, &ana).as_deref(), Some("Ana"));
    // Rui's name asks to merge instead.
    right_click(&mut t, &row_id(&ana));
    click(&mut t, "menuitem:people.rename");
    t.d.app_mut().state_mut().dialogs.last_mut().unwrap().fields.insert("name".into(), json!("rui"));
    click(&mut t, "button:dialog.ok");
    let d = t.d.state().dialogs.last().cloned().expect("the merge question");
    assert_eq!((d.kind, d.fields["into"].clone()), (DialogKind::MergePeople, json!(rui)));
    assert_eq!(t.d.state().dialogs.len(), 1, "in Rename's place");
    t.d.key("Escape").unwrap();
    t.d.settle(FRAMES).unwrap();
    assert_eq!(name_of(&t, &ana).as_deref(), Some("Ana"), "nothing changed");
    // Merge Into ▸ Rui: Ana's faces become Rui's; one undo brings her back with them.
    right_click(&mut t, &row_id(&ana));
    t.d.hover("submenu:Person/Merge Into", 300).unwrap();
    t.d.settle(FRAMES).unwrap();
    assert!(!has(&t, &format!("menuitem:people.merge/{ana}")), "not into herself");
    click(&mut t, &format!("menuitem:people.merge/{rui}"));
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::MergePeople));
    click(&mut t, "button:dialog.merge");
    assert!(name_of(&t, &ana).is_none() && !has(&t, &row_id(&ana)));
    assert_eq!(person_of(&t, face_of(&t, 0)), Some(rui.clone()));
    assert_eq!(widget(&t.d, &row_id(&rui)).label, "Rui (5)");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!((name_of(&t, &ana).as_deref(), person_of(&t, face_of(&t, 0))), (Some("Ana"), Some(ana.clone())));
    // Delete Person asks, then sends her faces back to Unnamed; one undo brings her back.
    right_click(&mut t, &row_id(&ana));
    click(&mut t, "menuitem:people.delete");
    assert_eq!(t.d.state().dialogs.last().map(|d| d.kind), Some(DialogKind::DeletePerson));
    assert!(name_of(&t, &ana).is_some(), "nothing before the answer");
    click(&mut t, "button:dialog.delete");
    assert!(name_of(&t, &ana).is_none() && person_of(&t, face_of(&t, 0)).is_none());
    assert_eq!(widget(&t.d, "tree:person:unnamed").label, "Unnamed (4)");
    run(&mut t, "edit.undo", json!({}));
    assert_eq!((name_of(&t, &ana).as_deref(), person_of(&t, face_of(&t, 1))), (Some("Ana"), Some(ana.clone())));
}

#[test]
fn play_starts_a_slideshow_of_her_photos() {
    let mut t = setup(LARGE.0, LARGE.1);
    let ana = t.ana.clone();
    click(&mut t, &row_id(&ana));
    let collage = widget(&t.d, &format!("section:person:{ana}/collage"));
    assert!(!collage.enabled, "Collage arrives with Collage");
    t.d.click(&format!("section:person:{ana}/play")).unwrap();
    t.d.step();
    assert_eq!(t.d.state().view, View::Slideshow);
    assert_eq!(t.d.state().slideshow.as_ref().map(|s| s.photos.clone()), Some(t.photos[..2].to_vec()));
}

/// Waits until every face drawn has its picture.
fn wait_for_faces(t: &mut T) {
    for _ in 0..200 {
        t.d.settle(FRAMES).unwrap();
        let s = t.d.app_mut().session_mut().unwrap();
        if s.face_tiles().pending() == 0 {
            t.d.settle(FRAMES).unwrap();
            if t.d.app().session().unwrap().face_tiles().pending() == 0 {
                return;
            }
        }
        t.d.app_mut().session_mut().unwrap().wait_face_tiles(Duration::from_secs(5));
    }
    panic!("the face pictures never arrived");
}

#[test]
fn the_tree_and_a_person_section_match_their_fingerprints() {
    let mut t = setup(LARGE.0, LARGE.1);
    let ana = t.ana.clone();
    click(&mut t, &row_id(&ana));
    wait_for_faces(&mut t);
    check_sizes(&mut t.d, "region:tree", "people-tree");
    wait_for_faces(&mut t);
    shot(&t.d, "person-section-whole-1400x900");
    click(&mut t, &format!("section:person:{ana}/zoomToFaces"));
    wait_for_faces(&mut t);
    check_region(&t.d, "region:lightTable", "person-section-1400x900");
    // Screenshots for the review: the person menu, and both at 900 × 600.
    right_click(&mut t, &row_id(&ana));
    t.d.hover("submenu:Person/Merge Into", 300).unwrap();
    t.d.settle(FRAMES).unwrap();
    shot(&t.d, "person-menu-1400x900");
    t.d.key("Escape").unwrap();
    t.d.key("Escape").unwrap();
    t.d.resize(900, 600);
    wait_for_faces(&mut t);
    shot(&t.d, "person-section-900x600");
    right_click(&mut t, &row_id(&ana));
    shot(&t.d, "person-menu-900x600");
}
