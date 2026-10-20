//! Save Frame as Picture and Export Clip (`editing.md` §11, `creating.md` §4; `roadmap.md` M15 →
//! *Video*, M15.25); the commands are `cmd::video_output`. Each runs as an `export` file job of
//! one step on a worker ([`crate::filejobs`]): the owner thread gathers what it needs from the
//! library and the player and never touches the disk. The original clip is only read, never
//! written; each new file is made through `fs`, never over another one.
//!
//! - **Save Frame as Picture** (`video.saveFrame`): the frame shown (paused: the one on screen;
//!   playing: the one at the position), decoded again at the clip's full size, upright, then
//!   through the one output pipeline (`create::video::frame_picture`: the video's own rotations
//!   and flips, JPEG at quality 92, an EXIF thumbnail) into `<Pictures>/Dalica/Video Frames`,
//!   named `<stem> <hh-mm-ss>.jpg` (`create::video::frame_name`, made safe by
//!   `core::names::sanitize`; a taken name gets `-001`, …, never replaced). Its date taken is the
//!   clip's date plus the frame's time. The folder is made when missing and marked
//!   `"project": "videoFrames"` (in a new sidecar, else in the model once the library holds it),
//!   so it lists in Projects; the JPEG enters the library as an export's files do. Refused, with
//!   the reason, for a clip Dalica cannot play.
//! - **Export Clip** (`video.exportClip {id?, path?}`): the trimmed part (the whole clip without a
//!   trim) copied without re-encoding (`create::video::cut`: from the key frame at or before the
//!   start), to `path` (an absolute file path whose folder exists; never over a file) or, without
//!   one, beside the clip as `<stem> trimmed.<ext>` (`-001`, … when taken). Works on any clip whose
//!   container Dalica reads, whether or not it plays. The job's result says the real cut
//!   (`startMs`, `endMs`). Over automation `path` is checked against the write roots
//!   (`control-protocol.md` §5.7, [`Session::export_clip_destination`]).

use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use dalica_core::names;
use dalica_core::{DateTime, FolderId, PhotoId};
use dalica_create::video::{self as output, Cut};
use dalica_image::Limits;
use dalica_image::formats::video::frames::{FrameOptions, FrameSource};
use dalica_library::{FolderInfo, Op, Project};
use dalica_meta::DateTaken;
use dalica_store::mapping::FolderState;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::filejobs::{Done, FileChange, FileItem, FileJob, Notice, Tally};
use crate::jobs::{JobId, JobKind, JobProgress, Outcome};
use crate::session::Session;

/// The projects directory's folder Save Frame as Picture writes into.
pub const VIDEO_FRAMES: &str = "Video Frames";
/// The job's label while Save Frame runs.
pub const FRAME_LABEL: &str = "Saving the frame";
/// The job's label while Export Clip runs.
pub const CLIP_LABEL: &str = "Exporting the clip";
/// The word Export Clip adds to a clip's name without a `path`: `IMG_0042 trimmed.mov`.
pub const TRIMMED: &str = "trimmed";
/// The longest `path` taken, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// The folder of kept originals: never a destination.
const ORIGINALS_DIR: &str = ".dalica-originals";
/// Decoding threads Save Frame's full-size decode uses besides its own.
const FRAME_THREADS: usize = 2;

/// Save Frame's and Export Clip's state.
#[derive(Debug, Default)]
pub struct VideoOutput {
    job: Option<JobId>,
}

impl VideoOutput {
    /// The job running or last run, if any.
    pub fn job(&self) -> Option<JobId> {
        self.job
    }
}

/// Unix seconds of `d` read as UTC (Howard Hinnant's `days_from_civil`).
fn unix_secs(d: DateTime) -> i64 {
    let date = d.date();
    let (y, m) = (i64::from(date.year()), i64::from(date.month()));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(date.day()) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(d.hour()) * 3600 + i64::from(d.minute()) * 60 + i64::from(d.second())
}

