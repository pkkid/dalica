//! Suggestions in the window (`organizing.md` §6.3–§6.4, `faces.md` §5): in a person's section the
//! strip "Suggestions:" with [Confirm All], [Confirm], [Reject], [Find More] and the toggle "Show
//! only suggestions"; ✓ and × on each suggested face, and "Name?" with ✓ / × on Unnamed faces and
//! in the People pane. Confirming makes a face its person's (and a reference for later
//! suggestions), rejecting records the pair so it is never suggested again; each through the
//! engine's `faces.confirm`, `faces.confirmAll`, `faces.reject` and `faces.findMore` (M13.26). The
//! ids are the UI's own (`suggestions.*`), never the engine's, so `engine.execute faces.confirm`
//! always reaches the engine.
//!
//! - **The suggestions the window shows** ([`Snapshot`], [`current`]) are one copy of the engine's
//!   `faces.suggest` (and of `faces.findMore` for the people Find More was asked for), made again
//!   only when what they depend on moved: the library's revision, the face index's generation,
//!   Suggest names and the Suggestion confidence, the rejected pairs and the engine's suggestion
//!   generation. Nothing is asked of the engine per frame, so the tree's "?" badge, the person
//!   sections, the Unnamed grid and the People pane all read the same copy. A library with no
//!   person or no face model asks nothing.
//! - **The faces a command acts on** are its `faces`, else the faces selected: in the Unnamed grid
//!   its selection; in a person's section the faces suggested (or found by Find More) for that
//!   person on the photos selected there, the person implied.
//! - **Confirm** (`suggestions.confirm {faces?, person?}`) names each face after its suggestion
//!   (`faces.confirm`); with `person`, a face Find More listed (no suggestion of its own, or
//!   another's) is named as that person instead (`faces.move`). One undo step either way.
//! - **Reject** (`suggestions.reject {faces?, person?}`) records each pair for good
//!   (`faces.reject`); no undo step.
//! - **Find More** (`suggestions.findMore {person}`) lists the person's near misses
//!   (`faces.findMore`); they join the person's section after the suggestions, each with ✓ / ×,
//!   until the window closes.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dalica_core::{PersonId, PhotoId};
use dalica_engine::{CmdError, Session};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::{self, FaceRef, MAX_PEOPLE};

/// Why Confirm and Reject are off with no suggestion selected.
pub const NO_SUGGESTION: &str = "Select a suggested face first.";
/// Why Confirm All is off for a person with nothing suggested.
pub const NONE_WAITING: &str = "No suggestions are waiting for this person.";
/// Why the commands refuse before the library is open.
pub const OPENING: &str = "The library is still opening.";
/// The most suggestions (and Find More faces per person) the window reads: the engine's page.
pub const MAX_SHOWN: usize = 10_000;
/// The most people whose Find More faces are kept.
pub const MAX_FIND_MORE: usize = 64;
/// How long a failed read waits before it is tried again.
const RETRY: Duration = Duration::from_secs(2);

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "suggestions.confirm",
        label: "Confirm",
        menu: "",
        keys: &[],
        params: "{faces?, person?}: confirm the suggested names of the faces (default: the faces selected); with person, name them as that person (Find More's faces); one undo step",
        run: confirm,
    },
    UiCommand {
        id: "suggestions.reject",
        label: "Reject",
        menu: "",
        keys: &[],
        params: "{faces?, person?}: reject the suggested names of the faces (default: the faces selected), or that they are `person`, for good",
        run: reject,
    },
    UiCommand {
        id: "suggestions.confirmAll",
        label: "Confirm All",
        menu: "",
        keys: &[],
        params: "{person}: confirm every suggestion of the person; one undo step",
        run: confirm_all,
    },
    UiCommand {
        id: "suggestions.findMore",
        label: "Find More",
        menu: "",
        keys: &[],
        params: "{person}: list the near misses for the person, most similar first; they join the person's section with ✓ / ×",
        run: find_more,
    },
    UiCommand {
        id: "suggestions.only",
        label: "Show only suggestions",
        menu: "",
        keys: &[],
        params: "{person, on?}: the person's section shows only their suggestions; toggles without `on`",
        run: only,
    },
];

