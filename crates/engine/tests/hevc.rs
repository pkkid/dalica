//! HEVC clips in the session (`roadmap.md` M16.13; `docs/video-decoders.md` §8, `formats.md` §1):
//! HEVC is a non-default build feature, as HEIC is. A procedural HEVC clip (every coding tree block
//! of every picture stored as PCM samples, H.265 §7.3.8.7, one slice segment each, in an MP4 of
//! the ISO/IEC 14496-12 boxes under an `hvc1` sample entry; nothing fetched) goes through every
//! path that plays a clip: `video.inspect`, `video.open` and `video.play`, its thumbnail, Export,
//! the slideshow and the collage. Built with `--features hevc` it plays as an H.264 clip does;
//! built without, it answers "This video can't be played (HEVC support isn't included in this
//! build)" everywhere and is never decoded. Every test here runs in both builds and checks the
//! answer of the one it is in: `cargo test -p dalica-engine --test all hevc::` for the default,
//! `--features hevc` for the other (the review's `test hevc` step).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use dalica_core::{PhotoId, VirtualClock};
use dalica_create::slideshow::{Settings, Transition};
use dalica_engine::cmd::slideshow::{Frame, Slideshow, clip_picture};
use dalica_engine::sound::Silent;
use dalica_engine::{CacheDir, CmdError, JobEntry, JobState, Options, Session, Thumb, ThumbOptions};
use dalica_fs::Expect;
use dalica_image::formats::video::frames::{FrameOptions, FrameSource};
use dalica_image::{Limits, Rgba8};
use serde_json::{Value, json};

use crate::common::{jpeg_of, read, virtual_clock};

const PATIENCE: Duration = Duration::from_secs(120);
/// Whether this build decodes HEVC.
const PLAYS: bool = cfg!(feature = "hevc");
/// What every path says of the clip in a build without the decoder.
const REASON: &str = "This video can't be played (HEVC support isn't included in this build)";

// --- An HEVC writer: every coding tree block PCM ------------------------------------------------------

/// Side of a coding tree block (and of the only coding block in it).
const CTB: u32 = 32;
/// The clip's size in coding tree blocks (128 × 96), its frames and those before it lights up.
const CTBS: (u32, u32) = (4, 3);
const WIDTH: u32 = CTB * CTBS.0;
const HEIGHT: u32 = CTB * CTBS.1;
const FRAMES: usize = 10;
const BLACK: usize = 2;
const FRAME_MS: u64 = 40;

/// The luma of frame `k` at (`x`, `y`): black frames first; then a light left half and a right
/// half whose grey names the frame.
fn luma(k: usize, x: u32, _y: u32) -> u8 {
    if k < BLACK {
        16
    } else if x < WIDTH / 2 {
        220
    } else {
        grey(k)
    }
}

/// The grey of frame `k`'s right half.
fn grey(k: usize) -> u8 {
    60 + 8 * k as u8
}

/// The 8-bit value a limited-range luma decodes to.
fn shown(luma: u8) -> f64 {
    ((f64::from(luma) - 16.0) / 219.0 * 255.0).round()
}

mod writer {
    //! Trimmed from the HEIC writer in `tests/save_formats.rs` (itself from
    //! `crates/image/tests/heif.rs`, adapted from LightCraft `crates/heif/src/testdata.rs` at
    //! revision 76b53778, MIT OR Apache-2.0, Copyright (c) 2026 ArtCraft Team and the LightCraft
    //! contributors; `NOTICE`): the same parameter sets at the clip's size, and each picture as one
    //! slice segment per coding tree block instead of one picture per tile.

    use super::{CTB, CTBS, HEIGHT, WIDTH};

    /// Main profile.
    pub const PROFILE: u32 = 1;

    #[derive(Default)]
    struct Bits {
        bytes: Vec<u8>,
        n: usize,
    }

    impl Bits {
        fn put(&mut self, v: u32, len: u32) {
            for i in (0..len).rev() {
                if self.n.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                if (v >> i) & 1 == 1
                    && let Some(b) = self.bytes.last_mut()
                {
                    *b |= 0x80 >> (self.n % 8);
                }
                self.n += 1;
            }
        }

