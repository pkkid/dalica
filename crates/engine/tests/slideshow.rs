//! What a slideshow plays and its previews made ahead (`roadmap.md` M11.11; `viewing.md`
//! §1.1–§1.2): `slideshow.list` for a folder, an album (in album order), a search and the tray's
//! ids, each starting at the photo asked for, hidden photos only while shown and videos left out
//! and counted; a show whose every decode takes 1.5 s (the preview's planted decode delay, on a
//! virtual clock) on a 3 s display time finds the next photo's preview ready at each of 30 changes; a photo deleted from the list is skipped without a stall;
//! graceful failures.

use std::sync::Arc;
use std::time::Duration;

use dalica_core::{AlbumId, Clock, DateTime, FolderId, PhotoId, Timestamp, VirtualClock};
use dalica_create::slideshow::{Settings, State, Transition};
use dalica_engine::cmd::delete::RESTORES;
use dalica_engine::cmd::slideshow::{Frame, Slideshow};
use dalica_engine::watching::WatchMode;
use dalica_engine::{CacheDir, CmdError, Options, Session, ThumbOptions};
use dalica_fs::Expect;
use dalica_library::{AlbumInfo, Facts, Folder, Library, Op, Photo};
use serde_json::{Value, json};

use crate::common::{jpeg, shared, virtual_clock, wait_files};

const PATIENCE: Duration = Duration::from_secs(120);
/// The display the previews cover.
const DISPLAY: (u32, u32) = (1920, 1080);
/// One frame of the virtual clock.
const FRAME: Duration = Duration::from_millis(100);

fn ids(v: &[u64]) -> Vec<PhotoId> {
    v.iter().map(|n| PhotoId::new(*n)).collect()
}

// --- The lists ------------------------------------------------------------------------------------

fn photo(id: u64, name: &str, minute: u32) -> Photo {
    Photo {
        id: PhotoId::new(id),
        folder: FolderId::new(1),
        name: name.into(),
        size: 1_000 * id,
        mtime: Timestamp::from_millis(1_450_000_000_000),
        width: 4000,
        height: 3000,
        date: Some(format!("2026-06-04T09:{minute:02}:00").parse::<DateTime>().unwrap()),
        star: false,
    }
}

/// A watched root holding no files (never scanned): `Lisbon` with photos 1–5 taken a minute apart
/// in that order, 4 hidden, and the video 6; "harbour" in the captions of 2 and 5; the album
/// "Best" holding 5, 1, 3 in that order.
fn lists() -> (tempfile::TempDir, Session, AlbumId) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("photos");
    let p = PhotoId::new;
    let album = AlbumId::parse("a000000000001").unwrap();
    let ops = vec![
        Op::AddFolder { folder: Folder { id: FolderId::new(1), path: root.join("Lisbon") } },
        Op::AddPhotos {
            photos: vec![
                photo(1, "IMG_0001.jpg", 1),
                photo(2, "IMG_0002.jpg", 2),
                photo(3, "IMG_0003.jpg", 3),
                photo(4, "IMG_0004.jpg", 4),
                photo(5, "IMG_0005.jpg", 5),
                photo(6, "CLIP_0006.mp4", 6),
            ],
        },
        Op::SetFacts { facts: vec![(p(6), Facts { make: String::new(), model: String::new(), video: true, geotagged: false })] },
        Op::SetCaptions { captions: vec![(p(2), "The harbour at dawn".into()), (p(5), "harbour lights".into())] },
        Op::SetHidden { hidden: vec![(p(4), true)] },
        Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named("Best"))] },
    ];
    let mut library = Library::new();
    library.apply(Op::Batch { ops }).unwrap();
    let add = library.album_add_op(&album, &[p(5), p(1), p(3)], None).unwrap();
    library.apply(add).unwrap();
    let lib_dir = tmp.path().join("library");
    std::fs::create_dir_all(&lib_dir).unwrap();
    dalica_store::snapshot::write(&lib_dir.join(dalica_store::snapshot::FILE), &library, 0).unwrap();
    let roots = json!({ "version": 1, "folders": [{ "path": root }] });
    std::fs::write(lib_dir.join(dalica_engine::session::FOLDERS_FILE), serde_json::to_vec(&roots).unwrap()).unwrap();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, ..ThumbOptions::default() } };
    let s = Session::open_with(&lib_dir, virtual_clock(), &options).unwrap();
    (tmp, s, album)
}

/// `slideshow.list` with `params`: the photos, the start and the videos left out.
fn list(s: &mut Session, params: Value) -> (Vec<PhotoId>, u64, u64) {
    let v = s.execute("slideshow.list", &params).unwrap();
    let photos = serde_json::from_value(v["photos"].clone()).unwrap();
    (photos, v["start"].as_u64().unwrap(), v["videos"].as_u64().unwrap())
}

