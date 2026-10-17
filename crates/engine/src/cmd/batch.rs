//! Batch Edit, Copy and Paste All Edits, and Undo All Edits (`editing.md` §5.1 and §9).
//!
//! - **`edit.apply {ids, op, params?}`** (Picture ▸ Batch Edit): appends one operation of the
//!   catalogue (`edit::catalog()`) to each photo's recipe with the edit view's rules: a rotation
//!   right after a rotation merges into it and the same flip twice cancels
//!   ([`dalica_edit::Recipe::push`]), a `tuning` right after a `tuning` replaces it (Tuning merges
//!   into a trailing Tuning), and a crop is appended (only the last crop counts when the photo is
//!   drawn). Each photo's redo list is cleared, as any new edit clears it. All the photos change
//!   in **one library undo step** named by the operation ("Undo Auto Fix"). An operation this
//!   build cannot draw yet is disabled with its reason ("Arrives with Effects").
//! - **`edit.copyRecipe {id}`** (Edit ▸ Copy All Edits): keeps the photo's whole recipe, verbatim,
//!   for this session (nothing is written).
//! - **`edit.pasteRecipe {ids}`** (Edit ▸ Paste All Edits in the library): replaces each photo's
//!   recipe with the copied one, verbatim (an operation Dalica cannot read comes along unchanged),
//!   as one library step ("Undo Paste All Edits"). **`{id, history: true}`** (the edit view) is one
//!   step of that photo's own history instead. **Nothing a photo holds is dropped silently**, as
//!   with `edit.recipe.set`: a photo holding an entry Dalica cannot read (a `legacy:` entry, an
//!   operation from a newer version, refused parameters) that the copied recipe lacks keeps its
//!   recipe; the library form skips it and names it with its reason under `skipped`, as it does
//!   videos, and the history form is refused.
//! - **`edit.undoAll {ids, confirm}`** (Picture ▸ Undo All Edits…): asks first (without
//!   `"confirm": true` it refuses with the question, `needs_confirm` over automation), then empties
//!   every unsaved recipe (and its redo list) as one library step ("Undo Remove Edits");
//!   **`{id, history: true, confirm}`** is one step of that photo's history. A saved photo keeps
//!   its saved pixels (Undo Save is the way back to its original) and is named in the answer.
//!
//! **The library step** is named by the change ([`Session::name_step`]: "Undo Auto Fix", "Undo
//! Paste All Edits", "Undo Remove Edits"; inside [`Session::execute_group`] the group names its one
//! step). `pasteRecipe` and `undoAll` are declared [`History::Photo`] because their `history` form
//! is a photo step; naming the step makes their library form a library step.

use dalica_core::PhotoId;
use dalica_edit::{Op as EditOp, Recipe as EditRecipe};
use dalica_library::{Op, Recipe};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::history::{history_of, state_json};
use crate::cmd::photo::known;
use crate::error::CmdError;
use crate::history::{Change, label};
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

