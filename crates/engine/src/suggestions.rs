//! Suggestions (`faces.md` §5, `organizing.md` §6.3): "Name?" for an unnamed face whose best
//! person scores at or above the Suggestion confidence's threshold and ahead of the runner-up, kept
//! current as faces are embedded and named; the pending count per person; Find More; the rejected
//! pairs kept in `people.json`; Evaluate on your photos. The maths is `dalica-faces`'s
//! (`matching`, `evaluate`); the commands are `cmd/suggest.rs`.
//!
//! - **Made on demand, kept per state.** The suggestions are worked out when asked for (a command,
//!   [`Session::pending_suggestions`]) and kept until what they depend on changes: the library's
//!   revision (a face named, moved, removed, a person merged …), the face index's generation (faces
//!   embedded, another model), the rejected pairs, Suggest names and the Suggestion confidence. So
//!   naming a face makes it a reference at the next ask, and a face the scan just embedded gets its
//!   "Name?" then. Nothing is done per frame.
//! - **References** are every named face with an embedding of the model in use; each person is
//!   scored against at most [`dalica_faces::matching::REFERENCES`] of theirs (the most recent and
//!   most varied, by the photos' dates). Decisions are made on the index's 8-bit embeddings
//!   ([`NoExact`]): the float32 vectors stay in the scan's cache.
//! - **Rejected pairs** (a face is not that person) are read from `people.json` once, on a helper
//!   thread, the first time suggestions are needed (the owner thread waits for it at most
//!   [`READ_WAIT`]; until it has them nothing is suggested), and written back through the I/O
//!   worker whenever they change: a pair names its photo by path, so it survives a restart and a
//!   rebuild; a pair whose photo moved or was renamed in the session follows it. The people of the
//!   write are the model's, plus the file's people the model lacks (never dropped here: that is
//!   `people_file.rs`'s to decide).
//! - Nothing here is logged but counts and paths; no name, rectangle or embedding (`faces.md` §7).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dalica_core::{DateTime, PersonId, PhotoId, Timestamp};
use dalica_faces::evaluate::{Named, Report};
use dalica_faces::matching::{Bars, DEFAULT_MARGIN, Gallery, NoExact, Packed, Reference};
use dalica_library::{FaceRef, Library, Photo};
use dalica_store::people::{PeopleState, PeopleWrite, Rejected};

use crate::error::CmdError;
use crate::faces_index::{FaceAt, FacesIndex};
use crate::session::{Msg, Session};

/// The most rejected pairs kept (the oldest go first beyond it).
pub const MAX_REJECTED: usize = 100_000;
/// How long a command waits for `people.json`'s rejected pairs the first time.
pub const READ_WAIT: Duration = Duration::from_secs(5);
/// What a command answers while the rejected pairs are still being read.
pub const STILL_READING: &str = "The rejected suggestions are still being read; try again in a moment.";

/// One "Name?".
#[derive(Clone, Debug, PartialEq)]
pub struct Made {
    /// The face, as commands name it.
    pub face: FaceRef,
    pub at: FaceAt,
    pub person: PersonId,
    /// The person's best match (cosine).
    pub score: f32,
    /// The next best person and their score.
    pub runner_up: Option<(PersonId, f32)>,
}

/// One rejected pair as `people.json` keeps it, and the photo it was last found as.
#[derive(Clone, Debug)]
struct Pair {
    rejected: Rejected,
    /// The photo, with its file's size and time when found: the same file found under another path
    /// later in the session is the photo moved or renamed.
    at: Option<(PhotoId, u64, Timestamp)>,
}

/// Reading `people.json`'s rejected pairs.
#[derive(Debug, Default)]
enum Load {
    #[default]
    NotStarted,
    Reading(Receiver<Vec<Rejected>>),
    Done,
}

/// What the suggestions depend on.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    revision: u64,
    index: u64,
    pairs: u64,
    on: bool,
    threshold: Option<u32>,
}

/// The suggestions made for one [`Key`].
struct Computed {
    key: Key,
    gallery: Gallery<FaceAt, PersonId>,
    /// The Suggestion threshold (cosine), `None` without a model.
    threshold: Option<f32>,
    /// In Unnamed's order (the photos', then the faces' on each).
    list: Vec<Made>,
    by_face: HashMap<FaceAt, usize>,
    counts: BTreeMap<PersonId, usize>,
}

