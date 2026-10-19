//! Save, Undo Save and Revert (`data-model.md` §6, `editing.md` §5.1, `control-protocol.md` §5.5):
//! `photo.save`, `photo.undoSave` and `photo.revert`, each run as a `save` file job
//! ([`crate::filejobs`]) through `create`'s save functions, the only path that changes a photo's
//! pixels.
//!
//! - `photo.save {ids, confirm?, dontAsk?}` (File ▸ Save, Ctrl+S) saves the photos named that have
//!   unsaved edits; the others are left out. While the first-time explanation is on (the `save`
//!   setting's `explain`), it refuses without `"confirm": true` and answers the explanation;
//!   `"dontAsk": true` turns the explanation off. Each photo goes through `create::save::save`: in
//!   place (the original kept in `.dalica-originals/`, the record written, a photo saved before
//!   rendered again from its kept original), losslessly for a rotate-only JPEG, or as a JPEG beside
//!   a format Dalica cannot write in place. Then, on the owner thread: in place, the photo takes
//!   the record and loses the edits it saved (edits made while it saved stay as unsaved edits);
//!   beside, the photo records the copy (`savedAs`) and the copy joins the library as a new photo.
//!   The runner refreshes the saved file's entry (its id kept, so the watcher imports nothing) and
//!   its thumbnails and sketch are made again.
//! - `photo.undoSave {ids}` puts each photo's kept original back and its recorded recipe in front
//!   of any newer edits, as unsaved edits.
//! - `photo.revert {ids, confirm, action?}` asks first (the Revert question, [`REVERT_QUESTION`]):
//!   with `"confirm": true` it puts the kept original back and drops the edits and the record;
//!   with `"action": "undoSave"` too it is Undo Save instead.
//! - A photo the previous organizer saved (M14.14, `data-model.md` §9) has its original in the
//!   folder's legacy originals folder (the scan's legacy-original fact) and no record of Dalica's
//!   own (which, once there, takes precedence): while its bytes differ from that original, Undo
//!   Save and Revert copy the original back through `create::save::legacy` (the saved file goes to
//!   the OS trash; nothing in the legacy folder is written, moved or removed). Undo Save puts the
//!   recipe the legacy folder held in front of any newer edits, as unsaved edits; Revert drops the
//!   edits. Either way the fact goes (the photo is its original again).
//!
//! Every check that needs no disk runs on the owner thread before the job starts (unknown photos,
//! offline folders, recipes holding an operation Dalica cannot render, which fail with the
//! operation's name, files of a format this build cannot decode (AVIF; HEIC without the `heif`
//! feature), by extension); the disk is touched only on the job's worker. One worker: two Saves in one
//! folder would race for its originals record. Cancel stops between photos. None of the three is a
//! library undo step: a Save's way back is Undo Save.
//!
//! Each photo is read only once its own metadata write-back is settled (M9.28): the worker has the
//! photo's queued write made first, on the writer thread ([`crate::metadata::WriteFlusher`]), and
//! expects the version Dalica's own writes left the file at, so a caption set just before Save is
//! written and the Save is not refused as changed on disk; a change made elsewhere still is.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use dalica_core::names::split_ext;
use dalica_core::{DateTime, PhotoId, Timestamp};
use dalica_create::save::legacy::{self as legacy_save, Kept};
use dalica_create::save::{self, Saving, Seen};
use dalica_edit::Recipe;
use dalica_image::Format;
use dalica_library::{Library, Op, SaveState, Saved, SavedAs};
use dalica_meta::{Changes, DateTaken, WriteOptions};
use dalica_sidecar::originals::{Entry, State};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Done, FileChange, FileItem, FileJob, Notice, Tally};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::metadata::{SETTLE_PATIENCE, WriteFlusher};
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, params};
use crate::session::Session;
use crate::settings::SaveSettings;

/// The most photos one command names.
pub const MAX_IDS: usize = 100_000;
/// The first-time explanation `photo.save` answers while it is on (the dialog's text).
pub const SAVE_EXPLANATION: &str = "Saving writes your edits into the photo's file. Dalica first keeps the original, unchanged, in a \
                                    hidden .dalica-originals folder beside it, so Undo Save and Revert can always bring it back.";
