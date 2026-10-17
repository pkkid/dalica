//! The preview's two panes (`roadmap.md` M11.10; `editing.md` §4) on temporary libraries of
//! `scenes` photos: two photos open in two panes render both; a change to one recipe renders only
//! its pane again (a step of the photo, or of A│A's variant in the pane showing it); closing a pane
//! cancels its jobs and leaves the other; the unedited pane equals the decoded source; graceful
//! failures.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dalica_core::{PhotoId, SystemClock};
use dalica_edit::{Recipe, Region, Target};
use dalica_engine::preview::PaneShows;
use dalica_engine::{CacheDir, Options, PreviewError, Quality, Session, ThumbOptions, UpdateKind};
use dalica_fs::Expect;
use dalica_image::Rgba8;
use serde_json::json;

const W: u32 = 640;
const H: u32 = 480;
const WAIT: Duration = Duration::from_secs(60);
const BIG: (u32, u32) = (4000, 4000);

fn photo_path(folder: &Path, i: usize) -> PathBuf {
    folder.join(format!("IMG_{:04}.jpg", i + 1))
}

/// A library watching `Porto` with `n` photos of `W × H`; ids in name order.
fn setup(n: usize) -> (tempfile::TempDir, PathBuf, Session, Vec<PhotoId>) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Porto");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        let bytes = dalica_scenes::jpeg(&dalica_scenes::Scene::new(i as u64 + 11, W, H), 85, None).unwrap();
        dalica_fs::replace(&photo_path(&folder, i), &bytes, Expect::Absent).unwrap();
    }
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 2, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    (dir, folder, s, ids.into_iter().map(|(_, id)| id).collect())
}

fn apply(s: &mut Session, id: PhotoId, op: &str) {
    s.execute("photo.history.apply", &json!({ "id": id, "op": op })).unwrap();
}

/// The photo decoded whole, upright (the preview's source at this size).
fn decoded(folder: &Path, i: usize) -> Rgba8 {
    let bytes = std::fs::read(photo_path(folder, i)).unwrap();
    dalica_image::decode(&bytes, &dalica_image::Limits::default()).unwrap()
}

fn strings(texts: &[&str]) -> Vec<String> {
    texts.iter().map(|t| t.to_string()).collect()
}

/// The renderer's picture of photo `i` with `texts`, at full scale.
fn rendered(folder: &Path, i: usize, texts: &[&str]) -> Rgba8 {
    let source = decoded(folder, i);
    dalica_edit::render(dalica_edit::Source::upright(&source), &Recipe::parse(texts).unwrap(), Target::Full).unwrap()
}

/// Two photos, one per pane, both render: each pane shows its photo with its recipe, the
/// renderer's output; A│A shows one photo twice, the second pane its variant.
#[test]
fn two_photos_open_in_two_panes_render_both() {
    let (_dir, folder, mut s, ids) = setup(2);
    apply(&mut s, ids[0], "rotate(90)");
    apply(&mut s, ids[1], "sepia()");
    s.open_preview_in(0, ids[0], BIG).unwrap();
    s.open_preview_in(1, ids[1], BIG).unwrap();
    assert!(s.preview_busy() > 0, "both panes read their photos");
    assert!(s.wait_preview(WAIT));
    let (left, right) = (s.preview_in(0).unwrap().clone(), s.preview_in(1).unwrap().clone());
    assert_eq!((left.photo, right.photo), (ids[0], ids[1]));
    assert_eq!((left.quality, right.quality), (Quality::Final, Quality::Final));
    assert_eq!((left.recipe.clone(), right.recipe.clone()), (strings(&["rotate(90)"]), strings(&["sepia()"])));
    assert!(*left.image == rendered(&folder, 0, &["rotate(90)"]), "pane 0 is the renderer's output");
    assert!(*right.image == rendered(&folder, 1, &["sepia()"]), "pane 1 is the renderer's output");
    assert_eq!(s.preview().map(|p| p.photo), Some(ids[0]), "the calls without a pane are pane 0's");
    let updates = s.take_preview_updates();
    assert!(updates.iter().any(|u| u.pane == 0 && u.photo == ids[0] && u.kind == UpdateKind::Final));
    assert!(updates.iter().any(|u| u.pane == 1 && u.photo == ids[1] && u.kind == UpdateKind::Final));
    assert_eq!(s.preview_busy(), 0);

    // A│A: the same photo in both panes, the second showing the variant.
    s.execute("edit.variant.open", &json!({ "id": ids[0] })).unwrap();
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "bw()", "variant": true })).unwrap();
    s.open_preview_in(1, ids[0], BIG).unwrap();
    s.set_preview_shows(1, PaneShows::Variant).unwrap();
    s.set_preview_focus(1).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_in(0).unwrap().recipe, strings(&["rotate(90)"]));
    assert_eq!(s.preview_in(1).unwrap().recipe, strings(&["rotate(90)", "bw()"]));
    assert!(*s.preview_in(1).unwrap().image == rendered(&folder, 0, &["rotate(90)", "bw()"]));
    assert_eq!(s.preview_stats().failures, 0);
}

