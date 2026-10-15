//! Deleting to the trash (`organizing.md` §9, `security.md` §5.7, `data-model.md` §8):
//! `photo.delete` and `folder.delete`, file operations (`fileops.rs`) whose undo restores from the
//! trash where the platform can.
//!
//! - **Asking.** Both ask first: without `"confirm": true` they refuse (`disabled`) with the
//!   dialog's question, unless Options → "Delete from disk without asking"
//!   (`organize.deleteWithoutAsking`) is on. Over automation the UI asks through its dialog host
//!   (`needs_confirm`) before the engine sees the command (`control-protocol.md` §5.5).
//! - **Only the trash.** Each photo, or the folder with everything in it (non-picture files and
//!   subfolders included), goes to the OS trash through `dalica-fs` (`trash::send`) on the file
//!   job's thread. A volume without a trash fails that item with an explanation pointing to the
//!   file manager ([`no_trash`]), and the file stays: there is no permanent delete, whatever the
//!   parameters.
//! - **The library.** A trashed photo leaves the model with everything said about it (star,
//!   caption, tags, recipe, album memberships and covers); a trashed folder takes every library
//!   folder at and under it. The ops' inverse, kept in the operation's one undo step, holds all of
//!   it. The photos' sidecar entries stay where they are (a trashed folder's sidecar is inside it):
//!   the stale sweep (`data-model.md` §7) prunes them after 30 days, so a photo put back from the
//!   trash outside Dalica within that time gets its data back too. An originals backup in
//!   `.dalica-originals/` is left alone.
//! - **Undo** brings the items back from the trash to their old paths (`trash::restore`: matched by
//!   name, folder and identity, never replacing anything there) and puts the kept data back. Where
//!   the trash cannot be listed (macOS) the step cannot be undone, and Undo says why
//!   ([`UNDO_UNSUPPORTED`]). Redo sends them to the trash again.
//! - **Refused** before anything starts: an unknown photo or folder, an offline folder, a volume
//!   root or the home folder.
//! - **A folder's pending sidecar writes go first:** the command asks the I/O worker to write
//!   everything pending (a star set a moment ago) and the folder waits for that before it goes to
//!   the trash ([`Flushed`]), so the sidecar in the trash is current and no write is left aiming at
//!   a folder that is gone. `folder.move` does the same.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use dalica_core::{FolderId, PhotoId};
use dalica_fs::trash::{self, Restore, Sent, Trashed};
use dalica_library::Op;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::fileops::{Driver, FileOp, FileOpKind, Ran};
use crate::registry::{CommandSpec, Context, Effect, History, always, params};
use crate::session::{Msg, Session};

/// Whether this platform's trash can give items back (Linux and the BSDs, Windows; `dalica-fs`'s
/// `trash::restore` answers `Unsupported` elsewhere): undoing a delete is offered only here.
pub const RESTORES: bool = cfg!(any(windows, all(unix, not(target_vendor = "apple"), not(target_os = "android"))));

/// Why a delete cannot be undone where the trash cannot be listed (macOS).
pub const UNDO_UNSUPPORTED: &str = "Dalica cannot take items back out of the Trash on this system: restore them from the Trash in Finder.";
/// The most photos one `photo.delete` takes.
pub const MAX_IDS: usize = 100_000;
/// An offline folder.
pub const OFFLINE: &str = "The folder is offline: connect its drive to delete its pictures.";
/// A volume root or the home folder.
pub const NOT_A_DRIVE: &str = "Dalica does not delete a whole drive or your home folder.";
/// How long a file operation's first item waits for the sidecar writes pending when it started.
pub const FLUSH_WAIT: Duration = Duration::from_secs(60);

/// The I/O worker's answer to "write everything pending now", asked for on the owner thread when a
/// file operation starts and waited for on the job's thread before its first item (once; undo and
/// redo do not wait).
#[derive(Debug, Default)]
pub struct Flushed(Mutex<Option<Receiver<()>>>);

impl Flushed {
    /// Asks `s`'s I/O worker to write everything pending; never touches the disk.
    pub(crate) fn ask(s: &mut Session) -> Flushed {
        let (ack, done) = mpsc::channel();
        s.send(Msg::Flush(ack));
        Flushed(Mutex::new(Some(done)))
    }

    /// Waits (the first time) until the writes are done, at most [`FLUSH_WAIT`].
    pub(crate) fn wait(&self) {
        let done = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(done) = done {
            let _ = done.recv_timeout(FLUSH_WAIT);
        }
    }
}