/// A folder in its display order, an album in its own order, a search's matches and the tray's ids
/// in theirs, each starting at the photo asked for (the first when it is not in the list); hidden
/// photos only while the view shows them; videos left out and counted.
#[test]
fn the_lists_of_a_folder_an_album_a_search_and_the_tray_start_at_the_photo_asked_for() {
    let (_tmp, mut s, album) = lists();
    let p = PhotoId::new;
    assert_eq!(list(&mut s, json!({ "section": "folder:1", "start": p(3) })), (ids(&[1, 2, 3, 5]), 2, 1), "the folder, 4 hidden");
    assert_eq!(list(&mut s, json!({ "section": format!("album:{album}"), "start": p(1) })), (ids(&[5, 1, 3]), 1, 0), "the album in its order");
    assert_eq!(list(&mut s, json!({ "search": "harbour", "start": p(5) })), (ids(&[2, 5]), 1, 0), "the search's matches");
    assert_eq!(list(&mut s, json!({ "ids": [p(5), p(1), p(5), p(6), p(4)], "start": p(1) })), (ids(&[5, 1]), 1, 1), "the tray");
    assert_eq!(list(&mut s, json!({ "section": "folder:1", "start": p(4) })), (ids(&[1, 2, 3, 5]), 0, 1), "a start not listed");
    assert_eq!(list(&mut s, json!({ "section": "folder:1" })), (ids(&[1, 2, 3, 5]), 0, 1), "no start");

    s.change_settings(|st| {
        let mut v = st.view_options();
        v.hidden_pictures = true;
        st.set_view_options(&v);
    });
    assert_eq!(list(&mut s, json!({ "section": "folder:1", "start": p(4) })), (ids(&[1, 2, 3, 4, 5]), 3, 1), "hidden shown");
    assert_eq!(list(&mut s, json!({ "ids": [p(4), p(2)] })), (ids(&[4, 2]), 0, 0), "hidden shown, the tray");
}

/// Hostile and impossible lists fail with the reason, and change nothing.
#[test]
fn the_list_fails_gracefully() {
    let (_tmp, mut s, _album) = lists();
    let before = s.library().model().clone();
    let cases = [
        json!({}),
        json!({ "section": "folder:1", "ids": [1] }),
        json!({ "section": "folder:1", "search": "x" }),
        json!({ "section": "shelf:1" }),
        json!({ "section": "folder:0" }),
        json!({ "section": "folder:99" }),
        json!({ "section": "album:a0000000000ff" }),
        json!({ "section": "album:search" }),
        json!({ "ids": [999] }),
        json!({ "ids": "1" }),
        json!({ "search": "x".repeat(1 << 20) }),
        json!({ "section": "folder:1", "start": "first" }),
        json!({ "section": "folder:1", "loop": true }),
    ];
    for bad in cases {
        let shown: String = bad.to_string().chars().take(80).collect();
        match s.execute("slideshow.list", &bad) {
            Err(CmdError::BadParams(_) | CmdError::NotFound(_) | CmdError::Disabled(_) | CmdError::Limit(_)) => {}
            other => panic!("{shown}: {other:?}"),
        }
    }
    let many: Vec<u64> = (1..=100_001).collect();
    assert!(matches!(s.execute("slideshow.list", &json!({ "ids": many })), Err(CmdError::Limit(_))));
    assert_eq!(s.library().model(), &before);
    assert_eq!(s.undo_stack().len(), 0);
}

// --- The show's previews --------------------------------------------------------------------------

/// A library watching `Porto` with `n` small photos, on a virtual clock: ids in name order.
fn porto(n: usize) -> (tempfile::TempDir, Arc<VirtualClock>, Session, Vec<PhotoId>) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Porto");
    dalica_fs::create_dir_all(&folder).unwrap();
    for i in 0..n {
        dalica_fs::replace(&folder.join(format!("IMG_{:04}.jpg", i + 1)), &jpeg(i as u64 + 1), Expect::Absent).unwrap();
    }
    let clock = virtual_clock();
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Off, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), clock.clone(), &options).unwrap();
    s.set_watching(WatchMode::Off);
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let mut ids: Vec<(String, PhotoId)> = s.library().photos().map(|p| (p.name.clone(), p.id)).collect();
    ids.sort();
    (dir, clock, s, ids.into_iter().map(|(_, id)| id).collect())
}

fn settings() -> Settings {
    Settings { display_secs: 3, transition: Transition::Dissolve, looped: false }
}

/// One frame: the clock moves on, the show ticks, and the preview's work asked for is done (the
/// virtual clock stands still meanwhile, so only the planted delay counts as decode time).
fn frame(clock: &VirtualClock, s: &mut Session, show: &mut Slideshow) -> Frame {
    clock.advance(FRAME);
    let f = show.tick(s);
    assert!(s.wait_preview(PATIENCE), "the preview's work ended");
    f
}

