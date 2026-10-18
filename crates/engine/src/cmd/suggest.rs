//! Suggestions (`faces.md` §5, `organizing.md` §6.3; the state is `crate::suggestions`):
//! `faces.suggest {ids? | person?}`, `faces.confirm {faces}`, `faces.confirmAll {person}`,
//! `faces.reject {faces, person?}`, `faces.findMore {person}` and `faces.evaluate`.
//!
//! - A suggestion is only a label: **confirming** names the face after its suggested person (one
//!   library undo step, whatever the number of faces), which makes it one of that person's
//!   references from then on; **Confirm All** confirms every face suggested as one person, as one
//!   step. **Rejecting** records the pair (the face is not that person) in `people.json`, so it is
//!   never suggested again, also after a restart; it is no undo step.
//! - Every command gives the face scan its turn first ([`Session::poll_face_scan`]), so faces
//!   embedded meanwhile count, and then brings the suggestions up to date.
//! - Replies and errors hold face ids, person ids, scores and counts, never a name, a rectangle or
//!   an embedding (`faces.md` §7), so they need no people permission over automation.

use std::collections::BTreeMap;

use dalica_core::{PersonId, PhotoId};
use dalica_library::{Face, FacePerson, FaceRef, Library, Op};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::faces::{DEFAULT_LIMIT, MAX_LIMIT, MAX_OFFSET, face_id, known_faces, known_person};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Disabled, Effect, History, always, params};
use crate::session::Session;
use crate::suggestions::Made;

/// What confirming answers while Suggest names is off.
pub const SUGGEST_OFF: &str = "Suggest names is off (Options ▸ People).";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "faces.suggest",
            label: "Name Suggestions",
            menu: "",
            keys: &[],
            params: r#"{"ids"?:[PhotoId] | "person"?:PersonId (at most one; none: every unnamed face), "offset"?:int=0, "limit"?:int=1000 (at most 10000)}: the unnamed faces with a "Name?" while Suggest names is on: a person scoring at or above the Suggestion confidence's threshold and 0.05 ahead of the runner-up, rejected pairs left out. Replies {on, model:id|null, threshold:number|null, suggestions:[{face:"<PhotoId>.<n>", person, score, runnerUp:{person, score}|null}] (in Unnamed's order), total, offset, limit, pending:{PersonId: count}}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: suggest,
        },
        CommandSpec {
            id: "faces.confirm",
            label: "Confirm",
            menu: "",
            keys: &[],
            params: r#"{"faces":["<PhotoId>.<n>"]}: each face (it must have a suggestion) is named after its suggested person and becomes one of that person's references. One undo step. Replies {changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: confirm_enabled,
            run: confirm,
        },
        CommandSpec {
            id: "faces.confirmAll",
            label: "Confirm All",
            menu: "",
            keys: &[],
            params: r#"{"person":PersonId}: every face suggested as that person is named after them. One undo step. Replies {changed}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: suggest_on,
            run: confirm_all,
        },
        CommandSpec {
            id: "faces.reject",
            label: "Reject",
            menu: "",
            keys: &[],
            params: r#"{"faces":["<PhotoId>.<n>"] (unnamed faces), "person"?:PersonId (default: each face's suggested person; give it for a face Find More listed)}: the face is not that person: the pair is never suggested again (kept in people.json, also after a restart). No undo step. Replies {rejected (new pairs)}"#,
            effect: Effect::Library,
            history: History::None,
            enabled: has_faces,
            run: reject,
        },
        CommandSpec {
            id: "faces.findMore",
            label: "Find More",
            menu: "",
            keys: &[],
            params: r#"{"person":PersonId, "offset"?:int=0, "limit"?:int=1000 (at most 10000)}: the unnamed faces scoring above 80 % of the Suggestion threshold against the person's faces, most similar first, for the user to review (also while Suggest names is off); rejected pairs left out. Replies {person, model:id|null, threshold:number|null, faces:[{face:"<PhotoId>.<n>", score}], total, offset, limit}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: find_more,
        },
        CommandSpec {
            id: "faces.evaluate",
            label: "Evaluate Suggestions",
            menu: "",
            keys: &[],
            params: r#"{}: Evaluate on your photos: each named face with an embedding is hidden in turn and suggested from the others (shots within 5 seconds of it left out). Replies {model:id|null, faces (hidden in turn), confidence (the setting), steps:[{confidence, threshold, suggested, right}] (50 … 95)}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: evaluate,
        },
    ]
}

