//! Colour summaries in the model (M16, `organizing.md` §8.1): set, replaced and cleared by a
//! derived op that moves no revision and round-trips through the journal's JSON as bytes, dropped
//! with a removed photo and put back by the removal's inverse.

use std::path::PathBuf;

use dalica_core::color_summary::{ColourSummary, SearchColour};
use dalica_core::{FolderId, PhotoId};
use dalica_library::{Error, Folder, Library, Op};

use crate::common::photo;

const TRIP: FolderId = FolderId::new(1);
const A: PhotoId = PhotoId::new(1);
const B: PhotoId = PhotoId::new(2);

fn summary(main: SearchColour, others: &[SearchColour]) -> ColourSummary {
    ColourSummary::new(main, others).unwrap()
}

fn fixture() -> Library {
    let mut lib = Library::new();
    lib.apply(Op::AddFolder { folder: Folder { id: TRIP, path: PathBuf::from("/p/Trip") } }).unwrap();
    lib.apply(Op::AddPhotos { photos: vec![photo(A, TRIP, "a.jpg"), photo(B, TRIP, "b.jpg")] }).unwrap();
    lib
}

/// Applies `op`, checks it is derived and moved neither revision, and returns its inverse.
fn derived(lib: &mut Library, op: Op) -> Op {
    let (rev, thumbs) = (lib.revision(), lib.thumbnail_revision());
    assert!(op.is_derived());
    let inverse = lib.apply(op).unwrap();
    assert!(inverse.is_derived());
    assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev, thumbs), "a summary moves no revision");
    inverse
}

#[test]
fn a_summary_is_set_replaced_and_cleared_moving_no_revision() {
    let mut lib = fixture();
    assert!(!lib.has_colours());
    assert_eq!(lib.colours(A), None);
    let red_blue = summary(SearchColour::Red, &[SearchColour::Blue]);
    let empty = lib.model().clone();
    let undo_set = derived(&mut lib, Op::SetColours { colours: vec![(A, Some(red_blue))] });
    assert_eq!(lib.colours(A), Some(red_blue));
    assert_eq!(lib.colours(B), None);
    assert!(lib.has_colours());
    assert_eq!(undo_set, Op::SetColours { colours: vec![(A, None)] });

    let set = lib.model().clone();
    let bw = summary(SearchColour::Bw, &[]);
    let undo_replace = derived(&mut lib, Op::SetColours { colours: vec![(A, Some(bw)), (B, Some(ColourSummary::EMPTY))] });
    assert_eq!((lib.colours(A), lib.colours(B)), (Some(bw), Some(ColourSummary::EMPTY)));
    assert_eq!(lib.colours_all().collect::<Vec<_>>(), vec![(A, bw), (B, ColourSummary::EMPTY)]);
    derived(&mut lib, undo_replace);
    assert_eq!(lib.model(), &set);

    derived(&mut lib, Op::SetColours { colours: vec![(A, None)] });
    assert_eq!(lib.colours(A), None);
    assert!(!lib.has_colours());
    assert_eq!(lib.model(), &empty);

    // An unknown photo is refused, changing nothing.
    let before = lib.model().clone();
    assert_eq!(lib.apply(Op::SetColours { colours: vec![(A, Some(bw)), (PhotoId::new(99), None)] }), Err(Error::UnknownPhoto(PhotoId::new(99))));
    assert_eq!(lib.model(), &before);
}

#[test]
fn the_journal_holds_each_summary_as_its_byte() {
    let green = summary(SearchColour::Green, &[SearchColour::Yellow]);
    let op = Op::SetColours { colours: vec![(A, Some(green)), (B, None)] };
    let json = serde_json::to_value(&op).unwrap();
    assert_eq!(json, serde_json::json!({"op": "setColours", "colours": [[1, green.to_byte()], [2, null]]}));
    assert_eq!(serde_json::from_value::<Op>(json).unwrap(), op);
    // A byte the form does not know is refused (bits for other hues beside `bw`).
    let bad = serde_json::json!({"op": "setColours", "colours": [[1, 0b1000_0111]]});
    assert!(serde_json::from_value::<Op>(bad).is_err());
}

#[test]
fn a_removed_photo_takes_its_summary_and_its_undo_puts_it_back() {
    let mut lib = fixture();
    let blue = summary(SearchColour::Blue, &[]);
    derived(&mut lib, Op::SetColours { colours: vec![(A, Some(blue)), (B, Some(blue))] });
    let before = lib.model().clone();
    let undo = lib.apply(Op::RemovePhotos { ids: vec![A] }).unwrap();
    assert_eq!((lib.colours(A), lib.colours(B)), (None, Some(blue)));
    lib.apply(undo).unwrap();
    assert_eq!(lib.colours(A), Some(blue));
    assert_eq!(lib.model(), &before);
    // And with its folder.
    let undo = lib.apply(Op::RemoveFolder { id: TRIP }).unwrap();
    assert!(!lib.has_colours());
    lib.apply(undo).unwrap();
    assert_eq!(lib.model(), &before);
}
