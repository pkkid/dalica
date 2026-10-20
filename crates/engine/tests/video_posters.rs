//! Poster frames in the session (`roadmap.md` M15.20; `docs/video-decoders.md`): in a folder of 20
//! photos and 5 clips (H.264 clips made here, every macroblock I_PCM, ITU-T H.264 §7.3.5, in an
//! MP4 of the ISO/IEC 14496-12 boxes; and clips of codecs this build does not decode) every
//! thumbnail fills, the photos' scans before the clips', the playable clips with their first lit
//! frame and the others with the video tile (never read, never the error badge); the edit view's
//! preview of a playable clip is its poster and of the others their reason; the face scan finds
//! the faces on a playable clip's poster frame and never reads an unplayable one; the owner thread
//! makes no `fs` call.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, SystemClock};
use dalica_engine::{CacheDir, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_image::Rgba8;
use serde_json::json;

use crate::common::jpeg_of;

const PATIENCE: Duration = Duration::from_secs(120);

// --- An H.264 clip -------------------------------------------------------------------------------

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
        let len = 64 - (v + 1).leading_zeros();
        self.u(len - 1, 0).u(len, v + 1)
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

/// A NAL unit with its header and emulation prevention.
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

/// The clip's size in macroblocks (256 × 192) and frames; frames before the picture lights up.
const MBS: (usize, usize) = (16, 12);
const FRAMES: usize = 4;
const BLACK: usize = 2;

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

/// The luma of frame `k` at (`x`, `y`): black frames first; then grey, with a light face painted
/// on the left half where the face scan's planted detector reports one.
fn luma(k: usize, x: usize, y: usize) -> u8 {
    if k < BLACK {
        16
    } else if (56..156).contains(&y) && x < 112 {
        220
    } else {
        150
    }
}

/// Picture `k` as one I slice of I_PCM macroblocks, chroma neutral.
fn slice(k: usize) -> Vec<u8> {
    let mut b = Bits::default();
    b.ue(0).ue(7).ue(0).u(4, k as u64 % 16);
    if k == 0 {
        b.ue(0).u(1, 0).u(1, 0);
    } else {
        b.u(1, 0);
    }
    b.se(0).ue(1);
    for my in 0..MBS.1 {
        for mx in 0..MBS.0 {
            b.ue(25).align();
            for y in 0..16 {
                for x in 0..16 {
                    b.u(8, u64::from(luma(k, mx * 16 + x, my * 16 + y)));
                }
            }
            for _ in 0..128 {
                b.u(8, 128);
            }
        }
    }
    nal(if k == 0 { 3 } else { 2 }, if k == 0 { 5 } else { 1 }, &b.trailing())
}

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = u32::try_from(payload.len() + 8).unwrap().to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(payload);
    v
}

fn full_box(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    bx(kind, &[&[0u8, 0, 0, 0][..], payload].concat())
}

