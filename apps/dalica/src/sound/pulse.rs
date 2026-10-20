//! Linux's sound output: a PulseAudio-protocol client (M15.27), served by PulseAudio and by
//! PipeWire's pulse server, over the `pulseaudio` crate's message types (its own blocking client
//! is not used: this one runs on the output's thread and one reader thread per stream).
//!
//! A stream connects to the server's socket (`$PULSE_SERVER`, `$PULSE_RUNTIME_PATH` or
//! `$XDG_RUNTIME_DIR`'s `pulse/native`, with the user's cookie), names the client "Dalica" and
//! creates a playback stream on the default sink as 32-bit floats (more than two channels mixed
//! down to stereo), with a short buffer (a target of [`TARGET_MS`]) so the clock follows what is
//! heard closely. The server asks for bytes; they are sent as they wait in the queue. Every
//! [`TIMING_EVERY`] the stream's latency is asked: what the sink has read, less what it holds,
//! is what has been heard, carried forward between answers by the time passed. When the queue
//! runs dry and the server stops short of what was written (it plays whole quanta), the rest is
//! padded with silence, which the clock leaves out ([`Offsets`]). No socket (or one
//! that refuses) is [`NO_OUTPUT`]. Every length the server sends is bounded
//! ([`MAX_MESSAGE`]) and a malformed message ends the stream as a failure, never a panic.

use std::collections::VecDeque;
use std::ffi::CString;
use std::io::{BufReader, Cursor, Read};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use dalica_engine::sound::{Format, NO_OUTPUT};
use pulseaudio::protocol::{self, Command, Descriptor, PlaybackLatency, TagStructReader};

use super::{Backend, Core, downmix};

/// The server's buffer for a stream: its target, in milliseconds.
pub const TARGET_MS: u32 = 150;
/// Playback starts once this much is buffered, in milliseconds.
const PREBUF_MS: u32 = 40;
/// The smallest request the server sends, in milliseconds.
const MIN_REQUEST_MS: u32 = 10;
/// How often the stream's latency is asked.
pub const TIMING_EVERY: Duration = Duration::from_millis(20);
/// The longest message read from the server.
pub const MAX_MESSAGE: u32 = protocol::MAX_MEMBLOCKQ_LENGTH as u32;
/// How long the handshake may take.
const HANDSHAKE: Duration = Duration::from_secs(2);

const SEQ_AUTH: u32 = 0;
const SEQ_NAME: u32 = 1;
const SEQ_CREATE: u32 = 2;
const SEQ_TIMING: u32 = 3;
const SEQ_DELETE: u32 = 4;
const SEQ_TRIGGER: u32 = 5;

/// The PulseAudio backend: the server's socket (none found: no output) and the user's cookie.
pub struct Pulse {
    socket: Option<PathBuf>,
    cookie: Vec<u8>,
}

impl Pulse {
    /// The server the environment names.
    pub fn from_env() -> Pulse {
        let cookie = pulseaudio::cookie_path_from_env().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
        Pulse { socket: pulseaudio::socket_path_from_env(), cookie }
    }

    /// The server at `socket` (`None`: there is none), without a cookie (`tests/sound.rs`).
    #[allow(dead_code)]
    pub fn at(socket: Option<PathBuf>) -> Pulse {
        Pulse { socket, cookie: Vec::new() }
    }

