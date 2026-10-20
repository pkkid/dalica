//! The window's sound output (`src/sound.rs`, M15.27; reserved decision 2): over a recording
//! backend, the output's clock is what the backend has heard, never past what was queued, a new
//! stream starts it again, sound too far ahead is refused, a failing output is opened again
//! [`RETRIES`] times before the stream answers the failure, and a system without an output
//! answers "No sound output on this system"; on the OS the test runs on, the real backend opens
//! the default output and plays half a second of silence where there is one (else the test says
//! so and why); on Linux a missing sound server, and one that sends an oversized message, are
//! answered without a panic. Nothing but silence is ever played.

#[allow(dead_code)]
#[path = "../src/sound.rs"]
pub mod output;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dalica_engine::sound::{Format, NO_OUTPUT, Sound, SoundError};
use output::{Backend, Core, Output, RETRIES, downmix};

const PATIENCE: Duration = Duration::from_secs(20);

/// A backend that takes the queue at real time (10 ms a round) and keeps what it took; it fails
/// its first `fails` plays.
#[derive(Clone, Default)]
struct Recorder {
    taken: Arc<Mutex<Vec<f32>>>,
    plays: Arc<Mutex<u32>>,
    fails: u32,
    probe: Option<String>,
}

impl Backend for Recorder {
    fn probe(&mut self) -> Result<(), String> {
        self.probe.clone().map_or(Ok(()), Err)
    }

    fn play(&mut self, core: &Arc<Core>, stream: u64, format: Format) -> Result<(), String> {
        let n = {
            let mut p = self.plays.lock().unwrap();
            *p += 1;
            *p
        };
        if n <= self.fails {
            return Err(format!("unplugged ({n})"));
        }
        let mut heard = 0u64;
        loop {
            std::thread::sleep(Duration::from_millis(10));
            let Some(got) = core.take(stream, format.rate as usize / 100) else { return Ok(()) };
            heard += (got.len() / usize::from(format.channels)) as u64;
            self.taken.lock().unwrap().extend(got);
            core.heard(stream, heard);
        }
    }
}

