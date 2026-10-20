//! Photos dragged out on X11: Dalica as an XDND source (the freedesktop.org XDND protocol,
//! version 5), over x11rb's pure-Rust connection, on a thread of its own. No `unsafe`.
//!
//! winit keeps the button's grab on Dalica's window during the drag, so the source does not grab:
//! it reads the pointer every frame (`QueryPointer`), finds the XDND-aware window under it (its
//! `XdndProxy` honoured, Dalica's own windows skipped), and speaks to it with client messages
//! (`XdndEnter`, `XdndPosition`, `XdndLeave`, `XdndDrop`), offering a copy (`XdndActionCopy`) of
//! `text/uri-list` (and the paths as UTF-8 text). The button let go over a window that accepted,
//! it drops and serves the files from the `XdndSelection` it owns until the target says
//! `XdndFinished` (at most [`FINISH_WITHIN`]); over anything else it leaves. The pill follows the
//! pointer in a small override-redirect window (`_NET_WM_WINDOW_TYPE_DND`).
//!
//! The protocol's decisions are [`Source`]'s, a state machine with no connection, tested alone;
//! the thread only carries its messages and answers selection requests ([`answer`]).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use dalica_ui::drag::out::Outgoing;
use x11rb::connection::{Connection, RequestConnection as _};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, AtomEnum, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
    ImageFormat, ImageOrder, KeyButMask, PropMode, SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

use crate::os::clipboard::uri_list;

/// The XDND version Dalica speaks; a target of an older one down to 3 is spoken to in its own.
pub const VERSION: u32 = 5;
const MIN_VERSION: u32 = 3;
/// How often the pointer is read.
const TICK: Duration = Duration::from_millis(16);
/// How long a target may take to fetch the files and say it has finished.
pub const FINISH_WITHIN: Duration = Duration::from_secs(10);
/// The longest a drag lasts before it is given up.
const MAX_DRAG: Duration = Duration::from_secs(600);
/// How deep the window tree is searched for the target.
const MAX_DEPTH: usize = 16;
/// Where the pill sits from the pointer, in pixels.
const PILL_OFFSET: i32 = 16;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        XdndAware,
        XdndSelection,
        XdndEnter,
        XdndPosition,
        XdndStatus,
        XdndLeave,
        XdndDrop,
        XdndFinished,
        XdndActionCopy,
        XdndTypeList,
        XdndProxy,
        TARGETS,
        UTF8_STRING,
        _NET_WM_PID,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DND,
        URI_LIST: b"text/uri-list",
        TEXT_UTF8: b"text/plain;charset=utf-8",
        TEXT: b"text/plain",
    }
}

/// The types offered, the files first.
fn types(a: &Atoms) -> [u32; 3] {
    [a.URI_LIST, a.TEXT_UTF8, a.UTF8_STRING]
}

/// What is under the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Under {
    /// No window that takes XDND.
    Nothing,
    /// One of Dalica's own windows: the drag is inside again.
    Own,
    /// An XDND-aware window: `window` is the target, `to` where its messages go (its proxy, or
    /// itself), `version` its `XdndAware`.
    Aware { window: u32, to: u32, version: u32 },
}