/// The Revert question (`data-model.md` §6).
pub const REVERT_QUESTION: &str = "Revert to the original? Your edits will be lost. To undo only the last save and keep the edits, choose Undo Save.";
/// `photo.save` with nothing to save.
pub const NOTHING_TO_SAVE: &str = "There are no unsaved edits to save.";
/// `photo.undoSave` and `photo.revert` with no saved photo.
pub const NOTHING_SAVED: &str = "These pictures have not been saved, so there is no original to bring back.";
/// A photo whose folder is offline.
pub const OFFLINE: &str = "The folder is offline: connect its drive to save or restore its pictures.";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.save",
            label: "Save",
            menu: "File/Save",
            keys: &[(Context::Library, "Ctrl+S"), (Context::EditView, "Ctrl+S")],
            params: r#"{"ids":[PhotoId], "confirm"?:bool, "dontAsk"?:bool}: writes each photo's unsaved edits into its file as a save job (no undo step; its way back is photo.undoSave): the original is kept in the folder's .dalica-originals/ the first time, a photo saved before is rendered again from it, a format Dalica cannot write in place gets a JPEG beside it (a new photo). Photos without unsaved edits are left out. While the first-time explanation is on it needs "confirm": true; "dontAsk": true turns the explanation off. Replies {job, pending:true, total}; the job's result is {total, done, photos:[PhotoId], failed:[{file, reason}], cancelled}"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_save,
            run: run_save,
        },
        CommandSpec {
            id: "photo.undoSave",
            label: "Undo Save",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]}: puts each saved photo's kept original back and its saved edits in front of any newer ones, as unsaved edits, as a save job (photos never saved are left out). A photo the previous organizer saved, with no record of Dalica's, has its legacy original copied back (the saved file goes to the trash) and the recipe the legacy originals folder held put back. Replies and results as photo.save"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_restore,
            run: run_undo_save,
        },
        CommandSpec {
            id: "photo.revert",
            label: "Revert…",
            menu: "File/Revert…",
            keys: &[],
            params: r#"{"ids":[PhotoId], "confirm":bool, "action"?:"revert"|"undoSave"}: asks first (needs "confirm": true): puts each saved photo's kept original back and drops its edits and its record, as a save job; "action": "undoSave" runs photo.undoSave instead. A photo the previous organizer saved, with no record of Dalica's, has its legacy original copied back (the saved file goes to the trash). Photos never saved are left out. Replies and results as photo.save"#,
            effect: Effect::Files,
            history: History::None,
            enabled: can_restore,
            run: run_revert,
        },
    ]
}

/// What a save job does to each photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Save,
    UndoSave,
    Revert,
}

impl Action {
    /// The job's label: the progress reads "Saving 1 of 3".
    fn label(self) -> &'static str {
        match self {
            Action::Save => "Saving",
            Action::UndoSave => "Undoing Save",
            Action::Revert => "Reverting",
        }
    }
}

/// Called on the job's worker with each item's place before it starts (tests hold items back
/// with it).
pub type BeforeItem = Arc<dyn Fn(usize) + Send + Sync>;

// --- which photos -------------------------------------------------------------------------------

/// The ids `p` names, when it names them as a list of ids; anything else is left to `run`.
fn named(p: &Value) -> Option<Vec<PhotoId>> {
    let list = p.get("ids")?.as_array()?;
    list.iter().map(|v| v.as_u64().map(PhotoId::new)).collect()
}

/// Why photo `id` cannot be saved with its edits now, if it cannot (without reading the disk).
fn save_refusal(s: &Session, id: PhotoId) -> Option<String> {
    let lib = s.library();
    let photo = lib.photo(id)?;
    if s.is_folder_offline(photo.folder) {
        return Some(OFFLINE.into());
    }
    if let Some(reason) = unreadable(&photo.name) {
        return Some(reason);
    }
    if let Some(op) = lib.unrenderable(id, save::renders) {
        let shown: String = op.chars().take(64).collect();
        return Some(format!("the edit {shown:?} cannot be rendered by this version of Dalica, so the photo is not saved"));
    }
    if lib.saved(id).is_some_and(|r| r.state == SaveState::Pending) {
        return Some("an earlier Save of this photo did not finish and is repaired first".into());
    }
    None
}

