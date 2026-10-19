//! Videos in the model (M15, `formats.md` §1): a video's facts set by a derived op that moves no
//! revision and round-trips through the journal's JSON, a photo's kind, a section's videos counted
//! once, the photos a still-only path takes, and the Videos filter reading the kind.

use std::path::PathBuf;

use dalica_core::{AlbumId, FolderId, PhotoId, PosKey};
use dalica_library::search::{Extract, Filters, Query, Scope, SearchIndex, Update};
use dalica_library::{
    AlbumInfo, AlbumRef, Error, Facts, Folder, Library, MAX_CODEC, Op, Photo, PhotoKind, Section, TreeOptions, VideoFacts, section_photos,
};

use crate::common::photo;

const TRIP: FolderId = FolderId::new(1);
const A: PhotoId = PhotoId::new(1);
const CLIP: PhotoId = PhotoId::new(2);
const B: PhotoId = PhotoId::new(3);
const OLD: PhotoId = PhotoId::new(4);

/// A photo large enough to show with Small Pictures off.
fn big(id: PhotoId, name: &str) -> Photo {
    Photo { width: 1920, height: 1080, ..photo(id, TRIP, name) }
}

/// An iPhone-like clip: H.264 with AAC sound, turned a quarter, 29.97 frames a second.
fn clip() -> VideoFacts {
    VideoFacts {
        duration_ms: 12_345,
        width: 1920,
        height: 1080,
        rotation: 1,
        video_codec: "avc1".into(),
        sound_codec: "mp4a".into(),
        sound: true,
        frame_rate_milli: 29_970,
    }
}

/// Trip holds a.jpg, clip.mov, b.jpg and old.mp4; clip.mov has video facts, old.mp4 only the
/// scan's search fact (an index written before M15).
fn fixture() -> Library {
    let mut lib = Library::new();
    lib.apply(Op::AddFolder { folder: Folder { id: TRIP, path: PathBuf::from("/p/Trip") } }).unwrap();
    let photos = vec![big(A, "a.jpg"), big(CLIP, "clip.mov"), big(B, "b.jpg"), big(OLD, "old.mp4")];
    lib.apply(Op::AddPhotos { photos }).unwrap();
    lib.apply(Op::SetFacts { facts: vec![(OLD, Facts { video: true, ..Facts::default() })] }).unwrap();
    lib.apply(Op::SetVideos { videos: vec![(CLIP, Some(clip()))] }).unwrap();
    lib
}

/// The facts come back as set, through the journal's JSON too; setting and clearing them moves
/// neither revision and is derived (never on an undo stack); the inverse restores the model, and
/// removing the photo takes them while its inverse puts them back.
#[test]
fn a_videos_facts_round_trip_and_move_no_revision() {
    let mut lib = fixture();
    assert_eq!(lib.video_facts(CLIP), Some(&clip()));
    assert_eq!(lib.video_facts(A), None);
    assert_eq!(clip().display_size(), (1080, 1920), "a quarter turn shows it upright");
    assert_eq!(VideoFacts { rotation: 2, ..clip() }.display_size(), (1920, 1080));

    let before = lib.model().clone();
    let (rev, thumbs) = (lib.revision(), lib.thumbnail_revision());
    let op = Op::SetVideos { videos: vec![(B, Some(VideoFacts::default())), (CLIP, None)] };
    assert!(op.is_derived());
    let inverse = lib.apply(op.clone()).unwrap();
    assert!(inverse.is_derived());
    assert_eq!((lib.revision(), lib.thumbnail_revision()), (rev, thumbs), "a derived fact moves no revision");
    assert_eq!((lib.kind(B), lib.kind(CLIP)), (PhotoKind::Video, PhotoKind::Still), "a record, even an empty one, makes a video");
    assert_eq!(lib.video_facts_all().map(|(id, _)| id).collect::<Vec<_>>(), [B]);
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before);
    assert_eq!(lib.revision(), rev);

    // The journal's record, by name, and back.
    let json = serde_json::to_value(Op::SetVideos { videos: vec![(CLIP, Some(clip()))] }).unwrap();
    assert_eq!(json["op"], "setVideos");
    assert_eq!(
        json["videos"][0][1],
        serde_json::json!({"durationMs": 12345, "width": 1920, "height": 1080, "rotation": 1, "videoCodec": "avc1",
            "soundCodec": "mp4a", "sound": true, "frameRateMilli": 29970})
    );
    assert_eq!(serde_json::from_value::<Op>(json).unwrap(), Op::SetVideos { videos: vec![(CLIP, Some(clip()))] });
    assert_eq!(serde_json::to_string(&VideoFacts::default()).unwrap(), "{}", "nothing known writes nothing");

    // Removing the photo takes its facts; the inverse puts them back.
    let inverse = lib.apply(Op::RemovePhotos { ids: vec![CLIP] }).unwrap();
    assert_eq!(lib.video_facts(CLIP), None);
    lib.apply(inverse).unwrap();
    assert_eq!(lib.model(), &before);
}