/// The photos question (the delete dialog's text) for `n` photos.
pub fn photos_question(n: usize) -> String {
    match n {
        1 => "Move the selected picture to the trash?".into(),
        n => format!("Move the {n} selected pictures to the trash?"),
    }
}

/// The folder question, naming the folder.
pub fn folder_question(name: &str) -> String {
    format!("Move the folder \"{name}\" and everything in it (including non-picture files) to the trash?")
}

/// Why an item stayed where it was: its volume has no trash (`security.md` §5.7).
pub fn no_trash(path: &Path) -> String {
    format!(
        "\"{}\" was not deleted: its drive has no trash, and Dalica never deletes anything permanently. Delete it from your file manager if you want it gone.",
        display_name(path)
    )
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.delete",
            label: "Delete from Disk…",
            menu: "File/Delete from Disk…",
            keys: &[(Context::Library, "Delete"), (Context::Library, "Ctrl+Delete")],
            params: r#"{"ids":[PhotoId] (1–100000), "confirm"?:bool}: moves the photos' files to the OS trash as a file job (never a permanent delete: a drive without a trash fails that photo with an explanation and the file stays); the photos leave the library with everything said about them, kept in the one undo step. Asks first: needs "confirm": true unless "Delete from disk without asking" is on. Undo brings them back from the trash where the system allows it (not on macOS). Replies {job, pending:true, total}"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: always,
            run: run_photos,
        },
        CommandSpec {
            id: "folder.delete",
            label: "Delete Folder…",
            menu: "Folder/Delete…",
            keys: &[],
            params: r#"{"folder":FolderId, "confirm"?:bool}: moves the folder with everything in it (non-picture files and subfolders included) to the OS trash as a file job; the folder and every library folder under it leave the library, kept in the one undo step. Asks first: needs "confirm": true unless "Delete from disk without asking" is on. Undo brings it back from the trash where the system allows it (not on macOS). Replies {job, pending:true, total}"#,
            effect: Effect::Files,
            history: History::Library,
            enabled: always,
            run: run_folder,
        },
    ]
}

// --- the driver ---------------------------------------------------------------------------------

/// What an item is in the library.
#[derive(Clone, Debug)]
pub enum What {
    Photo(PhotoId),
    /// The folder and every library folder under it.
    Folder(Vec<FolderId>),
}

/// Which way an item goes.
#[derive(Clone, Debug)]
pub enum Way {
    /// To the trash.
    Send,
    /// Back out of it (undo).
    Restore(Trashed),
}

/// One photo or folder to trash, or to bring back.
#[derive(Clone, Debug)]
pub struct Item {
    pub path: PathBuf,
    pub what: What,
    pub way: Way,
}

/// The trash driver: one item per photo, or one for a folder, which waits for the sidecar writes
/// pending when it started.
#[derive(Debug, Default)]
pub struct Delete {
    flushed: Flushed,
}

impl Driver for Delete {
    type Item = Item;
    /// What the trash recorded for an item sent there (`None` once brought back).
    type Done = Option<Trashed>;

    fn source<'a>(&self, item: &'a Item) -> &'a Path {
        &item.path
    }

    fn target<'a>(&self, _item: &'a Item) -> Option<&'a Path> {
        None
    }

    fn run(&self, item: &Item) -> Result<Ran<Option<Trashed>>, String> {
        self.flushed.wait();
        match &item.way {
            Way::Send => match trash::send(&item.path).map_err(|e| e.to_string())? {
                Sent::Trashed(t) => Ok(Ran::Done(Some(t))),
                Sent::NoTrash => Err(no_trash(&item.path)),
            },
            Way::Restore(t) => {
                let answers = trash::restore(std::slice::from_ref(t)).map_err(|e| e.to_string())?;
                let name = display_name(&item.path);
                match answers.first() {
                    Some(Restore::Restored) => Ok(Ran::Done(None)),
                    Some(Restore::Exists) => Err(format!("Something named \"{name}\" is at its old place now, so it stays in the trash.")),
                    Some(Restore::NotInTrash) => Err(format!("\"{name}\" is no longer in the trash.")),
                    Some(Restore::Unsupported) | None => Err(UNDO_UNSUPPORTED.into()),
                }
            }
        }
    }

    fn ops(&self, s: &Session, item: &Item, _done: &Option<Trashed>) -> Result<Vec<Op>, String> {
        let lib = s.library();
        let op = match &item.what {
            What::Photo(id) => {
                let p = lib.photo(*id).ok_or("the photo had already left the library")?;
                let at = lib.folder(p.folder).map(|f| f.path.join(&p.name));
                if at.as_deref() != Some(item.path.as_path()) {
                    return Err("the photo had moved in the library meanwhile".into());
                }
                Op::RemovePhotos { ids: vec![*id] }
            }
            What::Folder(ids) => {
                let ops: Vec<Op> = ids.iter().filter(|id| lib.folder(**id).is_some()).map(|id| Op::RemoveFolder { id: *id }).collect();
                if ops.is_empty() {
                    return Ok(Vec::new());
                }
                Op::Batch { ops }
            }
        };
        Ok(vec![lib.stamped(op, s.clock().now())])
    }

    fn reverse(&self, item: &Item, done: &Option<Trashed>) -> Result<Item, String> {
        match (&item.way, done) {
            (Way::Send, Some(t)) if RESTORES => Ok(Item { way: Way::Restore(t.clone()), ..item.clone() }),
            (Way::Send, Some(_)) => Err(UNDO_UNSUPPORTED.into()),
            _ => Err("this item has no way back".into()),
        }
    }
}

