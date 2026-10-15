//! Notices: what the session reports on its own (a metadata write that failed, a file changed in
//! another application, a file job that ended) shown as toasts (`design-system.md` §6 `toast`) at
//! the bottom of the light table. The app polls the session once a frame ([`Notices::poll`]), draws
//! the toasts still showing ([`Notices::show`], `toast:<n>`) and reports them to `ui.inspect` →
//! `toasts` ([`Notices::inspect`]). Each toast stays its kind's time (1.4 s, cautions and failures
//! 6 s; one with an action 6 s too, so there is time to press it), then is dropped. Its text wraps
//! to at most three lines; only a longer one is cut short, in the middle, and never inside the file
//! name it is about.
//!
//! - **Metadata writes** that failed (`dalica_engine::metadata`): each becomes one `caution` toast
//!   naming the file and the fields (nothing is lost: the folder sidecar keeps the values;
//!   `data-model.md` §5, `xmp-interop.md` §1).
//! - **File jobs** (Save, Save a Copy, Save As, Export; `dalica_engine::filejobs`): each job that
//!   ends becomes one toast ([`event_toast`]): its message ("Exported 48 pictures", with "; 2
//!   skipped" when some were skipped; a saved copy's name), `info`; when items failed, `caution`,
//!   naming the first file that failed and why ("Could not save IMG_0041.jpg: the file is
//!   read-only", "and 2 more" after it). A job that names a folder (an export's) gets the
//!   **Show Folder** button (`button:toast.showFolder`; `creating.md` §2): until OS integration
//!   (M15) it brings the folder up in the library: back from the edit view, the folder selected as
//!   a unit and the light table scrolled to it ([`act`]; while the folder's scan is still bringing
//!   it in, it waits for it).
//!
//!   A file operation's job (kind `files`: photos or folders moved, copied, renamed or trashed)
//!   gives no toast of its own here ([`job_toast`]); its notice comes from its `FileOpEvent`.
//! - **File operations** (`dalica_engine::FileOpEvent`, `organizing.md` §9): each operation, undo
//!   or redo that ends becomes one toast ([`op_toast`]), worded from its kind and direction with
//!   its count: "Moved 3 photos to Lisbon", "Copied 1 photo to Porto", "Renamed 3 photos", "Moved 2
//!   photos to the Trash", "Moved the folder to Archive", "Renamed the folder to Lisbon 2026"; an
//!   undo says what went back ("Moved 3 photos back", "Moved 3 copies to the Trash", "Restored 2
//!   photos from the Trash"); skipped duplicates are counted ("; 2 duplicates skipped"), Cancel
//!   too. `info`; when items failed, `caution`, naming the first file that failed and why ("Could
//!   not move IMG_0041.jpg: the file is in use", "and 2 more" after it). Move to New Folder's
//!   count leaves out the step that makes (or, undone, removes) the folder.
//!
//! The app's own confirmations come through [`Notices::push`].

use std::path::{Path, PathBuf};

use dalica_engine::{Direction, FileJobEvent, FileOpEvent, FileOpKind, JobId, JobKind, Session};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{self, ToastKind, ToastText};
use egui::{Rect, Ui};
use serde_json::{Value, json};

use crate::app::App;
use crate::state::Section;

/// The most toasts kept at once; a new one beyond it drops the oldest.
pub const MAX_TOASTS: usize = 8;
/// The longest toast text kept, in characters.
pub const MAX_TEXT: usize = 300;

/// The Show Folder button's automation id and label.
pub const SHOW_FOLDER_ID: &str = "button:toast.showFolder";
pub const SHOW_FOLDER: &str = "Show Folder";
/// How long a toast with an action shows at least, in seconds.
pub const ACTION_SECS: f64 = 6.0;
/// How many frames a Show Folder waits for its folder once no scan runs (the light table takes
/// in a folder the frame after the library has it).
const SHOW_FOLDER_FRAMES: u32 = 30;
/// The most files tried kept for jobs not ended yet ([`Notices::name_tried`]).
const MAX_TRIED: usize = 8;
/// Why Show Folder found nothing.
pub const NOT_IN_LIBRARY: &str = "The folder is not in the library.";

/// What a toast's button does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Brings this folder up in the library ([`act`]).
    ShowFolder(PathBuf),
}

