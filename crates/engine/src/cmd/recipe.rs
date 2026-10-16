//! `edit.recipe.set {ids, recipe, label?, dropUnknown?}` (`mcp.md` §4.2 `set_recipe`,
//! `data-model.md` §3.1, `menus-and-shortcuts.md` §1.9): replaces photos' recipes, each as one step
//! of its own history ([`Change::Replace`]: one undo gives the whole recipe before it back). Paste
//! All Edits and agents use it.
//!
//! - `recipe` is the string form (entries joined by `|`, as `get_recipe` prints it; `""` is the
//!   empty recipe) or a list whose items are entries (`"crop(0.1,0.1,0.9,0.9)"`) or
//!   `{op, params?}` objects (`{"op": "crop", "params": "0.1,0.1,0.9,0.9"}`, `params` the text
//!   between the parentheses). Entries are stored verbatim, in order, without merging.
//! - **Nothing an operation cannot read is lost silently.** An entry Dalica cannot read (a
//!   `legacy:` entry, an unknown operation, or a known one whose module refuses its parameters) may
//!   stand in the new recipe only if the photo's recipe already holds it; one the photo holds and
//!   the new recipe leaves out is refused by name unless `dropUnknown` is true.
//! - **All or nothing:** every entry and every photo is checked before any photo changes.
//! - A photo whose recipe already is the new one takes no step. `label` is accepted for the
//!   caller's own use and not shown yet: the step is named by the new recipe's last entry
//!   ("Undo Sepia"; "Undo Edit" for an empty recipe), as every photo step is.

use dalica_core::PhotoId;
use dalica_edit::Op as EditOp;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::history::{history_of, state_json};
use crate::cmd::photo::{has_ids, known};
use crate::error::CmdError;
use crate::history::Change;
use crate::registry::{CommandSpec, Effect, History, params};
use crate::session::Session;

/// The longest `label`, in bytes.
pub const MAX_LABEL: usize = 256;
/// How many refused entries an error names.
const NAMED: usize = 4;

pub fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "edit.recipe.set",
        label: "Set Recipe",
        menu: "",
        keys: &[],
        params: r#"{"ids":[PhotoId], "recipe":"op(…)|op(…)" | ["op(…)" | {"op":name, "params"?:"text"}], "label"?:string, "dropUnknown"?:bool (false)}: replaces each photo's whole recipe, verbatim, as one step of its own history; refuses by name to drop an entry Dalica cannot read (legacy:, unknown, or refused parameters) unless dropUnknown, and to add one the photo does not hold; every photo and entry is checked before any changes; answers {"photos":[photo.history.state + "changed"]}"#,
        effect: Effect::Library,
        history: History::Photo,
        enabled: has_ids,
        run: set,
    }]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Set {
    ids: Vec<PhotoId>,
    recipe: RecipeParam,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    drop_unknown: bool,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum RecipeParam {
    Text(String),
    List(Vec<Entry>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Text(String),
    Op(OpEntry),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OpEntry {
    op: String,
    #[serde(default)]
    params: String,
}

/// The entries `recipe` stands for, within the caps.
fn entries(recipe: RecipeParam) -> Result<Vec<String>, CmdError> {
    let list: Vec<String> = match recipe {
        RecipeParam::Text(t) if t.is_empty() => Vec::new(),
        RecipeParam::Text(t) => t.split('|').map(str::to_string).collect(),
        RecipeParam::List(items) => items
            .into_iter()
            .map(|e| match e {
                Entry::Text(t) => t,
                Entry::Op(OpEntry { op, params }) => format!("{op}({params})"),
            })
            .collect(),
    };
    if list.len() > dalica_edit::MAX_OPERATIONS {
        return Err(CmdError::Limit(format!("a recipe holds at most {} operations", dalica_edit::MAX_OPERATIONS)));
    }
    if list.iter().any(|e| e.len() > dalica_library::MAX_OPERATION) {
        return Err(CmdError::Limit(format!("an operation is at most {} bytes", dalica_library::MAX_OPERATION)));
    }
    Ok(list)
}

/// Whether Dalica can read `entry`: a known operation whose module (if it has one) takes its
/// parameters.
fn readable(entry: &str) -> bool {
    let op = EditOp::parse(entry);
    op.is_known() && !matches!(op.typed(), Some(Err(_)))
}

/// `entry` for a message: quoted, at most 64 characters.
fn shown(entry: &str) -> String {
    format!("{:?}", entry.chars().take(64).collect::<String>())
}

/// `reasons` for a message: the first few, then how many more.
fn first_few(reasons: impl ExactSizeIterator<Item = String>) -> String {
    let total = reasons.len();
    let mut out: Vec<String> = reasons.take(NAMED).collect();
    if total > NAMED {
        out.push(format!("and {} more", total - NAMED));
    }
    out.join("; ")
}

/// Checks `new` against photo `id`'s recipe `old`: the unreadable entries of `new` must come from
/// `old` (each at most as often), and those of `old` must all stay unless `drop_unknown`.
fn check(id: PhotoId, old: &[String], new: &[String], drop_unknown: bool) -> Result<(), CmdError> {
    let mut held: Vec<&String> = old.iter().filter(|e| !readable(e)).collect();
    let mut added = Vec::new();
    for entry in new.iter().filter(|e| !readable(e)) {
        match held.iter().position(|h| *h == entry) {
            Some(i) => {
                held.swap_remove(i);
            }
            None => added.push(entry),
        }
    }
    if !added.is_empty() {
        let why = added.iter().map(|e| match EditOp::parse(e).typed() {
            Some(Err(err)) => err.to_string(),
            _ => format!("{} is not an operation Dalica applies", shown(e)),
        });
        return Err(CmdError::BadParams(format!("recipe: {}", first_few(why))));
    }
    if !held.is_empty() && !drop_unknown {
        return Err(CmdError::BadParams(format!(
            "photo {id}: the new recipe drops operations Dalica cannot read: {}; pass \"dropUnknown\": true to drop them",
            first_few(held.iter().map(|e| shown(e)))
        )));
    }
    Ok(())
}

fn set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Set { ids, recipe, label, drop_unknown } = params(p)?;
    if label.as_ref().is_some_and(|l| l.len() > MAX_LABEL) {
        return Err(CmdError::Limit(format!("label: at most {MAX_LABEL} bytes")));
    }
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let ids = known(s, ids)?;
    let new = entries(recipe)?;
    // Every photo is checked before any changes.
    for id in &ids {
        check(*id, &s.library().info(*id).edits, &new, drop_unknown)?;
    }
    let mut photos = Vec::with_capacity(ids.len());
    for id in ids {
        let changed = s.photo_step(id, Change::Replace(new.clone()), None)?.is_some();
        let mut state = state_json(id, &history_of(s, id)?);
        if let Some(o) = state.as_object_mut() {
            o.insert("changed".into(), json!(changed));
        }
        photos.push(state);
    }
    Ok(json!({ "photos": photos }))
}
