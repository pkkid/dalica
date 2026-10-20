//! W17, Video (`mcp.md` §9; `editing.md` §11, `creating.md` §2 and §4): `scan_folder` on a folder
//! of two photos and two clips, an H.264 one (a codec `docs/video-decoders.md` passes) and an
//! MPEG-4 Part 2 one (no decoder); `photo_info` shows each clip's kind, duration and date;
//! `cmd_video_inspect` says the H.264 clip plays and the other does not, with the reason. The
//! H.264 clip plays headless (the session's silent `Sound`), takes `trim(500,1500)` through
//! `cmd_edit_recipe_set`, and `cmd_video_saveFrame` at 1 s writes a JPEG into the Video Frames
//! project; the other opens with the reason and its Save Frame is refused with it, writing nothing.
//! `cmd_export_run` of the folder as first frames writes three JPEGs (the photos and the H.264
//! clip's first frame) and counts the other clip, skipped with the reason; the `export` tool with
//! `videos: "full"` copies both clips byte for byte beside the photos; the clips are only read.
//!
//! The clips are H.264 made here, every macroblock I_PCM (ITU-T H.264 §7.3.5), muxed by hand from
//! ISO/IEC 14496-12: the writer of `crates/engine/tests/video_play.rs`, copied (a test file of
//! another crate cannot be called from here; the image, thumbs, create and engine tests each keep
//! a copy of their own too).

use std::path::{Path, PathBuf};

use dalica_engine::automation::Policy;
use dalica_mcp::{Headless, Source};
use serde_json::{Value, json};

use crate::common::{Client, initialize};
use crate::harness::{Workspace, bytes, eventually, ids, photo_name};

/// What Dalica says of a clip it cannot play.
const REASON: &str = "This video can't be played (MPEG-4 Part 2)";

// --- An H.264 writer: every macroblock I_PCM -------------------------------------------------------

#[derive(Default)]
struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn u(&mut self, bits: u32, v: u64) -> &mut Bits {
        for i in (0..bits).rev() {
            self.acc = (self.acc << 1) | ((v >> i) & 1);
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                (self.acc, self.n) = (0, 0);
            }
        }
        self
    }
    fn ue(&mut self, v: u64) -> &mut Bits {
        let v1 = v + 1;
        let len = 64 - v1.leading_zeros();
        self.u(len - 1, 0).u(len, v1)
    }
    fn se(&mut self, v: i64) -> &mut Bits {
        self.ue(if v > 0 { (2 * v - 1) as u64 } else { (-2 * v) as u64 })
    }
    fn align(&mut self) -> &mut Bits {
        while self.n != 0 {
            self.u(1, 0);
        }
        self
    }
    fn trailing(mut self) -> Vec<u8> {
        self.u(1, 1);
        self.align();
        self.out
    }
}

fn nal(ref_idc: u8, kind: u8, rbsp: &[u8]) -> Vec<u8> {
    let mut out = vec![(ref_idc << 5) | kind];
    let mut zeros = 0;
    for &b in rbsp {
        if zeros >= 2 && b <= 3 {
            out.push(3);
            zeros = 0;
        }
        out.push(b);
        zeros = if b == 0 { zeros + 1 } else { 0 };
    }
    out
}

const FPS: usize = 25;
const FRAME_MS: u64 = 40;
const GOP: usize = 10;
const MBS: (usize, usize) = (10, 6);
const FRAMES: usize = 50;
const SILENT_AAC: [u8; 4] = [0, 0, 0, 7];

fn grey(k: usize) -> u8 {
    40 + 3 * k as u8
}

fn sps() -> Vec<u8> {
    let mut b = Bits::default();
    b.u(8, 66).u(8, 0xc0).u(8, 30).ue(0);
    b.ue(0).ue(2).ue(1).u(1, 0);
    b.ue(MBS.0 as u64 - 1).ue(MBS.1 as u64 - 1).u(1, 1).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 7, &b.trailing())
}

fn pps() -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(0).u(1, 0).u(1, 0).ue(0).ue(0).ue(0).u(1, 0).u(2, 0);
    b.se(0).se(0).se(0).u(1, 1).u(1, 0).u(1, 0);
    nal(3, 8, &b.trailing())
}