/// The suggestions' state. Its `Debug` gives counts only: never an embedding (`faces.md` §7).
#[derive(Default)]
pub struct Suggestions {
    generation: u64,
    load: Load,
    pairs: Vec<Pair>,
    /// Rises each time the pairs change.
    pairs_generation: u64,
    /// The library revision the pairs were last found in.
    resolved: Option<u64>,
    /// The pairs last sent to `people.json` (`None`: nothing sent yet, or the write failed).
    written: Option<Vec<Rejected>>,
    /// What the I/O worker's writes reported (and whether the write failed).
    reports: Arc<Mutex<Vec<(String, bool)>>>,
    computed: Option<Computed>,
}

impl std::fmt::Debug for Suggestions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Suggestions")
            .field("generation", &self.generation)
            .field("ready", &self.ready())
            .field("rejected", &self.pairs.len())
            .field("suggested", &self.computed.as_ref().map(|c| c.list.len()))
            .finish_non_exhaustive()
    }
}

impl Suggestions {
    /// Rises each time the suggestions are made again.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether `people.json`'s rejected pairs have been read.
    pub fn ready(&self) -> bool {
        matches!(self.load, Load::Done)
    }

    /// How many rejected pairs are kept.
    pub fn rejected(&self) -> usize {
        self.pairs.len()
    }
}

/// Seconds since the Unix epoch of `d` (UTC; Howard Hinnant's `days_from_civil`).
fn unix_secs(d: DateTime) -> i64 {
    let date = d.date();
    let (m, day) = (i64::from(date.month()), i64::from(date.day()));
    let y = i64::from(date.year()) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + i64::from(d.hour()) * 3600 + i64::from(d.minute()) * 60 + i64::from(d.second())
}

/// When photo `id` was taken (seconds), if anything says.
fn taken(lib: &Library, id: PhotoId) -> Option<i64> {
    lib.effective_date(id).map(unix_secs)
}

/// The path of photo `id`.
fn path_of(lib: &Library, id: PhotoId) -> Option<PathBuf> {
    let p = lib.photo(id)?;
    Some(lib.folder(p.folder)?.path.join(&p.name))
}

/// The photo at `path`.
fn photo_at<'a>(lib: &'a Library, path: &Path) -> Option<&'a Photo> {
    let folder = lib.folder_by_path(path.parent()?)?;
    lib.photo_by_name(folder, path.file_name()?.to_str()?)
}

/// The unnamed faces with an embedding, in Unnamed's order.
fn unnamed<'a>(lib: &'a Library, index: &'a FacesIndex) -> impl Iterator<Item = (FaceRef, FaceAt, &'a Packed)> + 'a {
    lib.unnamed_faces().iter().filter_map(|r| {
        let face = lib.face(*r)?;
        let vec = index.embedding(lib.photo(r.photo)?, &face.rect)?;
        Some((*r, FaceAt::new(r.photo, &face.rect), vec))
    })
}

/// Every named face with an embedding, by person.
fn named(lib: &Library, index: &FacesIndex) -> BTreeMap<PersonId, Vec<Reference<FaceAt>>> {
    let mut out: BTreeMap<PersonId, Vec<Reference<FaceAt>>> = BTreeMap::new();
    for (id, _) in lib.people() {
        let refs: Vec<Reference<FaceAt>> = lib
            .person_faces(id)
            .iter()
            .filter_map(|r| {
                let face = lib.face(*r)?;
                let vec = index.embedding(lib.photo(r.photo)?, &face.rect)?.clone();
                Some(Reference { face: FaceAt::new(r.photo, &face.rect), taken: taken(lib, r.photo), vec })
            })
            .collect();
        out.insert(id.clone(), refs);
    }
    out
}

/// `people.json`'s rejected pairs (none when there is no file or it cannot be read: the people
/// file's own write reports and sets aside a damaged one).
fn read_rejected(dir: &Path) -> Vec<Rejected> {
    let Ok(Some(read)) = dalica_fs::read(&dalica_store::people::path(dir), dalica_store::people::MAX_BYTES) else { return Vec::new() };
    let Ok(root) = dalica_store::people::parse(&read.bytes) else { return Vec::new() };
    dalica_store::people::reading(&root).rejected
}

/// `state` with the people of the file at `dir` (read here, on the I/O worker) that it lacks, as the
/// file says them, so a write of the rejected pairs never drops a person.
fn with_file_people(dir: &Path, mut state: PeopleState) -> PeopleState {
    let Ok(Some(read)) = dalica_fs::read(&dalica_store::people::path(dir), dalica_store::people::MAX_BYTES) else { return state };
    let Ok(root) = dalica_store::people::parse(&read.bytes) else { return state };
    for (id, def) in dalica_store::people::reading(&root).people {
        state.people.entry(id).or_insert(def);
    }
    state
}