pub const CHECKS: &[(&str, Check)] = &[
    ("suggestions.confirm", has_selected),
    ("suggestions.reject", has_selected),
    ("suggestions.confirmAll", has_waiting),
    ("suggestions.findMore", is_open),
];

// --- the suggestions the window shows ------------------------------------------------------------

/// Where a face of a person's section comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The engine suggests the person for it.
    Suggested,
    /// Find More listed it for the person.
    More,
}

/// The suggestions as the window shows them (see the module's doc).
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Rises each time the copy is made again (0: never made).
    pub generation: u64,
    /// Every "Name?", in Unnamed's order.
    pub list: Vec<(FaceRef, PersonId)>,
    /// The suggestions waiting per person (the "?" badge).
    pub pending: BTreeMap<PersonId, usize>,
    /// Find More's faces per person, most similar first (those suggested as the person left out).
    pub more: BTreeMap<PersonId, Vec<FaceRef>>,
    by_face: HashMap<FaceRef, PersonId>,
    /// The first face of each person's section on a photo, and where it comes from.
    by_photo: HashMap<(PersonId, PhotoId), (FaceRef, Kind)>,
}

impl Snapshot {
    fn new(generation: u64, list: Vec<(FaceRef, PersonId)>, pending: BTreeMap<PersonId, usize>, more: BTreeMap<PersonId, Vec<FaceRef>>) -> Snapshot {
        let by_face: HashMap<FaceRef, PersonId> = list.iter().map(|(f, p)| (*f, p.clone())).collect();
        let mut by_photo = HashMap::new();
        for (f, p) in &list {
            by_photo.entry((p.clone(), f.photo)).or_insert((*f, Kind::Suggested));
        }
        let more: BTreeMap<PersonId, Vec<FaceRef>> = more
            .into_iter()
            .map(|(p, faces)| {
                let faces: Vec<FaceRef> = faces.into_iter().filter(|f| by_face.get(f) != Some(&p)).collect();
                for f in &faces {
                    by_photo.entry((p.clone(), f.photo)).or_insert((*f, Kind::More));
                }
                (p, faces)
            })
            .collect();
        Snapshot { generation, list, pending, more, by_face, by_photo }
    }

    /// The person suggested for `face`, if any.
    pub fn suggested(&self, face: FaceRef) -> Option<&PersonId> {
        self.by_face.get(&face)
    }

    /// How many suggestions wait for `person`.
    pub fn pending(&self, person: &PersonId) -> usize {
        self.pending.get(person).copied().unwrap_or(0)
    }

    /// Whether `person`'s section has faces to review: suggestions or Find More's.
    pub fn has(&self, person: &PersonId) -> bool {
        self.pending(person) > 0 || self.more.get(person).is_some_and(|m| !m.is_empty())
    }

    /// The face of `person`'s section on `photo` (a suggestion first, else Find More's), if any.
    pub fn on_photo(&self, person: &PersonId, photo: PhotoId) -> Option<(FaceRef, Kind)> {
        self.by_photo.get(&(person.clone(), photo)).copied()
    }

    /// The photos of `person`'s suggestions, then of Find More's faces, each once, in order.
    pub fn photos(&self, person: &PersonId) -> Vec<PhotoId> {
        let mut seen = HashSet::new();
        let suggested = self.list.iter().filter(|(_, p)| p == person).map(|(f, _)| f.photo);
        let more = self.more.get(person).into_iter().flatten().map(|f| f.photo);
        suggested.chain(more).filter(|id| seen.insert(*id)).collect()
    }
}

/// What the copy was made from.
#[derive(Debug, Clone, PartialEq)]
struct Key {
    dir: PathBuf,
    revision: u64,
    index: u64,
    on: bool,
    confidence: u32,
    rejected: usize,
    engine: u64,
    more: Vec<PersonId>,
}

impl Key {
    fn of(s: &Session, more: &[PersonId]) -> Key {
        let people = s.settings().people();
        Key {
            dir: s.dir().to_path_buf(),
            revision: s.library().revision(),
            index: s.faces_index().generation(),
            on: people.suggest_names,
            confidence: people.suggestion_confidence,
            rejected: s.suggestions().rejected(),
            engine: s.suggestions().generation(),
            more: more.to_vec(),
        }
    }
}