    /// Connects and introduces the client: the socket, a reader over a second handle of it, and
    /// the protocol version agreed.
    fn connect(&self) -> Result<(UnixStream, BufReader<UnixStream>, u16), String> {
        let path = self.socket.as_ref().ok_or_else(|| NO_OUTPUT.to_string())?;
        let sock = UnixStream::connect(path).map_err(|_| NO_OUTPUT.to_string())?;
        let io = |e: std::io::Error| format!("the sound server: {e}");
        sock.set_read_timeout(Some(HANDSHAKE)).map_err(io)?;
        sock.set_write_timeout(Some(HANDSHAKE)).map_err(io)?;
        let mut reader = BufReader::new(sock.try_clone().map_err(io)?);
        let mut w = sock;
        let auth = protocol::AuthParams { version: protocol::MAX_VERSION, supports_shm: false, supports_memfd: false, cookie: self.cookie.clone() };
        send(&mut w, SEQ_AUTH, &Command::Auth(auth), protocol::MAX_VERSION)?;
        let auth: protocol::AuthReply = reply(&mut reader, protocol::MAX_VERSION, SEQ_AUTH)?;
        let version = protocol::MAX_VERSION.min(auth.version);
        if version < protocol::MIN_VERSION {
            return Err(format!("the sound server speaks protocol {version}, too old"));
        }
        let mut props = protocol::Props::new();
        props.set(protocol::Prop::ApplicationName, CString::from(c"Dalica"));
        send(&mut w, SEQ_NAME, &Command::SetClientName(props), version)?;
        let _: protocol::SetClientNameReply = reply(&mut reader, version, SEQ_NAME)?;
        Ok((w, reader, version))
    }
}

fn send(w: &mut UnixStream, seq: u32, cmd: &Command, version: u16) -> Result<(), String> {
    protocol::write_command_message(w, seq, cmd, version).map_err(|e| format!("the sound server: {e}"))
}

/// One message from the server: its descriptor and payload (at most [`MAX_MESSAGE`] bytes).
fn message(r: &mut impl Read) -> Result<(Descriptor, Vec<u8>), String> {
    let desc = protocol::read_descriptor(r).map_err(|e| format!("the sound server: {e}"))?;
    if desc.length > MAX_MESSAGE {
        return Err("the sound server sent an oversized message".into());
    }
    let mut payload = vec![0u8; desc.length as usize];
    r.read_exact(&mut payload).map_err(|e| format!("the sound server: {e}"))?;
    Ok((desc, payload))
}

/// Reads messages until the reply to `seq`, its payload read as `T`; an error reply is the
/// server's refusal.
fn reply<T: protocol::CommandReply>(r: &mut impl Read, version: u16, seq: u32) -> Result<T, String> {
    loop {
        let (desc, payload) = message(r)?;
        if desc.channel != u32::MAX {
            continue;
        }
        let mut cur = Cursor::new(payload.as_slice());
        let (got, cmd) = Command::read_tag_prefixed(&mut cur, version).map_err(|e| format!("the sound server: {e}"))?;
        match cmd {
            Command::Reply if got == seq => {
                return TagStructReader::new(&mut cur, version).read::<T>().map_err(|e| format!("the sound server: {e}"));
            }
            Command::Error(e) if got == seq => return Err(format!("the sound server refused: {e:?}")),
            _ => {}
        }
    }
}

/// What the reader thread passes on.
enum Event {
    /// The server asks for this many bytes.
    Request(u32),
    /// The stream's latency.
    Latency(PlaybackLatency),
    /// The stream ended or the connection failed: why.
    Ended(String),
}

/// Reads the server's messages for stream `channel` until the connection ends.
fn read_events(mut r: BufReader<UnixStream>, version: u16, channel: u32, tx: &Sender<Event>) {
    let why = loop {
        let (desc, payload) = match message(&mut r) {
            Ok(m) => m,
            Err(e) => break e,
        };
        if desc.channel != u32::MAX {
            continue;
        }
        let mut cur = Cursor::new(payload.as_slice());
        let event = match Command::read_tag_prefixed(&mut cur, version) {
            Ok((_, Command::Request(req))) if req.channel == channel => Event::Request(req.length),
            Ok((SEQ_TIMING, Command::Reply)) => match TagStructReader::new(&mut cur, version).read::<PlaybackLatency>() {
                Ok(l) => Event::Latency(l),
                Err(e) => break format!("the sound server: {e}"),
            },
            Ok((_, Command::PlaybackStreamKilled(c))) if c == channel => break "the sound server ended the stream".into(),
            Ok((_, Command::Error(e))) => break format!("the sound server: {e:?}"),
            Ok(_) => continue,
            Err(e) => break format!("the sound server: {e}"),
        };
        if tx.send(event).is_err() {
            return;
        }
    };
    let _ = tx.send(Event::Ended(why));
}