impl Session {
    /// The suggestions' state (`faces.md` §5).
    pub fn suggestions(&self) -> &Suggestions {
        &self.suggestions
    }

    /// The suggestions waiting per person (`people.list`'s `pending`); none before the rejected
    /// pairs are read or without a model.
    pub fn pending_suggestions(&mut self) -> BTreeMap<PersonId, usize> {
        match self.refresh_suggestions() {
            Ok(()) => self.suggestions.computed.as_ref().map(|c| c.counts.clone()).unwrap_or_default(),
            Err(_) => BTreeMap::new(),
        }
    }

    /// Reads the rejected pairs (the first time), follows their photos, writes them when they
    /// changed, and makes the suggestions again when what they depend on changed. `Disabled` while
    /// the rejected pairs are still being read.
    pub(crate) fn refresh_suggestions(&mut self) -> Result<(), CmdError> {
        self.take_suggestion_reports();
        self.load_rejected()?;
        self.resolve_rejected();
        self.write_rejected();
        let people = self.settings().people();
        let info = self.faces_index.model_info();
        let threshold = info.map(|m| m.threshold(people.suggestion_confidence));
        let key = Key {
            revision: self.library.revision(),
            index: self.faces_index.generation(),
            pairs: self.suggestions.pairs_generation,
            on: people.suggest_names,
            threshold: threshold.map(f32::to_bits),
        };
        if self.suggestions.computed.as_ref().is_some_and(|c| c.key == key) {
            return Ok(());
        }
        let computed = compute(&self.library, &self.faces_index, &self.suggestions.pairs, key, threshold);
        log::debug!("suggestions made: {} faces, {} people", computed.list.len(), computed.counts.len());
        self.suggestions.computed = Some(computed);
        self.suggestions.generation = self.suggestions.generation.wrapping_add(1);
        Ok(())
    }

    /// The suggestions made (after [`Session::refresh_suggestions`]): the list in Unnamed's order and
    /// the threshold (`None` without a model).
    pub(crate) fn suggestion_list(&self) -> (&[Made], Option<f32>) {
        match &self.suggestions.computed {
            Some(c) => (&c.list, c.threshold),
            None => (&[], None),
        }
    }

    /// The "Name?" of `face`, if it has one (after [`Session::refresh_suggestions`]).
    pub(crate) fn suggestion_for(&self, face: FaceRef) -> Option<&Made> {
        let c = self.suggestions.computed.as_ref()?;
        let rect = self.library.face(face)?.rect;
        c.by_face.get(&FaceAt::new(face.photo, &rect)).and_then(|i| c.list.get(*i))
    }

    /// The suggestions waiting per person (after [`Session::refresh_suggestions`]).
    pub(crate) fn suggestion_counts(&self) -> BTreeMap<PersonId, usize> {
        self.suggestions.computed.as_ref().map(|c| c.counts.clone()).unwrap_or_default()
    }

    /// Find More for `person` (after [`Session::refresh_suggestions`]): the unnamed faces scoring
    /// above 80 % of the Suggestion threshold against the person's references, most similar first;
    /// confirmed faces and rejected pairs left out. Empty without a model.
    pub(crate) fn find_more(&self, person: &PersonId) -> (Vec<(FaceRef, f32)>, Option<f32>) {
        let Some(c) = &self.suggestions.computed else { return (Vec::new(), None) };
        let Some(threshold) = c.threshold else { return (Vec::new(), None) };
        let mut refs: HashMap<FaceAt, FaceRef> = HashMap::new();
        let candidates: Vec<(FaceAt, &Packed)> = unnamed(&self.library, &self.faces_index)
            .map(|(r, at, vec)| {
                refs.insert(at, r);
                (at, vec)
            })
            .collect();
        let found = c.gallery.find_more(person, candidates, threshold);
        (found.into_iter().filter_map(|(at, score)| Some((*refs.get(&at)?, score))).collect(), Some(threshold))
    }

    /// Evaluate on your photos (`faces.evaluate`): each named face hidden in turn, per confidence
    /// step. `None` without a model.
    pub(crate) fn evaluate_suggestions(&self) -> Option<Report> {
        let info = self.faces_index.model_info()?;
        let all: Vec<Named<FaceAt, PersonId>> = named(&self.library, &self.faces_index)
            .into_iter()
            .flat_map(|(person, refs)| refs.into_iter().map(move |r| Named { face: r.face, person: person.clone(), taken: r.taken, vec: r.vec }))
            .collect();
        Some(dalica_faces::evaluate::evaluate(&all, &info.calibration, DEFAULT_MARGIN, &NoExact))
    }