/// Why an effect is greyed until it renders (`editing.md` §9).
pub const ARRIVES_WITH_EFFECTS: &str = "Arrives with Effects";
/// Why video trimming is greyed until it renders.
pub const ARRIVES_WITH_VIDEO: &str = "Arrives with video editing";
/// Why any other operation that does not render yet is greyed.
pub const ARRIVES_LATER: &str = "Arrives in a later version";
/// Paste All Edits with nothing copied.
pub const NOTHING_COPIED: &str = "Copy a photo's edits first (Edit ▸ Copy All Edits).";
/// Copy All Edits on a photo without edits.
pub const NO_EDITS_TO_COPY: &str = "The photo has no edits to copy.";
/// Undo All Edits on photos without edits.
pub const NO_EDITS: &str = "None of these photos has edits to remove.";
/// The library step of Paste All Edits ("Undo Paste All Edits").
pub const PASTE_LABEL: &str = "Paste All Edits";
/// The library step of Undo All Edits ("Undo Remove Edits").
pub const REMOVE_LABEL: &str = "Remove Edits";
/// How many saved photos the question names, and how many entries a refusal names.
const NAMED: usize = 4;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "edit.apply",
            label: "Batch Edit",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "op":name | "name(params)", "params"?:"text between the parentheses" (the catalogue's defaults when left out)}: appends the operation to each photo's recipe with the edit view's rules (a rotation after a rotation merges, the same flip twice cancels, a tuning after a tuning replaces it), clearing each redo list, as one library undo step named by the operation ("Undo Auto Fix"); an operation this build cannot draw yet is disabled with its reason ("Arrives with Effects"); answers {ids (the photos changed), operation, label}"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: apply_enabled,
            run: apply,
        },
        CommandSpec {
            id: "edit.copyRecipe",
            label: "Copy All Edits",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: keeps the photo's whole recipe, verbatim, for Paste All Edits (this session only; nothing is written); disabled for a photo without edits; answers {id, recipe:[entry]}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: copy_enabled,
            run: copy,
        },
        CommandSpec {
            id: "edit.pasteRecipe",
            label: "Paste All Edits",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId]} | {"id":PhotoId, "history":true}: replaces each photo's recipe with the copied one, verbatim, clearing its redo list: as one library undo step ("Undo Paste All Edits"; videos skipped), or with "history": true as one step of that photo's own history (the edit view); disabled until a recipe is copied; a photo holding an entry Dalica cannot read (legacy:, from a newer version, refused parameters) that the copied recipe lacks keeps its recipe and is skipped with its reason (the history form is refused); answers {ids (changed), skipped:[{id, reason}] (videos, unreadable entries)} or, with history, the photo's history as photo.history.state does"#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: paste_enabled,
            run: paste,
        },
        CommandSpec {
            id: "edit.undoAll",
            label: "Undo All Edits",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "confirm"?:bool} | {"id":PhotoId, "history":true, "confirm"?:bool}: removes every unsaved edit (each recipe and redo list emptied) as one library undo step ("Undo Remove Edits"), or with "history": true as one step of that photo's own history; asks first: needs "confirm": true; a saved photo keeps its saved pixels (photo.undoSave gives its original back); answers {ids (changed), saved:[{id, name}]} (with history, also the photo's history)"#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: undo_all_enabled,
            run: undo_all,
        },
    ]
}

// ----- Shared helpers -----

/// The recipe copied in `s`, if any.
pub fn copied(s: &Session) -> Option<Vec<String>> {
    s.copied_recipe.clone()
}

/// Sets `recipes` as one library undo step named `name` ("Undo `name`").
fn library_step(s: &mut Session, name: &'static str, recipes: Vec<(PhotoId, Recipe)>) -> Result<(), CmdError> {
    if recipes.is_empty() {
        return Ok(());
    }
    s.name_step(name);
    s.apply(Op::SetRecipes { recipes }).map(|_| ())
}

/// The photos `ids` names, all known, at least one.
fn photos(s: &Session, ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    known(s, ids)
}

/// `ids` or `{id, history: true}`: the photos and whether the change is a photo step.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    #[serde(default)]
    ids: Option<Vec<PhotoId>>,
    #[serde(default)]
    id: Option<PhotoId>,
    #[serde(default)]
    history: bool,
    #[serde(default)]
    confirm: bool,
}

/// What a [`Target`] stands for.
enum Mode {
    Library(Vec<PhotoId>),
    Photo(PhotoId),
}

impl Target {
    fn mode(&self, s: &Session) -> Result<Mode, CmdError> {
        match (&self.ids, self.id, self.history) {
            (Some(ids), None, false) => Ok(Mode::Library(photos(s, ids.clone())?)),
            (None, Some(id), true) => {
                history_of(s, id)?;
                Ok(Mode::Photo(id))
            }
            (_, _, true) => Err(CmdError::BadParams("\"history\": true takes one photo as \"id\" (and no \"ids\")".into())),
            _ => Err(CmdError::BadParams("give \"ids\" (a library step), or \"id\" with \"history\": true (a step of the photo's history)".into())),
        }
    }
}

/// The photo ids `p` names, as far as `enabled` can tell (`ids`, or `id`).
fn named_ids(p: &Value) -> Vec<PhotoId> {
    let one = p.get("id").and_then(Value::as_u64).map(PhotoId::new);
    let many = p.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(PhotoId::new).collect::<Vec<_>>());
    many.unwrap_or_else(|| one.into_iter().collect())
}