/// Why a photo of this file name can never be saved by this build, judged by its extension
/// without reading the disk (`formats.md` §1): a format the build cannot decode, AVIF always and
/// HEIC without the `heif` feature. A file whose content is another format than its extension
/// says still fails in the job, with the same reason.
fn unreadable(name: &str) -> Option<String> {
    let format = match split_ext(name).1?.to_ascii_lowercase().as_str() {
        "avif" => Format::Avif,
        "heic" | "heif" => Format::Heif,
        _ => return None,
    };
    (!format.supported()).then(|| format!("{}, so Dalica cannot save its edits", dalica_image::Error::Unsupported(format).reason()))
}

/// Enabled for photos with unsaved edits, at least one of which can be saved now; otherwise the
/// reason (the first photo's when every one would fail).
fn can_save(s: &Session, p: &Value) -> Result<(), Disabled> {
    let Some(ids) = named(p) else { return Ok(()) };
    let lib = s.library();
    if ids.iter().any(|id| lib.photo(*id).is_none()) {
        return Ok(());
    }
    let edited: Vec<PhotoId> = ids.into_iter().filter(|id| lib.has_unsaved_edits(*id)).collect();
    if edited.is_empty() {
        return Err(Disabled(NOTHING_TO_SAVE.into()));
    }
    match edited.iter().map(|id| save_refusal(s, *id)).collect::<Option<Vec<String>>>() {
        Some(reasons) => Err(Disabled(capitalised(reasons.first().map(String::as_str).unwrap_or(NOTHING_TO_SAVE)))),
        None => Ok(()),
    }
}

/// Whether Undo Save and Revert apply to photo `id`: a finished in-place save of Dalica's, or a
/// legacy original offered while there is none.
fn restorable(lib: &Library, id: PhotoId) -> bool {
    lib.can_undo_save(id) || lib.can_undo_legacy_save(id)
}

/// The legacy original Undo Save and Revert copy back for photo `id`, with the recipe it gives:
/// only when Dalica has no record of its own for it.
fn legacy_of(lib: &Library, id: PhotoId) -> Option<(Kept, Vec<String>)> {
    if !lib.can_undo_legacy_save(id) {
        return None;
    }
    lib.legacy_original(id).map(|o| (Kept { size: o.size, digest: o.digest.clone() }, o.recipe.clone()))
}

/// Enabled for photos with a finished in-place save or a legacy original, at least one of them
/// online.
fn can_restore(s: &Session, p: &Value) -> Result<(), Disabled> {
    let Some(ids) = named(p) else { return Ok(()) };
    let lib = s.library();
    if ids.iter().any(|id| lib.photo(*id).is_none()) {
        return Ok(());
    }
    let saved: Vec<PhotoId> = ids.into_iter().filter(|id| restorable(lib, *id)).collect();
    if saved.is_empty() {
        return Err(Disabled(NOTHING_SAVED.into()));
    }
    if saved.iter().all(|id| lib.photo(*id).is_some_and(|p| s.is_folder_offline(p.folder))) {
        return Err(Disabled(OFFLINE.into()));
    }
    Ok(())
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `ids` checked: at most [`MAX_IDS`], each known; repeats dropped, order kept.
fn known(s: &Session, ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    if ids.len() > MAX_IDS {
        return Err(CmdError::Limit(format!("at most {MAX_IDS} photos at once")));
    }
    let mut seen = HashSet::with_capacity(ids.len());
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        if s.library().photo(id).is_none() {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        }
        if seen.insert(id) {
            out.push(id);
        }
    }
    Ok(out)
}

// --- the commands -------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct SaveParams {
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
    #[serde(default)]
    dont_ask: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevertParams {
    ids: Vec<PhotoId>,
    #[serde(default)]
    confirm: bool,
    action: Option<String>,
}

fn run_save(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let SaveParams { ids, confirm, dont_ask } = params(p)?;
    let edited: Vec<PhotoId> = known(s, ids)?.into_iter().filter(|id| s.library().has_unsaved_edits(*id)).collect();
    if edited.is_empty() {
        return Err(CmdError::Disabled(NOTHING_TO_SAVE.into()));
    }
    let settings = s.settings().save();
    if settings.explain && !dont_ask && !confirm {
        return Err(CmdError::Disabled(format!("{SAVE_EXPLANATION} Send \"confirm\": true to save (and \"dontAsk\": true to stop asking).")));
    }
    if dont_ask && settings.explain {
        s.change_settings(|st| st.set_save(&SaveSettings { explain: false, ..settings }));
    }
    start(s, Action::Save, &edited, None)
}

