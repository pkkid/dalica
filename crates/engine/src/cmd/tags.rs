//! Tags and quick tags (`organizing.md` §5.2, `menus-and-shortcuts.md` §1.9, §2.2, §4):
//! `photo.tags.add` / `.remove` / `.toggle`, `tags.list`, `tags.rename`, `tags.quick.get` / `.set` /
//! `.apply`, and `tags.quick.1` … `tags.quick.10` (Ctrl+Alt+1 … Ctrl+Alt+0 in the library).
//!
//! **Tag text.** Typed text is split at commas (`photo.tags.add` / `.remove` given one string; a
//! list gives whole tags, so a tag from a file may hold a comma); each tag has control characters
//! made spaces and is trimmed; empty ones are dropped and a tag equal to an earlier one ignoring
//! case is dropped (the case rule of `dalica_library::tags`). A tag past [`MAX_TAG`] bytes is
//! refused (`limit`), never cut. Errors never carry tag text (`security.md` §4.4).
//!
//! **One step.** Each command that changes tags applies one library op, so it is one undo step
//! however many photos it touches, and a command that would change nothing records no step.
//!
//! **Quick tags.** Ten slots kept in settings (`tags.quick`); with "Use the top two buttons for
//! recently used tags" (`useRecent`, on) slots 1 and 2 show the two most recently added tags that
//! are not in slots 3–10, and their stored tags are not used. Applying a quick tag toggles it on
//! the photos. Recently used tags are the ones added through `photo.tags.add` (the Tags pane's add
//! field and its tag menu), noted most recent first with the case rule applied; toggling, quick
//! tags and renames do not reorder them, so the buttons stay where they are while used.

use std::collections::HashSet;

use dalica_core::PhotoId;
use dalica_library::tags::fold;
use dalica_library::{MAX_TAG, MAX_TAGS, Op, PartialCount, TagCount};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::CmdError;
use crate::registry::{CommandSpec, Context, Disabled, Effect, History, always, params};
use crate::session::Session;
use crate::settings::{MAX_RECENT_TAGS, QUICK_TAGS, QuickTags};

/// The most photo ids one command takes.
pub const MAX_IDS: usize = 100_000;
/// The most tags `tags.list` gives for a `prefix` without `ids` (autocomplete).
pub const MAX_COMPLETIONS: usize = 100;
/// The quick tags that follow the recently used tags when `useRecent` is on (slots 1 and 2).
pub const RECENT_SLOTS: usize = 2;
/// The most comma-separated pieces or list entries one `tags` parameter may hold.
const MAX_PIECES: usize = 10_000;

const SELECT_FIRST: &str = "Select some photos first.";