// ----- edit.apply -----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    ids: Vec<PhotoId>,
    op: String,
    #[serde(default)]
    params: Option<String>,
}

/// The operation name `op` starts with (`autofix`, `rotate(90)` → `rotate`).
fn op_name(op: &str) -> &str {
    op.split('(').next().unwrap_or(op).trim()
}

/// Why operation `name` is greyed in this build, if it is: a catalogue operation that does not
/// render yet.
pub fn not_yet(name: &str) -> Option<&'static str> {
    let row = dalica_edit::catalog::lookup(name)?;
    if row.renders() {
        return None;
    }
    Some(match row.milestone {
        "M12" => ARRIVES_WITH_EFFECTS,
        "M15" => ARRIVES_WITH_VIDEO,
        _ => ARRIVES_LATER,
    })
}

fn apply_enabled(_: &Session, p: &Value) -> Result<(), Disabled> {
    if p.get("ids").and_then(Value::as_array).is_some_and(Vec::is_empty) {
        return Err(Disabled("Select some photos first.".into()));
    }
    match p.get("op").and_then(Value::as_str).and_then(|op| not_yet(op_name(op))) {
        Some(why) => Err(Disabled(why.into())),
        None => Ok(()),
    }
}

/// The operation `edit.apply` was given: known, drawable, its parameters taken by its module.
fn operation(op: &str, params: Option<&str>) -> Result<EditOp, CmdError> {
    let limit = dalica_library::MAX_OPERATION;
    if op.len() > limit || params.is_some_and(|p| p.len() > limit) {
        return Err(CmdError::Limit(format!("an operation is at most {limit} bytes")));
    }
    let shown = |t: &str| format!("{:?}", t.chars().take(64).collect::<String>());
    let name = op_name(op);
    let row = dalica_edit::catalog::lookup(name).ok_or_else(|| CmdError::BadParams(format!("op: not an operation Dalica applies: {}", shown(op))))?;
    if let Some(why) = not_yet(name) {
        return Err(CmdError::Disabled(why.into()));
    }
    let text = match (op.contains('('), params) {
        (true, None) => op.to_string(),
        (true, Some(_)) => return Err(CmdError::BadParams("op: give the parameters in \"op\" or in \"params\", not both".into())),
        (false, Some(p)) => format!("{name}({p})"),
        (false, None) => row.default_entry().ok_or_else(|| CmdError::BadParams(format!("op: {name} needs \"params\" ({})", row.syntax)))?,
    };
    let parsed = EditOp::parse(&text);
    if !parsed.is_known() || parsed.name() != Some(row.name) {
        return Err(CmdError::BadParams(format!("op: not an operation Dalica applies: {}", shown(&text))));
    }
    if let Some(Err(e)) = parsed.typed() {
        return Err(CmdError::BadParams(format!("op: {e}")));
    }
    Ok(parsed)
}

/// `edits` with `op` appended by the edit view's rules (see the module).
fn appended(id: PhotoId, edits: &[String], op: &EditOp) -> Result<Vec<String>, CmdError> {
    let limit = |e: dalica_edit::Error| CmdError::Limit(format!("photo {id}: {e}"));
    let trailing_tuning = edits.last().is_some_and(|last| EditOp::parse(last).name() == Some("tuning"));
    if op.name() == Some("tuning") && trailing_tuning {
        let mut out = edits.to_vec();
        if let Some(last) = out.last_mut() {
            *last = op.to_string();
        }
        return Ok(out);
    }
    let mut recipe = EditRecipe::parse(edits).map_err(limit)?;
    recipe.push(op.clone()).map_err(limit)?;
    Ok(recipe.print())
}

fn apply(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Apply { ids, op, params: given } = params(p)?;
    let ids = photos(s, ids)?;
    let op = operation(&op, given.as_deref())?;
    let mut recipes = Vec::new();
    for id in &ids {
        let info = s.library().info(*id);
        let edits = appended(*id, &info.edits, &op)?;
        if edits != info.edits || !info.redo.is_empty() {
            recipes.push((*id, Recipe { edits, redo: Vec::new() }));
        }
    }
    let changed: Vec<PhotoId> = recipes.iter().map(|(id, _)| *id).collect();
    let name = label(&op);
    library_step(s, name, recipes)?;
    Ok(json!({ "ids": changed, "operation": op.to_string(), "label": name }))
}