fn run_undo_save(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let ids = known(s, ids)?;
    restore(s, Action::UndoSave, ids)
}

fn run_revert(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let RevertParams { ids, confirm, action } = params(p)?;
    let action = match action.as_deref() {
        None | Some("revert") => Action::Revert,
        Some("undoSave") => Action::UndoSave,
        Some(other) => {
            let shown: String = other.chars().take(32).collect();
            return Err(CmdError::BadParams(format!("action: \"revert\" or \"undoSave\", not {shown:?}")));
        }
    };
    let ids = known(s, ids)?;
    if !confirm {
        return Err(CmdError::Disabled(format!(
            "{REVERT_QUESTION} Send \"confirm\": true to revert, with \"action\": \"undoSave\" to undo the save and keep the edits."
        )));
    }
    restore(s, action, ids)
}

/// Undo Save or Revert of the saved photos among `ids` (each known).
fn restore(s: &mut Session, action: Action, ids: Vec<PhotoId>) -> Result<Value, CmdError> {
    let saved: Vec<PhotoId> = ids.into_iter().filter(|id| restorable(s.library(), *id)).collect();
    if saved.is_empty() {
        return Err(CmdError::Disabled(NOTHING_SAVED.into()));
    }
    start(s, action, &saved, None)
}

// --- the job ------------------------------------------------------------------------------------

/// One photo of a save job: everything its work needs, read from the model on the owner thread.
struct Item {
    index: usize,
    action: Action,
    id: PhotoId,
    folder: PathBuf,
    name: String,
    path: PathBuf,
    /// The edits to save (Save).
    edits: Vec<String>,
    changes: Changes,
    options: save::Options,
    /// Undo Save and Revert from a legacy original: what it must still be, and its recipe.
    legacy: Option<(Kept, Vec<String>)>,
    /// Why it fails without touching the disk, if it does.
    refused: Option<String>,
    before: Option<BeforeItem>,
    /// Settles the photo's queued metadata write before the photo is read.
    flusher: Option<WriteFlusher>,
}

impl FileItem for Item {
    fn file(&self) -> &Path {
        &self.path
    }
}

/// What one item did, for the owner thread to turn into ops.
#[derive(Debug)]
enum Did {
    /// Saved in place: the record as written and the edits it saved.
    InPlace { id: PhotoId, saved: Saved, edits: Vec<String> },
    /// A JPEG written beside the photo: its name and the recipe it holds.
    Beside { id: PhotoId, name: String, recipe: Vec<String> },
    /// The original put back (Undo Save, or Revert).
    Restored { id: PhotoId, keep_edits: bool },
    /// A legacy original copied back: Undo Save's recipe to put back, `None` for Revert.
    LegacyRestored { id: PhotoId, recipe: Option<Vec<String>> },
}

impl Did {
    fn id(&self) -> PhotoId {
        match self {
            Did::InPlace { id, .. } | Did::Beside { id, .. } | Did::Restored { id, .. } | Did::LegacyRestored { id, .. } => *id,
        }
    }
}

/// The model's record for the entry a Save wrote, as the scan reads one, so a rescan finds the
/// same record and changes nothing.
fn saved_of(entry: Entry) -> Saved {
    Saved {
        recipe: entry.recipe,
        size: entry.size,
        mtime: Timestamp::from_millis(entry.mtime),
        width: entry.width,
        height: entry.height,
        digest: entry.digest,
        state: match entry.state {
            State::Pending => SaveState::Pending,
            State::Done => SaveState::Done,
        },
        time: entry.time.map(Timestamp::from_millis),
    }
}