/// One keyed quick-tag command: slot `$n`, default key `$key` in the library.
macro_rules! quick_slot {
    ($n:literal, $key:literal) => {
        CommandSpec {
            id: concat!("tags.quick.", $n),
            label: concat!("Apply Quick Tag ", $n),
            menu: "",
            keys: &[(Context::Library, $key)],
            params: concat!(r#"{"ids":[PhotoId]}: toggles quick tag "#, $n, r#" on the photos (tags.quick.apply with "n":"#, $n, ")"),
            effect: Effect::Library,
            history: History::Library,
            enabled: |s, p| slot_enabled(s, p, $n),
            run: |s, p| {
                let Ids { ids } = params(p)?;
                apply_slot(s, ids, $n)
            },
        }
    };
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.tags.add",
            label: "Add Tags",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "tags":string (commas separate tags)|[string]}: adds the tags to every photo lacking them (ignoring case; a tag in use keeps its spelling); one undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: add,
        },
        CommandSpec {
            id: "photo.tags.remove",
            label: "Remove Tags",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "tags":string (commas separate tags)|[string]}: removes the tags (ignoring case) from the photos; one undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: remove,
        },
        CommandSpec {
            id: "photo.tags.toggle",
            label: "Toggle Tag",
            menu: "",
            keys: &[],
            params: r#"{"ids":[PhotoId], "tag":string}: removes the tag from all the photos when all hold it, else adds it to all; one undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: toggle,
        },
        CommandSpec {
            id: "tags.list",
            label: "List Tags",
            menu: "",
            keys: &[],
            params: r#"{"ids"?:[PhotoId], "prefix"?:string}: query. Without ids: every tag with its count ({"tags":[{"tag","count"}]}), with prefix the first 100 starting with it (ignoring case). With ids: the tags of those photos ({"photos":n, "tags":[{"tag","count","of","partial","label"}]}, label "beach (3 of 8)"), prefix filtering them"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: list,
        },
        CommandSpec {
            id: "tags.rename",
            label: "Rename Tag",
            menu: "",
            keys: &[],
            params: r#"{"from":string, "to":string, "confirm":bool}: renames the tag (ignoring case) on every photo; onto an existing tag the two merge, keeping its spelling; asks first: needs "confirm": true; one undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: always,
            run: rename,
        },
        CommandSpec {
            id: "tags.quick.get",
            label: "Quick Tags",
            menu: "",
            keys: &[],
            params: r#"{}: query: {"slots":[string|null] (10, as stored), "useRecent":bool, "shown":[string|null] (10, what each button applies: slots 1–2 follow recent tags when useRecent)}"#,
            effect: Effect::Query,
            history: History::None,
            enabled: always,
            run: quick_get,
        },
        CommandSpec {
            id: "tags.quick.set",
            label: "Configure Quick Tags",
            menu: "",
            keys: &[],
            params: r#"{"slots"?:[string|null] (at most 10; "" or null unassigned, missing ones unassigned), "useRecent"?:bool}: at least one; answers as tags.quick.get"#,
            effect: Effect::Library,
            history: History::None,
            enabled: always,
            run: quick_set,
        },
        CommandSpec {
            id: "tags.quick.apply",
            label: "Apply Quick Tag",
            menu: "",
            keys: &[],
            params: r#"{"n":1–10, "ids":[PhotoId]}: toggles quick tag n (as tags.quick.get shows it) on the photos; one undo step"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: quick_apply_enabled,
            run: quick_apply,
        },
        quick_slot!(1, "Ctrl+Alt+1"),
        quick_slot!(2, "Ctrl+Alt+2"),
        quick_slot!(3, "Ctrl+Alt+3"),
        quick_slot!(4, "Ctrl+Alt+4"),
        quick_slot!(5, "Ctrl+Alt+5"),
        quick_slot!(6, "Ctrl+Alt+6"),
        quick_slot!(7, "Ctrl+Alt+7"),
        quick_slot!(8, "Ctrl+Alt+8"),
        quick_slot!(9, "Ctrl+Alt+9"),
        quick_slot!(10, "Ctrl+Alt+0"),
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ids {
    ids: Vec<PhotoId>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdsTags {
    ids: Vec<PhotoId>,
    tags: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdsTag {
    ids: Vec<PhotoId>,
    tag: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct List {
    ids: Option<Vec<PhotoId>>,
    prefix: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rename {
    from: String,
    to: String,
    #[serde(default)]
    confirm: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuickSet {
    slots: Option<Vec<Value>>,
    #[serde(rename = "useRecent")]
    use_recent: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuickApply {
    n: u64,
    ids: Vec<PhotoId>,
}

/// Disabled with "Select some photos first." when `ids` is present and empty. Malformed params
/// are left to `run`, which reports them as bad parameters.
fn has_ids(_: &Session, p: &Value) -> Result<(), Disabled> {
    match p.get("ids").and_then(Value::as_array) {
        Some(ids) if ids.is_empty() => Err(Disabled(SELECT_FIRST.into())),
        _ => Ok(()),
    }
}

/// [`has_ids`], and disabled while quick tag `n` shows no tag.
fn slot_enabled(s: &Session, p: &Value, n: usize) -> Result<(), Disabled> {
    has_ids(s, p)?;
    match quick_tags_shown(s).get(n.wrapping_sub(1)) {
        Some(Some(_)) => Ok(()),
        _ => Err(Disabled(unassigned(n))),
    }
}

/// [`slot_enabled`] for the `n` given, when it is a slot number (anything else is left to `run`).
fn quick_apply_enabled(s: &Session, p: &Value) -> Result<(), Disabled> {
    match p.get("n").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()) {
        Some(n) if (1..=QUICK_TAGS).contains(&n) => slot_enabled(s, p, n),
        _ => has_ids(s, p),
    }
}

fn unassigned(n: usize) -> String {
    format!("Quick tag {n} is not assigned.")
}

/// The ids, de-duplicated in order, all known; `NotFound` names the first unknown one.
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

/// [`known`], refusing an empty list as the status line does.
fn selection(s: &Session, ids: Vec<PhotoId>) -> Result<Vec<PhotoId>, CmdError> {
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled(SELECT_FIRST.into()));
    }
    Ok(ids)
}

/// One piece of typed text as a tag: control characters made spaces, trimmed; `None` when
/// nothing is left; `limit` past [`MAX_TAG`] bytes.
fn tag_of(piece: &str) -> Result<Option<String>, CmdError> {
    let spaced: String = piece.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let tag = spaced.trim();
    if tag.len() > MAX_TAG {
        return Err(CmdError::Limit(format!("a tag is at most {MAX_TAG} bytes")));
    }
    Ok((!tag.is_empty()).then(|| tag.to_string()))
}

/// A one-tag parameter (commas kept), not empty.
fn one_tag(text: &str, what: &str) -> Result<String, CmdError> {
    tag_of(text)?.ok_or_else(|| CmdError::BadParams(format!("`{what}` holds no tag")))
}

/// The `tags` parameter: text split at commas, or a list of whole tags; cleaned, de-duplicated by
/// the case rule (the first spelling kept), at least one and at most [`MAX_TAGS`].
fn tags_of(value: &Value) -> Result<Vec<String>, CmdError> {
    let pieces: Vec<&str> = match value {
        Value::String(text) => text.split(',').collect(),
        Value::Array(list) => {
            list.iter().map(|v| v.as_str().ok_or_else(|| CmdError::BadParams("`tags` entries must be text".into()))).collect::<Result<_, _>>()?
        }
        _ => return Err(CmdError::BadParams("`tags` must be text (commas separate tags) or a list of tags".into())),
    };
    if pieces.len() > MAX_PIECES {
        return Err(CmdError::Limit(format!("at most {MAX_TAGS} tags at once")));
    }
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for piece in pieces {
        if let Some(tag) = tag_of(piece)?
            && seen.insert(fold(&tag))
        {
            tags.push(tag);
        }
    }
    if tags.is_empty() {
        return Err(CmdError::BadParams("`tags` holds no tag".into()));
    }
    if tags.len() > MAX_TAGS {
        return Err(CmdError::Limit(format!("at most {MAX_TAGS} tags at once")));
    }
    Ok(tags)
}

/// The folded tags photo `id` holds.
fn folded(s: &Session, id: PhotoId) -> HashSet<String> {
    s.library().tags_of(id).iter().map(|t| fold(t)).collect()
}

/// The spelling `tag` has in the library now (the one in use), else as given.
fn spelling(s: &Session, tag: &str) -> String {
    s.library().tag_index().spelling(tag).unwrap_or(tag).to_string()
}

/// Adds `tags` to `ids` (one op), refusing (`limit`) when a photo would hold more than
/// [`MAX_TAGS`]. Returns how many photos changed.
fn add_to(s: &mut Session, ids: &[PhotoId], tags: &[String]) -> Result<usize, CmdError> {
    let keys: Vec<String> = tags.iter().map(|t| fold(t)).collect();
    let mut changed = 0usize;
    for &id in ids {
        let have = folded(s, id);
        let missing = keys.iter().filter(|k| !have.contains(*k)).count();
        if missing > 0 {
            if have.len().saturating_add(missing) > MAX_TAGS {
                return Err(CmdError::Limit(format!("a photo holds at most {MAX_TAGS} tags")));
            }
            changed += 1;
        }
    }
    if changed > 0 {
        s.apply(Op::AddTags { ids: ids.to_vec(), tags: tags.to_vec() })?;
    }
    Ok(changed)
}

/// Removes `tags` from `ids` (one op). Returns how many photos changed.
fn remove_from(s: &mut Session, ids: &[PhotoId], tags: &[String]) -> Result<usize, CmdError> {
    let keys: HashSet<String> = tags.iter().map(|t| fold(t)).collect();
    let changed = ids.iter().filter(|id| s.library().tags_of(**id).iter().any(|t| keys.contains(&fold(t)))).count();
    if changed > 0 {
        s.apply(Op::RemoveTags { ids: ids.to_vec(), tags: tags.to_vec() })?;
    }
    Ok(changed)
}

/// Notes `tags` (as spelled in the library) as the most recently used, applying the case rule:
/// an older entry equal to one of them ignoring case is dropped.
fn note_recent(s: &mut Session, tags: &[String]) {
    let before = s.settings().recent_tags();
    let mut seen = HashSet::new();
    let list: Vec<String> = tags.iter().chain(before.iter()).filter(|t| seen.insert(fold(t))).take(MAX_RECENT_TAGS).cloned().collect();
    if list != before {
        s.change_settings(|st| st.set_recent_tags(&list));
    }
}

fn add(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let IdsTags { ids, tags } = params(p)?;
    let ids = selection(s, ids)?;
    let tags = tags_of(&tags)?;
    let changed = add_to(s, &ids, &tags)?;
    let tags: Vec<String> = tags.iter().map(|t| spelling(s, t)).collect();
    note_recent(s, &tags);
    Ok(json!({ "ids": ids, "tags": tags, "changed": changed }))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let IdsTags { ids, tags } = params(p)?;
    let ids = selection(s, ids)?;
    let tags = tags_of(&tags)?;
    let changed = remove_from(s, &ids, &tags)?;
    Ok(json!({ "ids": ids, "tags": tags, "changed": changed }))
}

/// Toggles `tag` on `ids`: removed from all when all hold it, else added to all.
fn toggle_on(s: &mut Session, ids: Vec<PhotoId>, tag: String) -> Result<Value, CmdError> {
    let ids = selection(s, ids)?;
    let key = fold(&tag);
    let all = ids.iter().all(|id| s.library().tags_of(*id).iter().any(|t| fold(t) == key));
    let tags = [tag];
    let changed = if all { remove_from(s, &ids, &tags)? } else { add_to(s, &ids, &tags)? };
    let [tag] = tags;
    Ok(json!({ "ids": ids, "tag": spelling(s, &tag), "on": !all, "changed": changed }))
}

fn toggle(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let IdsTag { ids, tag } = params(p)?;
    let tag = one_tag(&tag, "tag")?;
    toggle_on(s, ids, tag)
}

fn count_json(c: &TagCount) -> Value {
    json!({ "tag": c.tag, "count": c.count })
}

fn partial_json(c: &PartialCount) -> Value {
    json!({ "tag": c.tag, "count": c.count, "of": c.of, "partial": c.is_partial(), "label": c.to_string() })
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let List { ids, prefix } = params(p)?;
    let key = prefix.as_deref().map(fold);
    match ids {
        Some(ids) => {
            let ids = known(s, ids)?;
            let counts = s.library().tag_counts_in(&ids);
            let tags: Vec<Value> =
                counts.iter().filter(|c| key.as_ref().is_none_or(|k| fold(&c.tag).starts_with(k.as_str()))).map(partial_json).collect();
            Ok(json!({ "photos": ids.len(), "tags": tags }))
        }
        None => {
            let index = s.library().tag_index();
            let counts = match &prefix {
                Some(prefix) => index.with_prefix(prefix, MAX_COMPLETIONS),
                None => index.counts(),
            };
            Ok(json!({ "tags": counts.iter().map(count_json).collect::<Vec<_>>() }))
        }
    }
}

fn rename(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Rename { from, to, confirm } = params(p)?;
    let from = one_tag(&from, "from")?;
    let to = one_tag(&to, "to")?;
    let (kf, kt) = (fold(&from), fold(&to));
    let respell = kf == kt;
    let target = if respell { to.clone() } else { spelling(s, &to) };
    // The photos holding `from`, how many of them already hold `to` (they merge), and whether
    // anything changes (a respelling to the spelling every photo has changes nothing).
    let (mut holders, mut merged, mut changes) = (0usize, 0usize, false);
    for (_, info) in s.library().infos() {
        let Some(old) = info.tags.iter().find(|t| fold(t) == kf) else { continue };
        holders += 1;
        if !respell && info.tags.iter().any(|t| fold(t) == kt) {
            merged += 1;
        }
        changes |= !respell || *old != target;
    }
    if holders == 0 {
        return Err(CmdError::NotFound("no photo holds that tag".into()));
    }
    if changes {
        if !confirm {
            let photos = if holders == 1 { "1 photo".to_string() } else { format!("{holders} photos") };
            let merging = if merged > 0 { " Photos that already hold the new name keep it once." } else { "" };
            return Err(CmdError::Disabled(format!("Rename Tag: Rename this tag on {photos}?{merging} Send \"confirm\": true to go ahead.")));
        }
        s.apply(Op::RenameTag { from, to })?;
    }
    Ok(json!({ "to": target, "photos": if changes { holders } else { 0 }, "merged": merged }))
}

/// What each quick tag button applies: the stored slots, with slots 1 and 2 following the most
/// recently used tags not in slots 3–10 when `useRecent` is on (empty while there are fewer).
pub fn quick_tags_shown(s: &Session) -> [Option<String>; QUICK_TAGS] {
    shown(&s.settings().quick_tags(), s.settings().recent_tags())
}

fn shown(q: &QuickTags, recent: Vec<String>) -> [Option<String>; QUICK_TAGS] {
    let mut shown = q.slots.clone();
    if q.use_recent {
        let mut seen: HashSet<String> = q.slots.iter().skip(RECENT_SLOTS).flatten().map(|t| fold(t)).collect();
        let mut recent = recent.into_iter().filter(|t| seen.insert(fold(t)));
        for slot in shown.iter_mut().take(RECENT_SLOTS) {
            *slot = recent.next();
        }
    }
    shown
}

fn quick_json(s: &Session) -> Value {
    let q = s.settings().quick_tags();
    json!({ "slots": q.slots, "useRecent": q.use_recent, "shown": shown(&q, s.settings().recent_tags()) })
}

fn quick_get(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let given: Map<String, Value> = params(p)?;
    if !given.is_empty() {
        return Err(CmdError::BadParams("`tags.quick.get` takes no parameters".into()));
    }
    Ok(quick_json(s))
}

fn quick_set(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let QuickSet { slots, use_recent } = params(p)?;
    if slots.is_none() && use_recent.is_none() {
        return Err(CmdError::BadParams("give `slots`, `useRecent` or both".into()));
    }
    let mut q = s.settings().quick_tags();
    if let Some(slots) = slots {
        if slots.len() > QUICK_TAGS {
            return Err(CmdError::BadParams(format!("at most {QUICK_TAGS} `slots`")));
        }
        let mut new: [Option<String>; QUICK_TAGS] = Default::default();
        for (slot, v) in new.iter_mut().zip(&slots) {
            *slot = match v {
                Value::Null => None,
                Value::String(text) => tag_of(text)?,
                _ => return Err(CmdError::BadParams("`slots` entries must be text or null".into())),
            };
        }
        q.slots = new;
    }
    if let Some(on) = use_recent {
        q.use_recent = on;
    }
    s.change_settings(|st| st.set_quick_tags(&q));
    Ok(quick_json(s))
}

/// Toggles quick tag `n` (1–10) on `ids`.
fn apply_slot(s: &mut Session, ids: Vec<PhotoId>, n: usize) -> Result<Value, CmdError> {
    let Some(text) = quick_tags_shown(s).get(n.wrapping_sub(1)).cloned().flatten() else {
        return Err(CmdError::Disabled(unassigned(n)));
    };
    let tag = one_tag(&text, "slot")?;
    let mut out = toggle_on(s, ids, tag)?;
    out["n"] = json!(n);
    Ok(out)
}

fn quick_apply(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let QuickApply { n, ids } = params(p)?;
    let n =
        usize::try_from(n).ok().filter(|n| (1..=QUICK_TAGS).contains(n)).ok_or_else(|| CmdError::BadParams(format!("`n` is 1 to {QUICK_TAGS}")))?;
    apply_slot(s, ids, n)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::cmd::testing::{hostile, library};

    fn tags(s: &Session, id: PhotoId) -> Vec<String> {
        s.library().tags_of(id).to_vec()
    }

    #[test]
    fn add_splits_at_commas_trims_and_dedups_by_case() {
        let (_dirs, mut s, ids) = library(2);
        let r = s.execute("photo.tags.add", &json!({ "ids": [ids[0]], "tags": " beach , Sunset,,BEACH, \tsun\nset " })).unwrap();
        assert_eq!(r["tags"], json!(["beach", "Sunset", "sun set"]));
        assert_eq!(r["changed"], json!(1));
        assert_eq!(tags(&s, ids[0]), ["beach", "Sunset", "sun set"]);
        // A tag in use keeps its spelling; a list gives whole tags (commas kept).
        let r = s.execute("photo.tags.add", &json!({ "ids": [ids[1]], "tags": ["SUNSET", "a, b"] })).unwrap();
        assert_eq!(r["tags"], json!(["Sunset", "a, b"]));
        assert_eq!(tags(&s, ids[1]), ["Sunset", "a, b"]);
        assert_eq!(s.undo_stack().len(), 2);
        // Adding what every photo holds changes nothing and records no step.
        let r = s.execute("photo.tags.add", &json!({ "ids": ids, "tags": "sunset" })).unwrap();
        assert_eq!(r["changed"], json!(0));
        assert_eq!(s.undo_stack().len(), 2);
    }

    #[test]
    fn graceful_failures_change_nothing() {
        let (_dirs, mut s, ids) = library(2);
        s.execute("photo.tags.add", &json!({ "ids": [ids[0]], "tags": "beach" })).unwrap();
        s.execute("tags.quick.set", &json!({ "slots": ["", "", "beach"], "useRecent": false })).unwrap();
        let (model, steps) = (s.library().model().clone(), s.undo_stack().len());
        let long = "x".repeat(MAX_TAG + 1);
        let refused = [
            ("photo.tags.add", json!({ "ids": [], "tags": "a" }), "disabled"),
            ("photo.tags.add", json!({ "ids": [999], "tags": "a" }), "not_found"),
            ("photo.tags.add", json!({ "ids": [ids[0]], "tags": " , ,," }), "bad_params"),
            ("photo.tags.add", json!({ "ids": [ids[0]], "tags": 7 }), "bad_params"),
            ("photo.tags.add", json!({ "ids": [ids[0]], "tags": ["a", 1] }), "bad_params"),
            ("photo.tags.add", json!({ "ids": [ids[0]], "tags": long }), "limit"),
            ("photo.tags.add", json!({ "ids": [ids[0]], "tags": ",".repeat(MAX_PIECES) }), "limit"),
            ("photo.tags.add", json!({ "ids": [ids[0]] }), "bad_params"),
            ("photo.tags.remove", json!({ "ids": [], "tags": "a" }), "disabled"),
            ("photo.tags.remove", json!({ "ids": [ids[0]], "tags": "" }), "bad_params"),
            ("photo.tags.toggle", json!({ "ids": [ids[0]], "tag": "  " }), "bad_params"),
            ("photo.tags.toggle", json!({ "ids": [ids[0]], "tags": "a" }), "bad_params"),
            ("photo.tags.toggle", json!({ "ids": [], "tag": "a" }), "disabled"),
            ("tags.list", json!({ "ids": [999] }), "not_found"),
            ("tags.list", json!({ "prefix": 3 }), "bad_params"),
            ("tags.rename", json!({ "from": "beach", "to": "shore" }), "disabled"),
            ("tags.rename", json!({ "from": "nothing", "to": "shore", "confirm": true }), "not_found"),
            ("tags.rename", json!({ "from": "beach", "to": "", "confirm": true }), "bad_params"),
            ("tags.rename", json!({ "from": "beach", "to": long, "confirm": true }), "limit"),
            ("tags.rename", json!({ "from": "beach", "to": "x", "confirm": "yes" }), "bad_params"),
            ("tags.quick.set", json!({}), "bad_params"),
            ("tags.quick.set", json!({ "slots": vec![""; 11] }), "bad_params"),
            ("tags.quick.set", json!({ "slots": [1] }), "bad_params"),
            ("tags.quick.set", json!({ "slots": [long] }), "limit"),
            ("tags.quick.set", json!({ "useRecent": "on" }), "bad_params"),
            ("tags.quick.get", json!({ "n": 1 }), "bad_params"),
            ("tags.quick.apply", json!({ "n": 0, "ids": [ids[0]] }), "bad_params"),
            ("tags.quick.apply", json!({ "n": 11, "ids": [ids[0]] }), "bad_params"),
            ("tags.quick.apply", json!({ "n": -1, "ids": [ids[0]] }), "bad_params"),
            ("tags.quick.apply", json!({ "n": 1, "ids": [ids[0]] }), "disabled"),
            ("tags.quick.apply", json!({ "n": 3, "ids": [] }), "disabled"),
            ("tags.quick.apply", json!({ "n": 3, "ids": [999] }), "not_found"),
            ("tags.quick.1", json!({ "ids": [ids[0]] }), "disabled"),
            ("tags.quick.3", json!({ "ids": [] }), "disabled"),
        ];
        for (id, p, code) in refused {
            let r = s.execute(id, &p);
            assert_eq!(r.as_ref().map_err(CmdError::code).err(), Some(code), "{id} {p}: {r:?}");
        }
        for spec in specs() {
            hostile(&mut s, spec.id);
        }
        assert_eq!(s.library().model(), &model);
        assert_eq!(s.undo_stack().len(), steps);
        // Errors never carry tag text.
        let r = s.execute("tags.rename", &json!({ "from": "beach", "to": "secret-name" }));
        assert!(matches!(&r, Err(CmdError::Disabled(m)) if !m.contains("beach") && !m.contains("secret")), "{r:?}");
    }

    #[test]
    fn recent_tags_follow_the_case_rule_and_fill_the_top_two_slots() {
        let (_dirs, mut s, ids) = library(1);
        s.execute("photo.tags.add", &json!({ "ids": ids, "tags": "Beach, tram" })).unwrap();
        s.execute("photo.tags.add", &json!({ "ids": ids, "tags": "BEACH" })).unwrap();
        // The spelling in use is noted, once, most recent first.
        assert_eq!(s.settings().recent_tags(), ["Beach", "tram"]);
        assert_eq!(quick_tags_shown(&s)[..2], [Some("Beach".to_string()), Some("tram".to_string())]);
        // A recent tag already in slots 3–10 is skipped.
        s.execute("tags.quick.set", &json!({ "slots": ["one", "two", "TRAM"] })).unwrap();
        assert_eq!(quick_tags_shown(&s)[..3], [Some("Beach".to_string()), None, Some("TRAM".to_string())]);
        // Without useRecent the stored slots show.
        let r = s.execute("tags.quick.set", &json!({ "useRecent": false })).unwrap();
        assert_eq!(r["shown"], json!(["one", "two", "TRAM", null, null, null, null, null, null, null]));
        assert_eq!(r["slots"], r["shown"]);
        assert_eq!(s.execute("tags.quick.get", &json!({})).unwrap(), r);
        s.execute("tags.quick.set", &json!({ "useRecent": true })).unwrap();
        // Toggling and quick tags never reorder the recent tags.
        s.execute("photo.tags.toggle", &json!({ "ids": ids, "tag": "tram" })).unwrap();
        s.execute("tags.quick.apply", &json!({ "n": 1, "ids": ids })).unwrap();
        assert_eq!(s.settings().recent_tags(), ["Beach", "tram"]);
        assert!(tags(&s, ids[0]).is_empty(), "{:?}", tags(&s, ids[0]));
    }
}