/// The date of a frame `ms` into a clip dated `clip`.
pub fn frame_date(clip: DateTime, ms: u64) -> Option<DateTime> {
    DateTime::from_unix_secs(unix_secs(clip).checked_add(i64::try_from(ms / 1000).ok()?)?)
}

/// `path` as a new clip's file: absolute, without `.` or `..`, valid Unicode of at most
/// [`MAX_PATH_BYTES`], naming a file, never inside a folder of kept originals.
fn check_file(path: &Path) -> Result<(PathBuf, String), CmdError> {
    let text = path.to_str().ok_or_else(|| CmdError::BadParams("path: not valid Unicode".into()))?;
    if text.len() > MAX_PATH_BYTES {
        return Err(CmdError::Limit(format!("path: at most {MAX_PATH_BYTES} bytes")));
    }
    if !path.is_absolute() || text.contains('\0') {
        return Err(CmdError::BadParams(format!("path: an absolute file path, not {:?}", names::fit(text, 200))));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(CmdError::BadParams("path: `.` and `..` are not allowed".into()));
    }
    if path.components().any(|c| c.as_os_str() == ORIGINALS_DIR) {
        return Err(CmdError::BadParams(format!("path: never inside {ORIGINALS_DIR}")));
    }
    let (Some(folder), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Err(CmdError::BadParams("path: names no file".into()));
    };
    if !dalica_fs::create::plain_name(name) {
        return Err(CmdError::BadParams(format!("path: {:?} is not a name a file can take", names::fit(name, 200))));
    }
    Ok((folder.to_path_buf(), name.to_string()))
}

/// Where a step writes: a free name of its own (`-001`, … when taken), or exactly this name.
#[derive(Clone, Debug)]
enum Place {
    Free { folder: PathBuf, name: String },
    Exactly { folder: PathBuf, name: String },
}

impl Place {
    fn folder(&self) -> &Path {
        match self {
            Place::Free { folder, .. } | Place::Exactly { folder, .. } => folder,
        }
    }

    fn file(&self) -> PathBuf {
        match self {
            Place::Free { folder, name } | Place::Exactly { folder, name } => folder.join(name),
        }
    }

    /// Writes `bytes` there; the path written.
    fn write(&self, bytes: &[u8]) -> Result<PathBuf, String> {
        match self {
            Place::Free { folder, name } => dalica_fs::create::new_file(folder, name, bytes).map(|c| folder.join(c.name)).map_err(|e| e.to_string()),
            Place::Exactly { folder, name } => match dalica_fs::exact_file(folder, name, bytes) {
                Ok(_) => Ok(folder.join(name)),
                Err(dalica_fs::Error::Exists(_) | dalica_fs::Error::Link(_)) => {
                    Err(format!("{} already exists: a new file never replaces one", folder.join(name).display()))
                }
                Err(e) => Err(e.to_string()),
            },
        }
    }
}

/// What a step does.
#[derive(Clone, Debug)]
enum Task {
    Frame { clip: PathBuf, at_ms: u64, edits: Vec<String>, taken: Option<DateTaken> },
    Clip { clip: PathBuf, start_ms: u64, end_ms: u64 },
}

/// The job's one step.
struct Step {
    task: Task,
    place: Place,
    file: PathBuf,
}

impl FileItem for Step {
    fn file(&self) -> &Path {
        &self.file
    }
}

/// What the step wrote.
#[derive(Clone, Debug)]
struct Wrote {
    path: PathBuf,
    /// The picture's size (Save Frame) or the real cut and its container (Export Clip).
    size: Option<(u32, u32)>,
    cut: Option<(u64, u64)>,
}

/// What the job wrote, and the photo it is in the library once applied.
type Written = Option<(Wrote, Option<PhotoId>)>;

