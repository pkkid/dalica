//! `dalica-library` M8 fields (`data-model.md` §6): a photo's saved record (index-only, so loading
//! one bumps no revision), the copy saved beside it, a folder's Exported mark; the ops of a Save,
//! an Undo Save, a Revert and a Save beside with their exact inverses; the unsaved-edit queries
//! (per photo, per folder, cached per revision), whether Undo Save and Revert apply, the
//! operations that cannot be rendered, and journal names (removal and restore: `ops.rs`).

use std::path::PathBuf;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Error, Folder, Library, MAX_DIGEST, MAX_FILE_NAME, MAX_OPERATIONS, Op, Project, Recipe, SaveState, Saved, SavedAs};
use serde_json::json;

use crate::common::photo;

/// Two folders, three photos in the first and one in the second.
fn library() -> (Library, [FolderId; 2], [PhotoId; 4]) {
    let mut lib = Library::new();
    let (a, b) = (lib.next_folder_id(), lib.next_folder_id());
    lib.apply(Op::AddFolder { folder: Folder { id: a, path: PathBuf::from("/photos/a") } }).unwrap();
    lib.apply(Op::AddFolder { folder: Folder { id: b, path: PathBuf::from("/photos/b") } }).unwrap();
    let ids = [lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id(), lib.next_photo_id()];
    lib.apply(Op::AddPhotos {
        photos: vec![photo(ids[0], a, "1.jpg"), photo(ids[1], a, "2.jpg"), photo(ids[2], a, "3.png"), photo(ids[3], b, "4.jpg")],
    })
    .unwrap();
    (lib, [a, b], ids)
}