/// One item, on the job's worker.
fn work(mut item: Item) -> Result<Done<Did>, String> {
    if let Some(before) = &item.before {
        before(item.index);
    }
    if let Some(reason) = item.refused {
        return Err(reason);
    }
    // The photo's own write-back lands first; the file is then expected at the version it left.
    if let (Some(flusher), Some(seen)) = (&item.flusher, item.options.seen)
        && let Some((size, mtime)) = flusher.settle(item.id, item.path.clone(), (seen.size, seen.mtime), SETTLE_PATIENCE)
    {
        item.options.seen = Some(Seen { size, mtime });
    }
    let replaced = vec![FileChange::Replaced { photo: item.id, path: item.path.clone() }];
    match item.action {
        Action::Save => {
            let recipe = Recipe::parse(&item.edits).map_err(|e| format!("its edits cannot be read: {e}"))?;
            match save::save(&item.folder, &item.name, &recipe, &item.changes, &item.options) {
                Ok(Saving::InPlace(done)) => {
                    Ok(Done { changes: replaced, value: Did::InPlace { id: item.id, saved: saved_of(done.entry), edits: item.edits } })
                }
                Ok(Saving::Beside(b)) => Ok(Done {
                    changes: vec![FileChange::Created { path: item.folder.join(&b.name) }],
                    value: Did::Beside { id: item.id, name: b.name, recipe: b.recipe },
                }),
                Err(e) => Err(e.reason()),
            }
        }
        Action::UndoSave | Action::Revert if item.legacy.is_some() => {
            let Some((kept, recipe)) = item.legacy else { return Err(NOTHING_SAVED.into()) };
            match legacy_save::restore(&item.folder, &item.name, &kept, &item.options) {
                Ok(_) => Ok(Done {
                    changes: replaced,
                    value: Did::LegacyRestored { id: item.id, recipe: (item.action == Action::UndoSave).then_some(recipe) },
                }),
                Err(e) => Err(e.reason()),
            }
        }
        Action::UndoSave | Action::Revert => {
            let keep_edits = item.action == Action::UndoSave;
            let put_back = if keep_edits { save::undo } else { save::revert };
            match put_back(&item.folder, &item.name, &item.options) {
                Ok(_) => Ok(Done { changes: replaced, value: Did::Restored { id: item.id, keep_edits } }),
                Err(e) => Err(e.reason()),
            }
        }
    }
}

/// The ops that bring what an item did into the model (on the owner thread, once the runner has
/// refreshed the files it changed; `photos` are the photos those files are now).
fn ops_for(s: &Session, did: Did, photos: &[(PathBuf, PhotoId)]) -> Result<Vec<Op>, String> {
    let lib = s.library();
    let op = match did {
        Did::InPlace { id, saved, edits } => {
            let info = lib.info(id);
            // Edits made while the photo was saved stay, as unsaved edits on the saved file.
            match info.edits.strip_prefix(edits.as_slice()) {
                Some(rest) if !rest.is_empty() => {
                    let recipe = dalica_library::Recipe { edits: rest.to_vec(), redo: info.redo.clone() };
                    Op::Batch { ops: vec![Op::SetSaved { saved: vec![(id, Some(saved))] }, Op::SetRecipes { recipes: vec![(id, recipe)] }] }
                }
                Some(_) => lib.save_op(vec![(id, saved)]).map_err(|e| e.to_string())?,
                None => {
                    log::warn!("photo {id}: its edits changed while it was saved; the saved file holds the edits it was saved with");
                    lib.save_op(vec![(id, saved)]).map_err(|e| e.to_string())?
                }
            }
        }
        Did::Beside { id, name, recipe } => {
            let mut ops = vec![Op::SetSavedAs { saved_as: vec![(id, Some(SavedAs { name, recipe }))] }];
            // The copy, added from its file's header, takes the source's owned fields.
            if let Some(&(_, copy)) = photos.first() {
                ops.extend(owned_ops(s, id, copy));
            }
            Op::Batch { ops }
        }
        Did::Restored { id, keep_edits: true } => lib.undo_save_op(&[id]).map_err(|e| e.to_string())?,
        Did::Restored { id, keep_edits: false } => lib.revert_op(&[id]).map_err(|e| e.to_string())?,
        Did::LegacyRestored { id, recipe } => {
            // The legacy recipe in front of any edits made since (the redo list stays), as M8's
            // Undo Save; Revert drops them all.
            let info = lib.info(id);
            let recipe = match recipe {
                Some(mut edits) => {
                    edits.extend(info.edits.iter().cloned());
                    dalica_library::Recipe { edits, redo: info.redo.clone() }
                }
                None => dalica_library::Recipe::default(),
            };
            Op::Batch { ops: vec![Op::SetLegacyOriginals { originals: vec![(id, None)] }, Op::SetRecipes { recipes: vec![(id, recipe)] }] }
        }
    };
    Ok(vec![op])
}