// ----- edit.copyRecipe -----

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
}

fn copy_enabled(s: &Session, p: &Value) -> Result<(), Disabled> {
    match p.get("id").and_then(Value::as_u64).map(PhotoId::new) {
        Some(id) if s.library().photo(id).is_some() && s.library().info(id).edits.is_empty() => Err(Disabled(NO_EDITS_TO_COPY.into())),
        _ => Ok(()),
    }
}

fn copy(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id } = params(p)?;
    history_of(s, id)?;
    let recipe = s.library().info(id).edits.clone();
    if recipe.is_empty() {
        return Err(CmdError::Disabled(NO_EDITS_TO_COPY.into()));
    }
    s.copied_recipe = Some(recipe.clone());
    Ok(json!({ "id": id, "recipe": recipe }))
}

// ----- edit.pasteRecipe -----

fn paste_enabled(s: &Session, _: &Value) -> Result<(), Disabled> {
    match copied(s) {
        Some(_) => Ok(()),
        None => Err(Disabled(NOTHING_COPIED.into())),
    }
}

fn is_video(s: &Session, id: PhotoId) -> bool {
    s.library().facts(id).video
}

/// Why Paste may not replace a recipe of `video` holding `held` with `new`, if it may not: a video,
/// or an entry Dalica cannot read (`legacy:`, an operation from a newer version, parameters its
/// module refuses) that the photo holds and the copied recipe lacks (each counted as often as it
/// stands), as `edit.recipe.set` refuses it.
fn paste_refusal(video: bool, held: &[String], new: &[String]) -> Option<String> {
    if video {
        return Some("Edits are not pasted onto videos.".into());
    }
    let readable = |e: &str| {
        let op = EditOp::parse(e);
        op.is_known() && !matches!(op.typed(), Some(Err(_)))
    };
    let mut kept: Vec<&String> = new.iter().filter(|e| !readable(e)).collect();
    let mut dropped = Vec::new();
    for entry in held.iter().filter(|e| !readable(e)) {
        match kept.iter().position(|k| *k == entry) {
            Some(i) => {
                kept.swap_remove(i);
            }
            None => dropped.push(format!("{:?}", entry.chars().take(64).collect::<String>())),
        }
    }
    if dropped.is_empty() {
        return None;
    }
    let total = dropped.len();
    let mut named: Vec<String> = dropped.into_iter().take(NAMED).collect();
    if total > NAMED {
        named.push(format!("and {} more", total - NAMED));
    }
    Some(format!("the photo holds operations Dalica cannot read that the copied edits lack: {}; its recipe is kept", named.join(", ")))
}

fn paste(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let target: Target = params(p)?;
    if target.confirm {
        return Err(CmdError::BadParams("confirm: Paste All Edits does not ask".into()));
    }
    let mode = target.mode(s)?;
    let recipe = copied(s).ok_or_else(|| CmdError::Disabled(NOTHING_COPIED.into()))?;
    match mode {
        Mode::Photo(id) => {
            if is_video(s, id) {
                return Err(CmdError::Disabled("Edits are not pasted onto videos.".into()));
            }
            if let Some(why) = paste_refusal(false, &s.library().info(id).edits, &recipe) {
                return Err(CmdError::BadParams(format!("photo {id}: {why}")));
            }
            s.photo_step(id, Change::Replace(recipe), None)?;
            Ok(state_json(id, &history_of(s, id)?))
        }
        Mode::Library(ids) => {
            let mut skipped = Vec::new();
            let mut recipes = Vec::new();
            for id in ids {
                let info = s.library().info(id);
                if let Some(why) = paste_refusal(is_video(s, id), &info.edits, &recipe) {
                    skipped.push(json!({ "id": id, "reason": why }));
                } else if info.edits != recipe || !info.redo.is_empty() {
                    recipes.push((id, Recipe { edits: recipe.clone(), redo: Vec::new() }));
                }
            }
            let changed: Vec<PhotoId> = recipes.iter().map(|(id, _)| *id).collect();
            library_step(s, PASTE_LABEL, recipes)?;
            Ok(json!({ "ids": changed, "skipped": skipped }))
        }
    }
}

