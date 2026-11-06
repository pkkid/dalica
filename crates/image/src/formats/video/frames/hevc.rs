//! HEVC frames through `rusty_h265` (=0.6.2 with `simd`; `docs/video-decoders.md` §2), only in
//! builds with the `hevc` feature.
//!
//! As for H.264 (`h264.rs`), Dalica's own part is the gate: every SPS (from the `hvcC` record and
//! in band) is parsed with the crate's own parser first, and one past the frame limits refuses
//! the clip before the decoder allocates for it; the colours the SPS signals (its VUI) describe the
//! frames, which the crate does not carry. Layers above the base (Dolby Vision's enhancement,
//! MV-HEVC) are left out by the crate. Every call runs inside `catch_unwind`. A seek starts a new
//! decoder primed with the parameter sets seen so far.
//!
//! The crate is Dalica's vendored copy (`vendor/rusty_h265`, M16.14): with
//! [`FrameOptions::threads`](super::FrameOptions) above one, a picture's wavefront rows and its
//! in-loop filters decode on that many threads, and the decoder runs on a thread of its own
//! ([`Worker`]) up to [`AHEAD`] samples ahead of the frames asked for, so the next picture
//! decodes while the caller converts this one. Together they take a phone's 1080p30 frame under
//! the playback bar (`docs/video-decoders.md` §1). With no threads it decodes on the caller's.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use rusty_h265::{Decoder, Frame};

use super::Gate;
use super::yuv::{Colour, Picture, Plane, Samples};
use crate::sniff::{Format, VideoKind};
use crate::{Error, Result};

/// NAL unit types: the parameter sets.
const VPS: u8 = 32;
const SPS: u8 = 33;
const PPS: u8 = 34;

/// Samples (access units) the decoder's thread may be given ahead of the frames taken.
pub const AHEAD: usize = super::HEVC_AHEAD;

pub struct Hevc {
    engine: Engine,
    length_size: usize,
    /// The parameter sets seen so far, by type and id order, to prime a new decoder.
    params: Vec<Vec<u8>>,
    colour: Colour,
    kind: VideoKind,
    /// Threads a picture decodes on (`FrameOptions::threads`; the vendored crate's wavefront
    /// rows and in-loop filters, `vendor/rusty_h265/README.dalica.md`).
    threads: usize,
}

impl Hevc {
    pub fn new(length_size: usize, params: &[Vec<u8>], threads: usize, kind: VideoKind, gate: &mut Gate) -> Result<Hevc, Refusal> {
        let mut h = Hevc { engine: Engine::new(threads, kind), length_size, params: Vec::new(), colour: Colour::UNSPECIFIED, kind, threads };
        for p in params {
            h.feed(p, None, gate)?;
        }
        Ok(h)
    }

    /// Feeds one NAL unit: a parameter set is remembered (an SPS checked first).
    fn feed(&mut self, nal: &[u8], pts: Option<i64>, gate: &mut Gate) -> Result<(), Refusal> {
        let Some(&b0) = nal.first() else { return Ok(()) };
        let kind = (b0 >> 1) & 0x3f;
        let layer = ((b0 & 1) << 5) | (nal.get(1).copied().unwrap_or(0) >> 3);
        if layer == 0 && kind == SPS {
            let rbsp = guard(self.kind, || rusty_h265::nal::unescape(nal.get(2..).unwrap_or_default()))?;
            let sps = match guard(self.kind, || rusty_h265::ps::parse_sps(&rbsp.data))? {
                Ok(sps) => sps,
                Err(rusty_h265::Error::Unsupported(what)) => return Err(Refusal::Unsupported(what)),
                Err(e) => return Err(Refusal::Error(Error::Malformed(format!("an HEVC sequence parameter set: {e}")))),
            };
            let chroma_halves = match sps.chroma_format_idc {
                0 => 0,
                1 => 1,
                2 => 2,
                _ => 4,
            };
            gate.admit(sps.width, sps.height, u32::from(sps.bit_depth_luma.max(sps.bit_depth_chroma)), chroma_halves, self.threads)?;
            if let Some(v) = &sps.vui {
                self.colour = Colour {
                    primaries: v.colour_primaries,
                    transfer: v.transfer_characteristics,
                    matrix: v.matrix_coeffs,
                    full_range: v.video_full_range_flag,
                };
            } else {
                self.colour = Colour::UNSPECIFIED;
            }
        }
        if layer == 0 && matches!(kind, VPS | SPS | PPS) && !self.params.iter().any(|p| p == nal) {
            if self.params.len() >= 64 {
                self.params.remove(0);
            }
            self.params.push(nal.to_vec());
        }
        self.engine.send(Job::Nal(nal.to_vec(), pts))
    }