/// Frame `k`, length-prefixed: one I slice of I_PCM macroblocks of grey `grey(k)`.
fn frame(k: usize) -> Vec<u8> {
    let idr = k.is_multiple_of(GOP);
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, (k % GOP) as u64);
    if idr {
        b.ue((k / GOP % 2) as u64).u(1, 0).u(1, 0);
    } else {
        b.u(1, 0);
    }
    b.se(0).ue(1);
    for _ in 0..MBS.0 * MBS.1 {
        b.ue(25).align();
        for _ in 0..256 {
            b.u(8, u64::from(grey(k)));
        }
        for _ in 0..128 {
            b.u(8, 128);
        }
    }
    let s = nal(if idr { 3 } else { 2 }, if idr { 5 } else { 1 }, &b.trailing());
    let mut out = (s.len() as u32).to_be_bytes().to_vec();
    out.extend(s);
    out
}

// --- An MP4 writer -------------------------------------------------------------------------------------

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0, 0, 0, 0][..], payload].concat())
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const IDENTITY: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];

struct Track {
    id: u32,
    handler: [u8; 4],
    entry: Vec<u8>,
    timescale: u32,
    duration: u32,
    syncs: Option<Vec<u32>>,
    size: (u32, u32),
    samples: Vec<Vec<u8>>,
}

fn trak(t: &Track, at: &mut u32) -> Vec<u8> {
    let total = t.duration * t.samples.len() as u32;
    let mut tkhd = vec![0u8; 8];
    tkhd.extend(be(t.id));
    tkhd.extend([0u8; 4]);
    tkhd.extend(be(total * 1000 / t.timescale));
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(t.size.0 << 16));
    tkhd.extend(be(t.size.1 << 16));
    let mut mdhd = vec![0u8; 8];
    mdhd.extend(be(t.timescale));
    mdhd.extend(be(total));
    mdhd.extend([0x55, 0xc4, 0, 0]);
    let mut hdlr = vec![0u8; 4];
    hdlr.extend(t.handler);
    hdlr.extend([0u8; 13]);
    let mut stsd = be(1).to_vec();
    stsd.extend(&t.entry);
    let stts = [be(1), be(t.samples.len() as u32), be(t.duration)].concat();
    let mut stsz = be(0).to_vec();
    stsz.extend(be(t.samples.len() as u32));
    let mut stco = be(t.samples.len() as u32).to_vec();
    for s in &t.samples {
        stsz.extend(be(s.len() as u32));
        stco.extend(be(*at));
        *at += s.len() as u32;
    }
    let stsc = [be(1), be(1), be(1), be(1)].concat();
    let mut stbl = [full(b"stsd", &stsd), full(b"stts", &stts), full(b"stsc", &stsc), full(b"stsz", &stsz), full(b"stco", &stco)].concat();
    if let Some(syncs) = &t.syncs {
        let mut stss = be(syncs.len() as u32).to_vec();
        syncs.iter().for_each(|s| stss.extend(be(*s)));
        stbl.extend(full(b"stss", &stss));
    }
    let mdia = bx(b"mdia", &[full(b"mdhd", &mdhd), full(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    bx(b"trak", &[full(b"tkhd", &tkhd), mdia].concat())
}

fn avc1(code: &[u8; 4]) -> Vec<u8> {
    let (sps, pps) = (sps(), pps());
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 16]);
    p.extend(((MBS.0 * 16) as u16).to_be_bytes());
    p.extend(((MBS.1 * 16) as u16).to_be_bytes());
    p.extend(be(0x48_0000));
    p.extend(be(0x48_0000));
    p.extend([0u8; 4]);
    p.extend([0, 1]);
    p.extend([0u8; 32]);
    p.extend([0, 0x18, 0xff, 0xff]);
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(&sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(&pps);
    p.extend(bx(b"avcC", &avcc));
    bx(code, &p)
}

/// A sound entry: AAC-LC mono at 48 kHz.
fn mp4a() -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(b"mp4a", &p)
}