// ----- edit.undoAll -----

fn undo_all_enabled(s: &Session, p: &Value) -> Result<(), Disabled> {
    let ids = named_ids(p);
    let all_known = !ids.is_empty() && ids.len() <= crate::cmd::photo::MAX_IDS && ids.iter().all(|id| s.library().photo(*id).is_some());
    if all_known && ids.iter().all(|id| s.library().info(*id).edits.is_empty()) {
        return Err(Disabled(NO_EDITS.into()));
    }
    Ok(())
}

/// The question Undo All Edits asks before it removes the edits of `n` photos, naming `saved`.
pub fn question(n: usize, saved: &[String]) -> String {
    let what = if n == 1 { "1 picture".to_string() } else { format!("{n} pictures") };
    let mut q = format!("Remove all edits from {what}?");
    if !saved.is_empty() {
        let mut names: Vec<String> = saved.iter().take(NAMED).cloned().collect();
        if saved.len() > NAMED {
            names.push(format!("{} more", saved.len() - NAMED));
        }
        q.push_str(&format!(" Saved pictures keep their saved pixels: {} (Undo Save gives the original back).", names.join(", ")));
    }
    q
}

fn undo_all(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let target: Target = params(p)?;
    let mode = target.mode(s)?;
    let ids = match &mode {
        Mode::Library(ids) => ids.clone(),
        Mode::Photo(id) => vec![*id],
    };
    let edited: Vec<PhotoId> = ids.iter().copied().filter(|id| !s.library().info(*id).edits.is_empty()).collect();
    if edited.is_empty() {
        return Err(CmdError::Disabled(NO_EDITS.into()));
    }
    let saved: Vec<(PhotoId, String)> =
        ids.iter().filter(|id| s.library().saved(**id).is_some()).filter_map(|id| s.library().photo(*id).map(|p| (*id, p.name.clone()))).collect();
    if !target.confirm {
        let names: Vec<String> = saved.iter().map(|(_, n)| n.clone()).collect();
        return Err(CmdError::Disabled(format!("{} Send \"confirm\": true to go ahead.", question(edited.len(), &names))));
    }
    let saved_json: Vec<Value> = saved.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
    match mode {
        Mode::Photo(id) => {
            s.photo_step(id, Change::Replace(Vec::new()), None)?;
            let mut state = state_json(id, &history_of(s, id)?);
            if let Some(o) = state.as_object_mut() {
                o.insert("ids".into(), json!([id]));
                o.insert("saved".into(), json!(saved_json));
            }
            Ok(state)
        }
        Mode::Library(_) => {
            let recipes = edited.iter().map(|id| (*id, Recipe::default())).collect();
            library_step(s, REMOVE_LABEL, recipes)?;
            Ok(json!({ "ids": edited, "saved": saved_json }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_question_counts_and_names_the_saved_photos() {
        assert_eq!(question(1, &[]), "Remove all edits from 1 picture?");
        let names: Vec<String> = (1..=6).map(|i| format!("IMG_{i}.jpg")).collect();
        let q = question(7, &names);
        assert!(q.starts_with("Remove all edits from 7 pictures? Saved pictures keep"), "{q}");
        assert!(q.contains("IMG_4.jpg, 2 more"), "{q}");
    }

    #[test]
    fn a_tuning_replaces_a_trailing_tuning_and_rotations_merge() {
        let id = PhotoId::new(1);
        let t = EditOp::parse;
        let edits = vec!["autofix()".to_string(), "tuning(0.2,0,0,0,none)".to_string()];
        assert_eq!(appended(id, &edits, &t("tuning(0.5,0,0,0,none)")).unwrap(), ["autofix()", "tuning(0.5,0,0,0,none)"]);
        let edits = vec!["tuning(0.2,0,0,0,none)".to_string(), "rotate(90)".to_string()];
        assert_eq!(appended(id, &edits, &t("tuning(0.5,0,0,0,none)")).unwrap().len(), 3, "only a trailing tuning merges");
        assert_eq!(appended(id, &edits, &t("rotate(90)")).unwrap(), ["tuning(0.2,0,0,0,none)", "rotate(180)"]);
    }
}
