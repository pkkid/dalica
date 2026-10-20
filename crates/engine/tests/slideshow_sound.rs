//! Videos and music in the slideshow (`roadmap.md` M15.27; `viewing.md` §1.1, §1.5), on the silent
//! `Sound` (recording what it is given) and a virtual clock: a show of a photo, a 2 s clip and a
//! photo holds the clip 2 s and shows its frames as they fall due; with Play video sound on the
//! clip's sound is what the output records, off it records nothing, and a clip whose sound Dalica
//! cannot decode plays muted and says why; a clip of a codec Dalica cannot play is held for the
//! display time as its tile; the music folder's files play in name order and loop, a file that does
//! not play is skipped with its reason, and the music fades out when the show ends. The clips are
//! H.264 made here with every macroblock I_PCM (ITU-T H.264 §7.3.5), muxed by hand from ISO/IEC
//! 14496-12 (the writer of `video_play.rs`); the music is 16-bit WAV written here.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, VirtualClock};
use dalica_create::slideshow::{Settings, State, Transition};
use dalica_engine::Session;
use dalica_engine::cmd::slideshow::{Frame, Slideshow, clip_picture};
use dalica_engine::sound::{Format, Silent, Sound};
use dalica_fs::Expect;
use dalica_image::formats::audio::CANNOT_PLAY;
use serde_json::json;

use crate::common::{jpeg, open_with, virtual_clock};

const PATIENCE: Duration = Duration::from_secs(120);
const FIT: (u32, u32) = (320, 240);

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

/// A sound entry: AAC-LC mono at 48 kHz (`mp4a`), or another code.
fn sound_entry(code: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 6];
    p.extend([0, 1]);
    p.extend([0u8; 8]);
    p.extend([0, 1, 0, 16, 0, 0, 0, 0]);
    p.extend(be(48_000 << 16));
    let esds = [3, 0x19, 0, 1, 0, 4, 0x11, 0x40, 0x15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5, 2, 0x11, 0x88, 6, 1, 2];
    p.extend(full(b"esds", &esds));
    bx(code, &p)
}