/// The step, on the worker.
fn step(item: Step) -> Result<Done<Wrote>, String> {
    let wrote = match &item.task {
        Task::Frame { clip, at_ms, edits, taken } => {
            let limits = Limits::default();
            let mut frames = FrameSource::open(clip, &limits, FrameOptions { max_side: None, threads: FRAME_THREADS }).map_err(|e| e.reason())?;
            let frame = frames.frame_at(*at_ms).map_err(|e| e.reason())?;
            drop(frames);
            let jpeg = output::frame_picture(&frame.image, edits, taken.clone()).map_err(|e| e.reason())?;
            let folder = item.place.folder();
            dalica_fs::create_dir_all(folder).map_err(|e| e.to_string())?;
            mark_folder(folder);
            let path = item.place.write(&jpeg)?;
            let header = dalica_image::read_header(&jpeg, &limits).ok();
            Wrote { path, size: header.map(|h| (h.width, h.height)), cut: None }
        }
        Task::Clip { clip, start_ms, end_ms } => {
            let Cut { bytes, start_ms, end_ms, .. } = output::cut(clip, *start_ms, *end_ms).map_err(|e| e.to_string())?;
            let path = item.place.write(&bytes)?;
            Wrote { path, size: None, cut: Some((start_ms, end_ms)) }
        }
    };
    Ok(Done { changes: vec![FileChange::Created { path: wrote.path.clone() }], value: wrote })
}

/// Marks the Video Frames folder in a new sidecar when it has none.
fn mark_folder(folder: &Path) {
    if !matches!(dalica_fs::stat(&folder.join(dalica_sidecar::FILE_NAME)), Ok(None)) {
        return;
    }
    let state = FolderState {
        id: FolderId::new(0),
        path: folder.to_path_buf(),
        folder: FolderInfo::default(),
        photos: Vec::new(),
        project: Some(Project::VideoFrames),
        ..FolderState::default()
    };
    let now = dalica_core::Timestamp::from_system_time(std::time::SystemTime::now());
    match dalica_store::writer::write_folder(&state, now) {
        dalica_store::writer::Outcome::Written | dalica_store::writer::Outcome::Unchanged => {}
        other => log::warn!("{}: the Video Frames mark was not written: {other:?}", folder.display()),
    }
}

/// `m:ss.t` of `ms`, for the job's message.
fn clock_text(ms: u64) -> String {
    format!("{}:{:02}.{}", ms / 60_000, ms / 1000 % 60, ms / 100 % 10)
}

impl Session {
    /// Save Frame's and Export Clip's state.
    pub fn video_output(&self) -> &VideoOutput {
        &self.video_output
    }

    /// `<Pictures>/Dalica/Video Frames`, when the Pictures folder is known.
    pub fn video_frames_dir(&self) -> Option<PathBuf> {
        self.projects_dir().map(|d| d.join(VIDEO_FRAMES))
    }

    /// The file Export Clip writes for `path` (the param, if given), checked as written, never on
    /// the disk; without one, `None`: a free name beside the clip (automation checks the answer
    /// against its write roots, `control-protocol.md` §5.7).
    pub fn export_clip_destination(&self, path: Option<&Path>) -> Result<Option<PathBuf>, CmdError> {
        path.map(|p| check_file(p).map(|(folder, name)| folder.join(name))).transpose()
    }

    /// `video.saveFrame`: starts Save Frame as Picture on the frame shown. Replies `{job, pending,
    /// folder, name, timeMs}`.
    pub(crate) fn start_save_frame(&mut self) -> Result<Value, CmdError> {
        self.video_frames_allowed()?;
        let (id, clip, _, at_ms) = self.video_target()?;
        let folder = self
            .video_frames_dir()
            .ok_or_else(|| CmdError::Disabled("Dalica does not know your Pictures folder, so it has nowhere to save the frame.".into()))?;
        let lib = self.library();
        let photo = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        let (stem, _) = names::split_ext(&photo.name);
        let name = output::frame_name(stem, at_ms);
        let edits = lib.info(id).edits.clone();
        let taken = lib.effective_date(id).and_then(|d| frame_date(d, at_ms)).map(DateTaken::local);
        let place = Place::Free { folder: folder.clone(), name: name.clone() };
        let task = Task::Frame { clip, at_ms, edits, taken };
        let reply = json!({ "folder": folder, "name": name, "timeMs": at_ms });
        self.start_video_job(FRAME_LABEL, task, place, Some(folder), reply)
    }