/// A message for a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Send {
    Enter { to: u32, window: u32, version: u32 },
    Position { to: u32, window: u32, x: i16, y: i16 },
    Leave { to: u32, window: u32 },
    Drop { to: u32, window: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Target {
    window: u32,
    to: u32,
    version: u32,
    accepted: bool,
}

/// The XDND source's decisions: what to send as the pointer moves, a target answers, the button
/// is let go or the drag is cancelled. One position at a time is in flight: a move while the
/// target has not answered the last is sent once it does.
#[derive(Debug, Default)]
pub struct Source {
    target: Option<Target>,
    at: (i16, i16),
    waiting: bool,
    moved: bool,
    dropped: bool,
}

impl Source {
    /// The pointer is at (`x`, `y`) on the root window, over `under`.
    pub fn pointer(&mut self, under: Under, x: i16, y: i16) -> Vec<Send> {
        let mut out = Vec::new();
        if self.dropped {
            return out;
        }
        let next = match under {
            Under::Aware { window, to, version } if version >= MIN_VERSION => {
                Some(Target { window, to, version: version.min(VERSION), accepted: false })
            }
            _ => None,
        };
        let moved = self.at != (x, y);
        self.at = (x, y);
        if next.map(|t| t.window) != self.target.map(|t| t.window) {
            if let Some(old) = self.target.take() {
                out.push(Send::Leave { to: old.to, window: old.window });
            }
            self.target = next;
            self.waiting = false;
            self.moved = false;
            if let Some(t) = next {
                out.push(Send::Enter { to: t.to, window: t.window, version: t.version });
                out.push(Send::Position { to: t.to, window: t.window, x, y });
                self.waiting = true;
            }
        } else if let Some(t) = self.target
            && moved
        {
            if self.waiting {
                self.moved = true;
            } else {
                out.push(Send::Position { to: t.to, window: t.window, x, y });
                self.waiting = true;
            }
        }
        out
    }

    /// `window` answered a position (`XdndStatus`): whether it would take a drop.
    pub fn status(&mut self, window: u32, accept: bool) -> Vec<Send> {
        let mut out = Vec::new();
        let Some(t) = self.target.as_mut().filter(|t| t.window == window) else { return out };
        t.accepted = accept;
        self.waiting = false;
        if self.moved && !self.dropped {
            self.moved = false;
            self.waiting = true;
            let (x, y) = self.at;
            out.push(Send::Position { to: t.to, window: t.window, x, y });
        }
        out
    }

    /// The button was let go: a drop on a target that accepted, else a leave. Returns the
    /// messages and whether to wait for `XdndFinished`.
    pub fn release(&mut self) -> (Vec<Send>, bool) {
        match self.target {
            Some(t) if t.accepted && !self.dropped => {
                self.dropped = true;
                (vec![Send::Drop { to: t.to, window: t.window }], true)
            }
            Some(t) if !self.dropped => {
                self.target = None;
                (vec![Send::Leave { to: t.to, window: t.window }], false)
            }
            _ => (Vec::new(), false),
        }
    }

    /// `window` said it has finished with the drop.
    pub fn finished(&self, window: u32) -> bool {
        self.dropped && self.target.is_some_and(|t| t.window == window)
    }

    /// The drag ended inside Dalica (or was given up): a leave for the target, if any.
    pub fn cancel(&mut self) -> Vec<Send> {
        match self.target.take() {
            Some(t) if !self.dropped => vec![Send::Leave { to: t.to, window: t.window }],
            _ => Vec::new(),
        }
    }
}

/// The client message for `send`, from `source` at server time `time`: where it goes and the event.
pub fn message(a: &Atoms, source: u32, time: u32, send: Send) -> (u32, ClientMessageEvent) {
    let (to, window, kind, data) = match send {
        Send::Enter { to, window, version } => {
            let [t1, t2, t3] = types(a);
            (to, window, a.XdndEnter, [source, version << 24, t1, t2, t3])
        }
        Send::Position { to, window, x, y } => {
            let (x, y) = (u32::from(x.max(0) as u16), u32::from(y.max(0) as u16));
            (to, window, a.XdndPosition, [source, 0, (x << 16) | y, time, a.XdndActionCopy])
        }
        Send::Leave { to, window } => (to, window, a.XdndLeave, [source, 0, 0, 0, 0]),
        Send::Drop { to, window } => (to, window, a.XdndDrop, [source, 0, time, 0, 0]),
    };
    (to, ClientMessageEvent::new(32, window, kind, data))
}

/// What the selection holds for `target`: its type, format and bytes; `None` for a type not
/// offered.
pub fn answer(a: &Atoms, target: u32, paths: &[PathBuf]) -> Option<(u32, u8, Vec<u8>)> {
    if target == a.TARGETS {
        let mut list = vec![a.TARGETS];
        list.extend(types(a));
        list.push(a.TEXT);
        return Some((AtomEnum::ATOM.into(), 32, list.iter().flat_map(|t| t.to_ne_bytes()).collect()));
    }
    if target == a.URI_LIST {
        return Some((target, 8, uri_list(paths).into_bytes()));
    }
    if target == a.TEXT_UTF8 || target == a.UTF8_STRING || target == a.TEXT {
        let text: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        return Some((target, 8, text.join("\n").into_bytes()));
    }
    None
}

/// A running drag: cancel it.
pub struct Handle {
    cancel: Sender<()>,
}

impl Handle {
    /// Ends the drag with nothing dropped (a leave to the target), unless it already dropped.
    pub fn cancel(&self) {
        let _ = self.cancel.send(());
    }
}

/// Starts the drag of `out` on a thread of its own; `own` is Dalica's window (skipped as a target).
/// A failure (no X server, a refused request) ends the drag quietly, in the log.
pub fn start(out: Outgoing, own: u32) -> Handle {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new().name("dalica-drag-out".into()).spawn(move || {
        if let Err(e) = run(&out, own, &rx) {
            log::warn!("dalica: the drag out ended: {e}");
        }
    });
    if let Err(e) = spawned {
        log::warn!("dalica: cannot start the drag out: {e}");
    }
    Handle { cancel: tx }
}

type R<T> = Result<T, Box<dyn std::error::Error>>;

struct X {
    conn: RustConnection,
    a: Atoms,
    root: u32,
    source: u32,
    own: u32,
    pid: u32,
}

fn run(out: &Outgoing, own: u32, cancel: &Receiver<()>) -> R<()> {
    let (conn, screen) = x11rb::connect(None)?;
    let (root, depth) = {
        let s = conn.setup().roots.get(screen).ok_or("the X server names no such screen")?;
        (s.root, s.root_depth)
    };
    let a = Atoms::new(&conn)?.reply()?;
    let source = conn.generate_id()?;
    let aux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::PROPERTY_CHANGE);
    conn.create_window(0, source, root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &aux)?;
    let x = X { conn, a, root, source, own, pid: std::process::id() };
    let result = drag(&x, out, depth, cancel);
    let _ = x.conn.destroy_window(x.source);
    let _ = x.conn.flush();
    result
}