fn be(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

const MATRIX: [i32; 9] = [0x1_0000, 0, 0, 0, 0x1_0000, 0, 0, 0, 0x4000_0000];

/// An MP4 of [`FRAMES`] frames at 25 fps whose video sample entry is `code` (`avc1`: decodable).
fn clip(code: &[u8; 4]) -> Vec<u8> {
    let (s, p) = (sps(), pps());
    let samples: Vec<Vec<u8>> = (0..FRAMES)
        .map(|k| {
            let n = slice(k);
            [&be(n.len() as u32)[..], &n].concat()
        })
        .collect();
    let (w, h) = ((MBS.0 * 16) as u16, (MBS.1 * 16) as u16);
    let mut entry = vec![0u8; 6];
    entry.extend([0, 1]);
    entry.extend([0u8; 16]);
    entry.extend(w.to_be_bytes());
    entry.extend(h.to_be_bytes());
    entry.extend(be(0x48_0000));
    entry.extend(be(0x48_0000));
    entry.extend([0u8; 4]);
    entry.extend([0, 1]);
    entry.extend([0u8; 32]);
    entry.extend([0, 0x18, 0xff, 0xff]);
    let mut avcc = vec![1, 66, 0xc0, 30, 0xff, 0xe1];
    avcc.extend((s.len() as u16).to_be_bytes());
    avcc.extend(&s);
    avcc.push(1);
    avcc.extend((p.len() as u16).to_be_bytes());
    avcc.extend(&p);
    entry.extend(bx(b"avcC", &avcc));
    let ftyp = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"avc1", b"mp41"].concat());
    let mut at = ftyp.len() as u32 + 8;
    let mut stco = be(samples.len() as u32).to_vec();
    for s in &samples {
        stco.extend(be(at));
        at += s.len() as u32;
    }
    let mut stts = be(1).to_vec();
    stts.extend(be(FRAMES as u32));
    stts.extend(be(40));
    let mut stsz = be(0).to_vec();
    stsz.extend(be(FRAMES as u32));
    for s in &samples {
        stsz.extend(be(s.len() as u32));
    }
    let stbl = [
        full_box(b"stsd", &[&be(1)[..], &bx(code, &entry)].concat()),
        full_box(b"stts", &stts),
        full_box(b"stsc", &[be(1), be(1), be(1), be(1)].concat()),
        full_box(b"stsz", &stsz),
        full_box(b"stco", &stco),
        full_box(b"stss", &[be(1), be(1)].concat()),
    ]
    .concat();
    let duration = FRAMES as u32 * 40;
    let mut tkhd = vec![0u8; 36];
    tkhd.extend(MATRIX.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(u32::from(w) << 16));
    tkhd.extend(be(u32::from(h) << 16));
    let mdhd = [&[0u8; 8][..], &be(1000), &be(duration), &[0x55, 0xc4, 0, 0]].concat();
    let hdlr = [&[0u8; 4][..], b"vide", &[0u8; 13]].concat();
    let mdia = bx(b"mdia", &[full_box(b"mdhd", &mdhd), full_box(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    let trak = bx(b"trak", &[full_box(b"tkhd", &tkhd), mdia].concat());
    let mut mvhd = [&[0u8; 8][..], &be(1000), &be(duration), &be(0x1_0000), &[1, 0], &[0u8; 10]].concat();
    mvhd.extend(MATRIX.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(2));
    [ftyp, bx(b"mdat", &samples.concat()), bx(b"moov", &[full_box(b"mvhd", &mvhd), trak].concat())].concat()
}

// --- The library ----------------------------------------------------------------------------------

/// The clips that play, and those this build does not decode (with the codec named in their
/// reason). HEVC is among the second only when `dalica-image`'s `hevc` feature is off.
/// A clip that plays: its name and bytes.
type Playable = (&'static str, Vec<u8>);
/// A clip that does not: its name, bytes and what its reason names.
type Refused = (&'static str, Vec<u8>, &'static str);

fn clips() -> (Vec<Playable>, Vec<Refused>) {
    let playable = vec![("clip1.mp4", clip(b"avc1")), ("clip2.mov", clip(b"avc1")), ("clip3.m4v", clip(b"avc1"))];
    let mut refused = vec![("prores.mov", clip(b"apcn"), "ProRes")];
    if dalica_thumbs::video::plays("hvc1") {
        refused.push(("mpeg4.mp4", clip(b"mp4v"), "MPEG-4 Part 2"));
    } else {
        refused.push(("hevc.mov", clip(b"hvc1"), "HEVC support isn't included in this build"));
    }
    (playable, refused)
}

/// A gate the thumbnail scans wait at (through `Session::set_thumbnail_scan_hook`) until it is
/// opened (60 s at most), recording the paths they read in order.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<Held>, Condvar)>);

/// Whether the gate is open, and the paths the scans read.
type Held = (bool, Vec<PathBuf>);

impl Gate {
    fn hook(&self) -> dalica_engine::thumbs::ScanHook {
        let gate = self.clone();
        Arc::new(move |path: &Path| {
            let (lock, cv) = &*gate.0;
            let mut g = lock.lock().unwrap_or_else(PoisonError::into_inner);
            g.1.push(path.to_path_buf());
            cv.notify_all();
            let _ = cv.wait_timeout_while(g, Duration::from_secs(60), |g| !g.0);
        })
    }

    fn read(&self) -> Vec<PathBuf> {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner).1.clone()
    }

    fn open(&self) {
        let (lock, cv) = &*self.0;
        lock.lock().unwrap_or_else(PoisonError::into_inner).0 = true;
        cv.notify_all();
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
}

fn is_clip(name: &str) -> bool {
    !name.ends_with(".jpg")
}

/// The mean of the red channel over a box of the picture, in fractions of its sides.
fn mean(img: &Rgba8, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> f64 {
    let (w, h) = (img.width as usize, img.height as usize);
    let (xa, xb, ya, yb) = ((x0 * w as f64) as usize, (x1 * w as f64) as usize, (y0 * h as f64) as usize, (y1 * h as f64) as usize);
    let (mut sum, mut n) = (0.0, 0.0);
    for y in ya..yb {
        for x in xa..xb {
            sum += f64::from(img.data[(y * w + x) * 4]);
            n += 1.0;
        }
    }
    sum / n
}

/// The 8-bit grey a limited-range luma decodes to.
fn grey(luma: u8) -> f64 {
    ((f64::from(luma) - 16.0) / 219.0 * 255.0).round()
}

#[test]
fn a_folder_of_photos_and_clips_fills_every_thumbnail_photos_first() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Photos").join("Party");
    dalica_fs::create_dir_all(&folder).unwrap();
    let (playable, refused) = clips();
    // The clips sort before the photos: their scans come last by priority, not by order.
    for (name, bytes) in playable.iter().map(|(n, b)| (*n, b)).chain(refused.iter().map(|(n, b, _)| (*n, b))) {
        dalica_fs::replace(&folder.join(format!("0-{name}")), bytes, Expect::Absent).unwrap();
    }
    for i in 0..20 {
        dalica_fs::replace(&folder.join(format!("IMG_{i:04}.jpg")), &jpeg_of(i, 120, 90), Expect::Absent).unwrap();
    }
    let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 1, ..ThumbOptions::default() } };
    let mut s = Session::open_with(&dir.path().join("library"), Arc::new(SystemClock), &options).unwrap();
    let gate = Gate::default();
    s.set_thumbnail_scan_hook(Some(gate.hook()));
    s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
    // Hold the first scan until every other one is queued: 20 photos and the 3 playable clips.
    let start = Instant::now();
    while !(gate.read().len() == 1 && s.thumbnails_pending() == 23) {
        s.poll();
        assert!(start.elapsed() < PATIENCE, "the scans were never all queued: {} pending", s.thumbnails_pending());
        std::thread::sleep(Duration::from_millis(2));
    }
    gate.open();
    let started = Instant::now();
    s.wait_idle();
    let fill_ms = started.elapsed().as_secs_f64() * 1000.0;
    // Photos first: after the one scan that started before the rest were queued, every photo's
    // scan ran before any clip's.
    let read: Vec<String> = gate.read().iter().map(|p| name_of(p)).collect();
    assert_eq!(read.len(), 23, "{read:?}");
    let after: Vec<bool> = read[1..].iter().map(|n| is_clip(n)).collect();
    let first_clip = after.iter().position(|c| *c).unwrap();
    assert!(after[first_clip..].iter().all(|c| *c), "photos first, then clips: {read:?}");
    for (name, _, _) in &refused {
        assert!(!read.iter().any(|r| r.ends_with(name)), "{name} is never read");
    }

    let id = |s: &Session, name: &str| s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name}")).id;
    let photos: Vec<PhotoId> = (0..20).map(|i| id(&s, &format!("IMG_{i:04}.jpg"))).collect();
    let good: Vec<PhotoId> = playable.iter().map(|(n, _)| id(&s, &format!("0-{n}"))).collect();
    let bad: Vec<(PhotoId, &str)> = refused.iter().map(|(n, _, why)| (id(&s, &format!("0-{n}")), *why)).collect();
    let refused_fs = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    for &p in &photos {
        assert!(matches!(s.thumbnail(p, 128), Thumb::Image { .. }), "every photo has its thumbnail");
    }
    for &c in &good {
        match s.thumbnail(c, 256) {
            Thumb::Image { image, covers, .. } => {
                assert!(covers && image.width == 256 && image.height == 192);
                // The first lit frame: its grey and its face, never the black start.
                assert!((mean(&image, (0.6, 0.0, 1.0, 0.25)) - grey(150)).abs() < 4.0);
                assert!((mean(&image, (0.05, 0.35, 0.4, 0.75)) - grey(220)).abs() < 4.0);
            }
            other => panic!("a playable clip has its poster: {other:?}"),
        }
        assert_eq!(s.thumbnail_failure(c), None);
    }
    for &(c, _) in &bad {
        assert_eq!(s.thumbnail(c, 256), Thumb::NotScanned, "the video tile");
        assert_eq!(s.thumbnail_failure(c), None, "never the error badge");
    }
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused_fs, "the owner thread made no fs call");
    let stats = s.thumbnail_stats();
    assert_eq!((stats.scanned, stats.failed), (23, 0), "{stats:?}");

    // The edit view's preview: a playable clip's poster at the screen size; the others' reason.
    let ui = dalica_fs::mark_ui_thread();
    s.open_preview(good[0], (800, 600)).unwrap();
    assert!(s.wait_preview(PATIENCE));
    let preview = s.preview().expect("the poster as the preview").image.clone();
    assert_eq!((preview.width, preview.height), (256, 192), "never enlarged past the clip");
    assert!((mean(&preview, (0.05, 0.35, 0.4, 0.75)) - grey(220)).abs() < 4.0);
    for &(c, why) in &bad {
        s.open_preview(c, (800, 600)).unwrap();
        assert!(s.wait_preview(PATIENCE));
        let failure = s.preview_failure().unwrap_or_default().to_string();
        assert!(failure.starts_with("This video can't be played") && failure.contains(why), "{failure}");
    }
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused_fs, "the owner thread made no fs call");
    s.close_preview();

    // Refresh Thumbnails (of the folder of the photos named) remakes the photos and the playable
    // clips, never the others.
    let refreshed = s.execute("section.refreshThumbs", &json!({ "ids": [bad[0].0] })).unwrap();
    assert_eq!(refreshed["photos"], 23, "{refreshed}");
    s.wait_idle();
    assert!(!gate.read().iter().any(|p| refused.iter().any(|(n, _, _)| p.ends_with(n))));
    eprintln!("20 photos and 3 posters filled in {fill_ms:.0} ms on one worker");
}