        fn ue(&mut self, v: u32) {
            let x = u64::from(v) + 1;
            let len = 64 - x.leading_zeros();
            self.put(0, len - 1);
            self.put((x & 0xffff_ffff) as u32, len);
        }

        fn align_zero(&mut self) {
            while !self.n.is_multiple_of(8) {
                self.put(0, 1);
            }
        }

        fn trailing(&mut self) {
            self.put(1, 1);
            self.align_zero();
        }
    }

    /// The CABAC arithmetic encoder of H.265 9.3.4.4, for the bins these pictures need.
    struct Cabac {
        low: u32,
        range: u32,
        first: bool,
        outstanding: u32,
    }

    impl Cabac {
        fn new() -> Cabac {
            Cabac { low: 0, range: 510, first: true, outstanding: 0 }
        }

        fn put_bit(&mut self, out: &mut Bits, b: u32) {
            if self.first {
                self.first = false;
            } else {
                out.put(b, 1);
            }
            while self.outstanding > 0 {
                out.put(1 - b, 1);
                self.outstanding -= 1;
            }
        }

        fn renorm(&mut self, out: &mut Bits) {
            while self.range < 256 {
                if self.low < 256 {
                    self.put_bit(out, 0);
                } else if self.low >= 512 {
                    self.low -= 512;
                    self.put_bit(out, 1);
                } else {
                    self.low -= 256;
                    self.outstanding += 1;
                }
                self.range <<= 1;
                self.low <<= 1;
            }
        }

        /// `part_mode` = PART_2Nx2N: the most probable symbol of a fresh context.
        fn part_mode_2nx2n(&mut self, out: &mut Bits) {
            let lps = [128, 176, 208, 240][((self.range >> 6) & 3) as usize];
            self.range -= lps;
            self.renorm(out);
        }

        /// A terminating bin equal to 1 (`pcm_flag`, `end_of_slice_segment_flag`), with the flush.
        fn terminate_one(&mut self, out: &mut Bits) {
            self.range -= 2;
            self.low += self.range;
            self.range = 2;
            self.renorm(out);
            self.put_bit(out, (self.low >> 9) & 1);
            out.put(((self.low >> 7) & 3) | 1, 2);
        }
    }

    fn nal(kind: u8, rbsp: &[u8]) -> Vec<u8> {
        let mut out = vec![kind << 1, 1];
        let mut zeros = 0;
        for &b in rbsp {
            if zeros >= 2 && b <= 3 {
                out.push(3);
                zeros = 0;
            }
            zeros = if b == 0 { zeros + 1 } else { 0 };
            out.push(b);
        }
        out
    }

    fn ptl(w: &mut Bits) {
        w.put(0, 2);
        w.put(0, 1);
        w.put(PROFILE, 5);
        w.put(1 << (31 - PROFILE), 32); // compatible with itself
        w.put(0b1001, 4); // progressive, frame only
        w.put(0, 22);
        w.put(0, 21);
        w.put(0, 1);
        w.put(93, 8); // level 3.1
    }