/// A change to one pane's recipe renders that pane again and leaves the other's preview as it was:
/// a step of the photo in pane 1, a candidate in pane 0, and a step of the variant on A│A.
#[test]
fn a_change_to_one_recipe_renders_only_its_pane_again() {
    let (_dir, _folder, mut s, ids) = setup(2);
    s.open_preview_in(0, ids[0], (320, 320)).unwrap();
    s.open_preview_in(1, ids[1], (320, 320)).unwrap();
    assert!(s.wait_preview(WAIT));
    s.take_preview_updates();
    let left = s.preview_in(0).unwrap().generation;

    apply(&mut s, ids[1], "rotate(90)");
    assert!(s.wait_preview(WAIT));
    let updates = s.take_preview_updates();
    assert!(!updates.is_empty() && updates.iter().all(|u| u.pane == 1), "{updates:?}");
    assert_eq!(s.preview_in(1).unwrap().recipe, strings(&["rotate(90)"]));
    assert_eq!(s.preview_in(0).unwrap().generation, left, "pane 0 was not rendered again");

    let right = s.preview_in(1).unwrap().generation;
    s.set_preview_candidate_in(0, Some(&strings(&["tuning(0.4,0,0,0,none)"]))).unwrap();
    assert!(s.wait_preview(WAIT));
    assert!(s.take_preview_updates().iter().all(|u| u.pane == 0));
    assert_eq!(s.preview_in(1).unwrap().generation, right);
    s.set_preview_candidate_in(0, None).unwrap();
    assert!(s.wait_preview(WAIT));

    // A│A: a step of the variant renders the variant's pane only, a step of the photo the photo's.
    s.execute("edit.variant.open", &json!({ "id": ids[0] })).unwrap();
    s.open_preview_in(1, ids[0], (320, 320)).unwrap();
    s.set_preview_shows(1, PaneShows::Variant).unwrap();
    assert!(s.wait_preview(WAIT));
    s.take_preview_updates();
    let left = s.preview_in(0).unwrap().generation;
    s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "sepia()", "variant": true })).unwrap();
    assert!(s.wait_preview(WAIT));
    let updates = s.take_preview_updates();
    assert!(!updates.is_empty() && updates.iter().all(|u| u.pane == 1), "{updates:?}");
    assert_eq!(s.preview_in(1).unwrap().recipe, strings(&["sepia()"]));
    assert_eq!(s.preview_in(0).unwrap().generation, left);
    let right = s.preview_in(1).unwrap().generation;
    apply(&mut s, ids[0], "flip_h()");
    assert!(s.wait_preview(WAIT));
    assert!(s.take_preview_updates().iter().all(|u| u.pane == 0));
    assert_eq!(s.preview_in(0).unwrap().recipe, strings(&["flip_h()"]));
    assert_eq!(s.preview_in(1).unwrap().generation, right);
}

/// Closing a pane cancels its jobs: nothing of it is shown afterwards, and the other pane renders
/// on. Two panes on one photo share its source: closing one leaves the other's.
#[test]
fn closing_a_pane_cancels_its_jobs() {
    let (_dir, _folder, mut s, ids) = setup(3);
    s.open_preview_in(0, ids[0], (640, 640)).unwrap();
    s.open_preview_in(1, ids[1], (640, 640)).unwrap();
    s.set_preview_window_in(1, Some(Region { scale: 1.0, x: 0, y: 0, width: 64, height: 64 })).unwrap();
    s.close_preview_in(1);
    assert_eq!(s.preview_photo_in(1), None);
    assert_eq!(s.preview_photo_in(0), Some(ids[0]));
    assert!(s.wait_preview(WAIT));
    assert!(s.preview_in(1).is_none() && s.preview_refined_in(1).is_none());
    let updates = s.take_preview_updates();
    assert!(updates.iter().all(|u| u.pane == 0), "nothing of the closed pane is shown: {updates:?}");
    assert!(s.preview_in(0).is_some(), "the other pane rendered on");
    let stats = s.preview_stats();
    assert!(stats.cancelled + stats.stale >= 1, "{stats:?}");
    assert_eq!(s.preview_pending(), 0);

    // One photo in both panes: closing pane 0 while its source is read leaves pane 1's.
    s.close_preview();
    assert!(s.preview_photo_in(0).is_none() && s.preview_photo_in(1).is_none(), "close_preview closes both");
    s.open_preview_in(0, ids[2], (64, 64)).unwrap();
    s.open_preview_in(1, ids[2], (64, 64)).unwrap();
    s.close_preview_in(0);
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_in(1).map(|p| p.photo), Some(ids[2]));
    assert!(s.preview_in(0).is_none());
}

