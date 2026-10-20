//! Videos in the index (M15.11, `formats.md` §1, `data-model.md` §10.1): a video's facts are
//! derived data the index keeps (the journal and the snapshot) and a rebuild's scan reads again;
//! its star, caption and tags live in the sidecar exactly as a photo's.

use std::collections::BTreeSet;
use std::path::Path;

use dalica_core::{FolderId, PhotoId, Timestamp};
use dalica_library::{Folder, Library, Op, Photo, VideoFacts};
use dalica_store::mapping::{PHOTO_DATA_M15, Source, VIDEO_FIELDS};
use dalica_store::rebuild::import_sidecars;
use dalica_store::{Opened, Outcome, SidecarWriter, Store};
use serde_json::Value;

use crate::common::{NOW, strings};

/// What a scan's probe of a phone clip gives.
fn clip_facts() -> VideoFacts {
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

/// A scan of `dir`: a clip, a video whose container could not be probed (the empty record) and a
/// JPEG, with the same ids each time, and the video facts the probe gives (the scan's ops).
fn scan(dir: &Path) -> (Library, FolderId, [PhotoId; 3], Vec<Op>) {
    let mut lib = Library::new();
    let folder = lib.next_folder_id();
    let ids = [PhotoId::new(1), PhotoId::new(2), PhotoId::new(3)];
    let photo = |id, name: &str, (width, height)| Photo {
        id,
        folder,
        name: name.into(),
        size: 10,
        mtime: Timestamp::from_millis(5),
        width,
        height,
        date: None,
        star: false,
    };
    let ops = vec![
        Op::AddFolder { folder: Folder { id: folder, path: dir.to_path_buf() } },
        Op::AddPhotos { photos: vec![photo(ids[0], "clip.mp4", (1080, 1920)), photo(ids[1], "cut.mp4", (0, 0)), photo(ids[2], "a.jpg", (4, 3))] },
        Op::SetVideos { videos: vec![(ids[0], Some(clip_facts())), (ids[1], Some(VideoFacts::default()))] },
    ];
    for op in &ops {
        lib.apply(op.clone()).unwrap();
    }
    (lib, folder, ids, ops)
}

fn record(o: &mut Opened, lib: &mut Library, op: &Op) {
    lib.apply(op.clone()).unwrap();
    o.store.append(op).unwrap();
}

#[test]
fn a_starred_captioned_video_survives_a_restart_and_a_rebuild_with_its_facts() {
    let photos = tempfile::tempdir().unwrap();
    let lib_dir = tempfile::tempdir().unwrap();
    let (_, folder, ids, scan_ops) = scan(photos.path());
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    let mut lib = Library::new();
    for op in &scan_ops {
        record(&mut o, &mut lib, op);
    }
    for op in [
        Op::SetStars { stars: vec![(ids[0], true)] },
        Op::SetCaptions { captions: vec![(ids[0], "Tram 28".into())] },
        Op::SetTags { tags: vec![(ids[0], strings(&["Lisbon"]))] },
    ] {
        record(&mut o, &mut lib, &op);
    }
    assert!(lib.is_video(ids[0]) && lib.is_video(ids[1]) && !lib.is_video(ids[2]));

    // A restart replays the journal; a snapshot then holds the facts on its own.
    drop(o);
    let mut o = Store::open(lib_dir.path(), NOW).unwrap();
    assert_eq!((o.report.clone(), o.library.model()), (vec![], lib.model()));
    o.store.snapshot(&o.library).unwrap();
    drop(o);
    let snap = std::fs::read_to_string(lib_dir.path().join(dalica_store::snapshot::FILE)).unwrap();
    assert!(snap.contains("\"videos\"") && snap.contains("\"durationMs\":12345"), "{snap}");
    let o = Store::open(lib_dir.path(), NOW).unwrap();
    assert_eq!((o.report.clone(), o.store.seq()), (vec![], o.store.snapshot_seq()));
    assert_eq!(o.library.model(), lib.model());
    assert_eq!(o.library.video_facts(ids[0]), Some(&clip_facts()));
    assert_eq!(o.library.video_facts(ids[1]), Some(&VideoFacts::default()), "an unprobeable video stays a video");
    drop(o);

    // The sidecar keeps the video's star, caption and tags as a photo's, and none of its facts.
    let mut w = SidecarWriter::new();
    w.mark_dirty(folder);
    assert!(w.flush(&lib, NOW).values().all(|o| *o == Outcome::Written));
    let text = std::fs::read_to_string(photos.path().join(".dalica.json")).unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let entry = &json["photos"]["clip.mp4"];
    assert_eq!((&entry["star"], &entry["caption"], &entry["tags"]), (&Value::Bool(true), &Value::from("Tram 28"), &serde_json::json!(["Lisbon"])));
    for derived in ["durationMs", "avc1", "mp4a", "29970", "video"] {
        assert!(!text.contains(derived), "{derived} in {text}");
    }

    // Delete the index; the rebuild's scan probes the videos again and the sidecar gives the rest.
    for e in std::fs::read_dir(lib_dir.path()).unwrap() {
        let p = e.unwrap().path();
        if p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("index.")) {
            std::fs::remove_file(p).unwrap();
        }
    }
    let (mut rebuilt, ..) = scan(photos.path());
    let import = import_sidecars(&rebuilt, NOW);
    assert!(import.problems.is_empty(), "{import:?}");
    assert!(!import.ops.iter().any(|op| matches!(op, Op::SetVideos { .. })), "the facts are the scan's, never a sidecar's");
    for op in import.ops {
        rebuilt.apply(op).unwrap();
    }
    assert_eq!(rebuilt.model(), lib.model());
}

/// Nothing M15.11 adds is silently dropped: every field of a video's facts is placed (index-only),
/// under the key the index writes it with. A new field breaks the destructuring until it is placed.
#[test]
fn every_video_fact_has_a_place() {
    let VideoFacts { duration_ms: _, width: _, height: _, rotation: _, video_codec: _, sound_codec: _, sound: _, frame_rate_milli: _ } =
        VideoFacts::default();
    assert_eq!(PHOTO_DATA_M15, [("video", Source::Fields("VIDEO_FIELDS"))]);
    assert!(VIDEO_FIELDS.iter().all(|(_, s)| *s == Source::Index), "derived: the index keeps them, a rebuild's scan reads them again");
    let keys: BTreeSet<String> = serde_json::to_value(clip_facts()).unwrap().as_object().unwrap().keys().cloned().collect();
    let named: BTreeSet<String> = VIDEO_FIELDS.iter().map(|(n, _)| n.to_string()).collect();
    assert_eq!(keys, named);
}