    /// The VPS, SPS and PPS: 8-bit 4:2:0 at the clip's size, coding tree and coding blocks of
    /// [`CTB`], PCM on, every loop filter off.
    pub fn parameter_sets() -> [Vec<u8>; 3] {
        let mut v = Bits::default();
        v.put(0, 4);
        v.put(3, 2);
        v.put(0, 6);
        v.put(0, 3);
        v.put(1, 1);
        v.put(0xffff, 16);
        ptl(&mut v);
        v.put(0, 1); // vps_sub_layer_ordering_info_present_flag
        v.ue(0);
        v.ue(0);
        v.ue(0);
        v.put(0, 6); // vps_max_layer_id
        v.ue(0); // vps_num_layer_sets_minus1
        v.put(0, 1); // vps_timing_info_present_flag
        v.put(0, 1); // vps_extension_flag
        v.trailing();

        let mut s = Bits::default();
        s.put(0, 4);
        s.put(0, 3);
        s.put(1, 1);
        ptl(&mut s);
        s.ue(0);
        s.ue(1); // 4:2:0
        s.ue(WIDTH);
        s.ue(HEIGHT);
        s.put(0, 1); // conformance_window_flag
        s.ue(0); // 8-bit luma
        s.ue(0); // 8-bit chroma
        s.ue(4);
        s.put(1, 1);
        s.ue(0);
        s.ue(0);
        s.ue(0);
        s.ue(2); // log2_min_luma_coding_block_size_minus3: 32
        s.ue(0); // log2_diff_max_min_luma_coding_block_size: CTB 32
        s.ue(0); // log2_min_luma_transform_block_size_minus2: 4
        s.ue(3); // log2_diff_max_min_luma_transform_block_size: 32
        s.ue(0);
        s.ue(0);
        s.put(0, 1); // scaling_list_enabled_flag
        s.put(0, 1); // amp_enabled_flag
        s.put(0, 1); // sample_adaptive_offset_enabled_flag
        s.put(1, 1); // pcm_enabled_flag
        s.put(7, 4); // pcm_sample_bit_depth_luma_minus1
        s.put(7, 4); // pcm_sample_bit_depth_chroma_minus1
        s.ue(2); // log2_min_pcm_luma_coding_block_size_minus3: 32
        s.ue(0); // log2_diff_max_min_pcm_luma_coding_block_size
        s.put(1, 1); // pcm_loop_filter_disabled_flag
        s.ue(0); // num_short_term_ref_pic_sets
        s.put(0, 1); // long_term_ref_pics_present_flag
        s.put(0, 1); // sps_temporal_mvp_enabled_flag
        s.put(0, 1); // strong_intra_smoothing_enabled_flag
        s.put(0, 1); // vui_parameters_present_flag
        s.put(0, 1); // sps_extension_present_flag
        s.trailing();

        let mut p = Bits::default();
        p.ue(0);
        p.ue(0);
        p.put(0, 6); // dependent slices, output flag, 3 extra header bits, sign hiding
        p.put(0, 1); // cabac_init_present_flag
        p.ue(0);
        p.ue(0);
        p.ue(0); // init_qp_minus26 (se(0) is the same bits)
        p.put(0, 3); // constrained intra, transform skip, cu_qp_delta
        p.ue(0);
        p.ue(0);
        p.put(0, 1);
        p.put(0, 2);
        p.put(0, 1); // transquant_bypass_enabled_flag
        p.put(0, 1); // tiles_enabled_flag
        p.put(0, 1); // entropy_coding_sync_enabled_flag
        p.put(0, 1); // pps_loop_filter_across_slices_enabled_flag
        p.put(1, 1); // deblocking_filter_control_present_flag
        p.put(0, 1); // deblocking_filter_override_enabled_flag
        p.put(1, 1); // pps_deblocking_filter_disabled_flag
        p.put(0, 1);
        p.put(0, 1);
        p.ue(0);
        p.put(0, 1);
        p.put(0, 1);
        p.trailing();
        [nal(32, &v.bytes), nal(33, &s.bytes), nal(34, &p.bytes)]
    }

    /// One coded picture of YCbCr samples `sample(x, y)`, an IDR picture of one I slice segment
    /// per coding tree block: the sample (length-prefixed NAL units).
    pub fn picture(sample: &dyn Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let count = CTBS.0 * CTBS.1;
        let address_bits = u32::BITS - (count - 1).leading_zeros();
        let mut out = Vec::new();
        for at in 0..count {
            let (x0, y0) = ((at % CTBS.0) * CTB, (at / CTBS.0) * CTB);
            let mut w = Bits::default();
            w.put(u32::from(at == 0), 1); // first_slice_segment_in_pic_flag
            w.put(0, 1); // no_output_of_prior_pics_flag
            w.ue(0); // slice_pic_parameter_set_id
            if at > 0 {
                w.put(at, address_bits); // slice_segment_address
            }
            w.ue(2); // I slice
            w.ue(0); // slice_qp_delta, se(0)
            w.trailing(); // byte_alignment()
            let mut c = Cabac::new();
            c.part_mode_2nx2n(&mut w);
            c.terminate_one(&mut w); // pcm_flag
            w.align_zero(); // pcm_alignment_zero_bit
            for y in 0..CTB {
                for x in 0..CTB {
                    w.put(u32::from(sample(x0 + x, y0 + y)[0]), 8);
                }
            }
            for k in 1..3 {
                for y in 0..CTB / 2 {
                    for x in 0..CTB / 2 {
                        w.put(u32::from(sample(x0 + 2 * x, y0 + 2 * y)[k]), 8);
                    }
                }
            }
            let mut c = Cabac::new();
            c.terminate_one(&mut w); // end_of_slice_segment_flag
            w.align_zero();
            let slice = nal(19, &w.bytes);
            out.extend((slice.len() as u32).to_be_bytes());
            out.extend(slice);
        }
        out
    }

