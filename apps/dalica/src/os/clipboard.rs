//! The system clipboard: files (so a paste in a file manager copies them), a picture and text,
//! through `arboard` as PhotoCraft does (`apps/photocraft/src/services.rs` at 5b087699 and
//! d95fdf31, MIT OR Apache-2.0), its Wayland data-control path included. On Wayland the files are
//! also offered as `x-special/gnome-copied-files`, which GNOME's Files pastes, beside
//! `text/uri-list` (wl-clipboard-rs, the protocol arboard's own Wayland path uses); on X11 arboard
//! offers `text/uri-list`, on Windows a file-drop list and on macOS file URLs.
//!
//! One thread owns the clipboard: on Linux what was copied is served from the process that copied
//! it, so the `arboard::Clipboard` must live on, and the calls never run on the UI thread.

use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use dalica_engine::os::{Clip, OsError};

#[cfg(target_os = "linux")]
use super::launch::file_uri;

/// How long a caller waits for the clipboard thread.
const ANSWER_WITHIN: Duration = Duration::from_secs(10);

/// The `text/uri-list` of `paths` (RFC 2483: one URI per line, CRLF). Linux only: Wayland's
/// data-control copy; elsewhere arboard writes the OS's own file list.
#[cfg(target_os = "linux")]
pub fn uri_list(paths: &[std::path::PathBuf]) -> String {
    paths.iter().map(|p| format!("{}\r\n", file_uri(p))).collect()
}

/// The `x-special/gnome-copied-files` of `paths`: `copy`, then one URI per line (Linux only).
#[cfg(target_os = "linux")]
pub fn gnome_copied_files(paths: &[std::path::PathBuf]) -> String {
    let mut out = String::from("copy");
    for p in paths {
        out.push('\n');
        out.push_str(&file_uri(p));
    }
    out
}

/// Checks what a clip asks for before the clipboard is touched: files that are absolute paths,
/// a picture whose bytes are exactly `width × height × 4`.
pub fn check(clip: &Clip) -> Result<(), OsError> {
    match clip {
        Clip::Files(paths) if paths.is_empty() => Err(OsError::Failed("there are no files to copy".into())),
        Clip::Files(paths) => match paths.iter().find(|p| !is_absolute(p)) {
            Some(p) => Err(OsError::Failed(format!("{} is not a full path", p.display()))),
            None => Ok(()),
        },
        Clip::Picture { width, height, rgba } => {
            let want = u64::from(*width).checked_mul(u64::from(*height)).and_then(|n| n.checked_mul(4));
            if *width == 0 || *height == 0 || want != Some(rgba.len() as u64) {
                return Err(OsError::Failed(format!("a {width} × {height} picture needs {} bytes, not {}", want.unwrap_or(u64::MAX), rgba.len())));
            }
            Ok(())
        }
        Clip::Text(_) => Ok(()),
    }
}

fn is_absolute(p: &Path) -> bool {
    p.is_absolute() || p.starts_with("/")
}

type Job = (Clip, Sender<Result<(), OsError>>);

/// The clipboard thread, started on first use.
#[derive(Default)]
pub struct Clipboard {
    jobs: Mutex<Option<Sender<Job>>>,
}

impl Clipboard {
    /// Puts `clip` on the clipboard and waits (at most [`ANSWER_WITHIN`]) for the answer.
    pub fn put(&self, clip: &Clip) -> Result<(), OsError> {
        check(clip)?;
        let (tx, rx) = channel();
        let sent = {
            let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if jobs.is_none() {
                let (jtx, jrx) = channel::<Job>();
                std::thread::Builder::new()
                    .name("dalica-clipboard".into())
                    .spawn(move || serve(jrx))
                    .map_err(|e| OsError::Failed(format!("no clipboard thread: {e}")))?;
                *jobs = Some(jtx);
            }
            jobs.as_ref().map(|j| j.send((clip.clone(), tx)))
        };
        if !matches!(sent, Some(Ok(()))) {
            // The thread is gone (it panicked in a backend): the next call starts a new one.
            *self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            return Err(OsError::Failed("the clipboard stopped".into()));
        }
        rx.recv_timeout(ANSWER_WITHIN).unwrap_or_else(|_| Err(OsError::Failed("the clipboard did not answer".into())))
    }
}

fn serve(jobs: Receiver<Job>) {
    let mut board: Option<arboard::Clipboard> = None;
    while let Ok((clip, answer)) = jobs.recv() {
        let _ = answer.send(put(&mut board, &clip));
    }
}

fn put(board: &mut Option<arboard::Clipboard>, clip: &Clip) -> Result<(), OsError> {
    #[cfg(target_os = "linux")]
    if let Clip::Files(paths) = clip {
        match wayland_files(paths) {
            Ok(()) => return Ok(()),
            Err(e) => log::debug!("os: no Wayland data-control copy of files ({e}); arboard's way instead"),
        }
    }
    let cb = match board {
        Some(cb) => cb,
        None => board.insert(arboard::Clipboard::new().map_err(unavailable)?),
    };
    match clip {
        Clip::Files(paths) => cb.set().file_list(paths).map_err(failed),
        Clip::Picture { width, height, rgba } => cb
            .set_image(arboard::ImageData { width: *width as usize, height: *height as usize, bytes: std::borrow::Cow::Borrowed(rgba) })
            .map_err(failed),
        Clip::Text(text) => cb.set_text(text.as_str()).map_err(failed),
    }
}

fn unavailable(e: arboard::Error) -> OsError {
    OsError::Unavailable(format!("there is no clipboard here: {e}"))
}

fn failed(e: arboard::Error) -> OsError {
    OsError::Failed(format!("the clipboard refused it: {e}"))
}

/// Wayland's data-control copy of files, served from its own thread until another copy replaces
/// it: `text/uri-list` and `x-special/gnome-copied-files`. `Err` off Wayland or when the
/// compositor has no data-control protocol (arboard then copies them through XWayland).
#[cfg(target_os = "linux")]
fn wayland_files(paths: &[std::path::PathBuf]) -> Result<(), String> {
    use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};
    if std::env::var_os("WAYLAND_DISPLAY").is_none_or(|d| d.is_empty()) {
        return Err("not on Wayland".into());
    }
    let source = |mime: &str, text: String| MimeSource {
        source: Source::Bytes(text.into_bytes().into_boxed_slice()),
        mime_type: MimeType::Specific(mime.into()),
    };
    let mut options = Options::new();
    options.omit_additional_text_mime_types(true);
    options
        .copy_multi(vec![source("text/uri-list", uri_list(paths)), source("x-special/gnome-copied-files", gnome_copied_files(paths))])
        .map_err(|e| e.to_string())
}