fn drag(x: &X, out: &Outgoing, depth: u8, cancel: &Receiver<()>) -> R<()> {
    let (conn, a) = (&x.conn, &x.a);
    // The offered types, and a server time from the property change that sets them.
    conn.change_property32(PropMode::REPLACE, x.source, a.XdndTypeList, AtomEnum::ATOM, &types(a))?;
    conn.flush()?;
    let time = server_time(x)?;
    conn.set_selection_owner(x.source, a.XdndSelection, time)?;
    if conn.get_selection_owner(a.XdndSelection)?.reply()?.owner != x.source {
        return Err("another client holds the XDND selection".into());
    }
    let pill = out.image.as_ref().and_then(|img| pill_window(x, img, depth).ok().flatten());
    let mut s = Source::default();
    let started = Instant::now();
    let result = (|| -> R<()> {
        loop {
            match cancel.try_recv() {
                Ok(()) | Err(TryRecvError::Disconnected) => {
                    send(x, time, s.cancel())?;
                    return Ok(());
                }
                Err(TryRecvError::Empty) => {}
            }
            if started.elapsed() > MAX_DRAG {
                send(x, time, s.cancel())?;
                return Err("the drag lasted too long".into());
            }
            let p = conn.query_pointer(x.root)?.reply()?;
            if let Some(w) = pill {
                let aux = ConfigureWindowAux::new().x(i32::from(p.root_x) + PILL_OFFSET).y(i32::from(p.root_y) + PILL_OFFSET);
                conn.configure_window(w, &aux)?;
            }
            if !p.mask.contains(KeyButMask::BUTTON1) {
                let (msgs, wait) = s.release();
                send(x, time, msgs)?;
                if let Some(w) = pill {
                    conn.destroy_window(w)?;
                }
                conn.flush()?;
                if wait {
                    let until = Instant::now() + FINISH_WITHIN;
                    while Instant::now() < until {
                        if events(x, &mut s, time, &out.paths)? {
                            return Ok(());
                        }
                        std::thread::sleep(TICK);
                    }
                }
                return Ok(());
            }
            let under = find(x, p.root_x, p.root_y)?;
            send(x, time, s.pointer(under, p.root_x, p.root_y))?;
            events(x, &mut s, time, &out.paths)?;
            conn.flush()?;
            std::thread::sleep(TICK);
        }
    })();
    if let Some(w) = pill {
        let _ = conn.destroy_window(w);
    }
    result
}

