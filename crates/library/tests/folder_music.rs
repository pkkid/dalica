//! M16's folder field (`data-model.md` §3, `viewing.md` §1.5): a folder's own slideshow music, the
//! folder of music files its slideshows play. It is set, replaced and cleared through
//! `Op::SetFolderInfo` with the folder's other fields, one step each with its exact inverse; the
//! empty value is not stored; `clean_music` brings any text inside the cap and an op past it is
//! refused.

use std::path::PathBuf;

use dalica_core::FolderId;
use dalica_library::{Error, Folder, FolderInfo, Library, MAX_MUSIC, Op, clean_music};

/// Two folders, nothing said about either.
fn library() -> (Library, [FolderId; 2]) {
    let mut lib = Library::new();
    let (a, b) = (lib.next_folder_id(), lib.next_folder_id());
    lib.apply(Op::AddFolder { folder: Folder { id: a, path: PathBuf::from("/photos/a") } }).unwrap();
    lib.apply(Op::AddFolder { folder: Folder { id: b, path: PathBuf::from("/photos/b") } }).unwrap();
    (lib, [a, b])
}

/// Sets folder `id`'s info to `info` with one op: one revision, only `id` touched, and the inverse
/// gives back exactly what was there. Leaves the op applied.
fn step(lib: &mut Library, id: FolderId, info: FolderInfo) {
    let (before, rev) = (lib.model().clone(), lib.revision());
    let inverse = lib.apply(Op::SetFolderInfo { folders: vec![(id, info.clone())] }).unwrap();
    assert_eq!(lib.revision(), rev + 1, "one step");
    assert_eq!(lib.folder_revision(id), lib.revision());
    let after = lib.model().clone();
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before, "undone");
    lib.apply(Op::SetFolderInfo { folders: vec![(id, info)] }).unwrap();
    assert_eq!(lib.model(), &after, "redone");
}

#[test]
fn music_is_set_replaced_and_cleared_one_step_each() {
    let (mut lib, [a, b]) = library();
    assert_eq!(lib.folder_info(a).music, "", "none by default");
    let place = FolderInfo { place: "Lisbon".into(), ..FolderInfo::default() };
    step(&mut lib, a, place.clone());

    let fado = FolderInfo { music: "/home/ana/Music/Fado".into(), ..place.clone() };
    step(&mut lib, a, fado.clone());
    assert_eq!(lib.folder_info(a), &fado, "set with the other fields kept");
    assert_eq!(lib.folder_info(b).music, "", "another folder's is its own");

    let jazz = FolderInfo { music: "D:\\Música\\Jazz".into(), ..place.clone() };
    step(&mut lib, a, jazz.clone());
    assert_eq!(lib.folder_info(a).music, "D:\\Música\\Jazz", "replaced");

    step(&mut lib, a, place.clone());
    assert_eq!(lib.folder_info(a), &place, "cleared");

    // A folder whose only word is its music keeps an info; clearing that leaves nothing stored.
    step(&mut lib, b, FolderInfo { music: "/music".into(), ..FolderInfo::default() });
    assert!(!lib.folder_info(b).is_empty());
    step(&mut lib, b, FolderInfo::default());
    assert_eq!(lib.folder_infos().map(|(id, _)| id).collect::<Vec<_>>(), [a], "the default is not stored");
}

#[test]
fn a_music_path_over_the_cap_is_cut_and_refused_uncut() {
    assert_eq!(clean_music("  /home/ana/Music  "), "/home/ana/Music", "trimmed");
    assert_eq!(clean_music("   "), "", "nothing left is none");
    // Cut at a character boundary: a two-byte character straddles the cap.
    let long = format!("/{}", "é".repeat(MAX_MUSIC));
    let cut = clean_music(&long);
    assert!(cut.len() <= MAX_MUSIC && cut.len() >= MAX_MUSIC - 1, "{}", cut.len());
    assert!(long.starts_with(&cut));

    let (mut lib, [a, _]) = library();
    let rev = lib.revision();
    let over = FolderInfo { music: long, ..FolderInfo::default() };
    assert!(matches!(lib.apply(Op::SetFolderInfo { folders: vec![(a, over)] }), Err(Error::TooLong { .. })));
    assert_eq!((lib.revision(), lib.folder_info(a).music.as_str()), (rev, ""), "a refused op changes nothing");
    lib.apply(Op::SetFolderInfo { folders: vec![(a, FolderInfo { music: cut.clone(), ..FolderInfo::default() })] }).unwrap();
    assert_eq!(lib.folder_info(a).music, cut);
}
