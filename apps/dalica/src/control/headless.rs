//! `--headless` (`control-protocol.md` §8): the app with no window. The headless driver runs the
//! real `App` on a virtual clock and paints with the CPU rasterizer; requests from the transport
//! are answered by the same handlers as in the window, on this thread, which is marked as the UI
//! thread so the `fs` guard refuses disk access from it. Between requests, frames run only while
//! the app has background work (a scan, thumbnails) to pick up. With `--view`, closing the photo
//! viewer (Esc, ×) ends the app too.

use std::collections::HashMap;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use dalica_engine::Session;
use dalica_ui::control::{Code, Connection, Control, Error, Player, serve};
use dalica_ui::{App, Driver};
use serde_json::json;

use super::transport::{Hub, Msg, Pending, QUIT};

/// The headless window: 1400 × 900 points at one pixel per point (`ui.resize` changes it).
pub const SIZE: (u32, u32) = (1400, 900);
/// How often an idle app looks for background work, and a busy one runs a frame.
const IDLE_TICK: Duration = Duration::from_millis(250);
const BUSY_TICK: Duration = Duration::from_millis(16);
/// How long the reply to `app.quit` may take to reach its client before the process exits.
const QUIT_REPLY_WAIT: Duration = Duration::from_secs(2);

/// Serves requests until `app.quit` (or until the transport is gone), then closes the app as the
/// window does ([`App::shutdown`]).
/// Returns whether everything was written.
pub fn run(hub: &Hub, control: &Control, session: Session) -> bool {
    let mut d = Driver::new(App::with_picker(Some(session), control.picker()), SIZE.0, SIZE.1, 1.0);
    let mut conns: HashMap<u64, Connection> = HashMap::new();
    let ui = dalica_fs::mark_ui_thread();
    let quit = loop {
        // `dalica --view`: the viewer closed with Esc or × quits, as `app.quit` does.
        if viewer_quit(d.app()) {
            break None;
        }
        let tick = if d.app().idle() { IDLE_TICK } else { BUSY_TICK };
        match hub.rx.recv_timeout(tick) {
            Ok(Msg::Request(p)) => {
                if !p.dispatch() {
                    continue;
                }
                if p.req.method == QUIT {
                    break Some(p);
                }
                let conn = conns.entry(p.conn).or_default();
                let reply = serve(control, conn, &mut d, &p.req);
                p.reply(reply);
            }
            Ok(Msg::Closed(id)) => {
                if let Some(mut conn) = conns.remove(&id) {
                    for input in conn.release() {
                        if let Err(e) = d.play(&input) {
                            log::warn!("control: releasing a held key failed: {e}");
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !d.app().idle() {
                    d.step();
                }
            }
            Err(RecvTimeoutError::Disconnected) => break None,
        }
    };
    // Quitting: the frames are over, so this thread may write again. Closing is the window's
    // (`App::shutdown`): drafts committed, metadata writes made, everything flushed.
    drop(ui);
    let mut app = d.into_app();
    let flushed = app.shutdown().map_err(|e| e.to_string());
    if let Err(e) = &flushed {
        log::error!("dalica: not every change was saved: {e}");
    }
    drop(app);
    if let Some(p) = quit {
        answer_quit(p, &flushed);
        hub.wait_quit_written(QUIT_REPLY_WAIT);
    }
    flushed.is_ok()
}

/// Whether the photo viewer of `dalica --view` was closed with Esc or ×.
fn viewer_quit(app: &App) -> bool {
    app.session().and_then(|s| s.viewer().closed()) == Some(dalica_engine::viewer::Closed::Quit)
}

fn answer_quit(p: Pending, flushed: &Result<(), String>) {
    let reply = match flushed {
        Ok(()) => Ok(json!({})),
        Err(e) => Err(Error::new(Code::Unsaved, format!("quitting, but not every change was written to disk: {e}"))),
    };
    p.reply(reply);
}