/// A server timestamp: the `PropertyNotify` of the type list just set on the source window.
fn server_time(x: &X) -> R<u32> {
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        while let Some(e) = x.conn.poll_for_event()? {
            if let Event::PropertyNotify(e) = e
                && e.window == x.source
            {
                return Ok(e.time);
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Err("the X server sent no timestamp".into())
}

fn send(x: &X, time: u32, msgs: Vec<Send>) -> R<()> {
    for m in msgs {
        let (to, ev) = message(&x.a, x.source, time, m);
        x.conn.send_event(false, to, EventMask::NO_EVENT, ev)?;
    }
    Ok(())
}

/// Handles the events waiting: targets' answers and requests for the files. Returns whether the
/// target said it has finished with the drop.
fn events(x: &X, s: &mut Source, time: u32, paths: &[PathBuf]) -> R<bool> {
    let mut finished = false;
    while let Some(e) = x.conn.poll_for_event()? {
        match e {
            Event::ClientMessage(m) if m.type_ == x.a.XdndStatus => {
                let [window, flags, ..] = m.data.as_data32();
                send(x, time, s.status(window, flags & 1 == 1))?;
            }
            Event::ClientMessage(m) if m.type_ == x.a.XdndFinished => {
                let [window, ..] = m.data.as_data32();
                finished |= s.finished(window);
            }
            Event::SelectionRequest(r) => serve(x, &r, paths)?,
            _ => {}
        }
    }
    x.conn.flush()?;
    Ok(finished)
}

/// Answers a request for the `XdndSelection`: the files in the type asked for, on the requestor's
/// property, then `SelectionNotify` (with no property when the type is not offered).
fn serve(x: &X, r: &SelectionRequestEvent, paths: &[PathBuf]) -> R<()> {
    // An obsolete requestor names no property: the target names it.
    let property = if r.property == u32::from(AtomEnum::NONE) { r.target } else { r.property };
    let answered = match (r.selection == x.a.XdndSelection).then(|| answer(&x.a, r.target, paths)).flatten() {
        Some((kind, format, bytes)) if bytes.len() <= x.conn.maximum_request_bytes().saturating_sub(64) => {
            let len = u32::try_from(bytes.len() / usize::from(format / 8)).unwrap_or(0);
            x.conn.change_property(PropMode::REPLACE, r.requestor, property, kind, format, len, &bytes)?;
            true
        }
        _ => false,
    };
    let ev = SelectionNotifyEvent {
        response_type: xproto::SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: r.time,
        requestor: r.requestor,
        selection: r.selection,
        target: r.target,
        property: if answered { property } else { AtomEnum::NONE.into() },
    };
    x.conn.send_event(false, r.requestor, EventMask::NO_EVENT, ev)?;
    Ok(())
}

/// The XDND-aware window under the root point (`rx`, `ry`): down the tree from the root to the
/// first window with `XdndAware`, at most [`MAX_DEPTH`] deep.
fn find(x: &X, rx: i16, ry: i16) -> R<Under> {
    let mut w = x.root;
    for _ in 0..MAX_DEPTH {
        let child = x.conn.translate_coordinates(x.root, w, rx, ry)?.reply()?.child;
        if child == u32::from(AtomEnum::NONE) {
            return Ok(Under::Nothing);
        }
        w = child;
        if w == x.own || card(x, w, x.a._NET_WM_PID, AtomEnum::CARDINAL.into())? == Some(x.pid) {
            return Ok(Under::Own);
        }
        // A proxy is honoured only when it names itself as its own proxy (the spec's check).
        let proxy = card(x, w, x.a.XdndProxy, AtomEnum::WINDOW.into())?;
        let proxy = match proxy {
            Some(p) if card(x, p, x.a.XdndProxy, AtomEnum::WINDOW.into()).ok().flatten() == Some(p) => Some(p),
            _ => None,
        };
        let version = card(x, proxy.unwrap_or(w), x.a.XdndAware, AtomEnum::ATOM.into())?;
        if let Some(version) = version {
            return Ok(Under::Aware { window: w, to: proxy.unwrap_or(w), version });
        }
    }
    Ok(Under::Nothing)
}

/// The first 32-bit value of property `prop` (of type `kind`) on `w`.
fn card(x: &X, w: u32, prop: u32, kind: u32) -> R<Option<u32>> {
    let reply = x.conn.get_property(false, w, prop, kind, 0, 1)?.reply()?;
    Ok(reply.value32().and_then(|mut v| v.next()))
}

/// The pill beside the pointer: an override-redirect window of `_NET_WM_WINDOW_TYPE_DND` showing
/// `img` (premultiplied RGBA, blended over its own middle colour, the pill's fill). Only on a
/// 24- or 32-bit screen of 32 bits per pixel; else none.
fn pill_window(x: &X, img: &dalica_ui_kit::Image, depth: u8) -> R<Option<u32>> {
    let setup = x.conn.setup();
    let bpp = setup.pixmap_formats.iter().find(|f| f.depth == depth).map(|f| f.bits_per_pixel);
    if !(depth == 24 || depth == 32) || bpp != Some(32) {
        return Ok(None);
    }
    let (w, h) = (u16::try_from(img.width)?, u16::try_from(img.height)?);
    if w == 0 || h == 0 {
        return Ok(None);
    }
    let bytes = pixels(img, setup.image_byte_order == ImageOrder::LSB_FIRST);
    let window = x.conn.generate_id()?;
    let aux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::NO_EVENT);
    x.conn.create_window(0, window, x.root, -1000, -1000, w, h, 0, WindowClass::INPUT_OUTPUT, 0, &aux)?;
    x.conn.change_property32(PropMode::REPLACE, window, x.a._NET_WM_WINDOW_TYPE, AtomEnum::ATOM, &[x.a._NET_WM_WINDOW_TYPE_DND])?;
    let pixmap = x.conn.generate_id()?;
    x.conn.create_pixmap(depth, pixmap, window, w, h)?;
    let gc = x.conn.generate_id()?;
    x.conn.create_gc(gc, pixmap, &CreateGCAux::new())?;
    // In bands of rows that fit a request.
    let row = usize::from(w) * 4;
    let rows = (x.conn.maximum_request_bytes().saturating_sub(64) / row.max(1)).clamp(1, usize::from(h));
    for (i, band) in bytes.chunks(row * rows).enumerate() {
        let y = i16::try_from(i * rows)?;
        let n = u16::try_from(band.len() / row)?;
        x.conn.put_image(ImageFormat::Z_PIXMAP, pixmap, gc, w, n, 0, y, 0, depth, band)?;
    }
    x.conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().background_pixmap(pixmap))?;
    x.conn.free_gc(gc)?;
    x.conn.free_pixmap(pixmap)?;
    x.conn.map_window(window)?;
    Ok(Some(window))
}