    /// The `hvcC` record: Main profile, 4:2:0, 8-bit, four-byte lengths, the three parameter sets.
    pub fn hvcc() -> Vec<u8> {
        let mut h = vec![1u8, PROFILE as u8];
        h.extend_from_slice(&(1u32 << (31 - PROFILE)).to_be_bytes());
        h.extend_from_slice(&[0x90, 0, 0, 0, 0, 0, 93, 0xf0, 0, 0xfc, 0xfd, 0xf8, 0xf8, 0, 0, 0x0f, 3]);
        for (kind, set) in [32u8, 33, 34].into_iter().zip(&parameter_sets()) {
            h.push(0x80 | kind);
            h.extend_from_slice(&1u16.to_be_bytes());
            h.extend_from_slice(&(set.len() as u16).to_be_bytes());
            h.extend_from_slice(set);
        }
        h
    }
}

// --- An MP4 writer ---------------------------------------------------------------------------------------

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

/// The clip: [`FRAMES`] frames of [`WIDTH`] × [`HEIGHT`] at 25 a second, every one a key frame,
/// no sound, in an MP4 whose video sample entry is `hvc1`.
fn clip() -> Vec<u8> {
    let samples: Vec<Vec<u8>> = (0..FRAMES).map(|k| writer::picture(&|x, y| [luma(k, x, y), 128, 128])).collect();
    let mut entry = vec![0u8; 6];
    entry.extend([0, 1]);
    entry.extend([0u8; 16]);
    entry.extend((WIDTH as u16).to_be_bytes());
    entry.extend((HEIGHT as u16).to_be_bytes());
    entry.extend(be(0x48_0000));
    entry.extend(be(0x48_0000));
    entry.extend([0u8; 4]);
    entry.extend([0, 1]);
    entry.extend([0u8; 32]);
    entry.extend([0, 0x18, 0xff, 0xff]);
    entry.extend(bx(b"hvcC", &writer::hvcc()));
    let ftyp = bx(b"ftyp", &[&b"isom"[..], &be(0x200), b"isom", b"hvc1", b"mp41"].concat());
    let mut at = ftyp.len() as u32 + 8;
    let mut stco = be(samples.len() as u32).to_vec();
    let mut stsz = be(0).to_vec();
    stsz.extend(be(samples.len() as u32));
    for s in &samples {
        stco.extend(be(at));
        stsz.extend(be(s.len() as u32));
        at += s.len() as u32;
    }
    let stts = [be(1), be(FRAMES as u32), be(FRAME_MS as u32)].concat();
    // No `stss`: every sample is a sync sample.
    let stbl = [
        full(b"stsd", &[&be(1)[..], &bx(b"hvc1", &entry)].concat()),
        full(b"stts", &stts),
        full(b"stsc", &[be(1), be(1), be(1), be(1)].concat()),
        full(b"stsz", &stsz),
        full(b"stco", &stco),
    ]
    .concat();
    let duration = FRAMES as u32 * FRAME_MS as u32;
    let mut tkhd = vec![0u8; 8];
    tkhd.extend(be(1));
    tkhd.extend([0u8; 4]);
    tkhd.extend(be(duration));
    tkhd.extend([0u8; 16]);
    tkhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    tkhd.extend(be(WIDTH << 16));
    tkhd.extend(be(HEIGHT << 16));
    let mdhd = [&[0u8; 8][..], &be(1000), &be(duration), &[0x55, 0xc4, 0, 0]].concat();
    let hdlr = [&[0u8; 4][..], b"vide", &[0u8; 13]].concat();
    let mdia = bx(b"mdia", &[full(b"mdhd", &mdhd), full(b"hdlr", &hdlr), bx(b"minf", &bx(b"stbl", &stbl))].concat());
    let trak = bx(b"trak", &[full(b"tkhd", &tkhd), mdia].concat());
    let mut mvhd = [&[0u8; 8][..], &be(1000), &be(duration), &be(0x1_0000), &[1, 0], &[0u8; 10]].concat();
    mvhd.extend(IDENTITY.iter().flat_map(|v| v.to_be_bytes()));
    mvhd.extend([0u8; 24]);
    mvhd.extend(be(2));
    [ftyp, bx(b"mdat", &samples.concat()), bx(b"moov", &[full(b"mvhd", &mvhd), trak].concat())].concat()
}