/// Two seconds of 160 × 96 at 25 frames a second (a key frame every 10) with a silent AAC track,
/// recorded 2026-10-19 14:02:00 UTC; the video's sample entry `video`.
fn clip(video: &[u8; 4]) -> Vec<u8> {
    let packets = (FRAMES * 48_000 / FPS).div_ceil(1024);
    let tracks = [
        Track {
            id: 1,
            handler: *b"vide",
            entry: avc1(video),
            timescale: 1000,
            duration: FRAME_MS as u32,
            syncs: Some((0..FRAMES as u32).filter(|k| (*k as usize).is_multiple_of(GOP)).map(|k| k + 1).collect()),
            size: ((MBS.0 * 16) as u32, (MBS.1 * 16) as u32),
            samples: (0..FRAMES).map(frame).collect(),
        },
        Track {
            id: 2,
            handler: *b"soun",
            entry: mp4a(),
            timescale: 48_000,
            duration: 1024,
            syncs: None,
            size: (0, 0),
            samples: vec![SILENT_AAC.to_vec(); packets],
        },
    ];
    let mut out = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
    let data: Vec<u8> = tracks.iter().flat_map(|t| t.samples.concat()).collect();
    let mut at = out.len() as u32 + 8;
    out.extend(bx(b"mdat", &data));
    let traks: Vec<u8> = tracks.iter().flat_map(|t| trak(t, &mut at)).collect();
    // Recorded 2026-10-19 14:02:00 UTC (seconds since 1904), modified then too.
    let mut mvhd = [be(3_875_263_320), be(3_875_263_320)].concat();
    mvhd.extend(be(1000));
    mvhd.extend(be((FRAMES as u64 * FRAME_MS) as u32));
    mvhd.extend(be(0x1_0000));
    mvhd.extend([1, 0]);
    mvhd.extend([0u8; 10]);
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(3));
    out.extend(bx(b"moov", &[full(b"mvhd", &mvhd), traks].concat()));
    out
}

// --- The agent -----------------------------------------------------------------------------------------

/// An agent on a headless server whose session knows a Pictures folder inside the workspace (so
/// Save Frame writes there), the workspace a read root, the write roots the defaults (the system's
/// temporary folder, which holds the workspace).
struct Viewer {
    client: Client,
    next: u64,
}

impl Viewer {
    fn start(ws: &Workspace, pictures: PathBuf) -> Viewer {
        let (library, root) = (ws.library(), ws.path().to_path_buf());
        let mut client = Client::start(move || {
            let mut h = Headless::open_with(Source::Library(library), &[], Policy::default(), false).unwrap();
            h.roots_mut().add_read(root);
            h.session_mut().set_pictures_dir(Some(pictures));
            Box::new(h)
        });
        initialize(&mut client);
        Viewer { client, next: 100 }
    }

    fn call(&mut self, name: &str, args: Value) -> Value {
        self.next += 1;
        self.client.call(self.next, name, args)
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.call(name, args.clone());
        assert_eq!(r["isError"], json!(false), "{name} {args}: {r}");
        r["structuredContent"].clone()
    }

    /// The player's state once `done` holds of it.
    fn until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let mut last = Value::Null;
        eventually(
            what,
            || {
                last = self.tool("cmd_video_state", json!({}));
                done(&last).then(|| last.clone())
            },
            String::new,
        )
    }
}