/// `img`'s pixels as the X server's 32-bit Z format (B, G, R, X in least-significant-byte-first
/// order; X, R, G, B otherwise), blended over the image's middle pixel.
pub fn pixels(img: &dalica_ui_kit::Image, lsb_first: bool) -> Vec<u8> {
    let bg = img.pixel(img.width / 2, img.height / 2).unwrap_or([0, 0, 0, 255]);
    let mut out = Vec::with_capacity(img.rgba.len());
    for &[r, g, b, a] in img.rgba.as_chunks::<4>().0 {
        let a = u16::from(a);
        let over = |c: u8, b: u8| (u16::from(c) + u16::from(b) * (255 - a) / 255).min(255) as u8;
        let (r, g, b) = (over(r, bg[0]), over(g, bg[1]), over(b, bg[2]));
        if lsb_first {
            out.extend_from_slice(&[b, g, r, 0xff]);
        } else {
            out.extend_from_slice(&[0xff, r, g, b]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Under = Under::Aware { window: 10, to: 10, version: 5 };
    const B: Under = Under::Aware { window: 20, to: 21, version: 4 };

    #[test]
    fn entering_moving_and_leaving_targets() {
        let mut s = Source::default();
        assert_eq!(s.pointer(Under::Nothing, 1, 1), []);
        assert_eq!(s.pointer(A, 5, 6), [Send::Enter { to: 10, window: 10, version: 5 }, Send::Position { to: 10, window: 10, x: 5, y: 6 }]);
        // One position at a time: a move before the answer waits for it.
        assert_eq!(s.pointer(A, 7, 8), []);
        assert_eq!(s.pointer(A, 9, 9), []);
        assert_eq!(s.status(10, true), [Send::Position { to: 10, window: 10, x: 9, y: 9 }]);
        assert_eq!(s.status(10, true), []);
        assert_eq!(s.pointer(A, 9, 9), [], "no move, no message");
        // To another target, through its proxy, in its own version.
        assert_eq!(
            s.pointer(B, 30, 30),
            [Send::Leave { to: 10, window: 10 }, Send::Enter { to: 21, window: 20, version: 4 }, Send::Position { to: 21, window: 20, x: 30, y: 30 }]
        );
        // An answer from a window left behind changes nothing.
        assert_eq!(s.status(10, true), []);
        // Back over Dalica: a leave, and inside again.
        assert_eq!(s.pointer(Under::Own, 40, 40), [Send::Leave { to: 21, window: 20 }]);
        assert_eq!(s.release(), (vec![], false));
        // A window of a version older than 3 is not spoken to.
        let mut s = Source::default();
        assert_eq!(s.pointer(Under::Aware { window: 3, to: 3, version: 2 }, 1, 1), []);
    }

    #[test]
    fn a_drop_only_where_the_target_accepted_then_wait_for_finished() {
        let mut s = Source::default();
        s.pointer(A, 5, 5);
        s.status(10, false);
        assert_eq!(s.release(), (vec![Send::Leave { to: 10, window: 10 }], false), "refused: a leave");
        let mut s = Source::default();
        s.pointer(A, 5, 5);
        s.status(10, true);
        assert_eq!(s.release(), (vec![Send::Drop { to: 10, window: 10 }], true));
        assert!(!s.finished(99));
        assert!(s.finished(10));
        // Dropped: nothing more is sent, and a cancel cannot take it back.
        assert_eq!(s.pointer(B, 1, 1), []);
        assert_eq!(s.cancel(), []);
        assert_eq!(s.release(), (vec![], false));
    }

    #[test]
    fn a_cancel_leaves_the_target() {
        let mut s = Source::default();
        assert_eq!(s.cancel(), []);
        s.pointer(B, 5, 5);
        assert_eq!(s.cancel(), [Send::Leave { to: 21, window: 20 }]);
        assert_eq!(s.cancel(), []);
    }

    fn atoms() -> Atoms {
        let mut n = 100;
        let mut next = || {
            n += 1;
            n
        };
        Atoms {
            XdndAware: next(),
            XdndSelection: next(),
            XdndEnter: next(),
            XdndPosition: next(),
            XdndStatus: next(),
            XdndLeave: next(),
            XdndDrop: next(),
            XdndFinished: next(),
            XdndActionCopy: next(),
            XdndTypeList: next(),
            XdndProxy: next(),
            TARGETS: next(),
            UTF8_STRING: next(),
            _NET_WM_PID: next(),
            _NET_WM_WINDOW_TYPE: next(),
            _NET_WM_WINDOW_TYPE_DND: next(),
            URI_LIST: next(),
            TEXT_UTF8: next(),
            TEXT: next(),
        }
    }

    #[test]
    fn the_messages_say_what_the_protocol_says() {
        let a = atoms();
        let (to, m) = message(&a, 7, 1234, Send::Enter { to: 21, window: 20, version: 5 });
        assert_eq!((to, m.window, m.type_, m.format), (21, 20, a.XdndEnter, 32));
        assert_eq!(m.data.as_data32(), [7, 5 << 24, a.URI_LIST, a.TEXT_UTF8, a.UTF8_STRING]);
        let (_, m) = message(&a, 7, 1234, Send::Position { to: 20, window: 20, x: 300, y: -4 });
        assert_eq!(m.data.as_data32(), [7, 0, (300 << 16), 1234, a.XdndActionCopy], "a point off the screen's top counts as 0");
        let (_, m) = message(&a, 7, 1234, Send::Drop { to: 20, window: 20 });
        assert_eq!((m.type_, m.data.as_data32()), (a.XdndDrop, [7, 0, 1234, 0, 0]));
        let (_, m) = message(&a, 7, 1234, Send::Leave { to: 20, window: 20 });
        assert_eq!((m.type_, m.data.as_data32()), (a.XdndLeave, [7, 0, 0, 0, 0]));
    }

    #[test]
    fn the_selection_holds_the_files_as_uris_and_as_text() {
        let a = atoms();
        let paths = [PathBuf::from("/photos/Lisbon/IMG 1.jpg"), PathBuf::from("/photos/Porto/b.jpg")];
        let (kind, format, bytes) = answer(&a, a.URI_LIST, &paths).unwrap();
        assert_eq!((kind, format), (a.URI_LIST, 8));
        assert_eq!(String::from_utf8(bytes).unwrap(), "file:///photos/Lisbon/IMG%201.jpg\r\nfile:///photos/Porto/b.jpg\r\n");
        let (_, _, text) = answer(&a, a.UTF8_STRING, &paths).unwrap();
        assert_eq!(String::from_utf8(text).unwrap(), "/photos/Lisbon/IMG 1.jpg\n/photos/Porto/b.jpg");
        let (kind, format, list) = answer(&a, a.TARGETS, &paths).unwrap();
        assert_eq!((kind, format, list.len()), (u32::from(AtomEnum::ATOM), 32, 5 * 4));
        assert!(answer(&a, 9999, &paths).is_none(), "a type not offered");
    }

    #[test]
    fn the_pill_pixels_are_blended_over_its_fill_in_the_servers_order() {
        // 3 × 1: a transparent corner, the opaque fill, a half-covered white.
        let img = dalica_ui_kit::Image { width: 3, height: 1, rgba: vec![0, 0, 0, 0, 40, 50, 60, 255, 128, 128, 128, 128] };
        assert_eq!(pixels(&img, true), [60, 50, 40, 255, 60, 50, 40, 255, 157, 152, 147, 255]);
        assert_eq!(&pixels(&img, false)[..4], [255, 40, 50, 60]);
    }

    /// Against the X server of this session, when there is one: an XDND-aware window of the test's
    /// own, mapped off the screen, takes a drop of two files from the source (the source's pointer
    /// fed by the test, not the real one) and reads them through the selection.
    #[test]
    fn a_drop_on_a_live_xdnd_window_hands_it_the_files() {
        let Ok((conn, screen)) = x11rb::connect(None) else {
            eprintln!("skipped: no X server");
            return;
        };
        let root = conn.setup().roots[screen].root;
        let a = Atoms::new(&conn).unwrap().reply().unwrap();
        // The target: XdndAware 5, at (-4000, -4000).
        let target = conn.generate_id().unwrap();
        let aux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::PROPERTY_CHANGE);
        conn.create_window(0, target, root, -4000, -4000, 50, 50, 0, WindowClass::INPUT_ONLY, 0, &aux).unwrap();
        conn.change_property32(PropMode::REPLACE, target, a.XdndAware, AtomEnum::ATOM, &[5]).unwrap();
        conn.map_window(target).unwrap();
        conn.flush().unwrap();
        // The source, on a connection of its own, as the drag thread has.
        let (sconn, _) = x11rb::connect(None).unwrap();
        let sa = Atoms::new(&sconn).unwrap().reply().unwrap();
        let source = sconn.generate_id().unwrap();
        let saux = CreateWindowAux::new().override_redirect(1).event_mask(EventMask::PROPERTY_CHANGE);
        sconn.create_window(0, source, root, -10, -10, 1, 1, 0, WindowClass::INPUT_ONLY, 0, &saux).unwrap();
        let x = X { conn: sconn, a: sa, root, source, own: 0, pid: 0 };
        x.conn.change_property32(PropMode::REPLACE, source, x.a.XdndTypeList, AtomEnum::ATOM, &types(&x.a)).unwrap();
        x.conn.flush().unwrap();
        let time = server_time(&x).unwrap();
        x.conn.set_selection_owner(source, x.a.XdndSelection, time).unwrap();
        // The source finds the target under the point and enters it.
        let under = find(&x, -3990, -3990).unwrap();
        assert_eq!(under, Under::Aware { window: target, to: target, version: 5 });
        let mut s = Source::default();
        send(&x, time, s.pointer(under, -3990, -3990)).unwrap();
        x.conn.flush().unwrap();
        let wait = |pred: &mut dyn FnMut(&Event) -> bool| {
            let until = Instant::now() + Duration::from_secs(5);
            while Instant::now() < until {
                if let Some(e) = conn.poll_for_event().unwrap()
                    && pred(&e)
                {
                    return e;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            panic!("no such event");
        };
        let enter = wait(&mut |e| matches!(e, Event::ClientMessage(m) if m.type_ == a.XdndEnter));
        let Event::ClientMessage(enter) = enter else { panic!("not a client message") };
        assert_eq!(enter.data.as_data32()[0], source);
        wait(&mut |e| matches!(e, Event::ClientMessage(m) if m.type_ == a.XdndPosition));
        // The target accepts, the button is let go: a drop.
        let status = ClientMessageEvent::new(32, source, a.XdndStatus, [target, 1, 0, 0, a.XdndActionCopy]);
        conn.send_event(false, source, EventMask::NO_EVENT, status).unwrap();
        conn.flush().unwrap();
        let paths = vec![PathBuf::from("/photos/a.jpg"), PathBuf::from("/photos/b c.jpg")];
        let until = Instant::now() + Duration::from_secs(5);
        while s.target.is_some_and(|t| !t.accepted) && Instant::now() < until {
            events(&x, &mut s, time, &paths).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        let (msgs, wait_finish) = s.release();
        assert!(wait_finish, "the target accepted");
        send(&x, time, msgs).unwrap();
        x.conn.flush().unwrap();
        wait(&mut |e| matches!(e, Event::ClientMessage(m) if m.type_ == a.XdndDrop));
        // The target reads the files, as a file manager does.
        conn.convert_selection(target, a.XdndSelection, a.URI_LIST, a.URI_LIST, time).unwrap();
        conn.flush().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut got = None;
        while got.is_none() && Instant::now() < until {
            events(&x, &mut s, time, &paths).unwrap();
            if let Some(Event::SelectionNotify(n)) = conn.poll_for_event().unwrap() {
                got = Some(n);
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let n = got.expect("the selection answered");
        assert_eq!(n.property, a.URI_LIST);
        let value = conn.get_property(true, target, a.URI_LIST, AtomEnum::ANY, 0, 1024).unwrap().reply().unwrap().value;
        assert_eq!(String::from_utf8(value).unwrap(), "file:///photos/a.jpg\r\nfile:///photos/b%20c.jpg\r\n");
        // Finished.
        let fin = ClientMessageEvent::new(32, source, a.XdndFinished, [target, 1, a.XdndActionCopy, 0, 0]);
        conn.send_event(false, source, EventMask::NO_EVENT, fin).unwrap();
        conn.flush().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let mut done = false;
        while !done && Instant::now() < until {
            done = events(&x, &mut s, time, &paths).unwrap();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(done, "the target finished");
        conn.destroy_window(target).unwrap();
        conn.flush().unwrap();
    }
}