fn until(what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < PATIENCE, "{what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

const MONO: Format = Format { rate: 48_000, channels: 1 };

#[test]
fn the_clock_is_what_the_backend_heard_and_a_failed_output_is_opened_again() {
    let rec = Recorder::default();
    let out = Output::start(rec.clone());
    assert!(out.wait_probed(PATIENCE) && out.status().is_ok());
    assert_eq!(out.queue(&[0.0]), Err(SoundError::Refused("the sound output is not open".into())));
    out.open(MONO).unwrap();
    let first = out.stream();
    out.queue(&vec![0.5; 12_000]).unwrap();
    until("a quarter second heard", || out.clock_ms() == 250);
    assert!(rec.taken.lock().unwrap().iter().all(|v| *v == 0.5) && rec.taken.lock().unwrap().len() == 12_000);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(out.clock_ms(), 250, "never past what was queued");
    assert!(matches!(out.queue(&vec![0.0; 48_000 * 11]), Err(SoundError::Refused(_))), "more than 10 s ahead");
    assert_eq!(out.queue(&[2.0, f32::NAN]), Ok(()), "out-of-range samples are clamped");
    until("clamped", || rec.taken.lock().unwrap().ends_with(&[1.0, 0.0]));
    out.open(Format { rate: 8_000, channels: 2 }).unwrap();
    assert!(out.stream() > first);
    assert_eq!(out.clock_ms(), 0, "a new stream's clock starts at zero");
    assert!(out.queue(&[0.1; 3]).is_err(), "a partial frame");
    out.stop();
    assert_eq!(out.clock_ms(), 0);

    // An output that fails every time: opened again RETRIES times, then the stream says why.
    let failing = Recorder { fails: u32::MAX, ..Recorder::default() };
    let out = Output::start(failing.clone());
    out.open(MONO).unwrap();
    out.queue(&[0.0; 480]).unwrap();
    until("the failure answered", || matches!(out.queue(&[0.0]), Err(SoundError::Failed(_))));
    assert_eq!(*failing.plays.lock().unwrap(), RETRIES + 1);
    // One that fails once plays on the next try.
    let once = Recorder { fails: 1, ..Recorder::default() };
    let out = Output::start(once.clone());
    out.open(MONO).unwrap();
    out.queue(&[0.25; 480]).unwrap();
    until("played after one failure", || out.clock_ms() == 10);
}

#[test]
fn no_output_answers_no_sound_output_on_this_system() {
    let out = Output::start(Recorder { probe: Some(NO_OUTPUT.into()), ..Recorder::default() });
    assert!(out.wait_probed(PATIENCE));
    assert_eq!(out.status(), Err(NO_OUTPUT.to_string()));
    assert_eq!(out.open(MONO), Err(SoundError::Unavailable(NO_OUTPUT.into())));
    let none = Output::none(NO_OUTPUT);
    assert_eq!((none.status(), none.open(MONO)), (Err(NO_OUTPUT.to_string()), Err(SoundError::Unavailable(NO_OUTPUT.into()))));
    // More channels than two are mixed down; mono is doubled.
    assert_eq!(downmix(&[0.5, 0.25], 1, 2), [0.5, 0.5, 0.25, 0.25]);
    assert_eq!(downmix(&[0.6, 0.0, 0.0, 0.6, 0.0, 0.0], 3, 2), [0.2, 0.2, 0.2, 0.2]);
    assert_eq!(downmix(&[0.2, 0.4], 2, 2), [0.2, 0.4]);
}

/// The real backend of this OS: where there is an output, half a second of silence plays through
/// the default one; where there is none, the test says so.
#[test]
fn on_this_os_the_default_output_plays_half_a_second_of_silence() {
    let out = output::output();
    let start = Instant::now();
    // The probe runs on the output's thread: the answer comes when it is known.
    let status = loop {
        match out.status() {
            Err(why) => break Err(why),
            Ok(()) if out.open(Format { rate: 48_000, channels: 2 }).is_ok() => break Ok(()),
            Ok(()) => {}
        }
        assert!(start.elapsed() < PATIENCE, "the probe answered");
        std::thread::sleep(Duration::from_millis(5));
    };
    if let Err(why) = status {
        assert_eq!(why, NO_OUTPUT);
        eprintln!("skipped: {why} (no sound server or output device here)");
        return;
    }
    out.queue(&vec![0.0; 48_000]).unwrap();
    let played = Instant::now();
    let start = Instant::now();
    while out.clock_ms() < 500 && !matches!(out.queue(&[]), Err(SoundError::Failed(_))) && start.elapsed() < PATIENCE {
        std::thread::sleep(Duration::from_millis(5));
    }
    if let Err(SoundError::Failed(why)) = out.queue(&[]) {
        eprintln!("skipped: the output failed: {why}");
        return;
    }
    if out.clock_ms() == 0 {
        // A device that takes no samples at all (a CI machine's placeholder output): nothing to
        // measure here.
        eprintln!("skipped: the default output took no samples in {} s", PATIENCE.as_secs());
        return;
    }
    assert!(out.clock_ms() >= 500, "half a second heard, not {} ms", out.clock_ms());
    assert!(played.elapsed() >= Duration::from_millis(300), "the clock follows what is heard, not what is queued");
    out.stop();
}

#[cfg(target_os = "linux")]
#[test]
fn a_missing_or_hostile_sound_server_is_answered_without_a_panic() {
    use output::pulse::Pulse;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    let dir = tempfile::tempdir().unwrap();
    for socket in [None, Some(dir.path().join("no-server"))] {
        let out = Output::start(Pulse::at(socket));
        assert!(out.wait_probed(PATIENCE));
        assert_eq!(out.status(), Err(NO_OUTPUT.to_string()));
    }
    // A server that answers the handshake with an oversized message, then with garbage.
    for reply in [
        &[0xffu8, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0][..],
        &[0, 0, 0, 3, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3],
    ] {
        let path = dir.path().join(format!("hostile-{}", reply.len()));
        let listener = UnixListener::bind(&path).unwrap();
        let reply = reply.to_vec();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut head = [0u8; 20];
            conn.read_exact(&mut head).unwrap();
            let _ = conn.write_all(&reply);
            std::thread::sleep(Duration::from_millis(200));
        });
        let out = Output::start(Pulse::at(Some(path)));
        assert!(out.wait_probed(PATIENCE));
        let why = out.status().unwrap_err();
        assert!(why.starts_with("the sound server"), "{why}");
        server.join().unwrap();
    }
}