/// Two seconds of 160 × 96 at 25 frames a second (a key frame every 10) with a sound track; the
/// video's sample entry `video` and the sound's `sound`.
fn clip(video: &[u8; 4], sound: &[u8; 4]) -> Vec<u8> {
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
            entry: sound_entry(sound),
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

// --- Music: 16-bit mono WAV ----------------------------------------------------------------------------

/// `frames` frames of the constant `value` at 8 kHz, mono.
fn wav(value: f32, frames: usize) -> Vec<u8> {
    let data = frames * 2;
    let mut out = b"RIFF".to_vec();
    out.extend(((36 + data) as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(8_000u32.to_le_bytes());
    out.extend(16_000u32.to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data as u32).to_le_bytes());
    let v = (value * 32_767.0) as i16;
    for _ in 0..frames {
        out.extend(v.to_le_bytes());
    }
    out
}

// --- The library -----------------------------------------------------------------------------------------

struct Lib {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    sound: Arc<Silent>,
    s: Session,
}

impl Lib {
    /// `Trip` holds two photos and four clips: `clip.mp4` (H.264 and AAC), `muted.mp4` (AC-3 sound)
    /// and `old.mov` (MPEG-4 Part 2); `Music` (not watched) holds 1, 2 and 3.wav (0.1 s each of 0.1,
    /// 0.2 and 0.3), a damaged `2b.mp3` and a text file. The sound output is a silent one on the
    /// session's virtual clock.
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Photos").join("Trip");
        dalica_fs::create_dir_all(&trip).unwrap();
        let put = |path: PathBuf, bytes: Vec<u8>| dalica_fs::replace(&path, &bytes, Expect::Absent).unwrap();
        put(trip.join("a.jpg"), jpeg(1));
        put(trip.join("c.jpg"), jpeg(2));
        put(trip.join("clip.mp4"), clip(b"avc1", b"mp4a"));
        put(trip.join("muted.mp4"), clip(b"avc1", b"ac-3"));
        put(trip.join("old.mov"), clip(b"mp4v", b"mp4a"));
        let music = dir.path().join("Music");
        dalica_fs::create_dir_all(&music).unwrap();
        put(music.join("1.wav"), wav(0.1, 800));
        put(music.join("2.wav"), wav(0.2, 800));
        put(music.join("2b.mp3"), vec![0xff, 0xfb, 0x90, 0x00, 1, 2, 3]);
        put(music.join("3.wav"), wav(0.3, 800));
        put(music.join("notes.txt"), b"not music".to_vec());
        let clock = virtual_clock();
        let mut s = open_with(&dir.path().join("library"), clock.clone());
        let sound = Arc::new(Silent::new(clock.clone()));
        s.set_sound(sound.clone());
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        Lib { dir, clock, sound, s }
    }

    fn music(&self) -> PathBuf {
        self.dir.path().join("Music")
    }

    fn id(&self, name: &str) -> PhotoId {
        self.s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id
    }

    /// Play video sound and the music (with its folder) as given.
    fn sound_settings(&mut self, video_sound: bool, music: Option<&Path>) {
        self.s.change_settings(|st| {
            let mut v = st.slideshow();
            v.video_sound = video_sound;
            v.music = music.is_some();
            v.music_folder = music.map(Path::to_path_buf);
            st.set_slideshow(&v);
        });
    }

    /// A show of `names` from the first, 3 s each with Cut.
    fn show(&mut self, names: &[&str], display_secs: u32) -> Slideshow {
        let ids = names.iter().map(|n| self.id(n)).collect();
        let settings = Settings { display_secs, transition: Transition::Cut, looped: false };
        Slideshow::with_settings(&mut self.s, ids, 0, settings, FIT)
    }

    /// Ticks the show (real time) until `done` holds of its frame.
    fn until(&mut self, show: &mut Slideshow, what: &str, done: impl Fn(&Lib, &Slideshow, &Frame) -> bool) -> Frame {
        let start = Instant::now();
        loop {
            let f = show.tick(&mut self.s);
            if done(self, show, &f) {
                return f;
            }
            assert!(start.elapsed() < PATIENCE, "{what}: {f:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn advance(&self, ms: u64) {
        self.clock.advance(Duration::from_millis(ms));
    }

    fn shown(&self, f: &Frame) -> Option<String> {
        f.photo.and_then(|(id, _)| self.s.library().photo(id)).map(|p| p.name.clone())
    }
}

/// The index of the clip frame the frame `f` shows, when it shows one.
fn clip_frame(f: &Frame) -> Option<usize> {
    f.clip.and_then(|c| clip_picture(c.key)).map(|p| p.index)
}

// --- The tests -------------------------------------------------------------------------------------------

/// A photo, a 2 s clip and a photo: the clip is held 2 s from when it starts playing, its frames
/// shown as they fall due, its sound (Play video sound on) what the output records.
#[test]
fn a_clip_is_held_to_its_end_and_plays_its_sound_with_video_sound_on() {
    let mut lib = Lib::new();
    lib.sound_settings(true, None);
    let mut show = lib.show(&["a.jpg", "clip.mp4", "c.jpg"], 3);
    let f = show.tick(&mut lib.s);
    assert_eq!((lib.shown(&f).as_deref(), f.clip), (Some("a.jpg"), None));
    lib.advance(3_000);
    let f = lib.until(&mut show, "the clip plays", |_, sh, f| f.clip.is_some_and(|c| c.playing) && sh.show().state() == State::Playing);
    assert_eq!(lib.shown(&f).as_deref(), Some("clip.mp4"));
    assert!(f.clip.unwrap().sound && show.show().held());
    assert_eq!(lib.sound.format(), Some(Format { rate: 48_000, channels: 1 }), "the sound opened at the clip's format");
    // A frame at a time, each when it is due, the show on the clip for its 2 s.
    for k in 1..=49u64 {
        lib.advance(FRAME_MS);
        let f = lib.until(&mut show, &format!("frame {k}"), |_, _, f| clip_frame(f) == Some(k as usize));
        assert_eq!(lib.shown(&f).as_deref(), Some("clip.mp4"), "held at {} ms", k * FRAME_MS);
    }
    lib.until(&mut show, "its sound queued", |l, _, _| l.sound.samples().len() == 96_000);
    lib.advance(FRAME_MS);
    let f = lib.until(&mut show, "the next photo", |l, _, f| l.shown(f).as_deref() == Some("c.jpg"));
    assert_eq!(f.clip, None);
    assert_eq!(lib.sound.format(), None, "the clip's sound closed at its end");
    assert_eq!(lib.sound.samples().len(), 96_000, "the clip's 2 s of sound, from its decoder");
    // The photo after it has its own display time.
    lib.advance(2_999);
    let f = show.tick(&mut lib.s);
    assert_eq!(lib.shown(&f).as_deref(), Some("c.jpg"));
    show.close(&mut lib.s);
}

/// Play video sound off: the clip plays and the output records nothing; on, a clip whose sound
/// Dalica cannot decode plays muted, says why, and records nothing either. Pausing the show pauses
/// the clip.
#[test]
fn video_sound_off_records_silence_and_a_muted_clip_says_why() {
    let mut lib = Lib::new();
    lib.sound_settings(false, None);
    let mut show = lib.show(&["clip.mp4"], 3);
    lib.until(&mut show, "the clip plays", |_, _, f| f.clip.is_some_and(|c| c.playing));
    lib.advance(1_000);
    let f = lib.until(&mut show, "a second in", |_, _, f| clip_frame(f) == Some(25));
    assert!(!f.clip.unwrap().sound);
    show.show_mut().play_pause();
    let f = lib.until(&mut show, "paused", |_, _, f| f.clip.is_some_and(|c| !c.playing));
    lib.advance(5_000);
    let g = show.tick(&mut lib.s);
    assert_eq!((g.clip.unwrap().position_ms, lib.shown(&g)), (f.clip.unwrap().position_ms, lib.shown(&f)), "paused, it waits");
    assert!(lib.sound.opens().is_empty() && lib.sound.samples().is_empty(), "silence: nothing played");
    show.close(&mut lib.s);

    lib.sound_settings(true, None);
    let mut show = lib.show(&["muted.mp4"], 3);
    lib.until(&mut show, "the muted clip plays", |_, _, f| f.clip.is_some_and(|c| c.playing));
    let state = show.clip_state(&lib.s).unwrap();
    assert_eq!(state.muted.as_deref(), Some("This clip plays without sound (AC-3 sound can't be played)"));
    assert!(!state.sound && lib.sound.opens().is_empty(), "nothing played");
    show.close(&mut lib.s);
}

/// A clip of a codec Dalica cannot play stays as its tile for the display time.
#[test]
fn a_clip_of_a_failed_codec_is_held_for_the_display_time_as_its_tile() {
    let mut lib = Lib::new();
    let mut show = lib.show(&["a.jpg", "old.mov", "c.jpg"], 3);
    lib.advance(3_000);
    let f = show.tick(&mut lib.s);
    assert_eq!((lib.shown(&f).as_deref(), f.clip, show.clip_state(&lib.s)), (Some("old.mov"), None, None));
    lib.advance(2_999);
    let f = show.tick(&mut lib.s);
    assert_eq!(lib.shown(&f).as_deref(), Some("old.mov"));
    lib.advance(1);
    let f = show.tick(&mut lib.s);
    assert_eq!(lib.shown(&f).as_deref(), Some("c.jpg"));
    show.close(&mut lib.s);
}

/// The music folder's files play in name order and loop (the damaged one skipped with its reason,
/// the text file never tried), and the music fades out when the show ends.
#[test]
fn the_music_folder_plays_in_name_order_loops_and_fades_out_at_the_end() {
    let mut lib = Lib::new();
    let folder = lib.music();
    lib.sound_settings(false, Some(&folder));
    let mut show = lib.show(&["a.jpg"], 2);
    for _ in 0..20 {
        lib.advance(50);
        lib.until(&mut show, "the music queued ahead", |l, _, _| l.sound.samples().len() as u64 / 8 >= l.sound.clock_ms() + 200);
    }
    let samples = lib.sound.samples();
    assert_eq!(lib.sound.opens(), [Format { rate: 8_000, channels: 1 }], "one stream for files of one format");
    assert!(samples.len() >= 4 * 800, "{}", samples.len());
    for (k, want) in [0.1f32, 0.2, 0.3, 0.1].iter().enumerate() {
        let part = &samples[k * 800..(k + 1) * 800];
        assert!(part.iter().all(|v| (v - want).abs() < 1e-3), "part {k} is {want}");
    }
    let music = show.music().unwrap();
    let names: Vec<String> = music.played().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names.get(..4), Some(&["1.wav", "2.wav", "3.wav", "1.wav"].map(String::from)[..]), "name order, looping");
    let skipped = music.skipped();
    assert!(!skipped.is_empty() && skipped.iter().all(|(p, why)| p.ends_with("2b.mp3") && why.starts_with(CANNOT_PLAY)), "{skipped:?}");
    // The show ends at 2 s: the music fades to silence and its output closes.
    lib.advance(1_000);
    lib.until(&mut show, "faded out", |l, sh, _| {
        l.advance(20);
        sh.music().is_some_and(|m| m.is_done())
    });
    assert_eq!(show.show().state(), State::Ended);
    let samples = lib.sound.samples();
    let tail = &samples[samples.len().saturating_sub(100)..];
    assert!(tail.iter().all(|v| v.abs() < 0.01), "faded to silence");
    assert_eq!(lib.sound.format(), None, "the output closed");
    show.close(&mut lib.s);
}