/// Plays `show` until it has made `changes` changes (or ended), checking every change: the photo
/// coming in is the one after the photo going out, and is drawn over it.
fn play(clock: &VirtualClock, s: &mut Session, show: &mut Slideshow, changes: u64) -> Vec<PhotoId> {
    let mut shown = vec![show.tick(s).photo.unwrap().0];
    for _ in 0..10_000 {
        if show.stats().changes >= changes || show.show().state() == State::Ended {
            break;
        }
        let f = frame(clock, s, show);
        let id = f.photo.unwrap().0;
        if shown.last() != Some(&id) {
            let (out, out_pane) = f.outgoing.expect("the outgoing photo is drawn while the change is under way");
            assert_eq!(Some(&out), shown.last());
            assert_ne!(Some(out_pane), f.photo.map(|(_, p)| p), "two panes");
            shown.push(id);
        }
    }
    shown
}

/// Every decode slowed to 1.5 s on a 3 s display time: over 30 changes the next photo's preview is
/// ready every time it comes in (zero waits), made at the display's size; at 2.5 s the prefetch
/// still covers it. A decode slower than the display time (3.5 s) shows what a wait is.
#[test]
fn with_every_decode_slowed_the_next_preview_is_ready_at_each_of_30_changes() {
    let (_dir, clock, mut s, ids) = porto(31);
    s.plant_decode_delay(Some(Duration::from_millis(1_500)));
    let mut show = Slideshow::with_settings(&mut s, ids.clone(), 0, settings(), DISPLAY);
    assert!(s.wait_preview(PATIENCE), "the decodes finished; their results wait for the delay");
    assert!(!show.tick(&mut s).ready, "the first photo's decode is held for 1.5 s");
    clock.advance(Duration::from_millis(1_500));
    assert!(s.wait_preview(PATIENCE));
    assert!(show.tick(&mut s).ready, "and is shown once they have passed");
    let shown = play(&clock, &mut s, &mut show, 30);
    assert_eq!(shown, ids[..31], "every photo in turn");
    assert_eq!(show.stats().changes, 30);
    assert_eq!(show.stats().waits, 0, "{:?}", show.stats());
    let f = frame(&clock, &mut s, &mut show);
    assert!(f.ready);
    let (id, pane) = f.photo.unwrap();
    let preview = s.preview_in(pane).unwrap();
    assert_eq!((preview.photo, preview.image.width, preview.image.height), (id, 96, 64), "the screen-size preview (never above the source)");
    show.close(&mut s);
    assert_eq!((s.preview_photo_in(0), s.preview_photo_in(1)), (None, None), "closing the show gives the panes back");

    // 2.5 s: only the prefetch made as the photo before comes in is early enough (the other pane
    // opens on the next photo 0.8 s later, when the change has ended).
    s.plant_decode_delay(Some(Duration::from_millis(2_500)));
    let mut early = Slideshow::with_settings(&mut s, ids[5..10].to_vec(), 0, settings(), DISPLAY);
    play(&clock, &mut s, &mut early, 4);
    assert_eq!((early.stats().changes, early.stats().waits), (4, 0), "the prefetch covers a 2.5 s decode");
    early.close(&mut s);

    s.plant_decode_delay(Some(Duration::from_millis(3_500)));
    let mut slow = Slideshow::with_settings(&mut s, ids[..5].to_vec(), 0, settings(), DISPLAY);
    play(&clock, &mut s, &mut slow, 4);
    assert_eq!(slow.stats().changes, 4);
    assert!(slow.stats().waits > 0, "a decode longer than the display time waits: {:?}", slow.stats());
}

/// A photo deleted while the show runs is skipped: the photo after it comes in when the deleted
/// one's turn would have begun, ready, with no wait and no stall.
#[test]
fn a_photo_deleted_from_the_list_is_skipped_without_a_stall() {
    let _faults = shared();
    let (_dir, clock, mut s, ids) = porto(5);
    let mut show = Slideshow::with_settings(&mut s, ids.clone(), 0, settings(), DISPLAY);
    assert!(s.wait_preview(PATIENCE));
    let started = clock.now();
    s.execute("photo.delete", &json!({ "ids": [ids[2]], "confirm": true })).unwrap();
    wait_files(&mut s);
    assert!(s.library().photo(ids[2]).is_none(), "the photo left the library");
    let mut times = vec![(ids[0], Duration::ZERO)];
    while show.stats().changes < 2 {
        let f = frame(&clock, &mut s, &mut show);
        let id = f.photo.unwrap().0;
        if times.last().map(|(last, _)| *last) != Some(id) {
            times.push((id, clock.now().since(started)));
        }
    }
    let expected = [(ids[0], Duration::ZERO), (ids[1], Duration::from_secs(3)), (ids[3], Duration::from_secs(6))];
    assert_eq!(times.len(), expected.len(), "{times:?}");
    for ((want, at), (got, when)) in expected.iter().zip(&times) {
        assert_eq!(got, want, "{times:?}");
        assert!(when.saturating_sub(*at) <= FRAME, "{got:?} came in at {when:?}, due at {at:?}");
    }
    assert_eq!(show.stats().waits, 0, "{:?}", show.stats());
    show.close(&mut s);
    if RESTORES {
        s.execute("edit.undo", &json!({})).unwrap();
        wait_files(&mut s);
    }
}