/// Disabled with nothing to act on: an empty `faces` list.
fn has_faces(_: &Session, p: &Value) -> Result<(), Disabled> {
    match p.get("faces").and_then(Value::as_array) {
        Some(list) if list.is_empty() => Err(Disabled("Select some faces first.".into())),
        _ => Ok(()),
    }
}

/// Disabled while Suggest names is off: there is nothing to confirm.
fn suggest_on(s: &Session, _: &Value) -> Result<(), Disabled> {
    if s.settings().people().suggest_names { Ok(()) } else { Err(Disabled(SUGGEST_OFF.into())) }
}

fn confirm_enabled(s: &Session, p: &Value) -> Result<(), Disabled> {
    has_faces(s, p)?;
    suggest_on(s, p)
}

/// A score as replies give it: four decimals.
fn score(v: f32) -> Value {
    json!((f64::from(v) * 1e4).round() / 1e4)
}

fn made_json(m: &Made) -> Value {
    json!({
        "face": face_id(m.face), "person": m.person, "score": score(m.score),
        "runnerUp": m.runner_up.as_ref().map(|(p, v)| json!({ "person": p, "score": score(*v) })),
    })
}

/// The page `offset`, `limit` of a list, checked.
fn page(offset: Option<usize>, limit: Option<usize>) -> Result<(usize, usize), CmdError> {
    let (offset, limit) = (offset.unwrap_or(0), limit.unwrap_or(DEFAULT_LIMIT));
    if offset > MAX_OFFSET {
        return Err(CmdError::BadParams(format!("offset is at most {MAX_OFFSET}")));
    }
    if limit > MAX_LIMIT {
        return Err(CmdError::BadParams(format!("limit is at most {MAX_LIMIT}")));
    }
    Ok((offset, limit))
}