    /// Records that each of `pairs` (a face, the person it is not) is not to be suggested again;
    /// returns how many were new. Written to `people.json` through the I/O worker.
    pub(crate) fn reject_suggestions(&mut self, pairs: &[(FaceRef, PersonId)]) -> usize {
        let mut known: BTreeSet<(PathBuf, [u64; 4], PersonId)> =
            self.suggestions.pairs.iter().map(|p| (p.rejected.photo.clone(), rect_bits(&p.rejected.rect), p.rejected.person.clone())).collect();
        let mut added = 0usize;
        for (r, person) in pairs {
            let (Some(face), Some(photo), Some(path)) = (self.library.face(*r), self.library.photo(r.photo), path_of(&self.library, r.photo)) else {
                continue;
            };
            if !known.insert((path.clone(), rect_bits(&face.rect), person.clone())) {
                continue;
            }
            let rejected = Rejected { photo: path, rect: face.rect, person: person.clone() };
            self.suggestions.pairs.push(Pair { rejected, at: Some((photo.id, photo.size, photo.mtime)) });
            added += 1;
        }
        if added > 0 {
            let over = self.suggestions.pairs.len().saturating_sub(MAX_REJECTED);
            self.suggestions.pairs.drain(..over);
            self.suggestions.pairs_generation = self.suggestions.pairs_generation.wrapping_add(1);
            self.write_rejected();
        }
        added
    }

    /// The I/O worker's reports of earlier writes, as scan warnings; a failed write is tried again.
    fn take_suggestion_reports(&mut self) {
        let reports = std::mem::take(&mut *self.suggestions.reports.lock().unwrap_or_else(PoisonError::into_inner));
        for (report, failed) in reports {
            if failed {
                self.suggestions.written = None;
            }
            self.scan_warnings.push(report);
        }
    }

    /// The first time: reads `people.json`'s rejected pairs on a helper thread and waits for them
    /// at most [`READ_WAIT`] (`Disabled` [`STILL_READING`] after that, still waiting).
    fn load_rejected(&mut self) -> Result<(), CmdError> {
        if matches!(self.suggestions.load, Load::NotStarted) {
            let (tx, rx) = mpsc::channel();
            let dir = self.dir.clone();
            let started = std::thread::Builder::new().name("dalica-rejected-read".into()).spawn(move || {
                let _ = tx.send(read_rejected(&dir));
            });
            self.suggestions.load = match started {
                Ok(_) => Load::Reading(rx),
                Err(e) => {
                    log::warn!("the rejected suggestions could not be read: {e}");
                    self.loaded(Vec::new());
                    Load::Done
                }
            };
        }
        let Load::Reading(rx) = &self.suggestions.load else { return Ok(()) };
        match rx.recv_timeout(READ_WAIT) {
            Ok(list) => self.loaded(list),
            Err(RecvTimeoutError::Timeout) => return Err(CmdError::Disabled(STILL_READING.into())),
            Err(RecvTimeoutError::Disconnected) => self.loaded(Vec::new()),
        }
        self.suggestions.load = Load::Done;
        Ok(())
    }

    /// Takes in the rejected pairs read (the latest [`MAX_REJECTED`], each once).
    fn loaded(&mut self, list: Vec<Rejected>) {
        let mut seen = BTreeSet::new();
        let mut pairs: Vec<Pair> = list
            .into_iter()
            .filter(|r| seen.insert((r.photo.clone(), rect_bits(&r.rect), r.person.clone())))
            .map(|rejected| Pair { rejected, at: None })
            .collect();
        let over = pairs.len().saturating_sub(MAX_REJECTED);
        pairs.drain(..over);
        // What the file says needs no write until it changes.
        self.suggestions.written = Some(pairs.iter().map(|p| p.rejected.clone()).collect());
        self.suggestions.pairs = pairs;
        self.suggestions.resolved = None;
        self.suggestions.pairs_generation = self.suggestions.pairs_generation.wrapping_add(1);
    }