#[test]
fn the_face_scan_finds_a_face_on_a_playable_clips_poster_and_never_reads_the_others() {
    let (lib, disk) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let models = crate::face_scan::models(disk.path(), false);
    let folder = disk.path().join("Clips");
    dalica_fs::create_dir_all(&folder).unwrap();
    let (playable, refused) = clips();
    dalica_fs::replace(&folder.join("party.mp4"), &playable[0].1, Expect::Absent).unwrap();
    for (name, bytes, _) in &refused {
        dalica_fs::replace(&folder.join(name), bytes, Expect::Absent).unwrap();
    }
    let mut s = crate::common::open(lib.path());
    s.execute("folders.add", &json!({ "path": folder })).unwrap();
    s.wait_idle();
    let refused_fs = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    crate::face_scan::scan(&mut s, &models);
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused_fs, "the owner thread made no fs call");
    let stat = |s: &mut Session, key: &str| crate::face_scan::stat(s, key);
    assert_eq!((stat(&mut s, "started"), stat(&mut s, "read"), stat(&mut s, "failed")), (1, 1, 0), "the playable clip, never the others");
    let lib = s.library();
    let party = lib.photos().find(|p| p.name == "party.mp4").unwrap().id;
    let faces = lib.faces(party);
    assert_eq!(faces.len(), 2, "the planted detector's two faces, on the poster frame");
    assert!(faces.iter().any(|f| f.rect.x0() < 0.5), "the one drawn on the left");
    for (name, _, _) in &refused {
        let id = lib.photos().find(|p| p.name == *name).unwrap().id;
        assert!(lib.faces(id).is_empty(), "{name}");
    }
}