/// The window's copy, kept for the window's thread.
#[derive(Default)]
struct Local {
    key: Option<Key>,
    /// When the last read failed: it is tried again after [`RETRY`].
    failed: Option<Instant>,
    snapshot: Arc<Snapshot>,
    /// The people Find More was asked for, in the order asked.
    more: Vec<PersonId>,
    generation: u64,
}

thread_local! {
    static LOCAL: RefCell<Local> = RefCell::new(Local::default());
}

fn local<R>(f: impl FnOnce(&mut Local) -> R) -> R {
    LOCAL.with(|l| f(&mut l.borrow_mut()))
}

/// The suggestions as last made, without asking the engine (what a part drawing from a frame reads;
/// the panels bring them up to date with [`current`] first).
pub fn latest() -> Arc<Snapshot> {
    local(|l| Arc::clone(&l.snapshot))
}

/// The suggestions now: the copy kept while what it depends on is as it was, else made again.
pub fn current(app: &mut App) -> Arc<Snapshot> {
    let Some(s) = app.session_mut() else {
        return local(|l| {
            if l.key.is_some() {
                *l = Local { generation: l.generation, ..Local::default() };
            }
            Arc::clone(&l.snapshot)
        });
    };
    let more = local(|l| l.more.clone());
    let key = Key::of(s, &more);
    let kept = local(|l| {
        let due = l.failed.is_none_or(|t| t.elapsed() >= RETRY);
        (l.key.as_ref() == Some(&key) && (l.failed.is_none() || !due)).then(|| Arc::clone(&l.snapshot))
    });
    if let Some(snapshot) = kept {
        return snapshot;
    }
    // A new library forgets the old one's Find More.
    let more = if local(|l| l.key.as_ref().is_some_and(|k| k.dir != key.dir)) {
        local(|l| l.more.clear());
        Vec::new()
    } else {
        more
    };
    let read = read(s, &more);
    let key = Key::of(s, &more);
    local(|l| {
        l.generation = l.generation.wrapping_add(1).max(1);
        let (list, pending, found, failed) = read;
        l.failed = failed.then(Instant::now);
        l.snapshot = Arc::new(Snapshot::new(l.generation, list, pending, found));
        l.key = Some(key);
        Arc::clone(&l.snapshot)
    })
}

type Read = (Vec<(FaceRef, PersonId)>, BTreeMap<PersonId, usize>, BTreeMap<PersonId, Vec<FaceRef>>, bool);