fn ops(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn edit(lib: &mut Library, id: PhotoId, edits: &[&str], redo: &[&str]) {
    lib.apply(Op::SetRecipes { recipes: vec![(id, Recipe { edits: ops(edits), redo: ops(redo) })] }).unwrap();
}

/// A finished record of a save holding `recipe`.
fn done(recipe: &[&str]) -> Saved {
    Saved {
        recipe: ops(recipe),
        size: 123_456,
        mtime: Timestamp::from_millis(1_700_000_000_000),
        width: 4000,
        height: 3000,
        digest: "sha256:5e1f".into(),
        state: SaveState::Done,
        time: Some(Timestamp::from_millis(1_790_000_000_000)),
    }
}

/// Applies `op`, checks the revision moved by `step` and exactly `folders` were touched, undoes
/// it and checks the model is back, then applies it again; returns the inverse.
fn round_trip(lib: &mut Library, op: Op, step: u64, folders: &[FolderId]) -> Op {
    let before = lib.model().clone();
    let (rev, thumbs) = (lib.revision(), lib.thumbnail_revision());
    let folder_revs: Vec<(FolderId, u64)> = lib.folders().map(|f| (f.id, lib.folder_revision(f.id))).collect();
    let inverse = lib.apply(op.clone()).unwrap();
    assert_eq!(lib.revision(), rev + step, "{op:?}");
    assert_eq!(lib.thumbnail_revision(), thumbs, "{op:?}");
    assert_eq!(inverse.is_derived(), op.is_derived());
    for (f, r) in folder_revs {
        assert_eq!(lib.folder_revision(f) != r, folders.contains(&f), "{op:?} touched {f}");
    }
    let after = lib.model().clone();
    let redo = lib.apply(inverse.clone()).unwrap();
    assert_eq!(lib.model(), &before, "{op:?} then {inverse:?}");
    lib.apply(redo).unwrap();
    assert_eq!(lib.model(), &after, "redo of {op:?}");
    inverse
}

#[test]
fn a_new_photo_and_folder_have_no_m8_fields() {
    let (lib, [a, _], ids) = library();
    assert_eq!((lib.saved(ids[0]), lib.saved_as(ids[0]), lib.project(a)), (None, None, None));
    assert!(lib.saved_all().next().is_none() && lib.saved_as_all().next().is_none() && lib.projects().next().is_none());
    assert!(!lib.has_unsaved_edits(ids[0]) && !lib.can_undo_save(ids[0]) && !lib.can_revert(ids[0]));
    assert_eq!(lib.unsaved_count(a), 0);
    assert!(lib.unsaved_counts().is_empty() && lib.unsaved_in(a).is_empty());
    // Unknown ids read as nothing, never a panic.
    let (p, f) = (PhotoId::new(999), FolderId::new(999));
    assert!(lib.saved(p).is_none() && !lib.has_unsaved_edits(p) && !lib.can_undo_save(p) && lib.unrenderable(p, |_| false).is_none());
    assert_eq!((lib.unsaved_count(f), lib.unsaved_in(f), lib.project(f)), (0, vec![], None));
}

/// `data-model.md` §6: a saved record read from `.dalica-originals/.dalica.json` is index-only, so
/// loading it is derived: no revision, no folder revision, no undo step.
#[test]
fn loading_a_record_bumps_no_revision() {
    let (mut lib, _, ids) = library();
    let pending = Saved { state: SaveState::Pending, ..done(&["sepia()"]) };
    let op = Op::SetSaved { saved: vec![(ids[0], Some(done(&["rotate(90)"]))), (ids[1], Some(pending.clone()))] };
    assert!(op.is_derived());
    round_trip(&mut lib, op, 0, &[]);
    assert_eq!(lib.saved(ids[0]), Some(&done(&["rotate(90)"])));
    assert!(lib.can_undo_save(ids[0]) && lib.can_revert(ids[0]));
    assert!(!lib.can_undo_save(ids[1]) && !lib.can_revert(ids[1]), "a pending record is a save to repair, not one to undo");
    assert_eq!(lib.saved_all().map(|(id, _)| id).collect::<Vec<_>>(), [ids[0], ids[1]]);
    // Clearing a record is derived too.
    round_trip(&mut lib, Op::SetSaved { saved: vec![(ids[1], None)] }, 0, &[]);
    assert!(lib.saved(ids[1]).is_none());
}

/// The unsaved count of a folder follows edits, a Save, an Undo Save, a Revert and a recipe
/// change after a Save beside.
#[test]
fn the_unsaved_count_follows_edits_saves_and_undo_save() {
    let (mut lib, [a, b], ids) = library();
    edit(&mut lib, ids[0], &["rotate(90)"], &[]);
    edit(&mut lib, ids[1], &["sepia()", "crop(0.1,0.1,0.9,0.9)"], &["glow(0.4,3.0)"]);
    edit(&mut lib, ids[3], &["bw()"], &[]);
    assert_eq!((lib.unsaved_count(a), lib.unsaved_count(b)), (2, 1));
    assert_eq!(lib.unsaved_in(a), [ids[0], ids[1]]);
    // A redo list alone is not an unsaved edit.
    edit(&mut lib, ids[2], &[], &["sepia()"]);
    assert!(!lib.has_unsaved_edits(ids[2]));
    assert_eq!(lib.unsaved_count(a), 2);

    // Save: the record set, the recipe and redo list cleared, the count down; one undoable op.
    let save = lib.save_op(vec![(ids[1], done(&["sepia()", "crop(0.1,0.1,0.9,0.9)"]))]).unwrap();
    assert!(!save.is_derived());
    round_trip(&mut lib, save, 1, &[a]);
    assert_eq!(lib.info(ids[1]).recipe(), Recipe::default());
    assert!(!lib.has_unsaved_edits(ids[1]) && lib.can_undo_save(ids[1]));
    assert_eq!(lib.unsaved_count(a), 1);
    assert_eq!(lib.unsaved_in(a), [ids[0]]);

    // A new edit after the save: unsaved again, and Undo Save puts the recorded recipe in front
    // of it, keeping the redo list.
    edit(&mut lib, ids[1], &["bw()"], &["warmify()"]);
    assert_eq!(lib.unsaved_count(a), 2);
    let undo = lib.undo_save_op(&[ids[1], ids[1]]).unwrap();
    round_trip(&mut lib, undo, 1, &[a]);
    assert_eq!(lib.info(ids[1]).recipe(), Recipe { edits: ops(&["sepia()", "crop(0.1,0.1,0.9,0.9)", "bw()"]), redo: ops(&["warmify()"]) });
    assert!(lib.saved(ids[1]).is_none() && !lib.can_undo_save(ids[1]));
    assert_eq!(lib.unsaved_count(a), 2);

    // Save again, then Revert: the record, the recipe and the redo list all go.
    lib.apply(lib.save_op(vec![(ids[1], done(&["sepia()", "crop(0.1,0.1,0.9,0.9)", "bw()"]))]).unwrap()).unwrap();
    edit(&mut lib, ids[1], &["sharpen(0.5)"], &["glow(0.4,3.0)"]);
    let revert = lib.revert_op(&[ids[1]]).unwrap();
    round_trip(&mut lib, revert, 1, &[a]);
    assert_eq!(lib.info(ids[1]).recipe(), Recipe::default());
    assert!(lib.saved(ids[1]).is_none());
    assert_eq!(lib.unsaved_count(a), 1);

    // A Save beside (a PNG): the photo keeps its recipe and stops counting until it changes.
    edit(&mut lib, ids[2], &["sepia()"], &[]);
    assert_eq!(lib.unsaved_count(a), 2);
    let beside = lib.save_beside_op(ids[2], "3.jpg".into()).unwrap();
    round_trip(&mut lib, beside, 1, &[a]);
    assert_eq!(lib.saved_as(ids[2]), Some(&SavedAs { name: "3.jpg".into(), recipe: ops(&["sepia()"]) }));
    assert_eq!(lib.info(ids[2]).edits, ops(&["sepia()"]), "the source keeps its recipe");
    assert!(!lib.has_unsaved_edits(ids[2]));
    assert_eq!(lib.unsaved_count(a), 1);
    assert!(!lib.can_undo_save(ids[2]), "Undo Save and Revert are for saves in place");
    edit(&mut lib, ids[2], &["sepia()", "bw()"], &[]);
    assert!(lib.has_unsaved_edits(ids[2]));
    assert_eq!(lib.unsaved_count(a), 2);
    // Undoing that edit brings back the saved recipe: saved again.
    edit(&mut lib, ids[2], &["sepia()"], &["bw()"]);
    assert_eq!(lib.unsaved_count(a), 1);
    assert_eq!(lib.unsaved_count(b), 1, "the other folder never moved");
}

/// The per-folder counts are worked out once per revision: a derived op keeps them, any other op
/// works them out again.
#[test]
fn unsaved_counts_are_cached_per_revision() {
    let (mut lib, [a, _], ids) = library();
    edit(&mut lib, ids[0], &["sepia()"], &[]);
    let first: *const _ = lib.unsaved_counts();
    assert!(std::ptr::eq(first, lib.unsaved_counts()), "the same revision reuses the counts");
    lib.apply(Op::SetSaved { saved: vec![(ids[1], Some(done(&["bw()"])))] }).unwrap();
    assert!(std::ptr::eq(first, lib.unsaved_counts()), "a derived op keeps them");
    assert_eq!(lib.unsaved_count(a), 1);
    edit(&mut lib, ids[1], &["bw()"], &[]);
    assert_eq!(lib.unsaved_count(a), 2, "an edit is counted at once");
    // A clone carries its own counts, equal to the original's.
    let copy = lib.clone();
    assert_eq!(copy.unsaved_counts(), lib.unsaved_counts());
}

/// Undo Save and Revert refuse a photo without a finished save, and change nothing.
#[test]
fn undo_save_and_revert_need_a_finished_save() {
    let (mut lib, _, ids) = library();
    let pending = Saved { state: SaveState::Pending, ..done(&["sepia()"]) };
    lib.apply(Op::SetSaved { saved: vec![(ids[0], Some(done(&["bw()"]))), (ids[1], Some(pending))] }).unwrap();
    assert_eq!(lib.undo_save_op(&[ids[0], ids[2]]), Err(Error::NotSaved(ids[2])));
    assert_eq!(lib.revert_op(&[ids[1]]), Err(Error::NotSaved(ids[1])));
    assert_eq!(lib.undo_save_op(&[PhotoId::new(999)]), Err(Error::UnknownPhoto(PhotoId::new(999))));
    assert_eq!(lib.save_op(vec![(PhotoId::new(999), done(&[]))]), Err(Error::UnknownPhoto(PhotoId::new(999))));
    assert_eq!(lib.save_beside_op(PhotoId::new(999), "x.jpg".into()), Err(Error::UnknownPhoto(PhotoId::new(999))));
    // Nothing to undo is an op that changes nothing; a finished save reverts to the original.
    let before = (lib.model().clone(), lib.saved(ids[0]).cloned());
    let nothing = lib.undo_save_op(&[]).unwrap();
    lib.apply(nothing).unwrap();
    assert_eq!((lib.model().clone(), lib.saved(ids[0]).cloned()), before);
    edit(&mut lib, ids[0], &["sepia()"], &["glow()"]);
    let revert = lib.revert_op(&[ids[0], ids[0]]).unwrap();
    lib.apply(revert).unwrap();
    assert!(lib.saved(ids[0]).is_none() && lib.info(ids[0]).edits.is_empty() && lib.info(ids[0]).redo.is_empty());
}

/// A recipe of more operations than the cap after Undo Save puts the recorded one in front is
/// refused whole.
#[test]
fn undo_save_past_the_cap_is_refused() {
    let (mut lib, _, ids) = library();
    let long = vec!["sepia()"; MAX_OPERATIONS];
    lib.apply(Op::SetSaved { saved: vec![(ids[0], Some(done(&long)))] }).unwrap();
    edit(&mut lib, ids[0], &["bw()"], &[]);
    let before = (lib.model().clone(), lib.revision());
    let undo = lib.undo_save_op(&[ids[0]]).unwrap();
    assert!(matches!(lib.apply(undo), Err(Error::TooMany { .. })));
    assert_eq!((lib.model().clone(), lib.revision()), before);
}

/// Caps on what the new fields hold (`security.md` §4.4); a refused op changes nothing.
#[test]
fn hostile_values_are_refused() {
    let (mut lib, [a, _], ids) = library();
    let before = (lib.model().clone(), lib.revision());
    let cases = [
        Op::SetSaved { saved: vec![(ids[0], Some(Saved { digest: "x".repeat(MAX_DIGEST + 1), ..done(&[]) }))] },
        Op::SetSaved { saved: vec![(ids[0], Some(Saved { recipe: vec!["x".repeat(dalica_library::MAX_OPERATION + 1)], ..done(&[]) }))] },
        Op::SetSaved { saved: vec![(ids[0], Some(done(&[]))), (PhotoId::new(999), None)] },
        Op::SetSavedAs { saved_as: vec![(ids[0], Some(SavedAs { name: String::new(), recipe: vec![] }))] },
        Op::SetSavedAs { saved_as: vec![(ids[0], Some(SavedAs { name: "x".repeat(MAX_FILE_NAME + 1), recipe: vec![] }))] },
        Op::SetSavedAs { saved_as: vec![(PhotoId::new(999), None)] },
        Op::SetProjects { projects: vec![(a, Some(Project::Exported)), (FolderId::new(999), None)] },
    ];
    for op in cases {
        assert!(lib.apply(op.clone()).is_err(), "{op:?}");
        assert_eq!((lib.model().clone(), lib.revision()), before, "{op:?}");
    }
    assert!(matches!(lib.save_beside_op(ids[0], String::new()), Err(Error::BadText { .. })));
}

/// The Exported mark: an ordinary, undoable folder op that touches only its folder.
#[test]
fn the_exported_mark() {
    let (mut lib, [a, b], _) = library();
    round_trip(&mut lib, Op::SetProjects { projects: vec![(b, Some(Project::Exported))] }, 1, &[b]);
    assert_eq!(lib.project(b), Some(Project::Exported));
    assert_eq!(lib.projects().collect::<Vec<_>>(), [(b, Project::Exported)]);
    round_trip(&mut lib, Op::SetProjects { projects: vec![(b, None), (a, Some(Project::Exported))] }, 1, &[a, b]);
    assert_eq!(lib.projects().collect::<Vec<_>>(), [(a, Project::Exported)]);
}

/// `edit`'s parse decides what can be rendered; the library finds the first operation a Save would
/// render that cannot be: the recorded recipe first, then the recipe.
#[test]
fn the_first_operation_that_cannot_be_rendered() {
    let (mut lib, _, ids) = library();
    let renders = |op: &str| !op.starts_with("legacy:") && !op.starts_with("future_");
    edit(&mut lib, ids[0], &["rotate(90)", "legacy:x(2)", "future_op(1)"], &[]);
    assert_eq!(lib.unrenderable(ids[0], renders), Some("legacy:x(2)"));
    edit(&mut lib, ids[1], &["sepia()"], &["future_op(1)"]);
    assert_eq!(lib.unrenderable(ids[1], renders), None, "the redo list is not rendered");
    lib.apply(Op::SetSaved { saved: vec![(ids[1], Some(done(&["future_op(2)"])))] }).unwrap();
    assert_eq!(lib.unrenderable(ids[1], renders), Some("future_op(2)"));
    assert_eq!(lib.unrenderable(ids[2], |_| false), None, "no recipe, nothing to render");
}

/// The journal names and shapes of the new ops and fields (`data-model.md` §4).
#[test]
fn journal_names() {
    let (p, f) = (PhotoId::new(3), FolderId::new(2));
    let cases = [
        (
            Op::SetSaved { saved: vec![(p, Some(done(&["rotate(90)"]))), (p, None)] },
            json!({"op": "setSaved", "saved": [[3, {
                "recipe": ["rotate(90)"], "size": 123_456, "mtime": 1_700_000_000_000_i64, "width": 4000, "height": 3000,
                "digest": "sha256:5e1f", "state": "done", "time": 1_790_000_000_000_i64
            }], [3, null]]}),
        ),
        (
            Op::SetSavedAs { saved_as: vec![(p, Some(SavedAs { name: "3.jpg".into(), recipe: ops(&["sepia()"]) })), (p, None)] },
            json!({"op": "setSavedAs", "savedAs": [[3, {"name": "3.jpg", "recipe": ["sepia()"]}], [3, null]]}),
        ),
        (
            Op::SetProjects { projects: vec![(f, Some(Project::Exported)), (f, None)] },
            json!({"op": "setProjects", "projects": [[2, "exported"], [2, null]]}),
        ),
    ];
    for (op, want) in cases {
        assert_eq!(serde_json::to_value(&op).unwrap(), want);
        assert_eq!(serde_json::from_value::<Op>(want).unwrap(), op);
    }
    // Keys left out of a record read as their defaults (a pending record with no time).
    let lenient: Saved = serde_json::from_value(json!({"recipe": ["bw()"]})).unwrap();
    assert_eq!(lenient, Saved { recipe: ops(&["bw()"]), ..Saved::default() });
    assert_eq!(lenient.state, SaveState::Pending);
}
