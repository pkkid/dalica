//! The operating system's ways with files (`organizing.md` §9, `viewing.md` §3.3, `security.md`
//! §5.11; the service is [`crate::os::Os`]): Locate on Disk, Open With, Open in External Editor,
//! Copy Path and the system clipboard (M15.17); Dalica as the default viewer (M15.24).
//!
//! - **`photo.locate {id}`, `folder.locate {id}`**: the photo's file (a video's too; an album's
//!   photo is its file in its folder) or the folder, selected in the system's file manager.
//! - **`photo.openWithList {id}`**: the applications that open the photo's type, the system's
//!   default first, Dalica itself left out ([`is_dalica`]), each with its icon as the system gives
//!   it (`icon`, left out where it gives none). **`photo.openWith {id, handler}`** opens it with one of them.
//! - **`photo.openExternal {ids}`**: the photos in the external editor, `files.externalEditor`, or
//!   the system's edit verb or default application when it is empty. More than [`ASK_ABOVE`] asks
//!   first: without `"confirm": true` it refuses (`disabled`) with [`editor_question`].
//! - **`photo.copyToSystem {ids, picture?}`**: the files on the system clipboard; with `picture`
//!   and one photo, its picture with its edits at its edited size (at most [`MAX_PICTURE`] pixels
//!   on the long side), rendered on a worker as a short job ([`PICTURE_LABEL`]; a video is copied
//!   as its file). **`photo.copyPath {ids}`**: their paths as text, one per line.
//! - **`viewer.fileTypes`** (`viewing.md` §3.3): each type Dalica can be the default viewer for
//!   ([`crate::os::viewer_file_types`]) with the application that opens it now, as the system says
//!   (`null` when it does not), whether that is Dalica, and the types last chosen
//!   (`viewer.defaultViewer`). **`viewer.setDefault {types}`** makes Dalica the default viewer for
//!   `types` and gives every other type it held back to the application it replaced (none: "Don't
//!   use Dalica as a viewer"), then remembers `types` in `viewer.defaultViewer`. Where only the
//!   user may choose (Windows, macOS) the reply says what to choose (`set: false`, `message`). It
//!   changes the system's settings, so it is `Effect::External`: automation runs it only with
//!   `--automation-allow-external`.
//!
//! **Who may run them.** The launching commands (Locate on Disk, Open With, Open in External
//! Editor) reach outside Dalica (`Effect::External`) and only the person at the window starts
//! them: they are in `automation::NEVER_AUTOMATED`, so every automation surface refuses them
//! (`external_denied`) whatever it was granted, `--automation-allow-external` included
//! (`security.md` §5.11, §6.6). The copy commands and the list are not launches; they ask the
//! service whether it can ([`Os::available`]), so a headless session (no window, no clipboard)
//! answers them `disabled` with the reason ([`crate::os::NO_CLIPBOARD`]), as does an OS that
//! cannot do one of the others.
//!
//! **Off the owner thread.** Everything the owner thread does is look the ids up in the library
//! (an unknown id is `not_found`, an offline folder [`OFFLINE`], both before anything else). Then
//! a helper thread looks at the file (a missing one answers [`missing`] and the system is never
//! asked) and makes the system call, and the command waits for it at most [`OS_WAIT`] (past it,
//! [`SLOW`]); the picture is a file job instead, whose failure is its item's reason. The system's
//! own refusal is `disabled` with its reason.

use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use dalica_core::{FolderId, PhotoId};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Done, FileItem, FileJob, Notice};
use crate::jobs::JobKind;
use crate::os::{Clip, DefaultSet, Handler, Os, OsError, Service, type_name, viewer_file_types};
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, params};
use crate::session::Session;

/// The most photos one command takes.
pub const MAX_IDS: usize = 100_000;
/// The most photos one `photo.openExternal` hands to the editor.
pub const MAX_EDITOR_IDS: usize = 1_000;
/// More photos than this ask before they open in the external editor.
pub const ASK_ABOVE: usize = 20;
/// The longest handler id taken, in bytes.
pub const MAX_HANDLER_BYTES: usize = 1_024;
/// The longest side of a picture put on the clipboard, in pixels.
pub const MAX_PICTURE: u32 = 8_192;
/// How long a command waits for the system's answer.
pub const OS_WAIT: Duration = Duration::from_secs(5);
/// Why a photo or folder on an offline drive cannot be shown or opened.
pub const OFFLINE: &str = "The folder is offline: connect its drive first.";
/// The system did not answer within [`OS_WAIT`].
pub const SLOW: &str = "The system did not answer in time; it may still finish.";
/// The picture copy's job label ("Copying the picture 0 of 1").
pub const PICTURE_LABEL: &str = "Copying the picture";
/// `picture` with more than one photo.
pub const ONE_PICTURE: &str = "`picture` copies one photo's picture: name exactly one photo";