    /// `video.exportClip {id?, path?}`: starts Export Clip on video `id` (the open one without it).
    /// Replies `{job, pending, path (null: a free name beside the clip), startMs, endMs}` with the
    /// trim asked for; the job's result has the real cut.
    pub(crate) fn start_export_clip(&mut self, id: Option<PhotoId>, path: Option<&Path>) -> Result<Value, CmdError> {
        let id = match id {
            Some(id) => id,
            None => self.video.open().ok_or_else(|| CmdError::Disabled(crate::video::NOT_OPEN.into()))?,
        };
        let lib = self.library();
        let photo = lib.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?;
        if !crate::video::is_video(lib, id) {
            return Err(CmdError::Disabled(format!("{} is not a video.", photo.name)));
        }
        let folder = lib.folder(photo.folder).ok_or_else(|| CmdError::NotFound(format!("no folder for photo {id}")))?.path.clone();
        let clip = folder.join(&photo.name);
        let length = lib.video_facts(id).map_or(0, |f| f.duration_ms);
        let whole = if length > 0 { length } else { dalica_edit::ops::video::MAX_MS };
        let (start_ms, end_ms) = crate::video::trim_of(&lib.info(id).edits).unwrap_or((0, whole));
        let place = match path {
            Some(p) => {
                let (dest_folder, name) = check_file(p)?;
                if dest_folder.join(&name) == clip {
                    return Err(CmdError::BadParams("path: the clip itself: a new file never replaces one".into()));
                }
                Place::Exactly { folder: dest_folder, name }
            }
            None => {
                let (stem, ext) = names::split_ext(&photo.name);
                let base = names::sanitize(&format!("{stem} {TRIMMED}"), TRIMMED);
                let name = match ext {
                    Some(ext) => format!("{base}.{ext}"),
                    None => base,
                };
                Place::Free { folder, name }
            }
        };
        let reply = json!({ "path": path, "startMs": start_ms, "endMs": end_ms });
        let task = Task::Clip { clip, start_ms, end_ms };
        let scan = Some(place.folder().to_path_buf());
        self.start_video_job(CLIP_LABEL, task, place, scan, reply)
    }

    /// Starts the one-step job of `task`, writing at `place`; `reply` gets `job` and `pending`.
    fn start_video_job(
        &mut self,
        label: &'static str,
        task: Task,
        place: Place,
        folder: Option<PathBuf>,
        mut reply: Value,
    ) -> Result<Value, CmdError> {
        let frame = matches!(task, Task::Frame { .. });
        let file = place.file();
        let written: Arc<Mutex<Written>> = Arc::default();
        let job_id: Arc<OnceLock<JobId>> = Arc::new(OnceLock::new());
        let (applied, finished, finish_job) = (Arc::clone(&written), Arc::clone(&written), Arc::clone(&job_id));
        let job = FileJob::new(JobKind::Export, label, vec![Step { task, place, file }], step)
            .on_applied(move |_, a| {
                let photo = a.photos.iter().find(|(p, _)| *p == a.value.path).map(|(_, id)| *id);
                *applied.lock().unwrap_or_else(PoisonError::into_inner) = Some((a.value, photo));
                Ok(Vec::new())
            })
            .on_finish(move |s, tally| s.video_job_finished(tally, frame, folder.as_deref(), &finished, finish_job.get().copied()));
        let id = self.start_file_job(job);
        let _ = job_id.set(id);
        self.video_output.job = Some(id);
        if let Some(out) = reply.as_object_mut() {
            out.insert("job".into(), json!(id));
            out.insert("pending".into(), json!(true));
        }
        Ok(reply)
    }