/// What a finished file job shows: the text, the part of it kept whole (the failed file's name),
/// the kind and the button.
#[derive(Debug, Clone, PartialEq)]
pub struct JobToast {
    pub text: String,
    pub whole: String,
    pub kind: ToastKind,
    pub action: Option<Action>,
}

/// One toast: its number (the `<n>` of `toast:<n>`), text, how many characters at the start of
/// the text stay whole if it is cut short, kind, when it appeared (seconds on the frame clock) and
/// its button, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub n: u64,
    pub text: String,
    pub keep: usize,
    pub kind: ToastKind,
    pub shown_at: f64,
    pub action: Option<Action>,
}

impl Toast {
    /// How long it shows, in seconds: its kind's time, at least [`ACTION_SECS`] with a button.
    fn life(&self) -> f64 {
        if self.action.is_some() { self.kind.life().max(ACTION_SECS) } else { self.kind.life() }
    }

    /// Its age as its kind's fade counts it: the extra time a button gives comes first, so the
    /// toast fades over the last moments of its whole life.
    fn fade_age(&self, now: f64) -> f64 {
        let age = now - self.shown_at - (self.life() - self.kind.life());
        if age.is_finite() { age.max(0.0) } else { f64::INFINITY }
    }

    /// The seconds it still shows at `now` (0 once over).
    pub fn remaining(&self, now: f64) -> f64 {
        let age = now - self.shown_at;
        if age.is_finite() { (self.life() - age.max(0.0)).max(0.0) } else { 0.0 }
    }
}

/// The toasts showing now.
#[derive(Debug, Default)]
pub struct Notices {
    toasts: Vec<Toast>,
    next: u64,
    /// The number of the last session event shown ([`dalica_engine::MetadataEvent::seq`]).
    seen: u64,
    /// The number of the last file-job event shown ([`FileJobEvent::seq`], which starts at 0).
    seen_jobs: Option<u64>,
    /// The number of the last file-operation event shown ([`FileOpEvent::seq`], from 0).
    seen_ops: Option<u64>,
    /// A Show Folder pressed and not done yet: the folder, and the frames it may still wait once
    /// no scan runs.
    want: Option<(PathBuf, u32)>,
    /// The file each running job tried to write, when the app knows it (Save As's chosen path):
    /// its failure names that file ([`Notices::name_tried`]).
    tried: Vec<(JobId, PathBuf)>,
}

impl Notices {
    /// Shows `text` as a toast of `kind` from `now` on.
    pub fn push(&mut self, text: &str, kind: ToastKind, now: f64) {
        self.push_keeping(text, "", kind, now);
    }

    /// Shows `text` as a toast of `kind` from `now` on; if it has to be cut short, the text up to
    /// and including `whole` (a file name in it; its occurrence followed by ": " first) stays
    /// whole.
    pub fn push_keeping(&mut self, text: &str, whole: &str, kind: ToastKind, now: f64) {
        self.push_with(text, whole, kind, None, now);
    }

    /// [`Notices::push_keeping`] with a button.
    pub fn push_with(&mut self, text: &str, whole: &str, kind: ToastKind, action: Option<Action>, now: f64) {
        let text = clean(text);
        let whole = clean(whole);
        let keep = if whole.is_empty() {
            0
        } else {
            let at = text.find(&format!("{whole}: ")).or_else(|| text.find(&whole));
            at.map_or(0, |i| text.get(..i).map_or(0, |s| s.chars().count()).saturating_add(whole.chars().count()))
        };
        self.next = self.next.saturating_add(1);
        if self.toasts.len() >= MAX_TOASTS {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { n: self.next, text, keep, kind, shown_at: if now.is_finite() { now } else { 0.0 }, action });
    }

    /// Notes that file job `job` writes `path` (Save As's chosen path), so a failure of it names
    /// the file tried, not the photo it was saving. Kept until the job's end is shown; at most
    /// [`MAX_TRIED`] jobs (the oldest dropped).
    pub fn name_tried(&mut self, job: JobId, path: &Path) {
        self.tried.retain(|(j, _)| *j != job);
        if self.tried.len() >= MAX_TRIED {
            self.tried.remove(0);
        }
        self.tried.push((job, path.to_path_buf()));
    }