    /// Finds each pair's photo when the library changed: a photo found before whose file (same
    /// size and time) is now under another path was moved or renamed, and its pair follows it.
    fn resolve_rejected(&mut self) {
        let revision = self.library.revision();
        if self.suggestions.resolved == Some(revision) {
            return;
        }
        self.suggestions.resolved = Some(revision);
        let lib = &self.library;
        let mut changed = false;
        for pair in &mut self.suggestions.pairs {
            let followed = pair.at.and_then(|(id, size, mtime)| lib.photo(id).filter(|p| p.size == size && p.mtime == mtime).map(|p| p.id));
            let at = match followed {
                Some(id) => {
                    if let Some(path) = path_of(lib, id).filter(|p| *p != pair.rejected.photo) {
                        pair.rejected.photo = path;
                    }
                    pair.at
                }
                None => photo_at(lib, &pair.rejected.photo).map(|p| (p.id, p.size, p.mtime)),
            };
            if at.map(|a| a.0) != pair.at.map(|a| a.0) {
                changed = true;
            }
            pair.at = at;
        }
        if changed {
            self.suggestions.pairs_generation = self.suggestions.pairs_generation.wrapping_add(1);
        }
    }

    /// Sends the rejected pairs to `people.json` when they differ from what was last sent.
    fn write_rejected(&mut self) {
        if !matches!(self.suggestions.load, Load::Done) {
            return;
        }
        let list: Vec<Rejected> = self.suggestions.pairs.iter().map(|p| p.rejected.clone()).collect();
        if self.suggestions.written.as_ref() == Some(&list) {
            return;
        }
        self.suggestions.written = Some(list.clone());
        let state = dalica_store::people::state(&self.library);
        let (dir, now, reports) = (self.dir.clone(), self.clock.now(), Arc::clone(&self.suggestions.reports));
        self.send(Msg::Run(Box::new(move || {
            let path = dalica_store::people::path(&dir);
            let mut state = with_file_people(&dir, state);
            state.rejected = Some(list);
            let report = match dalica_store::people::write(&dir, &state, now) {
                PeopleWrite::Written | PeopleWrite::Unchanged => None,
                PeopleWrite::SetAsideAndWritten { why, kept } => {
                    let kept = kept.map(|k| format!("kept as {}", k.display())).unwrap_or_else(|| "it could not be kept aside".into());
                    Some((format!("{}: {why}; {kept}; a new one was written", path.display()), false))
                }
                PeopleWrite::Failed(why) => Some((format!("{}: rejected suggestions not written: {why}", path.display()), true)),
            };
            if let Some(r) = report {
                // Paths and reasons only: never a name (`faces.md` §7).
                log::warn!("{}", r.0);
                reports.lock().unwrap_or_else(PoisonError::into_inner).push(r);
            }
        })));
    }
}

/// A rectangle's corners by their bits (a `Rect01` is always finite).
fn rect_bits(r: &dalica_core::Rect01) -> [u64; 4] {
    [r.x0(), r.y0(), r.x1(), r.y1()].map(f64::to_bits)
}

/// The suggestions for `key`: the references of every person, the rejected pairs found in the
/// library, and (with Suggest names on and a model) a "Name?" for each unnamed face that passes
/// both bars.
fn compute(lib: &Library, index: &FacesIndex, pairs: &[Pair], key: Key, threshold: Option<f32>) -> Computed {
    let mut gallery: Gallery<FaceAt, PersonId> = Gallery::new();
    for (person, refs) in named(lib, index) {
        gallery.set_person(person, refs);
    }
    for pair in pairs {
        if let Some((id, _, _)) = pair.at {
            gallery.reject(FaceAt::new(id, &pair.rejected.rect), pair.rejected.person.clone());
        }
    }
    let mut list = Vec::new();
    if let (true, Some(threshold)) = (key.on, threshold) {
        let bars = Bars::new(threshold);
        for (face, at, vec) in unnamed(lib, index) {
            if let Some(s) = gallery.suggest(&at, vec, bars, &NoExact) {
                list.push(Made { face, at, person: s.person, score: s.score, runner_up: s.runner_up });
            }
        }
    }
    let by_face = list.iter().enumerate().map(|(i, m)| (m.at, i)).collect();
    let mut counts: BTreeMap<PersonId, usize> = BTreeMap::new();
    for m in &list {
        *counts.entry(m.person.clone()).or_default() += 1;
    }
    Computed { key, gallery, threshold, list, by_face, counts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_seconds_round_trip_through_dates() {
        for secs in [0i64, 1_780_602_130, -1, 951_782_400, 4_102_444_799, -62_135_596_800] {
            let d = DateTime::from_unix_secs(secs).unwrap();
            assert_eq!(unix_secs(d), secs, "{d}");
        }
    }
}