/// Facts the caps refuse, or of a photo the library does not have, change nothing.
#[test]
fn bad_video_facts_are_refused() {
    let mut lib = fixture();
    let before = lib.model().clone();
    let cases = [
        (VideoFacts { rotation: 4, ..clip() }, CLIP),
        (VideoFacts { video_codec: "x".repeat(MAX_CODEC + 1), ..clip() }, CLIP),
        (VideoFacts { sound_codec: "mp4a\n".into(), ..clip() }, CLIP),
        (clip(), PhotoId::new(99)),
    ];
    for (facts, id) in cases {
        let op = Op::SetVideos { videos: vec![(A, None), (id, Some(facts))] };
        assert!(lib.apply(op.clone()).is_err(), "{op:?}");
        assert_eq!(lib.model(), &before, "{op:?}");
    }
    assert_eq!(lib.apply(Op::SetVideos { videos: vec![(PhotoId::new(99), None)] }), Err(Error::UnknownPhoto(PhotoId::new(99))));
}

/// A photo's kind: a video by its facts, or by the scan's search fact alone; a still otherwise,
/// and for an unknown photo.
#[test]
fn the_kind_reads_the_facts() {
    let lib = fixture();
    assert_eq!(lib.kind(A), PhotoKind::Still);
    assert_eq!(lib.kind(CLIP), PhotoKind::Video);
    assert_eq!(lib.kind(OLD), PhotoKind::Video, "the search fact alone");
    assert_eq!(lib.kind(PhotoId::new(99)), PhotoKind::Still);
    assert!(lib.is_video(CLIP) && !lib.is_video(B));
}

/// A section's videos are counted once: a folder section, an album section and a list that names
/// a video twice (two sections showing it, or a repeated id) each count it once; unknown ids are
/// left out.
#[test]
fn a_sections_videos_are_counted_once() {
    let mut lib = fixture();
    let album = AlbumId::parse("a0001").unwrap();
    lib.apply(Op::CreateAlbums { albums: vec![(album.clone(), AlbumInfo::named("Clips"))] }).unwrap();
    let key = |k: &str| PosKey::parse(k).unwrap();
    lib.apply(Op::AddMembers { members: vec![(album.clone(), CLIP, key("a0")), (album.clone(), A, key("a1"))] }).unwrap();

    let options = TreeOptions { small_pictures: true, ..TreeOptions::default() };
    let sections = section_photos(&lib, &[Section::Album(AlbumRef::User(album)), Section::Folder(TRIP)], &options);
    let counts: Vec<usize> = sections.iter().map(|(_, ids)| lib.video_count(ids)).collect();
    assert_eq!(counts, [1, 2], "the album holds the clip; the folder the clip and old.mp4");
    let all: Vec<PhotoId> = sections.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
    assert_eq!(lib.video_count(&all), 2, "the clip shown in two sections counts once");
    assert_eq!(lib.videos_among(&[OLD, CLIP, OLD, PhotoId::new(99), A]), [OLD, CLIP], "in the order given, each once");
}

/// The still-only query leaves the videos out, keeps the order given and takes each photo once.
#[test]
fn the_still_only_query_leaves_videos_out() {
    let lib = fixture();
    assert_eq!(lib.stills_among(&[B, CLIP, A, OLD, B, PhotoId::new(99)]), [B, A]);
    assert_eq!(lib.stills_among(&[CLIP, OLD]), Vec::<PhotoId>::new());
}

fn search(lib: &Library, query: &Query) -> Vec<PhotoId> {
    let index = SearchIndex::from_library(lib);
    let mut found = index.search(query, &Scope::default()).ids;
    found.sort();
    assert_eq!(found, Extract::from_library(lib).matching(query, &Scope::default()), "index and matcher disagree on {query:?}");
    found
}

/// The Videos filter keeps only the videos, by their kind (facts or the search fact), and an
/// index kept up to date through the video ops equals one built afresh.
#[test]
fn the_videos_filter_keeps_only_them() {
    let mut lib = fixture();
    let videos = Query { filters: Filters { videos: true, ..Filters::default() }, ..Query::default() };
    assert_eq!(search(&lib, &videos), [CLIP, OLD]);
    assert_eq!(search(&lib, &Query::default()), [A, CLIP, B, OLD]);

    let mut index = SearchIndex::from_library(&lib);
    for op in [Op::SetVideos { videos: vec![(B, Some(clip()))] }, Op::SetVideos { videos: vec![(CLIP, None)] }] {
        let inverse = lib.apply(op.clone()).unwrap();
        index.apply(Update::for_op(&lib, &op));
        assert!(index == SearchIndex::from_library(&lib), "after {op:?}");
        lib.apply(inverse.clone()).unwrap();
        index.apply(Update::for_op(&lib, &inverse));
        assert!(index == SearchIndex::from_library(&lib), "after undoing {op:?}");
        lib.apply(op.clone()).unwrap();
        index.apply(Update::for_op(&lib, &op));
    }
    assert_eq!(search(&lib, &videos), [B, OLD]);
    assert_eq!(index.search(&videos, &Scope::default()).ids.len(), 2);
}