    /// Turns what `session` reported since the last poll into toasts, and drops the toasts whose
    /// time is over. Cheap when nothing happened: call it once a frame.
    pub fn poll(&mut self, session: &Session, now: f64) {
        self.toasts.retain(|t| t.remaining(now) > 0.0);
        let fresh: Vec<(u64, String, String)> = session
            .metadata_events()
            .filter(|e| e.seq > self.seen)
            .map(|e| (e.seq, e.message(), e.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
            .collect();
        for (seq, text, file) in fresh {
            self.seen = self.seen.max(seq);
            self.push_keeping(&text, &file, ToastKind::Caution, now);
        }
        let ended: Vec<(u64, JobId, Option<JobToast>)> = session
            .file_job_events()
            .filter(|e| self.seen_jobs.is_none_or(|s| e.seq > s))
            .map(|e| (e.seq, e.job, job_toast(e, self.tried.iter().find(|(j, _)| *j == e.job).map(|(_, p)| p.as_path()))))
            .collect();
        for (seq, job, t) in ended {
            self.tried.retain(|(j, _)| *j != job);
            self.seen_jobs = Some(self.seen_jobs.map_or(seq, |s| s.max(seq)));
            if let Some(t) = t {
                self.push_with(&t.text, &t.whole, t.kind, t.action, now);
            }
        }
        let ops: Vec<(u64, JobToast)> =
            session.file_op_events().filter(|e| self.seen_ops.is_none_or(|s| e.seq > s)).map(|e| (e.seq, op_toast(e))).collect();
        for (seq, t) in ops {
            self.seen_ops = Some(self.seen_ops.map_or(seq, |s| s.max(seq)));
            self.push_with(&t.text, &t.whole, t.kind, t.action, now);
        }
    }

    /// The folder a Show Folder pressed asks for, while it waits ([`act`]).
    pub fn wanted(&self) -> Option<&Path> {
        self.want.as_ref().map(|(p, _)| p.as_path())
    }

    /// The toasts still showing at `now`, oldest first.
    pub fn showing(&self, now: f64) -> impl Iterator<Item = &Toast> {
        self.toasts.iter().filter(move |t| t.remaining(now) > 0.0)
    }

    /// Draws the newest toast still showing at the bottom of `area` (the light table), with its
    /// button. A button pressed does its work this frame ([`act`]) and the toast goes.
    pub fn show(&mut self, ui: &Ui, t: &Tokens, area: Rect, now: f64) {
        let Some(toast) = self.showing(now).last() else { return };
        let text = ToastText::new(&toast.text, toast.keep);
        let button = toast.action.as_ref().map(|_| (SHOW_FOLDER_ID, SHOW_FOLDER));
        let pressed =
            widgets::toast(ui, t, &format!("toast:{}", toast.n), area, text, toast.kind, toast.fade_age(now), button).is_some_and(|r| r.clicked());
        if pressed {
            let (n, action) = (toast.n, toast.action.clone());
            self.toasts.retain(|t| t.n != n);
            if let Some(Action::ShowFolder(path)) = action {
                self.want = Some((path, SHOW_FOLDER_FRAMES));
            }
        }
    }

    /// `ui.inspect` → `toasts`: `[{id, text, kind, remaining}]` (`kind` `info`, `caution` or `error`,
    /// `remaining` in seconds).
    pub fn inspect(&self, now: f64) -> Value {
        let list: Vec<Value> = self
            .showing(now)
            .map(|toast| {
                let kind = match toast.kind {
                    ToastKind::Info => "info",
                    ToastKind::Caution => "caution",
                    ToastKind::Error => "error",
                };
                let mut v = json!({ "id": format!("toast:{}", toast.n), "text": toast.text, "kind": kind, "remaining": toast.remaining(now) });
                if let (Some(Action::ShowFolder(folder)), Some(o)) = (&toast.action, v.as_object_mut()) {
                    o.insert("action".into(), json!({ "id": SHOW_FOLDER_ID, "folder": folder }));
                }
                v
            })
            .collect();
        Value::Array(list)
    }
}

/// The toast file job `e`'s end shows, if any: none for a file operation (moved, copied, renamed
/// or trashed files, kind `files`), whose notice comes from its own event
/// (`dalica_engine::FileOpEvent`), not from the job's generic end; [`event_toast`] for the others,
/// its failure naming `tried` when the app knows the file the job tried to write.
pub fn job_toast(e: &FileJobEvent, tried: Option<&Path>) -> Option<JobToast> {
    (e.kind != JobKind::Files).then(|| tried_toast(e, tried))
}

/// What file job `e`'s end shows: its message, `info`; with failures, `caution`, the first file
/// that failed and why after the message ("Could not save IMG_0041.jpg: …" alone when nothing was
/// done), "and n more" when several did; Show Folder when the job names a folder.
pub fn event_toast(e: &FileJobEvent) -> JobToast {
    tried_toast(e, None)
}

/// [`event_toast`], the first failure naming `tried` (the file the job tried to write: Save As's
/// chosen name, "Could not save Second Try.jpg: a file of that name already exists") rather than
/// the photo it was saving.
fn tried_toast(e: &FileJobEvent, tried: Option<&Path>) -> JobToast {
    let action = e.folder.clone().map(Action::ShowFolder);
    let Some(first) = e.failures.first() else {
        return JobToast { text: e.message.clone(), whole: String::new(), kind: ToastKind::Info, action };
    };
    let verb = match e.kind {
        JobKind::Export => "export",
        JobKind::Import => "import",
        _ => "save",
    };
    let file = tried.unwrap_or(&first.file);
    let name = file.file_name().map_or_else(|| file.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
    let mut detail = format!("Could not {verb} {name}: {}", first.reason.trim_end_matches('.'));
    let more = e.failures.len().saturating_sub(1);
    if more > 0 {
        detail.push_str(&format!(", and {more} more"));
    }
    // A Save that did nothing says only what failed; an export (or a Save that did some) keeps
    // its counts in front.
    let text = if e.done == 0 && e.kind != JobKind::Export { detail } else { format!("{}. {detail}", e.message.trim_end_matches('.')) };
    JobToast { text, whole: name, kind: ToastKind::Caution, action }
}

/// The file name of `path` as a toast writes it.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

/// Whether `kind` acts on folders rather than photos.
fn on_folders(kind: FileOpKind) -> bool {
    matches!(kind, FileOpKind::MoveFolder | FileOpKind::RenameFolder | FileOpKind::TrashFolder)
}

/// "1 photo", "3 photos", "the folder", "2 folders"; an undone copy's are "copies".
fn counted(kind: FileOpKind, dir: Direction, n: usize) -> String {
    let (one, many) = match (kind, dir) {
        (FileOpKind::Copy, Direction::Undo) => ("copy", "copies"),
        _ if on_folders(kind) => ("folder", "folders"),
        _ => ("photo", "photos"),
    };
    match n {
        1 if on_folders(kind) => "the folder".into(),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// What finished file operation `e` shows (`organizing.md` §9): what was done, worded from its kind
/// and direction ([`op_toast`]'s module doc), `info`; when items failed, `caution`, the first file
/// that failed and why after it, "and n more" when several did.
pub fn op_toast(e: &FileOpEvent) -> JobToast {
    // Move to New Folder's first item makes the folder (its undo's last one removes it).
    let made = usize::from(e.label == dalica_engine::cmd::files::NEW_FOLDER_LABEL);
    let done = e.done.saturating_sub(made);
    let what = counted(e.kind, e.direction, done);
    let dest = e.destination.as_deref().map(name_of);
    let to = dest.as_ref().map_or_else(String::new, |d| format!(" to {d}"));
    let (head, past, verb, after) = match (e.kind, e.direction) {
        (FileOpKind::Move | FileOpKind::MoveFolder, Direction::Undo) => (format!("Moved {what} back"), "moved back", "move", " back"),
        (FileOpKind::Copy, Direction::Undo) => (format!("Moved {what} to the Trash"), "removed", "remove the copy", ""),
        (FileOpKind::Rename | FileOpKind::RenameFolder, Direction::Undo) => (format!("Renamed {what} back"), "renamed back", "rename", " back"),
        (FileOpKind::Trash | FileOpKind::TrashFolder, Direction::Undo) => (format!("Restored {what} from the Trash"), "restored", "restore", ""),
        (FileOpKind::Move | FileOpKind::MoveFolder, _) => (format!("Moved {what}{to}"), "moved", "move", ""),
        (FileOpKind::Copy, _) => (format!("Copied {what}{to}"), "copied", "copy", ""),
        (FileOpKind::Rename, _) => (format!("Renamed {what}"), "renamed", "rename", ""),
        (FileOpKind::RenameFolder, _) => (format!("Renamed {what}{to}"), "renamed", "rename", ""),
        (FileOpKind::Trash | FileOpKind::TrashFolder, _) => (format!("Moved {what} to the Trash"), "moved to the Trash", "move", " to the Trash"),
    };
    let skipped = e.skipped.len();
    let duplicates = if matches!(e.kind, FileOpKind::Move | FileOpKind::Copy) { "duplicate" } else { "item" };
    let skip_text = match skipped {
        0 => String::new(),
        1 => format!("1 {duplicates} skipped"),
        n => format!("{n} {duplicates}s skipped"),
    };
    let mut text = if done == 0 && skipped > 0 { format!("Nothing {past}: {skip_text}") } else { head };
    if done > 0 && skipped > 0 {
        text.push_str(&format!("; {skip_text}"));
    }
    if e.cancelled {
        text.push_str("; the rest was cancelled");
    }
    let Some(first) = e.failures.first() else {
        return JobToast { text, whole: String::new(), kind: ToastKind::Info, action: None };
    };
    let name = name_of(&first.file);
    let mut detail = format!("Could not {verb} {name}{after}: {}", first.reason.trim_end_matches('.'));
    let more = e.failures.len().saturating_sub(1);
    if more > 0 {
        detail.push_str(&format!(", and {more} more"));
    }
    // An operation that did nothing says only what failed.
    let text = if done == 0 && skipped == 0 && !e.cancelled { detail } else { format!("{text}. {detail}") };
    JobToast { text, whole: name, kind: ToastKind::Caution, action: None }
}

/// Does what a toast's button asked for (once a frame, after the toasts are drawn): Show Folder
/// goes back to the library, selects the folder as a unit and scrolls the light table to it. A
/// folder not in the library yet is waited for while a scan runs (an export's folder comes in by
/// the scan its job starts at its end), then a few frames more; past that the toast says so.
pub fn act(app: &mut App) {
    let Some((path, frames)) = app.notices_mut().want.take() else { return };
    let folder = app.session().and_then(|s| s.library().folder_by_path(&path));
    let scanning = app.session().is_some_and(Session::busy);
    let shown = folder.is_some_and(|folder| {
        let section = Section::Folder(folder);
        if app.state().view == crate::state::View::Edit && app.run("view.library", &json!({})).is_err() {
            return false;
        }
        app.run("select.section", &json!({ "section": section })).is_ok() && app.run("section.scrollTo", &json!({ "section": section })).is_ok()
    });
    if shown {
        return;
    }
    if scanning || frames > 0 {
        app.notices_mut().want = Some((path, if scanning { frames } else { frames.saturating_sub(1) }));
    } else {
        app.toast(NOT_IN_LIBRARY, ToastKind::Caution);
    }
}

/// `text` at most [`MAX_TEXT`] characters long, without control characters.
fn clean(text: &str) -> String {
    text.chars().take(MAX_TEXT).filter(|c| !c.is_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_show_their_time_and_are_bounded() {
        let mut n = Notices::default();
        assert_eq!(n.inspect(0.0), json!([]));
        n.push("Starred 12 photos", ToastKind::Info, 10.0);
        n.push("Could not write the caption\u{7} of IMG_1.jpg", ToastKind::Error, 10.5);
        let v = n.inspect(11.0);
        assert_eq!(v[0]["id"], "toast:1");
        assert_eq!((v[1]["id"].as_str(), v[1]["kind"].as_str()), (Some("toast:2"), Some("error")));
        assert_eq!(v[1]["text"], "Could not write the caption of IMG_1.jpg", "control characters dropped");
        assert!((v[0]["remaining"].as_f64().unwrap() - 0.4).abs() < 1e-9);
        // The info toast is over at 11.4; the failure stays 6 s.
        assert_eq!(n.showing(11.5).count(), 1);
        assert_eq!(n.showing(16.5).count(), 0);
        for i in 0..20 {
            n.push(&"x".repeat(1000), ToastKind::Info, 20.0 + f64::from(i));
        }
        assert_eq!(n.toasts.len(), MAX_TOASTS);
        assert!(n.toasts.iter().all(|t| t.text.chars().count() == MAX_TEXT));
        assert_eq!(n.toasts.last().map(|t| t.n), Some(22));
        // Hostile clocks never show a toast forever.
        let mut h = Notices::default();
        h.push("a", ToastKind::Info, f64::NAN);
        assert_eq!(h.showing(f64::INFINITY).count(), 0);
        assert_eq!(h.showing(f64::NAN).count(), 0);
    }

    fn event(kind: JobKind, message: &str, done: usize, total: usize, failures: &[(&str, &str)], folder: Option<&str>) -> FileJobEvent {
        FileJobEvent {
            seq: 0,
            job: 1,
            kind,
            message: message.into(),
            done,
            total,
            cancelled: false,
            failures: failures.iter().map(|(f, r)| dalica_engine::ItemFailure { file: PathBuf::from(f), reason: (*r).into() }).collect(),
            folder: folder.map(PathBuf::from),
        }
    }

    #[test]
    fn a_file_job_gives_one_toast_worded_by_how_it_ended() {
        let exported = event_toast(&event(JobKind::Export, "Exported 48 pictures", 48, 48, &[], Some("/p/Lisbon")));
        assert_eq!((exported.text.as_str(), exported.kind), ("Exported 48 pictures", ToastKind::Info));
        assert_eq!(exported.action, Some(Action::ShowFolder(PathBuf::from("/p/Lisbon"))));
        // Skipped items are counted in the message and are no caution.
        let skipped = event_toast(&event(JobKind::Export, "Exported 46 pictures; 2 skipped", 48, 48, &[], Some("/p/L")));
        assert_eq!((skipped.text.as_str(), skipped.kind), ("Exported 46 pictures; 2 skipped", ToastKind::Info));
        let failed = event_toast(&event(
            JobKind::Export,
            "Exported 46 pictures; 2 failed",
            46,
            48,
            &[("/a/b.jpg", "no space left."), ("/a/c.jpg", "x")],
            Some("/p/L"),
        ));
        assert_eq!(failed.text, "Exported 46 pictures; 2 failed. Could not export b.jpg: no space left, and 1 more");
        assert_eq!((failed.whole.as_str(), failed.kind), ("b.jpg", ToastKind::Caution));
        assert!(failed.action.is_some(), "what was written can still be shown");
        // A Save that did nothing says what failed; one that did some keeps its message.
        let save = event_toast(&event(JobKind::Save, "Saving: 0 of 1 done, 1 failed", 0, 1, &[("/a/IMG_1.jpg", "the file is read-only")], None));
        assert_eq!((save.text.as_str(), save.kind, save.action), ("Could not save IMG_1.jpg: the file is read-only", ToastKind::Caution, None));
        let some = event_toast(&event(JobKind::Save, "Saving: 2 of 3 done, 1 failed", 2, 3, &[("/a/IMG_1.jpg", "busy")], None));
        assert_eq!(some.text, "Saving: 2 of 3 done, 1 failed. Could not save IMG_1.jpg: busy");
        let copy = event_toast(&event(JobKind::Save, "Saved a copy as IMG_1-001.jpg", 1, 1, &[], None));
        assert_eq!((copy.text.as_str(), copy.kind, copy.action), ("Saved a copy as IMG_1-001.jpg", ToastKind::Info, None));
    }

    #[test]
    fn a_file_operation_job_gives_no_generic_toast() {
        let moved = event(JobKind::Files, "Moved 3 photos", 3, 3, &[], None);
        assert_eq!(job_toast(&moved, None), None, "its FileOpEvent words it");
        let failed = event(JobKind::Files, "Moved 2 of 3 photos", 2, 3, &[("/a/b.jpg", "busy")], None);
        assert_eq!(job_toast(&failed, None), None);
        let exported = event(JobKind::Export, "Exported 1 picture", 1, 1, &[], None);
        assert_eq!(job_toast(&exported, None), Some(event_toast(&exported)));
    }

    fn op(kind: FileOpKind, direction: Direction, label: &'static str, done: usize, skipped: usize, failures: &[(&str, &str)]) -> FileOpEvent {
        let item = |f: &str, r: &str| dalica_engine::ItemFailure { file: PathBuf::from(f), reason: r.into() };
        FileOpEvent {
            seq: 0,
            op: 1,
            job: 1,
            kind,
            direction,
            label,
            total: done + skipped + failures.len(),
            done,
            skipped: (0..skipped).map(|i| item(&format!("/p/s{i}.jpg"), "a file of that name is there")).collect(),
            failures: failures.iter().map(|(f, r)| item(f, r)).collect(),
            cancelled: false,
            destination: Some(PathBuf::from("/p/Lisbon")),
        }
    }

    #[test]
    fn a_file_operation_toast_says_what_was_done_with_its_count() {
        use Direction::{Do, Redo, Undo};
        use FileOpKind::*;
        let text = |e: FileOpEvent| op_toast(&e).text;
        assert_eq!(text(op(Move, Do, "Move", 3, 0, &[])), "Moved 3 photos to Lisbon");
        assert_eq!(text(op(Move, Redo, "Move", 1, 0, &[])), "Moved 1 photo to Lisbon");
        assert_eq!(text(op(Move, Undo, "Move", 3, 0, &[])), "Moved 3 photos back");
        assert_eq!(text(op(Copy, Do, "Copy", 2, 0, &[])), "Copied 2 photos to Lisbon");
        assert_eq!(text(op(Copy, Undo, "Copy", 2, 0, &[])), "Moved 2 copies to the Trash");
        assert_eq!(text(op(Rename, Do, "Rename", 3, 0, &[])), "Renamed 3 photos");
        assert_eq!(text(op(Rename, Undo, "Rename", 3, 0, &[])), "Renamed 3 photos back");
        assert_eq!(text(op(Trash, Do, "Delete", 2, 0, &[])), "Moved 2 photos to the Trash");
        assert_eq!(text(op(Trash, Undo, "Delete", 1, 0, &[])), "Restored 1 photo from the Trash");
        assert_eq!(text(op(MoveFolder, Do, "Move Folder", 1, 0, &[])), "Moved the folder to Lisbon");
        assert_eq!(text(op(RenameFolder, Do, "Rename Folder", 1, 0, &[])), "Renamed the folder to Lisbon");
        assert_eq!(text(op(TrashFolder, Do, "Delete Folder", 1, 0, &[])), "Moved the folder to the Trash");
        assert_eq!(text(op(TrashFolder, Undo, "Delete Folder", 1, 0, &[])), "Restored the folder from the Trash");
        // Move to New Folder counts the photos, not the step that makes (or removes) the folder.
        assert_eq!(text(op(Move, Do, dalica_engine::cmd::files::NEW_FOLDER_LABEL, 4, 0, &[])), "Moved 3 photos to Lisbon");
        assert_eq!(text(op(Move, Undo, dalica_engine::cmd::files::NEW_FOLDER_LABEL, 4, 0, &[])), "Moved 3 photos back");
        // Skipped duplicates are counted; with nothing done, that is all it says.
        assert_eq!(text(op(Copy, Do, "Copy", 1, 2, &[])), "Copied 1 photo to Lisbon; 2 duplicates skipped");
        assert_eq!(text(op(Move, Do, "Move", 0, 1, &[])), "Nothing moved: 1 duplicate skipped");
        let mut cancelled = op(Move, Do, "Move", 2, 0, &[]);
        cancelled.cancelled = true;
        assert_eq!(text(cancelled), "Moved 2 photos to Lisbon; the rest was cancelled");
        let info = op_toast(&op(Move, Do, "Move", 3, 0, &[]));
        assert_eq!((info.kind, info.whole.as_str(), info.action), (ToastKind::Info, "", None));
    }

    #[test]
    fn a_failed_file_operation_is_a_caution_naming_the_file_and_why() {
        use Direction::{Do, Undo};
        use FileOpKind::*;
        let some = op_toast(&op(Move, Do, "Move", 2, 0, &[("/a/IMG_0041.jpg", "the file is in use."), ("/a/b.jpg", "x")]));
        assert_eq!(some.text, "Moved 2 photos to Lisbon. Could not move IMG_0041.jpg: the file is in use, and 1 more");
        assert_eq!((some.kind, some.whole.as_str()), (ToastKind::Caution, "IMG_0041.jpg"));
        let none = op_toast(&op(Copy, Do, "Copy", 0, 0, &[("/a/b.jpg", "no space left")]));
        assert_eq!(none.text, "Could not copy b.jpg: no space left", "nothing done: only what failed");
        assert_eq!(op_toast(&op(Move, Undo, "Move", 0, 0, &[("/a/b.jpg", "busy")])).text, "Could not move b.jpg back: busy");
        assert_eq!(op_toast(&op(Trash, Do, "Delete", 0, 0, &[("/a/b.jpg", "no trash")])).text, "Could not move b.jpg to the Trash: no trash");
        assert_eq!(
            op_toast(&op(Copy, Undo, "Copy", 0, 0, &[("/a/b-001.jpg", "it changed")])).text,
            "Could not remove the copy b-001.jpg: it changed"
        );
        assert_eq!(op_toast(&op(Rename, Undo, "Rename", 0, 0, &[("/a/b.jpg", "taken")])).text, "Could not rename b.jpg back: taken");
        assert_eq!(op_toast(&op(Trash, Undo, "Delete", 0, 0, &[("/a/b.jpg", "gone")])).text, "Could not restore b.jpg: gone");
    }

    #[test]
    fn a_save_as_that_failed_names_the_file_tried() {
        let taken = event(JobKind::Save, "Save As failed: taken.", 0, 1, &[("/a/IMG_0001.jpg", "A file of that name already exists")], None);
        assert_eq!(
            event_toast(&taken).text,
            "Could not save IMG_0001.jpg: A file of that name already exists",
            "the photo, when nothing else is known"
        );
        let t = job_toast(&taken, Some(Path::new("/b/IMG_0002.jpg"))).unwrap();
        assert_eq!((t.text.as_str(), t.whole.as_str()), ("Could not save IMG_0002.jpg: A file of that name already exists", "IMG_0002.jpg"));
        let saved = event(JobKind::Save, "Saved as IMG_0002.jpg", 1, 1, &[], None);
        assert_eq!(job_toast(&saved, Some(Path::new("/b/IMG_0002.jpg"))).unwrap().text, "Saved as IMG_0002.jpg");
        // The names tried are bounded and replaced per job.
        let mut n = Notices::default();
        for job in 0..20 {
            n.name_tried(job, Path::new("/b/x.jpg"));
        }
        n.name_tried(19, Path::new("/b/y.jpg"));
        assert_eq!(n.tried.len(), MAX_TRIED);
        assert_eq!(n.tried.last(), Some(&(19, PathBuf::from("/b/y.jpg"))));
    }

    #[test]
    fn a_toast_with_a_button_stays_long_enough_to_press_it() {
        let mut n = Notices::default();
        n.push_with("Exported 3 pictures", "", ToastKind::Info, Some(Action::ShowFolder(PathBuf::from("/p/L"))), 10.0);
        n.push("Starred 1 photo", ToastKind::Info, 10.0);
        let v = n.inspect(15.0);
        assert_eq!(v.as_array().map(Vec::len), Some(1), "the plain toast is over, the one with a button is not: {v}");
        assert_eq!(v[0]["action"], json!({ "id": SHOW_FOLDER_ID, "folder": "/p/L" }));
        assert!(n.inspect(10.5)[1].get("action").is_none());
        // It fades over its kind's last moments, not before.
        let t = n.toasts.first().unwrap();
        assert_eq!(t.fade_age(10.0), 0.0);
        assert_eq!(t.fade_age(14.6), 0.0);
        assert!((t.fade_age(15.9) - 1.3).abs() < 1e-9);
        assert_eq!(t.fade_age(f64::NAN), f64::INFINITY);
        assert_eq!(n.showing(16.1).count(), 0);
        assert_eq!(n.wanted(), None);
    }

    #[test]
    fn show_folder_waits_a_little_then_says_the_folder_is_missing() {
        let mut app = App::new(None);
        act(&mut app);
        assert_eq!(app.notices().showing(0.0).count(), 0, "nothing asked, nothing done");
        app.notices_mut().want = Some((PathBuf::from("/nowhere"), 2));
        act(&mut app);
        act(&mut app);
        assert_eq!(app.notices().wanted(), Some(Path::new("/nowhere")), "still waiting");
        act(&mut app);
        assert_eq!(app.notices().wanted(), None);
        let toasts: Vec<(&str, ToastKind)> = app.notices().showing(0.0).map(|t| (t.text.as_str(), t.kind)).collect();
        assert_eq!(toasts, [(NOT_IN_LIBRARY, ToastKind::Caution)]);
    }

    #[test]
    fn the_file_name_is_what_stays_whole() {
        let mut n = Notices::default();
        n.push_keeping("Could not write the caption into a.jpg: the file is read-only", "a.jpg", ToastKind::Caution, 0.0);
        n.push_keeping("Could not write the caption into caption: no", "caption", ToastKind::Caution, 0.0);
        n.push_keeping("Could not write tags into b\u{7}.jpg", "b\u{7}.jpg", ToastKind::Caution, 0.0);
        n.push_keeping("No file here", "c.jpg", ToastKind::Caution, 0.0);
        n.push("Starred 1 photo", ToastKind::Info, 0.0);
        let keeps: Vec<usize> = n.toasts.iter().map(|t| t.keep).collect();
        assert_eq!(keeps, [38, 40, 31, 0, 0], "the name followed by \": \" first; control characters dropped from both");
        assert_eq!(n.inspect(1.0)[0]["kind"], "caution");
    }
}