/// The ops that give photo `copy` the owned fields of photo `source` it lacks: its caption, tags,
/// star and the date the user set.
fn owned_ops(s: &Session, source: PhotoId, copy: PhotoId) -> Vec<Op> {
    let lib = s.library();
    let (from, to) = (lib.info(source), lib.info(copy));
    let mut ops = Vec::new();
    if from.caption != to.caption {
        ops.push(Op::SetCaptions { captions: vec![(copy, from.caption.clone())] });
    }
    if from.tags != to.tags {
        ops.push(Op::SetTags { tags: vec![(copy, from.tags.clone())] });
    }
    let star = lib.photo(source).is_some_and(|p| p.star);
    if lib.photo(copy).is_some_and(|p| p.star != star) {
        ops.push(Op::SetStars { stars: vec![(copy, star)] });
    }
    if from.date.is_some() && from.date != to.date {
        ops.push(Op::SetDates { dates: vec![(copy, from.date)] });
    }
    ops
}

/// The library's owned fields of photo `id`, merged into a saved file under the write-back
/// options as the write-back would (the place stays as the file has it).
fn owned_changes(s: &Session, id: PhotoId, star: bool) -> Changes {
    let info = s.library().info(id);
    Changes {
        caption: Some((!info.caption.is_empty()).then(|| info.caption.clone())),
        tags: Some(info.tags.clone()),
        star: Some(star),
        place: None,
        date_taken: info.date.map(DateTaken::local),
        faces: None,
    }
}

/// The write-back options of the settings (Options ▸ General ▸ Files) at `now`, over `base`.
fn write_options(s: &Session, base: WriteOptions, now: Timestamp) -> WriteOptions {
    let w = s.settings().metadata_writes();
    WriteOptions {
        captions_tags_places: w.captions_tags_places,
        stars_as_ratings: w.stars_as_ratings,
        dates: w.dates,
        metadata_date: DateTime::from_unix_secs(now.as_secs()).and_then(|d| DateTaken::new(d, Some(0), None)),
        ..base
    }
}

/// "Saved 3 photos", "Saved 2 of 3 photos; 1 failed", "Save cancelled: 1 of 3 photos saved".
fn message(action: Action, done: usize, total: usize, failed: usize, cancelled: bool) -> String {
    let photos = |n: usize| if n == 1 { "1 photo".to_string() } else { format!("{n} photos") };
    let (verb, noun, past) = match action {
        Action::Save => ("Saved", "Save", "saved"),
        Action::UndoSave => ("Undid the save of", "Undo Save", "restored"),
        Action::Revert => ("Reverted", "Revert", "reverted"),
    };
    let head = if cancelled {
        format!("{noun} cancelled: {done} of {} {past}", photos(total))
    } else if failed > 0 {
        format!("{verb} {done} of {}", photos(total))
    } else {
        format!("{verb} {}", photos(done))
    };
    if failed > 0 { format!("{head}; {failed} failed") } else { head }
}