    /// The job's end: the Video Frames folder marked if the library holds it by now, the message,
    /// and the job closed with its result.
    fn video_job_finished(&mut self, tally: &Tally, frame: bool, folder: Option<&Path>, written: &Mutex<Written>, job: Option<JobId>) -> Notice {
        if frame
            && let Some(dir) = folder
            && let Some(id) = self.library().folder_by_path(dir)
            && self.library().project(id) != Some(Project::VideoFrames)
            && let Err(e) = self.apply(Op::SetProjects { projects: vec![(id, Some(Project::VideoFrames))] })
        {
            log::warn!("{}: not marked Video Frames: {e}", dir.display());
        }
        let written = written.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let file = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let failure = tally.failures.first().map(|f| f.reason.clone());
        let text = match (&written, frame) {
            (Some((w, _)), true) => format!("Saved the frame as {}", file(&w.path)),
            (Some((w, _)), false) => {
                let (a, b) = w.cut.unwrap_or_default();
                format!("Exported {} ({} to {})", file(&w.path), clock_text(a), clock_text(b))
            }
            (None, true) => failure.as_ref().map_or_else(|| "The frame was not saved".to_string(), |f| format!("The frame was not saved: {f}")),
            (None, false) => failure.as_ref().map_or_else(|| "The clip was not exported".to_string(), |f| format!("The clip was not exported: {f}")),
        };
        if let Some(job) = job {
            let photo = written.as_ref().and_then(|(w, p)| p.or_else(|| self.collage_photo_at(&w.path)));
            let result = match &written {
                Some((w, _)) => json!({
                    "path": w.path,
                    "photo": photo,
                    "width": w.size.map(|s| s.0),
                    "height": w.size.map(|s| s.1),
                    "startMs": w.cut.map(|c| c.0),
                    "endMs": w.cut.map(|c| c.1),
                    "lengthMs": w.cut.map(|c| c.1.saturating_sub(c.0)),
                }),
                None => json!({ "path": null, "cancelled": tally.cancelled }),
            };
            let applied = tally.done.saturating_add(tally.failures.len());
            self.jobs.progress(job, JobProgress { done: applied as u64, total: tally.total as u64 }, text.clone());
            let outcome = match (&written, tally.cancelled, failure) {
                (Some(_), _, _) => Outcome::Done(result),
                (None, true, _) => Outcome::Cancelled(result),
                (None, false, Some(f)) => Outcome::Failed(f),
                (None, false, None) => Outcome::Failed(text.clone()),
            };
            let now = self.clock.now();
            self.jobs.finish(job, outcome, text.clone(), now);
        }
        Notice { message: text, folder: written.as_ref().and_then(|(w, _)| w.path.parent().map(Path::to_path_buf)) }
    }
}

#[cfg(test)]
mod tests {
    use super::{clock_text, frame_date, unix_secs};
    use dalica_core::DateTime;

    #[test]
    fn a_frames_date_is_the_clips_plus_its_time() {
        let d: DateTime = "2026-10-19T23:59:58".parse().unwrap();
        assert_eq!(DateTime::from_unix_secs(unix_secs(d)), Some(d));
        assert_eq!(frame_date(d, 1_500), "2026-10-19T23:59:59".parse().ok());
        assert_eq!(frame_date(d, 3_000), "2026-10-20T00:00:01".parse().ok());
        let leap: DateTime = "2024-02-28T12:00:00".parse().unwrap();
        assert_eq!(frame_date(leap, 86_400_000), "2024-02-29T12:00:00".parse().ok());
        assert_eq!(clock_text(1_520), "0:01.5");
        assert_eq!(clock_text(61_000), "1:01.0");
    }
}