/// The face scan's turn, then the suggestions brought up to date.
fn refresh(s: &mut Session) -> Result<(), CmdError> {
    s.poll_face_scan();
    s.refresh_suggestions()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestParams {
    ids: Option<Vec<PhotoId>>,
    person: Option<PersonId>,
    offset: Option<usize>,
    limit: Option<usize>,
}

fn suggest(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: SuggestParams = params(p)?;
    if p.ids.is_some() && p.person.is_some() {
        return Err(CmdError::BadParams("give at most one of ids and person".into()));
    }
    let (offset, limit) = page(p.offset, p.limit)?;
    let ids: Option<std::collections::HashSet<PhotoId>> = match p.ids {
        Some(ids) => Some(crate::cmd::photo::known(s, ids)?.into_iter().collect()),
        None => None,
    };
    if let Some(person) = &p.person {
        known_person(s.library(), person)?;
    }
    refresh(s)?;
    let (list, threshold) = s.suggestion_list();
    let chosen: Vec<&Made> = list
        .iter()
        .filter(|m| ids.as_ref().is_none_or(|ids| ids.contains(&m.face.photo)))
        .filter(|m| p.person.as_ref().is_none_or(|person| *person == m.person))
        .collect();
    let shown: Vec<Value> = chosen.iter().skip(offset).take(limit).map(|m| made_json(m)).collect();
    Ok(json!({
        "on": s.settings().people().suggest_names, "model": s.faces_index().model(), "threshold": threshold.map(score),
        "suggestions": shown, "total": chosen.len(), "offset": offset, "limit": limit, "pending": s.suggestion_counts(),
    }))
}

/// The op naming each of `faces` after its person, one `SetFaces` for all their photos, and how
/// many faces it changes; `None` when none would change.
fn name_each(lib: &Library, faces: &[(FaceRef, PersonId)]) -> (Option<Op>, usize) {
    let mut lists: BTreeMap<PhotoId, Vec<Face>> = BTreeMap::new();
    let mut changed = 0usize;
    for (r, person) in faces {
        let list = lists.entry(r.photo).or_insert_with(|| lib.faces(r.photo).to_vec());
        let who = FacePerson::Person(person.clone());
        if let Some(face) = list.get_mut(r.index)
            && face.person != who
        {
            face.person = who;
            changed += 1;
        }
    }
    if changed == 0 {
        return (None, 0);
    }
    let faces: Vec<(PhotoId, Vec<Face>)> = lists.into_iter().filter(|(p, list)| lib.faces(*p) != list.as_slice()).collect();
    (Some(Op::SetFaces { faces }), changed)
}

/// Applies `faces` named after their people as one step; replies how many changed.
fn name_all(s: &mut Session, faces: &[(FaceRef, PersonId)]) -> Result<Value, CmdError> {
    let (op, changed) = name_each(s.library(), faces);
    if let Some(op) = op {
        s.apply(op)?;
    }
    Ok(json!({ "changed": changed }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FacesParams {
    faces: Vec<String>,
}

fn confirm(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: FacesParams = params(p)?;
    let faces = known_faces(s.library(), &p.faces)?;
    refresh(s)?;
    let mut named = Vec::with_capacity(faces.len());
    for r in faces {
        let Some(m) = s.suggestion_for(r) else {
            return Err(CmdError::BadParams(format!("face {} has no suggestion to confirm", face_id(r))));
        };
        named.push((r, m.person.clone()));
    }
    name_all(s, &named)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PersonParams {
    person: PersonId,
}

fn confirm_all(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: PersonParams = params(p)?;
    known_person(s.library(), &p.person)?;
    refresh(s)?;
    let (list, _) = s.suggestion_list();
    let named: Vec<(FaceRef, PersonId)> = list.iter().filter(|m| m.person == p.person).map(|m| (m.face, m.person.clone())).collect();
    name_all(s, &named)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RejectParams {
    faces: Vec<String>,
    person: Option<PersonId>,
}

fn reject(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: RejectParams = params(p)?;
    let faces = known_faces(s.library(), &p.faces)?;
    if let Some(person) = &p.person {
        known_person(s.library(), person)?;
    }
    for r in &faces {
        if s.library().face(*r).is_some_and(|f| f.person != FacePerson::Unnamed) {
            return Err(CmdError::BadParams(format!("face {} is not unnamed: only an unnamed face's suggestion can be rejected", face_id(*r))));
        }
    }
    refresh(s)?;
    let mut pairs = Vec::with_capacity(faces.len());
    for r in faces {
        let person = match &p.person {
            Some(person) => person.clone(),
            None => match s.suggestion_for(r) {
                Some(m) => m.person.clone(),
                None => return Err(CmdError::BadParams(format!("face {} has no suggestion to reject; give person", face_id(r)))),
            },
        };
        pairs.push((r, person));
    }
    let rejected = s.reject_suggestions(&pairs);
    Ok(json!({ "rejected": rejected }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FindMoreParams {
    person: PersonId,
    offset: Option<usize>,
    limit: Option<usize>,
}

fn find_more(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let p: FindMoreParams = params(p)?;
    let (offset, limit) = page(p.offset, p.limit)?;
    known_person(s.library(), &p.person)?;
    refresh(s)?;
    let (found, threshold) = s.find_more(&p.person);
    let shown: Vec<Value> = found.iter().skip(offset).take(limit).map(|(r, v)| json!({ "face": face_id(*r), "score": score(*v) })).collect();
    Ok(json!({
        "person": p.person, "model": s.faces_index().model(), "threshold": threshold.map(score),
        "faces": shown, "total": found.len(), "offset": offset, "limit": limit,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluateParams {}

fn evaluate(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let _: EvaluateParams = params(p)?;
    s.poll_face_scan();
    let confidence = s.settings().people().suggestion_confidence;
    let Some(report) = s.evaluate_suggestions() else {
        return Ok(json!({ "model": Value::Null, "faces": 0, "confidence": confidence, "steps": [] }));
    };
    let steps: Vec<Value> = report
        .steps
        .iter()
        .map(|st| json!({ "confidence": st.confidence.value(), "threshold": score(st.threshold), "suggested": st.suggested, "right": st.right }))
        .collect();
    Ok(json!({ "model": s.faces_index().model(), "faces": report.faces, "confidence": confidence, "steps": steps }))
}