/// The text of a refused tool call (`isError`).
fn refusal(r: &Value) -> String {
    assert_eq!(r["isError"], json!(true), "refused: {r}");
    r["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

/// The row of the projects group of `doc_inspect`'s tree for the folder at `path`.
fn project_row(inspected: &Value, path: &Path) -> Option<Value> {
    let groups = inspected["tree"]["groups"].as_array()?;
    let projects = groups.iter().find(|g| g["group"] == json!("projects"))?;
    projects["rows"].as_array()?.iter().find(|r| r["path"].as_str().map(Path::new) == Some(path)).cloned()
}

#[test]
fn w17_video() {
    let ws = Workspace::new();
    let folder = ws.folder("Trip", 2, 1700);
    let (clip_mp4, old_mov) = (folder.join("clip.mp4"), folder.join("old.mov"));
    std::fs::write(&clip_mp4, clip(b"avc1")).unwrap();
    std::fs::write(&old_mov, clip(b"mp4v")).unwrap();
    let originals: Vec<(PathBuf, Vec<u8>)> = [folder.join(photo_name(1)), folder.join(photo_name(2)), clip_mp4.clone(), old_mov.clone()]
        .into_iter()
        .map(|p| (p.clone(), bytes(&p)))
        .collect();
    let pictures = ws.path().join("Pictures");
    let frames = pictures.join("Dalica").join("Video Frames");
    let mut agent = Viewer::start(&ws, pictures);

    // The scan: two photos and two clips, each clip a video of two seconds, dated its recording.
    let trip = agent.tool("scan_folder", json!({ "path": folder }))["section"].as_str().unwrap().to_string();
    let four = ids(&agent.tool("doc_inspect", json!({ "photos": trip, "limit": 1000 })));
    assert_eq!(four.len(), 4, "{four:?}");
    let by_name = |agent: &mut Viewer, name: &str| -> (u64, Value) {
        for id in &four {
            let info = agent.tool("photo_info", json!({ "id": id }));
            if info["name"] == json!(name) {
                return (*id, info);
            }
        }
        panic!("no photo {name}");
    };
    let (clip_id, info) = by_name(&mut agent, "clip.mp4");
    assert_eq!((&info["kind"], &info["durationMs"]), (&json!("video"), &json!(2000)), "{info}");
    assert!(info["dates"]["taken"].as_str().is_some_and(|d| d.starts_with("2026-10-19")), "the clip's recording date: {info}");
    let (old_id, info) = by_name(&mut agent, "old.mov");
    assert_eq!((&info["kind"], &info["durationMs"]), (&json!("video"), &json!(2000)), "{info}");
    let (_, info) = by_name(&mut agent, &photo_name(1));
    assert_eq!((&info["kind"], &info["durationMs"]), (&json!("still"), &Value::Null), "{info}");

    // Inspect: the H.264 clip plays, the MPEG-4 Part 2 one does not, with the reason.
    let inspected = agent.tool("cmd_video_inspect", json!({ "id": clip_id }));
    assert_eq!((&inspected["playable"], &inspected["reason"], &inspected["videoCodec"]), (&json!(true), &Value::Null, &json!("avc1")), "{inspected}");
    let inspected = agent.tool("cmd_video_inspect", json!({ "id": old_id }));
    assert_eq!((&inspected["playable"], &inspected["reason"]), (&json!(false), &json!(REASON)), "{inspected}");

    // The H.264 clip plays headless, on the session's silent sound, and takes its trim.
    let opened = agent.tool("cmd_video_open", json!({ "id": clip_id }));
    assert_eq!((&opened["open"], &opened["playable"]), (&json!(true), &json!(true)), "{opened}");
    agent.until("the player has the clip", |v| v["ready"] == json!(true));
    let played = agent.tool("cmd_video_play", json!({}));
    assert_eq!((&played["playing"], &played["sound"]), (&json!(true), &json!("plays")), "{played}");
    agent.until("the clip plays on", |v| v["positionMs"].as_u64().is_some_and(|ms| ms > 0));
    agent.tool("cmd_video_pause", json!({}));
    let set = agent.tool("cmd_edit_recipe_set", json!({ "ids": [clip_id], "recipe": "trim(500,1500)" }));
    assert_eq!(set["photos"][0]["changed"], json!(true), "{set}");
    let inspected = agent.tool("cmd_video_inspect", json!({ "id": clip_id }));
    assert_eq!(inspected["trim"], json!({ "startMs": 500, "endMs": 1500 }), "{inspected}");

    // Save Frame at 1 s: a JPEG of the clip's size into the Video Frames project.
    agent.tool("cmd_video_seek", json!({ "ms": 1000 }));
    let at = agent.until("seeked to 1 s", |v| v["pending"] == json!(false) && v["frame"]["index"] == json!(25));
    assert_eq!(at["frame"]["timeMs"], json!(1000), "{at}");
    let saved = agent.tool("cmd_video_saveFrame", json!({}));
    assert_eq!(saved["state"], json!("done"), "{saved}");
    let jpeg = PathBuf::from(saved["result"]["path"].as_str().unwrap_or_else(|| panic!("no path: {saved}")));
    assert_eq!(jpeg, frames.join("clip 00-00-01.jpg"), "{saved}");
    let file = bytes(&jpeg);
    assert!(file.starts_with(&[0xFF, 0xD8, 0xFF]), "{}: a JPEG", jpeg.display());
    let header = dalica_image::read_header(&file, &dalica_image::Limits::default()).unwrap();
    assert_eq!((header.width, header.height), (160, 96));
    let section = eventually(
        "the Video Frames folder joins the library's projects with the frame",
        || {
            let inspected = agent.tool("doc_inspect", json!({}));
            project_row(&inspected, &frames).filter(|r| r["count"] == json!(1)).map(|r| r["section"].as_str().unwrap().to_string())
        },
        String::new,
    );
    let listed = ids(&agent.tool("doc_inspect", json!({ "photos": section, "limit": 10 })));
    assert_eq!(listed.len(), 1, "{listed:?}");
    let info = agent.tool("photo_info", json!({ "id": listed[0] }));
    assert_eq!((Path::new(info["path"].as_str().unwrap()), &info["kind"]), (jpeg.as_path(), &json!("still")), "{info}");
    agent.tool("cmd_video_close", json!({}));

    // The clip Dalica cannot play opens with the reason; its Save Frame is refused, writing nothing.
    let opened = agent.tool("cmd_video_open", json!({ "id": old_id }));
    assert_eq!((&opened["playable"], &opened["reason"]), (&json!(false), &json!(REASON)), "{opened}");
    let refused = agent.call("cmd_video_saveFrame", json!({}));
    assert!(refusal(&refused).contains(REASON), "{refused}");
    agent.tool("cmd_video_close", json!({}));
    let in_frames =
        std::fs::read_dir(&frames).unwrap().flatten().filter(|e| e.path().is_file() && !e.file_name().to_string_lossy().starts_with('.')).count();
    assert_eq!(in_frames, 1, "the refused Save Frame wrote nothing");

    // Export as first frames: the photos and the H.264 clip's first frame, the other clip counted.
    let out = ws.path().join("Out");
    let exported = agent.tool("cmd_export_run", json!({ "section": trip, "dir": out, "folderName": "Trip" }));
    assert_eq!(exported["state"], json!("done"), "{exported}");
    let result = &exported["result"];
    let written: Vec<PathBuf> =
        result["written"].as_array().unwrap_or_else(|| panic!("{exported}")).iter().map(|p| PathBuf::from(p.as_str().unwrap())).collect();
    assert_eq!(written.len(), 3, "{exported}");
    for path in &written {
        let file = bytes(path);
        assert!(file.starts_with(&[0xFF, 0xD8, 0xFF]), "{}: a JPEG", path.display());
    }
    let skipped = result["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1, "{exported}");
    assert!(skipped[0]["file"].as_str().is_some_and(|f| f.ends_with("old.mov")), "{exported}");
    assert!(skipped[0]["reason"].as_str().is_some_and(|r| r.contains("MPEG-4 Part 2")), "{exported}");

    // The `export` tool as full videos: the photos as pictures, each clip copied byte for byte.
    let full = agent.tool("export", json!({ "section": trip, "dir": out, "folder_name": "Trip Full", "videos": "full" }));
    assert_eq!((&full["skipped"], &full["failed"]), (&json!([]), &json!([])), "{full}");
    let files = full["files"].as_array().unwrap_or_else(|| panic!("{full}"));
    assert_eq!(files.len(), 4, "{full}");
    for (source, id) in [(&clip_mp4, clip_id), (&old_mov, old_id)] {
        let name = source.file_name().unwrap().to_str().unwrap();
        let copy = files.iter().find(|f| f["path"].as_str().is_some_and(|p| p.ends_with(name))).unwrap_or_else(|| panic!("no {name}: {full}"));
        assert_eq!(copy["id"], json!(id), "the copy names its clip: {full}");
        assert_eq!(bytes(Path::new(copy["path"].as_str().unwrap())), bytes(source), "{name}: copied as it is");
    }
    for (path, original) in &originals {
        assert_eq!(&bytes(path), original, "{}: only read", path.display());
    }
}