// --- the commands -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhotosParams {
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FolderParams {
    folder: FolderId,
    #[serde(default)]
    confirm: bool,
}

/// Refuses (as `disabled`, with the question and how to go ahead) unless `confirm` or the setting
/// says not to ask.
fn ask(s: &Session, confirm: bool, question: String) -> Result<(), CmdError> {
    if confirm || s.settings().organize().delete_without_asking {
        Ok(())
    } else {
        Err(CmdError::Disabled(format!("Delete: {question} Send \"confirm\": true to go ahead.")))
    }
}

fn run_photos(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let PhotosParams { mut ids, confirm } = params(p)?;
    if ids.is_empty() {
        return Err(CmdError::BadParams("`ids` names no photo".into()));
    }
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    let mut items = Vec::with_capacity(ids.len());
    for id in &ids {
        let lib = s.library();
        let photo = lib.photo(*id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        let folder = lib.folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder {}", photo.folder)))?;
        if s.is_folder_offline(photo.folder) {
            return Err(CmdError::Disabled(OFFLINE.into()));
        }
        items.push(Item { path: folder.path.join(&photo.name), what: What::Photo(*id), way: Way::Send });
    }
    ask(s, confirm, photos_question(items.len()))?;
    let total = items.len();
    let job = s.start_file_op(FileOp::new(Delete::default(), FileOpKind::Trash, "Delete", items));
    Ok(json!({ "job": job, "pending": true, "total": total }))
}

fn run_folder(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let FolderParams { folder, confirm } = params(p)?;
    let path = s.library().folder(folder).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {folder}")))?;
    if s.is_folder_offline(folder) {
        return Err(CmdError::Disabled(OFFLINE.into()));
    }
    if path.parent().is_none() || path.file_name().is_none() || s.is_drive_or_home(&path) {
        return Err(CmdError::Disabled(NOT_A_DRIVE.into()));
    }
    // Every library folder at and under it goes with it (they are inside it on disk).
    let ids: Vec<FolderId> = s.library().folders().filter(|f| f.path.starts_with(&path)).map(|f| f.id).collect();
    ask(s, confirm, folder_question(&display_name(&path)))?;
    let item = Item { path, what: What::Folder(ids), way: Way::Send };
    let driver = Delete { flushed: Flushed::ask(s) };
    let job = s.start_file_op(FileOp::new(driver, FileOpKind::TrashFolder, "Delete Folder", vec![item]));
    Ok(json!({ "job": job, "pending": true, "total": 1 }))
}

/// The last component of `path` as text (for messages).
pub(crate) fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::testing;

    #[test]
    fn questions_name_the_count_and_the_folder() {
        assert_eq!(photos_question(1), "Move the selected picture to the trash?");
        assert_eq!(photos_question(3), "Move the 3 selected pictures to the trash?");
        assert_eq!(folder_question("Lisbon"), "Move the folder \"Lisbon\" and everything in it (including non-picture files) to the trash?");
        assert!(no_trash(Path::new("/p/IMG_0001.jpg")).contains("file manager"));
    }

    #[test]
    fn hostile_parameters_change_nothing() {
        let (_dirs, mut s, _ids) = testing::library(2);
        testing::hostile(&mut s, "photo.delete");
        testing::hostile(&mut s, "folder.delete");
    }
}