// --- The library ----------------------------------------------------------------------------------------

struct Lib {
    dir: tempfile::TempDir,
    clock: Arc<VirtualClock>,
    s: Session,
    /// a.jpg, b.jpg, clip.mp4 (the HEVC clip).
    ids: [PhotoId; 3],
    /// The paths the thumbnail scans read, in order.
    thumbs_read: Arc<Mutex<Vec<PathBuf>>>,
}

impl Lib {
    /// A library watching `Photos`, whose `Trip` holds two photos and the HEVC clip; `Pictures`
    /// is the user's Pictures folder. The sound output is a silent one on the session's virtual
    /// clock.
    fn new() -> Lib {
        let dir = tempfile::tempdir().unwrap();
        let trip = dir.path().join("Photos").join("Trip");
        dalica_fs::create_dir_all(&trip).unwrap();
        dalica_fs::replace(&trip.join("a.jpg"), &jpeg_of(1, 160, 120), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("b.jpg"), &jpeg_of(2, 120, 160), Expect::Absent).unwrap();
        dalica_fs::replace(&trip.join("clip.mp4"), &clip(), Expect::Absent).unwrap();
        let clock = virtual_clock();
        let options = Options { thumbs: ThumbOptions { cache: CacheDir::Temp, workers: 1, ..ThumbOptions::default() } };
        let mut s = Session::open_with(&dir.path().join("library"), clock.clone(), &options).unwrap();
        s.set_sound(Arc::new(Silent::new(clock.clone())));
        let thumbs_read = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&thumbs_read);
        s.set_thumbnail_scan_hook(Some(Arc::new(move |path: &Path| seen.lock().unwrap_or_else(PoisonError::into_inner).push(path.to_path_buf()))));
        s.set_pictures_dir(Some(dir.path().join("Pictures")));
        s.execute("folders.add", &json!({ "path": dir.path().join("Photos") })).unwrap();
        s.wait_idle();
        let id = |name: &str| s.library().photos().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} listed")).id;
        let ids = [id("a.jpg"), id("b.jpg"), id("clip.mp4")];
        Lib { dir, clock, s, ids, thumbs_read }
    }

    fn clip(&self) -> PhotoId {
        self.ids[2]
    }

    fn run(&mut self, id: &str, p: Value) -> Value {
        self.s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
    }

    /// Runs `id` and waits for its job and the scans it started.
    fn output(&mut self, id: &str, p: Value) -> JobEntry {
        let reply = self.run(id, p);
        let job = reply["job"].as_u64().unwrap_or_else(|| panic!("no job in {reply}"));
        let entry = self.s.wait_job(job, PATIENCE).unwrap();
        assert!(entry.state.is_finished(), "{entry:?}");
        self.s.wait_idle();
        entry
    }

    /// Polls `video.state` (real time) until `done` holds.
    fn until(&mut self, what: &str, done: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let v = self.run("video.state", json!({}));
            if done(&v) {
                return v;
            }
            assert!(start.elapsed() < PATIENCE, "{what}: {v}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Ticks the show (real time) until `done` holds of its frame.
    fn tick_until(&mut self, show: &mut Slideshow, what: &str, done: impl Fn(&Frame) -> bool) -> Frame {
        let start = Instant::now();
        loop {
            let f = show.tick(&mut self.s);
            if done(&f) {
                return f;
            }
            assert!(start.elapsed() < PATIENCE, "{what}: {f:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Whether the thumbnail scans ever read the clip.
    fn clip_read(&self) -> bool {
        self.thumbs_read.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|p| p.ends_with("clip.mp4"))
    }
}

fn disabled(r: Result<Value, CmdError>) -> String {
    match r {
        Err(CmdError::Disabled(why)) => why,
        other => panic!("expected disabled, got {other:?}"),
    }
}

/// The mean of the red channel over a box of the picture, in fractions of its sides.
fn mean(img: &Rgba8, (x0, y0, x1, y1): (f64, f64, f64, f64)) -> f64 {
    let (w, h) = (f64::from(img.width), f64::from(img.height));
    let (xa, xb, ya, yb) = ((x0 * w) as usize, (x1 * w) as usize, (y0 * h) as usize, (y1 * h) as usize);
    let (mut sum, mut n) = (0.0, 0.0);
    for y in ya..yb.max(ya + 1) {
        for x in xa..xb.max(xa + 1) {
            sum += f64::from(img.data[(y * img.width as usize + x) * 4]);
            n += 1.0;
        }
    }
    sum / n
}

/// Whether `img` is frame `k` of the clip: the light left half and the frame's grey on the right.
fn is_frame(img: &Rgba8, k: usize) -> bool {
    let (left, right) = (mean(img, (0.1, 0.2, 0.4, 0.8)), mean(img, (0.6, 0.2, 0.9, 0.8)));
    (left - shown(220)).abs() < 4.0 && (right - shown(grey(k))).abs() < 4.0
}

// --- The tests ---------------------------------------------------------------------------------------

#[test]
fn the_writer_makes_an_hevc_clip_of_the_size_asked() {
    let file = clip();
    let info = dalica_image::formats::video::probe(&file, &Limits::default()).unwrap();
    assert_eq!((info.video_codec.as_deref(), info.size, info.duration_ms), (Some("hvc1"), Some((WIDTH, HEIGHT)), Some(400)));
    let opened = FrameSource::from_bytes(file, &Limits::default(), FrameOptions::default());
    if PLAYS {
        let mut src = opened.unwrap();
        assert_eq!(src.frame_count(), FRAMES);
        for k in [0, BLACK, FRAMES - 1] {
            let f = src.frame(k).unwrap();
            assert_eq!((f.index, f.image.width, f.image.height), (k, WIDTH, HEIGHT));
            if k < BLACK {
                assert!(mean(&f.image, (0.0, 0.0, 1.0, 1.0)) < 3.0, "frame {k} is black");
            } else {
                assert!(is_frame(&f.image, k), "frame {k}");
            }
        }
    } else {
        assert_eq!(opened.err().map(|e| e.to_string()).as_deref(), Some(REASON));
    }
}

/// `video.inspect`, `video.open` and `video.play`: with the decoder the clip plays a frame at a time
/// as the clock reaches it; without, each says why and nothing plays.
#[test]
fn the_player_plays_its_frames_or_says_why_not() {
    let mut lib = Lib::new();
    let clip = lib.clip();
    let v = lib.run("video.inspect", json!({ "id": clip }));
    assert_eq!(v["videoCodec"], "hvc1");
    let refused = dalica_fs::guard::refused();
    let ui = dalica_fs::mark_ui_thread();
    if PLAYS {
        assert_eq!((v["playable"].clone(), v["reason"].clone()), (json!(true), Value::Null), "{v}");
        lib.run("video.open", json!({ "id": clip }));
        let v = lib.until("opened", |v| v["ready"] == true && v["frame"].is_object());
        assert_eq!((v["playable"].clone(), v["frameCount"].clone(), v["frame"]["index"].clone()), (json!(true), json!(FRAMES), json!(0)), "{v}");
        lib.run("video.play", json!({}));
        for k in 1..FRAMES as u64 {
            lib.clock.advance(Duration::from_millis(FRAME_MS));
            let v = lib.until(&format!("frame {k}"), |v| v["frame"]["index"] == k);
            assert_eq!(v["positionMs"], k * FRAME_MS);
            let frame = lib.s.video_frame().expect("a frame shown");
            assert_eq!(frame.index as u64, k);
            if k as usize >= BLACK {
                assert!(is_frame(&frame.image, k as usize), "frame {k} shows its own picture");
            }
        }
        lib.clock.advance(Duration::from_millis(FRAME_MS));
        let v = lib.until("stopped at the end", |v| v["playing"] == false);
        assert_eq!(v["positionMs"], 400);
    } else {
        assert_eq!((v["playable"].clone(), v["reason"].clone()), (json!(false), json!(REASON)), "{v}");
        let v = lib.run("video.open", json!({ "id": clip }));
        assert_eq!((v["playable"].clone(), v["reason"].clone(), v["pending"].clone()), (json!(false), json!(REASON), json!(false)));
        for (id, p) in [("video.play", json!({})), ("video.step", json!({ "frames": 1 })), ("video.seek", json!({ "ms": 100 }))] {
            assert_eq!(disabled(lib.s.execute(id, &p)), REASON, "{id}");
        }
        assert!(lib.s.video_frame().is_none(), "never decoded");
    }
    drop(ui);
    assert_eq!(dalica_fs::guard::refused(), refused, "the owner thread made no fs call");
    lib.run("video.close", json!({}));
}

/// Its thumbnail: the poster frame (the first lit one) with the decoder; without, the video tile,
/// the clip never read.
#[test]
fn its_poster_becomes_its_thumbnail_or_it_keeps_the_video_tile() {
    let mut lib = Lib::new();
    let clip = lib.clip();
    if PLAYS {
        match lib.s.thumbnail(clip, 256) {
            Thumb::Image { image, .. } => {
                assert_eq!((image.width, image.height), (WIDTH, HEIGHT), "never enlarged past the clip");
                assert!(is_frame(&image, BLACK), "the first lit frame, never the black start");
            }
            other => panic!("the clip has its poster: {other:?}"),
        }
        assert!(lib.clip_read());
    } else {
        assert_eq!(lib.s.thumbnail(clip, 256), Thumb::NotScanned, "the video tile");
        assert!(!lib.clip_read(), "the clip is never read");
    }
    assert_eq!(lib.s.thumbnail_failure(clip), None, "never the error badge");
}

/// Export as first frames: the poster as a JPEG with the decoder; without, skipped with the reason.
#[test]
fn export_writes_its_frame_as_a_jpeg_or_skips_it_with_the_reason() {
    let mut lib = Lib::new();
    let out = lib.dir.path().join("Out");
    let reply = lib.run("export.run", json!({ "ids": lib.ids, "dir": out, "folderName": "First", "size": 100 }));
    let (total, left_out) = if PLAYS { (3, 0) } else { (2, 1) };
    assert_eq!((reply["total"].clone(), reply["videos"].clone()), (json!(total), json!(left_out)), "{reply}");
    let done = lib.s.wait_job(reply["job"].as_u64().unwrap(), PATIENCE).unwrap();
    assert_eq!(done.state, JobState::Done, "{done:?}");
    let result = done.result.unwrap();
    let jpeg = out.join("First").join("clip.jpg");
    if PLAYS {
        assert_eq!(done.label, "Exported 3 pictures", "{result}");
        let img = dalica_image::decode(&read(&jpeg), &Limits::default()).unwrap();
        assert_eq!((img.width, img.height), (100, 75), "the poster at the export's size");
        let (left, right) = (mean(&img, (0.1, 0.2, 0.4, 0.8)), mean(&img, (0.6, 0.2, 0.9, 0.8)));
        assert!((left - shown(220)).abs() < 6.0 && (right - shown(grey(BLACK))).abs() < 6.0, "left {left}, right {right}");
    } else {
        assert_eq!(done.label, "Exported 2 pictures; 1 skipped");
        assert_eq!(result["skipped"][0]["reason"], REASON, "{result}");
        assert!(!jpeg.exists());
        assert!(!lib.clip_read(), "the clip is never read for a thumbnail");
    }
}

/// The slideshow: with the decoder it plays the clip's frames as they fall due; without, it holds
/// the clip for the display time as its tile.
#[test]
fn a_slideshow_plays_it_or_holds_its_tile() {
    let mut lib = Lib::new();
    let ids = vec![lib.ids[0], lib.clip(), lib.ids[1]];
    let settings = Settings { display_secs: 3, transition: Transition::Cut, looped: false };
    let mut show = Slideshow::with_settings(&mut lib.s, ids, 1, settings, (320, 240));
    let name = |s: &Session, f: &Frame| f.photo.and_then(|(id, _)| s.library().photo(id)).map(|p| p.name.clone());
    let clip_frame = |f: &Frame| f.clip.and_then(|c| clip_picture(c.key));
    if PLAYS {
        lib.tick_until(&mut show, "the clip plays", |f| f.clip.is_some_and(|c| c.playing));
        for k in 1..FRAMES {
            lib.clock.advance(Duration::from_millis(FRAME_MS));
            let f = lib.tick_until(&mut show, &format!("frame {k}"), |f| clip_frame(f).is_some_and(|p| p.index == k));
            let picture = clip_frame(&f).unwrap();
            assert_eq!(name(&lib.s, &f).as_deref(), Some("clip.mp4"));
            if k >= BLACK {
                assert!(is_frame(&picture.image, k), "frame {k} shows its own picture");
            }
        }
        lib.clock.advance(Duration::from_millis(FRAME_MS));
        let f = lib.tick_until(&mut show, "the next photo", |f| f.clip.is_none());
        assert_eq!(name(&lib.s, &f).as_deref(), Some("b.jpg"), "held to the clip's end, not the display time");
    } else {
        let f = show.tick(&mut lib.s);
        assert_eq!((name(&lib.s, &f).as_deref(), f.clip, show.clip_state(&lib.s)), (Some("clip.mp4"), None, None));
        lib.clock.advance(Duration::from_millis(2_999));
        let f = show.tick(&mut lib.s);
        assert_eq!(name(&lib.s, &f).as_deref(), Some("clip.mp4"), "held for the display time");
        lib.clock.advance(Duration::from_millis(1));
        let f = show.tick(&mut lib.s);
        assert_eq!(name(&lib.s, &f).as_deref(), Some("b.jpg"));
        assert!(!lib.clip_read(), "the clip is never read");
    }
    show.close(&mut lib.s);
}

/// The collage: with the decoder it places the clip's poster as it does an H.264 clip's; without,
/// it leaves the clip out with the notice.
#[test]
fn a_collage_places_its_poster_or_leaves_it_out() {
    let mut lib = Lib::new();
    let [a, b, clip] = lib.ids;
    let opened = lib.run("create.collage", json!({ "ids": [a, clip, b], "seed": "5eed" }));
    if !PLAYS {
        assert_eq!((opened["clips"].clone(), opened["videos"].clone()), (json!(2), json!(1)), "{opened}");
        assert_eq!(opened["notice"], "1 video left out: videos can't go in a collage yet.");
        lib.run("collage.close", json!({ "confirm": true }));
        let why = lib.s.execute("create.collage", &json!({ "ids": [clip] })).unwrap_err().to_string();
        assert!(why.contains("Videos can't go in a collage yet."), "{why}");
        assert!(!lib.clip_read(), "the clip is never read");
        return;
    }
    assert_eq!((opened["clips"].clone(), opened.get("videos")), (json!(3), None), "{opened}");
    let aspects = lib.s.collage_aspects();
    assert!((aspects[1] - f64::from(WIDTH) / f64::from(HEIGHT)).abs() < 1e-9, "at the poster's ratio: {aspects:?}");
    // The clip alone on the page, in a grid without spacing, so its place is known.
    lib.run("collage.unplace", json!({ "ids": [a, b] }));
    lib.run("collage.set", json!({ "style": "grid", "shadows": false }));
    let place = lib.s.collage_placements().into_iter().find(|p| p.id == clip).expect("the clip is placed");
    let entry = lib.output("collage.create", json!({}));
    let jpeg = PathBuf::from(entry.result.unwrap()["path"].as_str().unwrap().to_string());
    let page = dalica_image::decode(&read(&jpeg), &Limits::default()).unwrap();
    let at = |fx: f64| {
        let shown = place.shown;
        let f = (fx - shown.x0()) / shown.width();
        let x = place.centre[0] + (f - 0.5) * place.size[0];
        let y = place.centre[1];
        let (dx, dy) = (place.size[0] * 0.05, place.size[1] * 0.1);
        mean(&page, (x - dx, y - dy, x + dx, y + dy))
    };
    let (left, right) = (at(0.2), at(0.8));
    assert!((left - shown(220)).abs() < 8.0 && (right - shown(grey(BLACK))).abs() < 8.0, "the poster frame: left {left}, right {right}");
    lib.run("collage.close", json!({ "confirm": true }));
}