/// Bytes for `ms` milliseconds of `channels` float channels at `rate`.
fn bytes_for(ms: u32, rate: u32, channels: u8) -> u32 {
    let b = u64::from(rate) * u64::from(channels) * 4 * u64::from(ms) / 1000;
    u32::try_from(b).unwrap_or(u32::MAX).max(4 * u32::from(channels))
}

/// Where a stream stands, in frames of the server's buffer: what was written (samples and the
/// silence that pads a dry queue out to the server's quantum) and what of it was silence.
#[derive(Debug, Default)]
struct Offsets {
    /// Frames written, silence included.
    written: u64,
    /// Frames of samples written.
    samples: u64,
    /// The silences written, as (where, how long), oldest first; those wholly read are folded
    /// into `padded`.
    pads: VecDeque<(u64, u64)>,
    padded: u64,
}

impl Offsets {
    fn wrote(&mut self, frames: u64) {
        self.written = self.written.saturating_add(frames);
        self.samples = self.samples.saturating_add(frames);
    }

    fn padded_with(&mut self, frames: u64) {
        if self.pads.len() >= 64
            && let Some((_, len)) = self.pads.pop_front()
        {
            self.padded = self.padded.saturating_add(len);
        }
        self.pads.push_back((self.written, frames));
        self.written = self.written.saturating_add(frames);
    }

    /// The samples heard once the server has played up to `pos` (silence left out).
    fn heard(&mut self, pos: u64) -> u64 {
        while let Some(&(at, len)) = self.pads.front() {
            if at.saturating_add(len) > pos {
                break;
            }
            self.padded = self.padded.saturating_add(len);
            self.pads.pop_front();
        }
        let partly: u64 = self.pads.iter().filter(|(at, _)| *at < pos).map(|(at, len)| (*len).min(pos - at)).sum();
        pos.saturating_sub(self.padded).saturating_sub(partly).min(self.samples)
    }
}

impl Backend for Pulse {
    fn probe(&mut self) -> Result<(), String> {
        self.connect().map(|_| ())
    }