    /// Feeds one sample (an access unit), shown as `pts`.
    pub fn push(&mut self, sample: &[u8], pts: i64, gate: &mut Gate) -> Result<(), Refusal> {
        let mut rest = sample;
        let mut first = true;
        while rest.len() > self.length_size {
            let (head, tail) = rest.split_at(self.length_size);
            let n = head.iter().fold(0usize, |v, &b| (v << 8) | usize::from(b));
            let Some(nal) = tail.get(..n) else {
                return Err(Refusal::Error(Error::Malformed("an HEVC NAL unit runs past its sample".into())));
            };
            rest = tail.get(n..).unwrap_or_default();
            self.feed(nal, first.then_some(pts), gate)?;
            first = false;
        }
        // A sample is one whole access unit: its picture is done now, not when the next one
        // starts, so a frame never waits for the picture after it.
        self.engine.send(Job::EndAu)
    }

    /// The next frame in output order, or `None` when the decoder wants more samples (or, once
    /// flushed, has given every frame).
    pub fn pull(&mut self) -> Result<Option<Frame>, Refusal> {
        self.engine.pull()
    }

    pub fn flush(&mut self) -> Result<(), Refusal> {
        self.engine.send(Job::Flush)
    }

    /// A new decoder for a seek, primed with the parameter sets seen so far (the old one's
    /// thread, if any, stops after the picture it is decoding).
    pub fn reset(&mut self, gate: &mut Gate) -> Result<(), Refusal> {
        self.engine = Engine::new(self.threads, self.kind);
        for p in std::mem::take(&mut self.params) {
            self.feed(&p, None, gate)?;
        }
        Ok(())
    }

    pub fn colour(&self) -> Colour {
        self.colour
    }
}

/// A decoder whose pictures decode on `threads` threads where the stream allows (one slice per
/// picture with a wavefront substream per CTB row, as phones write them), on the calling thread
/// otherwise; the frames are the same either way.
fn decoder(threads: usize) -> Decoder {
    let mut dec = Decoder::new();
    dec.set_threads(threads);
    dec
}

/// What the decoder is told.
enum Job {
    Nal(Vec<u8>, Option<i64>),
    /// The sample's last NAL unit was sent.
    EndAu,
    Flush,
}

/// What the decoder's thread answers.
enum Out {
    Frame(Frame),
    /// A sample (or the flush) is done: every frame it let out was sent before this.
    Done,
    Unsupported(String),
    /// The decoder panicked; its thread is gone.
    Panicked,
}

/// Where the decoder runs: on the caller's thread, or on a thread of its own.
enum Engine {
    Here { dec: Box<Decoder>, kind: VideoKind },
    Worker(Worker),
}

/// The decoder on a thread of its own, at most [`AHEAD`] samples ahead.
struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Out>,
    /// Set when this worker is replaced: its thread stops at the next job.
    stop: Arc<AtomicBool>,
    kind: VideoKind,
    /// Samples (and a flush) sent but not yet done.
    in_flight: usize,
    flushing: bool,
    frames: VecDeque<Frame>,
    failed: Option<Out>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Engine {
    fn new(threads: usize, kind: VideoKind) -> Engine {
        if threads == 0 {
            return Engine::Here { dec: Box::new(decoder(0)), kind };
        }
        let (tx, jobs) = channel::<Job>();
        let (out, rx) = channel::<Out>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let spawned = std::thread::Builder::new().name("dalica-hevc".into()).spawn(move || work(decoder(threads), kind, jobs, out, stopped));
        if spawned.is_err() {
            // No thread to be had: decode here.
            return Engine::Here { dec: Box::new(decoder(threads)), kind };
        }
        Engine::Worker(Worker { tx, rx, stop, kind, in_flight: 0, flushing: false, frames: VecDeque::new(), failed: None })
    }

    fn send(&mut self, job: Job) -> Result<(), Refusal> {
        match self {
            Engine::Here { dec, kind } => run(dec, job, *kind),
            Engine::Worker(w) => {
                w.check()?;
                let counts = matches!(job, Job::EndAu | Job::Flush);
                if matches!(job, Job::Flush) {
                    w.flushing = true;
                }
                if w.tx.send(job).is_err() {
                    return Err(Refusal::Error(Error::Panic(Format::Video(w.kind))));
                }
                if counts {
                    w.in_flight += 1;
                }
                Ok(())
            }
        }
    }

    fn pull(&mut self) -> Result<Option<Frame>, Refusal> {
        match self {
            Engine::Here { dec, kind } => Ok(guard(*kind, || dec.next_frame().ok())?),
            Engine::Worker(w) => w.pull(),
        }
    }
}

impl Worker {
    /// A failure the thread reported, as the synchronous decoder would have returned it.
    fn check(&mut self) -> Result<(), Refusal> {
        match self.failed.take() {
            None => Ok(()),
            Some(Out::Unsupported(what)) => Err(Refusal::Unsupported(what)),
            Some(_) => Err(Refusal::Error(Error::Panic(Format::Video(self.kind)))),
        }
    }