/// A pane showing its photo unedited (before/after) shows the decoded source, whatever the recipe
/// or a candidate; showing the edits again renders the recipe.
#[test]
fn the_unedited_pane_equals_the_decoded_source() {
    let (_dir, folder, mut s, ids) = setup(1);
    apply(&mut s, ids[0], "rotate(90)");
    apply(&mut s, ids[0], "tuning(0.3,0,0,0.2,none)");
    s.open_preview_in(0, ids[0], BIG).unwrap();
    s.open_preview_in(1, ids[0], BIG).unwrap();
    s.set_preview_shows(1, PaneShows::Unedited).unwrap();
    assert_eq!(s.preview_shows(1), Some(PaneShows::Unedited));
    assert!(s.wait_preview(WAIT));
    let unedited = s.preview_in(1).unwrap();
    assert!(unedited.recipe.is_empty());
    assert!(*unedited.image == decoded(&folder, 0), "the unedited pane is the decoded source");
    assert!(*s.preview_in(0).unwrap().image == rendered(&folder, 0, &["rotate(90)", "tuning(0.3,0,0,0.2,none)"]));
    // A candidate does not show through.
    s.set_preview_candidate_in(1, Some(&strings(&["sepia()"]))).unwrap();
    assert!(s.wait_preview(WAIT));
    assert!(*s.preview_in(1).unwrap().image == decoded(&folder, 0));
    // A step of the photo leaves it unedited too.
    apply(&mut s, ids[0], "bw()");
    assert!(s.wait_preview(WAIT));
    assert!(s.preview_in(1).unwrap().recipe.is_empty());
    // Showing the edits again renders the candidate, then without it the recipe.
    s.set_preview_shows(1, PaneShows::Edited).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_in(1).unwrap().recipe, strings(&["sepia()"]));
    s.set_preview_candidate_in(1, None).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_in(1).unwrap().recipe, strings(&["rotate(90)", "tuning(0.3,0,0,0.2,none)", "bw()"]));
    // A variant pane without a variant shows the photo's own recipe.
    s.set_preview_shows(1, PaneShows::Variant).unwrap();
    assert!(s.wait_preview(WAIT));
    assert_eq!(s.preview_in(1).unwrap().recipe, strings(&["rotate(90)", "tuning(0.3,0,0,0.2,none)", "bw()"]));
}

/// Graceful failures: a pane that is not one, a pane that is not open, and hostile values in the
/// second pane are errors, never panics.
#[test]
fn graceful_failures() {
    let (_dir, _folder, mut s, ids) = setup(1);
    for pane in [2, usize::MAX] {
        assert_eq!(s.open_preview_in(pane, ids[0], (10, 10)), Err(PreviewError::BadPane(pane)));
        assert_eq!(s.set_preview_fit_in(pane, (10, 10)), Err(PreviewError::BadPane(pane)));
        assert_eq!(s.set_preview_candidate_in(pane, None), Err(PreviewError::BadPane(pane)));
        assert_eq!(s.set_preview_window_in(pane, None), Err(PreviewError::BadPane(pane)));
        assert_eq!(s.set_preview_shows(pane, PaneShows::Unedited), Err(PreviewError::BadPane(pane)));
        assert_eq!(s.set_preview_focus(pane), Err(PreviewError::BadPane(pane)));
        s.close_preview_in(pane);
        assert!(s.preview_in(pane).is_none() && s.preview_photo_in(pane).is_none() && s.preview_shows(pane).is_none());
    }
    assert_eq!(s.preview_focus(), 0);
    assert_eq!(s.set_preview_fit_in(1, (10, 10)), Err(PreviewError::NotOpen));
    assert_eq!(s.set_preview_candidate_in(1, None), Err(PreviewError::NotOpen));
    assert_eq!(s.set_preview_window_in(1, None), Err(PreviewError::NotOpen));
    assert_eq!(s.set_preview_shows(1, PaneShows::Variant), Err(PreviewError::NotOpen));
    assert_eq!(s.open_preview_in(1, PhotoId::new(999_999), (10, 10)), Err(PreviewError::NoPhoto));
    s.open_preview_in(1, ids[0], (0, u32::MAX)).unwrap();
    let too_many: Vec<String> = (0..=dalica_edit::MAX_OPERATIONS).map(|_| "rotate(90)".to_string()).collect();
    assert!(matches!(s.set_preview_candidate_in(1, Some(&too_many)), Err(PreviewError::BadRecipe(_))));
    assert!(matches!(s.set_preview_window_in(1, Some(Region { scale: f64::NAN, x: 0, y: 0, width: 1, height: 1 })), Err(PreviewError::BadWindow(_))));
    assert!(s.wait_preview(WAIT));
    assert!(s.preview_in(1).is_some());
    assert!(s.preview_in(0).is_none(), "pane 0 was never opened");
}