/// Starts the save job doing `action` to `ids` (each known; the commands have checked that they
/// have edits or a kept original), calling `before` on the worker before each item. Never touches
/// the disk. Replies `{job, pending: true, total}`; the job's result is `{total, done, photos,
/// failed, cancelled}`.
pub fn start(s: &mut Session, action: Action, ids: &[PhotoId], before: Option<BeforeItem>) -> Result<Value, CmdError> {
    let now = s.clock.now();
    let flusher = s.metadata_flusher();
    let mut items = Vec::with_capacity(ids.len());
    for (index, &id) in ids.iter().enumerate() {
        let lib = s.library();
        let Some((photo, folder)) = lib.photo(id).and_then(|p| Some((p.clone(), lib.folder(p.folder)?.path.clone()))) else {
            return Err(CmdError::NotFound(format!("no photo {id}")));
        };
        let base = save::Options::new(now);
        let options =
            save::Options { write: write_options(s, base.write.clone(), now), seen: Some(Seen { size: photo.size, mtime: photo.mtime }), ..base };
        let refused = match action {
            Action::Save => save_refusal(s, id),
            Action::UndoSave | Action::Revert => s.is_folder_offline(photo.folder).then(|| OFFLINE.to_string()),
        };
        items.push(Item {
            index,
            action,
            id,
            path: folder.join(&photo.name),
            folder,
            name: photo.name.clone(),
            edits: if action == Action::Save { lib.info(id).edits.clone() } else { Vec::new() },
            changes: owned_changes(s, id, photo.star),
            options,
            legacy: if action == Action::Save { None } else { legacy_of(lib, id) },
            refused,
            before: before.clone(),
            flusher: flusher.clone(),
        });
    }
    let total = items.len();
    let done_ids: Arc<Mutex<Vec<PhotoId>>> = Arc::new(Mutex::new(Vec::new()));
    let job_id: Arc<OnceLock<JobId>> = Arc::new(OnceLock::new());
    let (applied, finish_job) = (Arc::clone(&done_ids), Arc::clone(&job_id));
    let job = FileJob::new(JobKind::Save, action.label(), items, work)
        .on_applied(move |s, a| {
            let id = a.value.id();
            let ops = ops_for(s, a.value, &a.photos)?;
            applied.lock().unwrap_or_else(PoisonError::into_inner).push(id);
            Ok(ops)
        })
        .on_finish(move |s, tally| finish(s, tally, action, &done_ids, finish_job.get().copied()));
    let id = s.start_file_job(job);
    let _ = job_id.set(id);
    Ok(json!({ "job": id, "pending": true, "total": total }))
}