    fn take(&mut self, m: Out) {
        match m {
            Out::Frame(f) => self.frames.push_back(f),
            Out::Done => self.in_flight = self.in_flight.saturating_sub(1),
            fail => {
                self.in_flight = 0;
                self.failed = Some(fail);
            }
        }
    }

    fn pull(&mut self) -> Result<Option<Frame>, Refusal> {
        loop {
            loop {
                match self.rx.try_recv() {
                    Ok(m) => self.take(m),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if self.in_flight > 0 && self.failed.is_none() {
                            self.in_flight = 0;
                            self.failed = Some(Out::Panicked);
                        }
                        break;
                    }
                }
            }
            if let Some(f) = self.frames.pop_front() {
                return Ok(Some(f));
            }
            self.check()?;
            // More samples may go ahead while fewer than `AHEAD` are in flight; once flushed,
            // every frame is waited for.
            if self.in_flight == 0 || (!self.flushing && self.in_flight < AHEAD) {
                if self.in_flight == 0 {
                    self.flushing = false;
                }
                return Ok(None);
            }
            match self.rx.recv() {
                Ok(m) => self.take(m),
                Err(_) => {
                    self.in_flight = 0;
                    self.failed = Some(Out::Panicked);
                }
            }
        }
    }
}

/// One job on a decoder (the frames it lets out are pulled after).
fn run(dec: &mut Decoder, job: Job, kind: VideoKind) -> Result<(), Refusal> {
    let r = match job {
        Job::Nal(nal, pts) => guard(kind, || dec.push_nal(&nal, pts))?,
        Job::EndAu => {
            guard(kind, || dec.end_access_unit())?;
            Ok(())
        }
        Job::Flush => {
            guard(kind, || dec.flush())?;
            Ok(())
        }
    };
    match r {
        Ok(()) => {}
        Err(rusty_h265::Error::Unsupported(what)) => return Err(Refusal::Unsupported(what)),
        // A damaged NAL unit: the decoder stays usable and conceals what it lost.
        Err(_) => {}
    }
    Ok(())
}

/// The decoder's thread: jobs in, frames and acknowledgements out, until the owner goes away or
/// replaces it.
fn work(mut dec: Decoder, kind: VideoKind, jobs: Receiver<Job>, out: Sender<Out>, stop: Arc<AtomicBool>) {
    for job in jobs {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let done = matches!(job, Job::EndAu | Job::Flush);
        match run(&mut dec, job, kind) {
            Ok(()) => {}
            Err(Refusal::Unsupported(what)) => {
                let _ = out.send(Out::Unsupported(what));
                return;
            }
            Err(Refusal::Error(_)) => {
                let _ = out.send(Out::Panicked);
                return;
            }
        }
        if done {
            loop {
                match catch_unwind(AssertUnwindSafe(|| dec.next_frame())) {
                    Ok(Ok(f)) => {
                        if out.send(Out::Frame(f)).is_err() {
                            return;
                        }
                    }
                    Ok(Err(_)) => break,
                    Err(_) => {
                        let _ = out.send(Out::Panicked);
                        return;
                    }
                }
            }
            if out.send(Out::Done).is_err() {
                return;
            }
        }
    }
}

/// Why a feed failed: a stream the crate does not decode, or an error.
pub enum Refusal {
    Unsupported(String),
    Error(Error),
}

impl From<Error> for Refusal {
    fn from(e: Error) -> Refusal {
        Refusal::Error(e)
    }
}

fn guard<T>(kind: VideoKind, f: impl FnOnce() -> T) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|_| Error::Panic(Format::Video(kind)))
}

/// The cropped planes of a decoded frame.
pub fn picture(f: &Frame, colour: Colour) -> Option<Picture<'_>> {
    let p = &f.picture;
    let (cx, cy, cw, ch) = p.crop;
    let (sw, sh) = match p.chroma_format_idc {
        1 => (2, 2),
        2 => (2, 1),
        _ => (1, 1),
    };
    let plane = |i: usize, sub_w: usize, sub_h: usize| {
        let q = p.planes.get(i)?;
        Some(Plane {
            samples: Samples::U16(&q.data),
            stride: q.stride,
            x0: cx / sub_w,
            y0: cy / sub_h,
            width: cw.div_ceil(sub_w),
            height: ch.div_ceil(sub_h),
        })
    };
    let luma = plane(0, 1, 1)?;
    let chroma = if p.chroma_format_idc == 0 { None } else { Some((plane(1, sw, sh)?, plane(2, sw, sh)?)) };
    Some(Picture { luma, chroma, bit_depth: p.bit_depth_luma.max(p.bit_depth_chroma), colour })
}