/// The question before more than [`ASK_ABOVE`] photos open in the external editor.
pub fn editor_question(n: usize) -> String {
    format!("Open {n} pictures in the external editor?")
}

/// Why a file the library knows is not there.
pub fn missing(path: &Path) -> String {
    format!("\"{}\" is no longer on disk: it was moved or deleted outside Dalica.", name_of(path))
}

/// Whether `handler` is Dalica itself (its desktop entry, ProgID or bundle id names it), which
/// Open With ▸ leaves out.
pub fn is_dalica(handler: &Handler) -> bool {
    handler.id.to_ascii_lowercase().contains("dalica") || handler.name.trim().eq_ignore_ascii_case("dalica")
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.locate",
            label: "Locate on Disk",
            menu: "File/Locate on Disk",
            keys: &[(Context::Library, "Ctrl+Enter")],
            params: r#"{"id":PhotoId}: shows the photo's file (a video's too) selected in the system's file manager; an offline folder or a missing file answers why and the system is not asked; never run over automation. Replies {path}"#,
            effect: Effect::External,
            history: History::None,
            enabled: |s, _| service(s, Service::Reveal),
            run: run_locate,
        },
        CommandSpec {
            id: "folder.locate",
            label: "Locate on Disk",
            menu: "Folder/Locate on Disk",
            keys: &[],
            params: r#"{"id":FolderId}: shows the folder selected in the system's file manager; an offline or missing folder answers why and the system is not asked; never run over automation. Replies {path}"#,
            effect: Effect::External,
            history: History::None,
            enabled: |s, _| service(s, Service::Reveal),
            run: run_locate_folder,
        },
        CommandSpec {
            id: "photo.openWithList",
            label: "Open With",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: the applications that open the photo's type, the system's default first, Dalica left out: {handlers:[{id, name, icon?}]} (icon: a theme icon's name or a file on Linux, "file,index" on Windows); a missing file answers why"#,
            effect: Effect::Query,
            history: History::None,
            enabled: |s, _| service(s, Service::OpenWith),
            run: run_list,
        },
        CommandSpec {
            id: "photo.openWith",
            label: "Open With",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "handler":string (an id photo.openWithList gave, 1–1024 bytes)}: opens the photo's file with that application; a missing file answers why; never run over automation. Replies {path, handler}"#,
            effect: Effect::External,
            history: History::None,
            enabled: |s, _| service(s, Service::OpenWith),
            run: run_open_with,
        },
        CommandSpec {
            id: "photo.openExternal",
            label: "Open in External Editor",
            menu: "File/Open in External Editor",
            keys: &[(Context::Library, "Ctrl+Shift+O"), (Context::EditView, "Ctrl+Shift+O")],
            params: r#"{"ids":[PhotoId] (1–1000), "confirm"?:bool}: opens the photos' files in the external editor (Options ▸ General ▸ External editor, files.externalEditor; empty: the system's editor); more than 20 ask first: needs "confirm": true; a missing file answers why and nothing opens; never run over automation. Replies {paths, editor}"#,
            effect: Effect::External,
            history: History::None,
            enabled: |s, _| service(s, Service::Editor),
            run: run_editor,
        },
        CommandSpec {
            id: "photo.copyToSystem",
            label: "Copy to the Clipboard",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId] (1–100000), "picture"?:bool}: puts the photos' files on the system clipboard; with "picture": true and one photo, its picture with its edits at its edited size (at most 8192 px on the long side), rendered as a job (a video goes as its file). A missing file answers why. Disabled without the system clipboard (a headless session). Replies {paths} or {job, pending:true, total:1}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: |s, _| service(s, Service::Clipboard),
            run: run_copy,
        },
        CommandSpec {
            id: "viewer.fileTypes",
            label: "File Types",
            menu: "",
            keys: &[],
            params: r#"{}: the file types Dalica can be the default viewer for, each with the application that opens it now: {types:[{type, name, handler:{id, name, icon?}|null, dalica:bool}], chosen:[type]} (chosen: viewer.defaultViewer)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: |s, _| service(s, Service::DefaultViewer),
            run: run_file_types,
        },
        CommandSpec {
            id: "viewer.setDefault",
            label: "Use Dalica as the Default Viewer",
            menu: "",
            keys: &[],
            params: r#"{"types":[string] (ids viewer.fileTypes lists, each once; [] is "Don't use Dalica as a viewer")}: makes Dalica the default viewer for these types and gives the others back to the applications it replaced; where only the user may choose, opens the system's settings. Needs --automation-allow-external over automation. Replies {types, set:bool, message?}"#,
            effect: Effect::External,
            history: History::None,
            enabled: |s, _| service(s, Service::DefaultViewer),
            run: run_set_default,
        },
        CommandSpec {
            id: "photo.copyPath",
            label: "Copy Path",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId] (1–100000)}: puts the photos' paths on the system clipboard as text, one per line, in the order given. Disabled without the system clipboard (a headless session). Replies {text}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: |s, _| service(s, Service::Clipboard),
            run: run_copy_path,
        },
    ]
}