/// The engine's suggestions and Find More's faces for `more`; whether a read failed.
fn read(s: &mut Session, more: &[PersonId]) -> Read {
    let mut out: Read = (Vec::new(), BTreeMap::new(), BTreeMap::new(), false);
    // Nothing to suggest from: no person, or no model to compare faces with.
    if s.library().people().next().is_none() || s.faces_index().model().is_none() {
        return out;
    }
    match s.execute("faces.suggest", &json!({ "limit": MAX_SHOWN })) {
        Ok(reply) => {
            out.0 = reply
                .get("suggestions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|m| Some((face_of(m.get("face")?)?, person_of(m.get("person")?)?)))
                .collect();
            out.1 = reply
                .get("pending")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter_map(|(p, n)| Some((PersonId::parse(p).ok()?, usize::try_from(n.as_u64()?).ok()?)))
                .collect();
        }
        Err(e) => {
            log::debug!("suggestions not read: {}", e.code());
            out.3 = true;
        }
    }
    for person in more {
        match s.execute("faces.findMore", &json!({ "person": person, "limit": MAX_SHOWN })) {
            Ok(reply) => {
                let faces = reply.get("faces").and_then(Value::as_array).into_iter().flatten().filter_map(|m| face_of(m.get("face")?)).collect();
                out.2.insert(person.clone(), faces);
            }
            // A person gone (deleted, merged) has nothing more.
            Err(CmdError::NotFound(_)) => {}
            Err(e) => {
                log::debug!("Find More not read: {}", e.code());
                out.3 = true;
            }
        }
    }
    out
}

fn face_of(v: &Value) -> Option<FaceRef> {
    v.as_str()?.parse().ok()
}

fn person_of(v: &Value) -> Option<PersonId> {
    PersonId::parse(v.as_str()?).ok()
}

/// Forgets Find More's faces and the copy (a test starting a fresh window on the same thread).
pub fn reset() {
    local(|l| *l = Local::default());
}

// --- the commands --------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Faces {
    #[serde(default)]
    faces: Option<Vec<FaceRef>>,
    #[serde(default)]
    person: Option<PersonId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Person {
    person: PersonId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Only {
    person: PersonId,
    #[serde(default)]
    on: Option<bool>,
}

/// The faces selected that have something to confirm or reject, and the person they imply: in the
/// Unnamed grid its selection's suggested faces; in a person's section that person's faces
/// (suggested or Find More's) on the photos selected there.
pub fn selected(app: &App, snapshot: &Snapshot) -> (Vec<FaceRef>, Option<PersonId>) {
    let st = app.state();
    let person = if st.unnamed.open { None } else { st.selection.section.and_then(state::Section::person) };
    match person {
        Some(person) => {
            let faces = st.selection.ids.iter().filter_map(|id| snapshot.on_photo(&person, *id).map(|(f, _)| f)).take(state::MAX_IDS).collect();
            (faces, Some(person))
        }
        None => (st.unnamed.selection.iter().copied().filter(|f| snapshot.suggested(*f).is_some()).collect(), None),
    }
}

fn is_open(app: &App, _: &Value) -> Result<(), String> {
    if app.session().is_some() { Ok(()) } else { Err(OPENING.into()) }
}

fn has_selected(app: &App, params: &Value) -> Result<(), String> {
    is_open(app, params)?;
    let named = params.get("faces").and_then(Value::as_array).is_some_and(|l| !l.is_empty());
    if named || !selected(app, &latest()).0.is_empty() { Ok(()) } else { Err(NO_SUGGESTION.into()) }
}

fn has_waiting(app: &App, params: &Value) -> Result<(), String> {
    is_open(app, params)?;
    let person = params.get("person").and_then(person_of);
    if person.is_some_and(|p| latest().pending(&p) > 0) { Ok(()) } else { Err(NONE_WAITING.into()) }
}

/// What a confirm or reject acts on: the faces, the person they are confirmed or rejected as, and
/// the suggestions they were read from.
type Targets = (Vec<FaceRef>, Option<PersonId>, Arc<Snapshot>);

/// The faces and person a confirm or reject acts on: those given, else the selection's.
fn targets(app: &mut App, p: Faces) -> Result<Targets, CmdError> {
    if app.session().is_none() {
        return Err(CmdError::Disabled(OPENING.into()));
    }
    let snapshot = current(app);
    let (faces, person) = match p.faces {
        Some(faces) => (faces, p.person),
        None => {
            let (faces, implied) = selected(app, &snapshot);
            (faces, p.person.or(implied))
        }
    };
    let faces = super::faces::known_faces(app, faces)?;
    if faces.is_empty() {
        return Err(CmdError::Disabled(NO_SUGGESTION.into()));
    }
    Ok((faces, person, snapshot))
}

fn confirm(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let (faces, person, snapshot) = targets(app, p)?;
    match person {
        // Find More's faces have no suggestion of their own: they are named as the person.
        Some(person) if faces.iter().any(|f| snapshot.suggested(*f) != Some(&person)) => {
            app.run("faces.move", &json!({ "faces": faces, "person": person }))
        }
        _ => app.run("faces.confirm", &json!({ "faces": faces })),
    }
}

fn reject(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let (faces, person, _) = targets(app, p)?;
    let mut ask = json!({ "faces": faces });
    if let Some(person) = &person {
        ask["person"] = json!(person);
    }
    let reply = app.run("faces.reject", &ask)?;
    // A rejected face leaves Find More's list at once (the engine leaves it out from now on).
    local(|l| l.key = None);
    Ok(reply)
}

fn confirm_all(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse(params)?;
    if app.session().is_none() {
        return Err(CmdError::Disabled(OPENING.into()));
    }
    app.run("faces.confirmAll", &json!({ "person": p.person }))
}

fn find_more(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse(params)?;
    if app.session().is_none() {
        return Err(CmdError::Disabled(OPENING.into()));
    }
    let reply = app.run("faces.findMore", &json!({ "person": p.person }))?;
    local(|l| {
        l.more.retain(|x| *x != p.person);
        if l.more.len() >= MAX_FIND_MORE {
            l.more.remove(0);
        }
        l.more.push(p.person.clone());
    });
    current(app);
    Ok(reply)
}

fn only(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Only = parse(params)?;
    let list = &app.state().people.only_suggestions;
    let now = list.contains(&p.person);
    let on = p.on.unwrap_or(!now);
    if on && !now && list.len() >= MAX_PEOPLE {
        return Err(CmdError::BadParams(format!("at most {MAX_PEOPLE} people show only their suggestions")));
    }
    let list = &mut app.state_mut().people.only_suggestions;
    if on && !now {
        list.push(p.person.clone());
    } else if !on {
        list.retain(|x| *x != p.person);
    }
    Ok(json!({ "person": p.person, "on": on }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(photo: u64, index: u16) -> FaceRef {
        FaceRef { photo: PhotoId::new(photo), index }
    }

    #[test]
    fn only_suggestions_toggles_and_the_others_wait_for_the_library() {
        reset();
        let mut app = App::new(None);
        assert_eq!(app.run("suggestions.only", &json!({ "person": "p00a1" })).unwrap(), json!({ "person": "p00a1", "on": true }));
        assert_eq!(app.state().people.only_suggestions.len(), 1);
        app.run("suggestions.only", &json!({ "person": "p00a1", "on": false })).unwrap();
        assert!(app.state().people.only_suggestions.is_empty());
        for (id, params) in [
            ("suggestions.confirm", json!({ "faces": ["5.1"] })),
            ("suggestions.reject", json!({})),
            ("suggestions.confirmAll", json!({ "person": "p00a1" })),
            ("suggestions.findMore", json!({ "person": "p00a1" })),
        ] {
            assert_eq!(app.run(id, &params), Err(CmdError::Disabled(OPENING.into())), "{id}");
        }
        assert!(matches!(app.run("suggestions.confirmAll", &json!({})), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("suggestions.confirm", &json!({ "faces": ["5"] })), Err(CmdError::BadParams(_))));
        assert!(matches!(app.run("suggestions.reject", &json!({ "person": "x" })), Err(CmdError::BadParams(_))));
        assert_eq!(current(&mut app).generation, 0, "no library, nothing read");
    }

    #[test]
    fn a_snapshot_puts_suggestions_before_find_more_and_each_photo_once() {
        let ana = PersonId::parse("p00a1").unwrap();
        let bo = PersonId::parse("p00b2").unwrap();
        let list = vec![(face(3, 0), ana.clone()), (face(4, 1), bo.clone()), (face(3, 1), ana.clone())];
        let pending = BTreeMap::from([(ana.clone(), 2), (bo.clone(), 1)]);
        // Find More lists a face already suggested as Ana (left out), one suggested as Bo, a new one.
        let more = BTreeMap::from([(ana.clone(), vec![face(3, 0), face(4, 1), face(7, 0)])]);
        let s = Snapshot::new(1, list, pending, more);
        assert_eq!(s.photos(&ana), [PhotoId::new(3), PhotoId::new(4), PhotoId::new(7)]);
        assert_eq!(s.photos(&bo), [PhotoId::new(4)]);
        assert_eq!(s.on_photo(&ana, PhotoId::new(3)), Some((face(3, 0), Kind::Suggested)), "the first suggestion on the photo");
        assert_eq!(s.on_photo(&ana, PhotoId::new(4)), Some((face(4, 1), Kind::More)));
        assert_eq!(s.on_photo(&bo, PhotoId::new(7)), None);
        assert_eq!((s.pending(&ana), s.suggested(face(4, 1))), (2, Some(&bo)));
        assert!(s.has(&ana) && s.has(&bo) && !s.has(&PersonId::parse("p00c3").unwrap()));
        assert_eq!(s.more[&ana], [face(4, 1), face(7, 0)]);
    }
}