    fn play(&mut self, core: &Arc<Core>, stream: u64, format: Format) -> Result<(), String> {
        let (mut w, mut r, version) = self.connect()?;
        let out: u8 = if format.channels >= 2 { 2 } else { 1 };
        let frame_bytes = 4 * u64::from(out);
        let params = protocol::PlaybackStreamParams {
            sample_spec: protocol::SampleSpec { format: protocol::SampleFormat::Float32Le, channels: out, sample_rate: format.rate },
            channel_map: if out == 2 { protocol::ChannelMap::stereo() } else { protocol::ChannelMap::mono() },
            sink_name: Some(protocol::DEFAULT_SINK.to_owned()),
            buffer_attr: protocol::stream::BufferAttr {
                max_length: u32::MAX,
                target_length: bytes_for(TARGET_MS, format.rate, out),
                pre_buffering: bytes_for(PREBUF_MS, format.rate, out),
                minimum_request_length: bytes_for(MIN_REQUEST_MS, format.rate, out),
                fragment_size: u32::MAX,
            },
            ..Default::default()
        };
        send(&mut w, SEQ_CREATE, &Command::CreatePlaybackStream(params), version)?;
        let created: protocol::CreatePlaybackStreamReply = reply(&mut r, version, SEQ_CREATE)?;
        let channel = created.channel;
        let io = |e: std::io::Error| format!("the sound server: {e}");
        w.set_read_timeout(None).map_err(io)?;
        let (tx, events): (Sender<Event>, Receiver<Event>) = mpsc::channel();
        std::thread::Builder::new().name("dalica-sound-pulse".into()).spawn(move || read_events(r, version, channel, &tx)).map_err(io)?;
        let rate = u64::from(format.rate.max(1));
        let mut owed = u64::from(created.requested_bytes);
        let mut at = Offsets::default();
        let mut asked: Option<Instant> = None;
        // Where the sink had played to at the last latency answer, when it came, and whether it
        // was playing then.
        let mut heard: Option<(u64, Instant, bool)> = None;
        // Since when the sink has read everything written (what it still holds then plays out).
        let mut drained: Option<(i64, Instant)> = None;
        // When samples were last sent, and the read offset last found stuck.
        let mut sent = Instant::now();
        let mut unstuck: Option<i64> = None;
        let write = |w: &mut UnixStream, bytes: &[u8]| protocol::write_memblock(w, channel, bytes, 0).map_err(|e| format!("the sound server: {e}"));
        loop {
            match events.recv_timeout(Duration::from_millis(5)) {
                Ok(Event::Request(n)) => owed = owed.saturating_add(u64::from(n)),
                Ok(Event::Latency(l)) => {
                    asked = None;
                    let read = u64::try_from(l.read_offset.max(0)).unwrap_or(0) / frame_bytes;
                    let mut held = l.sink_usec.saturating_mul(rate) / 1_000_000;
                    if l.read_offset >= l.write_offset {
                        let since = match drained {
                            Some((off, t)) if off == l.read_offset => t,
                            _ => {
                                drained = Some((l.read_offset, Instant::now()));
                                Instant::now()
                            }
                        };
                        held = held.saturating_sub(u64::try_from(since.elapsed().as_micros()).unwrap_or(0).saturating_mul(rate) / 1_000_000);
                    } else {
                        drained = None;
                    }
                    // Stopped short of what was written, with nothing more to come (below the
                    // pre-buffer, or short of a whole quantum): the server is told to play what it
                    // holds, and the rest is padded with silence it then plays. Once per place.
                    if !l.playing && l.read_offset < l.write_offset && sent.elapsed() >= TIMING_EVERY && unstuck != Some(l.read_offset) {
                        unstuck = Some(l.read_offset);
                        send(&mut w, SEQ_TRIGGER, &Command::TriggerPlaybackStream(channel), version)?;
                        let pad = owed.max(u64::from(bytes_for(PREBUF_MS + MIN_REQUEST_MS, format.rate, out))) / frame_bytes;
                        let pad = pad.min(u64::from(bytes_for(TARGET_MS, format.rate, out)) / frame_bytes);
                        write(&mut w, &vec![0u8; usize::try_from(pad * frame_bytes).unwrap_or(0)])?;
                        owed = owed.saturating_sub(pad * frame_bytes);
                        at.padded_with(pad);
                    }
                    heard = Some((read.saturating_sub(held), Instant::now(), l.playing));
                }
                Ok(Event::Ended(why)) => return Err(why),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("the sound server's connection ended".into()),
            }
            if !core.is_current(stream, format) {
                // Closing the connection deletes the stream.
                let _ = send(&mut w, SEQ_DELETE, &Command::DeletePlaybackStream(channel), version);
                return Ok(());
            }
            // Send what the server asked for, as far as it waits in the queue.
            let want = usize::try_from(owed / frame_bytes).unwrap_or(usize::MAX);
            if want > 0 {
                let Some(samples) = core.take(stream, want) else { return Ok(()) };
                if !samples.is_empty() {
                    sent = Instant::now();
                    let mixed = downmix(&samples, format.channels, u16::from(out));
                    let bytes: Vec<u8> = mixed.iter().flat_map(|v| v.to_le_bytes()).collect();
                    write(&mut w, &bytes)?;
                    owed = owed.saturating_sub(bytes.len() as u64);
                    at.wrote(bytes.len() as u64 / frame_bytes);
                }
            }
            if asked.is_none_or(|t| t.elapsed() >= TIMING_EVERY * 10) && heard.is_none_or(|(_, t, _)| t.elapsed() >= TIMING_EVERY) {
                let latency = protocol::LatencyParams { channel, now: std::time::SystemTime::now() };
                send(&mut w, SEQ_TIMING, &Command::GetPlaybackLatency(latency), version)?;
                asked = Some(Instant::now());
            }
            if let Some((pos, t, playing)) = heard {
                let since = if playing { u64::try_from(t.elapsed().as_micros()).unwrap_or(0).saturating_mul(rate) / 1_000_000 } else { 0 };
                let pos = pos.saturating_add(since).min(at.written);
                core.heard(stream, at.heard(pos));
            }
        }
    }
}