// --- parameters ---------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct One {
    id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Folder {
    id: FolderId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct With {
    id: PhotoId,
    handler: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Editor {
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Copy {
    ids: Vec<PhotoId>,
    #[serde(default)]
    picture: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Paths {
    ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Nothing {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Types {
    types: Vec<String>,
}

// --- the commands -------------------------------------------------------------------------------

fn run_locate(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let One { id } = params(p)?;
    let path = photo_path(s, id)?;
    let os = s.os();
    let shown = path.clone();
    off_owner(move || {
        present(&path)?;
        os.reveal(&path).map_err(refused)
    })?;
    Ok(json!({ "path": shown }))
}

fn run_locate_folder(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Folder { id } = params(p)?;
    let path = s.library().folder(id).map(|f| f.path.clone()).ok_or_else(|| CmdError::NotFound(format!("no folder {id}")))?;
    if s.is_folder_offline(id) {
        return Err(CmdError::Disabled(OFFLINE.into()));
    }
    let os = s.os();
    let shown = path.clone();
    off_owner(move || {
        present(&path)?;
        os.reveal(&path).map_err(refused)
    })?;
    Ok(json!({ "path": shown }))
}

fn run_list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let One { id } = params(p)?;
    let path = photo_path(s, id)?;
    let os = s.os();
    let handlers = off_owner(move || {
        present(&path)?;
        os.handlers(&path).map_err(refused)
    })?;
    let handlers: Vec<Value> = handlers.iter().filter(|h| !is_dalica(h)).map(handler_json).collect();
    Ok(json!({ "handlers": handlers }))
}

/// A handler as the replies give it: `{id, name, icon?}` (no `icon` where the system gives none).
fn handler_json(h: &Handler) -> Value {
    let mut v = json!({ "id": h.id, "name": h.name });
    if let (Some(icon), Some(o)) = (&h.icon, v.as_object_mut()) {
        o.insert("icon".into(), json!(icon));
    }
    v
}

fn run_file_types(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Nothing {} = params(p)?;
    let types = viewer_file_types();
    let os = s.os();
    let asked = types.clone();
    let defaults = off_owner(move || asked.iter().map(|t| os.default_handler(*t).map_err(refused)).collect::<Result<Vec<_>, _>>())?;
    let rows: Vec<Value> = types
        .iter()
        .zip(&defaults)
        .map(|(t, h)| {
            json!({
                "type": t.id(),
                "name": type_name(*t),
                "handler": h.as_ref().map(handler_json),
                "dalica": h.as_ref().is_some_and(is_dalica),
            })
        })
        .collect();
    let chosen: Vec<&str> = s.settings().viewer().default_viewer.iter().filter(|t| types.contains(t)).map(|t| t.id()).collect();
    Ok(json!({ "types": rows, "chosen": chosen }))
}

fn run_set_default(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Types { types: names } = params(p)?;
    let offered = viewer_file_types();
    if names.len() > offered.len() {
        return Err(CmdError::BadParams(format!("`types` names at most {} file types", offered.len())));
    }
    let mut types = Vec::with_capacity(names.len());
    for name in &names {
        let ty = offered.iter().copied().find(|t| t.id() == name).ok_or_else(|| {
            let ids: Vec<String> = offered.iter().map(|t| format!("\"{}\"", t.id())).collect();
            CmdError::BadParams(format!("`types` takes {}, not {:?}", ids.join(", "), name.chars().take(32).collect::<String>()))
        })?;
        if !types.contains(&ty) {
            types.push(ty);
        }
    }
    let types = crate::settings::viewer_types(&types);
    let os = s.os();
    let asked = types.clone();
    let outcome = off_owner(move || os.set_default_viewer(&asked).map_err(refused))?;
    s.change_settings(|st| {
        let mut v = st.viewer();
        v.default_viewer = types.clone();
        st.set_viewer(&v);
    });
    let ids: Vec<&str> = types.iter().map(|t| t.id()).collect();
    Ok(match outcome {
        DefaultSet::Set => json!({ "types": ids, "set": true }),
        DefaultSet::SettingsOpened(message) | DefaultSet::Choose(message) => json!({ "types": ids, "set": false, "message": message }),
    })
}

fn run_open_with(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let With { id, handler } = params(p)?;
    if handler.trim().is_empty() || handler.len() > MAX_HANDLER_BYTES || handler.chars().any(char::is_control) {
        return Err(CmdError::BadParams(format!("`handler` must be an application's id of 1–{MAX_HANDLER_BYTES} bytes")));
    }
    let path = photo_path(s, id)?;
    let os = s.os();
    let (shown, chosen) = (path.clone(), handler.clone());
    off_owner(move || {
        present(&path)?;
        os.open_with(&path, &handler).map_err(refused)
    })?;
    Ok(json!({ "path": shown, "handler": chosen }))
}

fn run_editor(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Editor { ids, confirm } = params(p)?;
    let paths = photo_paths(s, ids, MAX_EDITOR_IDS)?;
    if paths.len() > ASK_ABOVE && !confirm {
        let question = editor_question(paths.len());
        return Err(CmdError::Disabled(format!("Open in External Editor: {question} Send \"confirm\": true to go ahead.")));
    }
    let editor = s.settings().external_editor();
    let os = s.os();
    let (shown, named) = (paths.clone(), editor.clone());
    off_owner(move || {
        paths.iter().try_for_each(|p| present(p))?;
        os.open_in_editor(&paths, &editor).map_err(refused)
    })?;
    Ok(json!({ "paths": shown, "editor": named }))
}

fn run_copy(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Copy { ids, picture } = params(p)?;
    if picture && ids.len() > 1 {
        return Err(CmdError::BadParams(ONE_PICTURE.into()));
    }
    let first = ids.first().copied();
    let paths = photo_paths(s, ids, MAX_IDS)?;
    let os = s.os();
    if let (true, Some(id)) = (picture, first)
        && !s.library().is_video(id)
        && let Some(path) = paths.first().cloned()
    {
        let item = Picture { path, edits: s.library().info(id).edits.clone(), os };
        let job = FileJob::new(JobKind::Analysis, PICTURE_LABEL, vec![item], copy_picture).on_finish(|_, t| Notice {
            message: match t.failures.first() {
                Some(f) => format!("The picture was not copied: {}", f.reason),
                None if t.done == 0 => "The picture was not copied.".into(),
                None => "Copied the picture.".into(),
            },
            folder: None,
        });
        let job = s.start_file_job(job);
        return Ok(json!({ "job": job, "pending": true, "total": 1 }));
    }
    let shown = paths.clone();
    off_owner(move || {
        paths.iter().try_for_each(|p| present(p))?;
        os.put_on_clipboard(&Clip::Files(paths)).map_err(refused)
    })?;
    Ok(json!({ "paths": shown }))
}

fn run_copy_path(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Paths { ids } = params(p)?;
    let paths = photo_paths(s, ids, MAX_IDS)?;
    let text = paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n");
    let os = s.os();
    let clip = Clip::Text(text.clone());
    // The paths are the library's: copied as they are, whether or not the files are still there.
    off_owner(move || os.put_on_clipboard(&clip).map_err(refused))?;
    Ok(json!({ "text": text }))
}

// --- the picture --------------------------------------------------------------------------------

/// The one photo whose picture goes on the clipboard.
struct Picture {
    path: PathBuf,
    edits: Vec<String>,
    os: Arc<dyn Os>,
}

impl FileItem for Picture {
    fn file(&self) -> &Path {
        &self.path
    }
}

/// On the job's worker: the file looked at, the picture rendered with its edits at its edited size
/// (the edit view's renderer, as Create Collage draws each picture), then put on the clipboard.
fn copy_picture(item: Picture) -> Result<Done<()>, String> {
    present(&item.path).map_err(|e| e.to_string())?;
    let recipe = dalica_edit::Recipe::parse(&item.edits).unwrap_or_default();
    let key = dalica_edit::version::key_with(&recipe, dalica_edit::ops::version);
    let edits = dalica_thumbs::ladder::Edits { recipe, key };
    let file = dalica_thumbs::ladder::Source { path: item.path.clone(), file_size: 0, mtime_ms: 0 };
    let render = dalica_thumbs::ladder::Render::default();
    let image =
        dalica_thumbs::ladder::render_edited(&file, &render, &edits, MAX_PICTURE, &dalica_image::Limits::default()).map_err(|e| e.to_string())?;
    let clip = Clip::Picture { width: image.width, height: image.height, rgba: image.data };
    item.os.put_on_clipboard(&clip).map_err(|e| e.to_string())?;
    Ok(Done { changes: Vec::new(), value: () })
}

// --- helpers ------------------------------------------------------------------------------------

/// `enabled`: whether the system offers `service` here.
fn service(s: &Session, service: Service) -> Result<(), Disabled> {
    s.os().available(service).map_err(|e| Disabled(e.to_string()))
}

/// The system's refusal or failure, with its reason.
fn refused(e: OsError) -> CmdError {
    CmdError::Disabled(e.to_string())
}

/// Photo `id`'s file, from the library alone: unknown is `not_found`, an offline folder
/// [`OFFLINE`].
fn photo_path(s: &Session, id: PhotoId) -> Result<PathBuf, CmdError> {
    let lib = s.library();
    let photo = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
    let folder = lib.folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder {}", photo.folder)))?;
    if s.is_folder_offline(photo.folder) {
        return Err(CmdError::Disabled(OFFLINE.into()));
    }
    Ok(folder.path.join(&photo.name))
}

/// The files of `ids` (1–`max`, repeats once, in the order given).
fn photo_paths(s: &Session, mut ids: Vec<PhotoId>, max: usize) -> Result<Vec<PathBuf>, CmdError> {
    if ids.is_empty() {
        return Err(CmdError::BadParams("`ids` names no photo".into()));
    }
    if ids.len() > max {
        return Err(CmdError::Limit(format!("at most {max} photos at once")));
    }
    let mut seen = HashSet::new();
    ids.retain(|id| seen.insert(*id));
    ids.into_iter().map(|id| photo_path(s, id)).collect()
}

/// Off the owner thread: whether something is at `path` (a link counts: a watched folder may be
/// one); a missing file is [`missing`].
fn present(path: &Path) -> Result<(), CmdError> {
    match dalica_fs::stat(path) {
        Ok(Some(_)) | Err(dalica_fs::Error::Link(_)) => Ok(()),
        Ok(None) => Err(CmdError::NotFound(missing(path))),
        Err(e) => Err(CmdError::Disabled(format!("\"{}\" cannot be reached: {e}", name_of(path)))),
    }
}

/// Runs `work` on a helper thread (a panic is `internal`) and waits for it at most [`OS_WAIT`]:
/// the owner thread makes no file or system call.
fn off_owner<T: Send + 'static>(work: impl FnOnce() -> Result<T, CmdError> + Send + 'static) -> Result<T, CmdError> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("dalica-os".into())
        .spawn(move || {
            let result = catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err(CmdError::Internal("the system call panicked".into())));
            let _ = tx.send(result);
        })
        .map_err(|e| CmdError::Internal(format!("no thread for the system call: {e}")))?;
    match rx.recv_timeout(OS_WAIT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(CmdError::Disabled(SLOW.into())),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(CmdError::Internal("the system call ended without an answer".into())),
    }
}

/// The file or folder name of `path`, for messages.
fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}