/// The job's end (on the owner thread): its message, and the job closed with the whole result.
fn finish(s: &mut Session, tally: &Tally, action: Action, done: &Mutex<Vec<PhotoId>>, job: Option<JobId>) -> Notice {
    let text = message(action, tally.done, tally.total, tally.failures.len(), tally.cancelled);
    // The job ends here with the photos it changed; the runner's own end after this is left out
    // (a job keeps its first end). Without an id (no worker thread could start, and the runner
    // ended the job before `start` had it) the runner's failure stands.
    if let Some(job) = job {
        let photos = done.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let result = json!({
            "total": tally.total,
            "done": tally.done,
            "photos": photos,
            "failed": tally.failures.iter().map(|f| json!({ "file": f.file, "reason": f.reason })).collect::<Vec<_>>(),
            "cancelled": tally.cancelled,
        });
        let applied = tally.done.saturating_add(tally.failures.len());
        s.jobs.progress(job, JobProgress { done: applied as u64, total: tally.total as u64 }, text.clone());
        let outcome = if tally.cancelled { Outcome::Cancelled(result) } else { Outcome::Done(result) };
        let now = s.clock.now();
        s.jobs.finish(job, outcome, text.clone(), now);
    }
    Notice { message: text, folder: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::testing::library;

    #[test]
    fn messages_count_what_happened() {
        assert_eq!(message(Action::Save, 3, 3, 0, false), "Saved 3 photos");
        assert_eq!(message(Action::Save, 1, 1, 0, false), "Saved 1 photo");
        assert_eq!(message(Action::Save, 2, 3, 1, false), "Saved 2 of 3 photos; 1 failed");
        assert_eq!(message(Action::Save, 1, 3, 0, true), "Save cancelled: 1 of 3 photos saved");
        assert_eq!(message(Action::UndoSave, 2, 2, 0, false), "Undid the save of 2 photos");
        assert_eq!(message(Action::Revert, 0, 1, 1, false), "Reverted 0 of 1 photo; 1 failed");
        assert_eq!(message(Action::Revert, 1, 2, 0, true), "Revert cancelled: 1 of 2 photos reverted");
    }

    fn record(recipe: &[&str]) -> Saved {
        Saved { recipe: recipe.iter().map(|s| s.to_string()).collect(), size: 10, state: SaveState::Done, ..Saved::default() }
    }

    fn apply_all(s: &mut Session, ops: Vec<Op>) {
        for op in ops {
            s.apply(op).unwrap();
        }
    }

    /// Edits made while a photo was being saved stay as unsaved edits on the saved file; edits
    /// changed underneath are all taken as saved.
    #[test]
    fn edits_made_while_saving_stay_unsaved() {
        let (_dirs, mut s, ids) = library(2);
        for op in ["autofix()", "sepia()"] {
            s.execute("photo.history.apply", &json!({ "id": ids[0], "op": op })).unwrap();
        }
        let did = Did::InPlace { id: ids[0], saved: record(&["autofix()"]), edits: vec!["autofix()".into()] };
        let ops = ops_for(&s, did, &[]).unwrap();
        apply_all(&mut s, ops);
        assert_eq!(s.library().info(ids[0]).edits, ["sepia()"]);
        assert!(s.library().can_undo_save(ids[0]) && s.library().has_unsaved_edits(ids[0]));

        s.execute("photo.history.apply", &json!({ "id": ids[1], "op": "autocolor()" })).unwrap();
        let did = Did::InPlace { id: ids[1], saved: record(&["autofix()"]), edits: vec!["autofix()".into()] };
        let ops = ops_for(&s, did, &[]).unwrap();
        apply_all(&mut s, ops);
        assert!(s.library().info(ids[1]).edits.is_empty());
        assert!(s.undo_stack().is_empty(), "no library step");
    }

    /// A Save beside the photo records the copy with the recipe it holds: the photo no longer
    /// counts as unsaved until its recipe changes; the copy (added from its file's header by the
    /// runner) takes the photo's caption, tags, star and date.
    #[test]
    fn a_save_beside_records_the_copy_and_gives_it_the_owned_fields() {
        let (_dirs, mut s, ids) = library(1);
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "autofix()" })).unwrap();
        let date: DateTime = "2025-05-01T10:00:00".parse().unwrap();
        let tags = vec!["bridge".to_string(), "river".to_string()];
        apply_all(
            &mut s,
            vec![
                Op::SetCaptions { captions: vec![(ids[0], "Ponte".into())] },
                Op::SetTags { tags: vec![(ids[0], tags.clone())] },
                Op::SetStars { stars: vec![(ids[0], true)] },
                Op::SetDates { dates: vec![(ids[0], Some(date))] },
            ],
        );
        let source = s.library().photo(ids[0]).unwrap().clone();
        let copy = s.library.next_photo_id();
        let name = "IMG_0001-001.jpg".to_string();
        let added = dalica_library::Photo { id: copy, name: name.clone(), star: false, date: None, ..source };
        s.apply(Op::AddPhotos { photos: vec![added] }).unwrap();
        let path = s.library().folder(source.folder).unwrap().path.join(&name);
        let did = Did::Beside { id: ids[0], name: name.clone(), recipe: vec!["autofix()".into()] };
        let ops = ops_for(&s, did, &[(path, copy)]).unwrap();
        apply_all(&mut s, ops);
        let info = s.library().info(copy);
        assert_eq!((info.caption.as_str(), &info.tags, info.date), ("Ponte", &tags, Some(date)));
        assert!(s.library().photo(copy).unwrap().star);
        assert!(info.edits.is_empty(), "the copy is an unedited photo");
        assert_eq!(s.library().saved_as(ids[0]).map(|c| c.name.as_str()), Some("IMG_0001-001.jpg"));
        assert!(!s.library().has_unsaved_edits(ids[0]));
        assert_eq!(s.library().info(ids[0]).edits, ["autofix()"], "the source keeps its recipe");
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "sepia()" })).unwrap();
        assert!(s.library().has_unsaved_edits(ids[0]));
        // Undo Save of a photo saved only beside it: nothing to bring back.
        assert_eq!(s.execute("photo.undoSave", &json!({ "ids": [ids[0]] })), Err(CmdError::Disabled(NOTHING_SAVED.into())));
    }

    #[test]
    fn formats_this_build_cannot_decode_are_refused_by_extension() {
        assert_eq!(unreadable("still.AVIF").as_deref(), Some("AVIF files can't be displayed, so Dalica cannot save its edits"));
        assert_eq!(unreadable("iphone.heic").is_some(), !Format::Heif.supported());
        assert_eq!(unreadable("iphone.HEIF").is_some(), !Format::Heif.supported());
        for name in ["a.jpg", "b.png", "c.tif", "d.webp", "noext", "avif"] {
            assert_eq!(unreadable(name), None, "{name}");
        }
    }

    #[test]
    fn reasons_read_as_sentences() {
        assert_eq!(capitalised("the edit"), "The edit");
        assert_eq!(capitalised(""), "");
    }
}
