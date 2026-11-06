//! The photo's own history (`architecture.md` §4, `editing.md` §5.1): `photo.history.apply`,
//! `photo.history.undo`, `photo.history.redo` and the `photo.history.state` query (which names
//! Undo Save for a saved photo with no newer edits, M8.15); the History pane's `photo.history` query
//! (every step as a row, the current one marked) and `photo.history.goto` (M16.33). Each takes the
//! photo explicitly; in the edit view the UI routes Edit ▸ Undo / Redo, the rotations, the flips
//! and the one-click fixes here for the open photo. A photo step is never a library undo step.
//! The rules (merging, the redo list, the labels) are in [`crate::history`]. With `"variant": true`
//! each walks A│A's variant of the photo instead (`variant.rs`, M11.10): the same rules over a recipe
//! held in the session only, which no sidecar sees until `edit.variant.close {keep: "right"}`.

use dalica_core::PhotoId;
use dalica_edit::Op as EditOp;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::CmdError;
use crate::history::{Change, PhotoHistory, Side};
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.history.apply",
            label: "Apply Edit",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "op":"name(params)", "variant"?:bool (false)}: one step of the photo's own history: the operation appended to its recipe (a rotation right after a rotation merges into it, the same flip twice cancels), its redo list cleared; with variant, a step of the photo's open A│A variant instead (edit.variant.open), which reaches no sidecar; answers the history as photo.history.state does"#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: crate::registry::always,
            run: apply,
        },
        CommandSpec {
            id: "photo.history.undo",
            label: "Undo Edit",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "variant"?:bool (false)}: undo the photo's last edit (it moves to the redo list; a merged step gives back the entry it merged into); with variant, the open A│A variant's last edit"#,
            effect: Effect::Library,
            history: History::None,
            enabled: |s, p| match target(s, p) {
                Some(h) if h.undo_label.is_none() => Err(Disabled("Nothing to undo.".into())),
                _ => Ok(()),
            },
            run: undo,
        },
        CommandSpec {
            id: "photo.history.redo",
            label: "Redo Edit",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "variant"?:bool (false)}: redo the photo's last undone edit; with variant, the open A│A variant's"#,
            effect: Effect::Library,
            history: History::None,
            enabled: |s, p| match target(s, p) {
                Some(h) if h.redo_label.is_none() => Err(Disabled("Nothing to redo.".into())),
                _ => Ok(()),
            },
            run: redo,
        },
        CommandSpec {
            id: "photo.history.state",
            label: "Photo History",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "variant"?:bool (false)}: the photo's recipe ("edits") and redo list, whether each way is possible, and the button labels ("Undo Crop", "Redo Tuning"; "Undo" / "Redo" when there is nothing); a saved photo with no newer edits (Dalica's save, or the previous organizer's with its original kept) answers "undoLabel": "Undo Save", "canUndo": true and "undoSave": true (its Undo runs photo.undoSave); with variant, the open A│A variant's history, with "variant": true (disabled when none is open)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: crate::registry::always,
            run: state,
        },
        CommandSpec {
            id: "photo.history",
            label: "History",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: the photo's history as the History pane lists it, oldest first: "steps" holds a first row "Original" (index 0, "kind": "original"; "Save" with "kind": "save" for a saved photo with no newer edits, since Undo Save restores the original file), then one row per entry of the recipe and of the redo list ("kind": "edit"), each with its "label" (Crop, Straighten, Tuning…), a short "detail" where it helps ("4:3", "1.4°", "Fill Light 0.30") and its operation text "op"; "current" is the index of the row the photo is at, the rows after it are redoable ("redo": true)"#,
            effect: Effect::Query,
            history: History::None,
            enabled: crate::registry::always,
            run: list,
        },
        CommandSpec {
            id: "photo.history.goto",
            label: "Go to History Step",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId, "index":int}: move the photo to row index of photo.history (0: Original, no edits) in one command, undoing or redoing as many steps as needed; the redoable steps stay until a new edit replaces them, so a later row is one goto away; never runs Undo Save; answers photo.history's rows with "moved": the steps taken"#,
            effect: Effect::Library,
            history: History::None,
            enabled: crate::registry::always,
            run: goto,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Id {
    id: PhotoId,
    #[serde(default)]
    variant: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Photo {
    id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Goto {
    id: PhotoId,
    index: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Apply {
    id: PhotoId,
    op: String,
    #[serde(default)]
    variant: bool,
}

/// The side a `variant` flag names.
fn side(variant: bool) -> Side {
    if variant { Side::Variant } else { Side::Photo }
}

/// The history of the photo `p` names (its variant's with `"variant": true`), when `p` names a
/// known photo and the history is there; anything else is left to `run`, which reports it.
fn target(s: &Session, p: &Value) -> Option<PhotoHistory> {
    let id = p.get("id").and_then(Value::as_u64)?;
    let variant = match p.get("variant") {
        None => false,
        Some(v) => v.as_bool()?,
    };
    s.history_on(PhotoId::new(id), side(variant)).ok()
}

/// Photo `id`'s history, `NotFound` for an unknown photo.
pub(crate) fn history_of(s: &Session, id: PhotoId) -> Result<PhotoHistory, CmdError> {
    s.photo_history(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))
}

/// What `photo.history.state` answers.
pub(crate) fn state_json(id: PhotoId, h: &PhotoHistory) -> Value {
    json!({
        "id": id,
        "edits": h.edits,
        "redo": h.redo,
        "canUndo": h.undo_label.is_some(),
        "canRedo": h.redo_label.is_some(),
        "undoLabel": h.undo_text(),
        "redoLabel": h.redo_text(),
    })
}

/// The operation `text` stands for, when Dalica knows it; legacy and unknown text is refused (only
/// a sidecar may hold it).
fn operation(text: &str) -> Result<EditOp, CmdError> {
    if text.len() > dalica_library::MAX_OPERATION {
        return Err(CmdError::Limit(format!("an operation is at most {} bytes", dalica_library::MAX_OPERATION)));
    }
    let op = EditOp::parse(text);
    if !op.is_known() {
        let shown: String = text.chars().take(64).collect();
        return Err(CmdError::BadParams(format!("op: not an operation Dalica applies: {shown:?}")));
    }
    Ok(op)
}

/// What `photo.history.state {variant: true}` answers.
pub(crate) fn variant_json(id: PhotoId, h: &PhotoHistory) -> Value {
    let mut v = state_json(id, h);
    v["variant"] = json!(true);
    v
}

fn apply(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Apply { id, op, variant } = params(p)?;
    let side = side(variant);
    s.history_on(id, side)?;
    let op = operation(&op)?;
    if let Some(why) = crate::video::edit_refusal(s.library(), id, &op) {
        return Err(CmdError::Disabled(why.into()));
    }
    s.step_on(id, side, Change::Append(op), None)?;
    let h = s.history_on(id, side)?;
    Ok(if variant { variant_json(id, &h) } else { state_json(id, &h) })
}

fn undo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id, variant } = params(p)?;
    if variant {
        s.history_on(id, Side::Variant)?;
        return match s.undo_on(id, Side::Variant)? {
            Some(label) => Ok(json!({ "id": id, "undone": label, "variant": true })),
            None => Err(CmdError::Disabled("Nothing to undo.".into())),
        };
    }
    undo_photo(s, id)
}

/// Undoes photo `id`'s last edit (also `edit.undo {id}`).
pub(crate) fn undo_photo(s: &mut Session, id: PhotoId) -> Result<Value, CmdError> {
    history_of(s, id)?;
    match s.photo_undo(id)? {
        Some(label) => Ok(json!({ "id": id, "undone": label })),
        None => Err(CmdError::Disabled("Nothing to undo.".into())),
    }
}

fn redo(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id, variant } = params(p)?;
    if variant {
        s.history_on(id, Side::Variant)?;
        return match s.redo_on(id, Side::Variant)? {
            Some(label) => Ok(json!({ "id": id, "redone": label, "variant": true })),
            None => Err(CmdError::Disabled("Nothing to redo.".into())),
        };
    }
    redo_photo(s, id)
}

/// Redoes photo `id`'s last undone edit (also `edit.redo {id}`).
pub(crate) fn redo_photo(s: &mut Session, id: PhotoId) -> Result<Value, CmdError> {
    history_of(s, id)?;
    match s.photo_redo(id)? {
        Some(label) => Ok(json!({ "id": id, "redone": label })),
        None => Err(CmdError::Disabled("Nothing to redo.".into())),
    }
}

fn state(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Id { id, variant } = params(p)?;
    if variant {
        return Ok(variant_json(id, &s.history_on(id, Side::Variant)?));
    }
    let h = history_of(s, id)?;
    let mut v = state_json(id, &h);
    // A saved photo without newer edits: Undo reads "Undo Save" and runs `photo.undoSave`
    // (`editing.md` §5.1), which `photo.history.undo` does not. A photo the previous organizer
    // saved, with no record of Dalica's, the same (its legacy original, M14.13).
    let lib = s.library();
    if h.edits.is_empty() && (lib.can_undo_save(id) || lib.can_undo_legacy_save(id)) {
        v["canUndo"] = json!(true);
        v["undoLabel"] = json!(UNDO_SAVE);
        v["undoSave"] = json!(true);
    }
    Ok(v)
}

/// The Undo button's label for a saved photo whose edits are all saved.
pub const UNDO_SAVE: &str = "Undo Save";

/// The History pane's first row: the photo with no edits.
pub const ORIGINAL: &str = "Original";
/// The first row of a saved photo with no newer edits (what Undo Save takes back).
pub const SAVED: &str = "Save";

/// Named shapes a crop's detail reads as, wide side first ("4:3"; "3:4" for a tall crop).
const RATIOS: [(u32, u32); 6] = [(1, 1), (5, 4), (4, 3), (7, 5), (3, 2), (16, 9)];

/// What `photo.history` answers for photo `id`.
pub(crate) fn rows_json(s: &Session, id: PhotoId) -> Result<Value, CmdError> {
    let h = history_of(s, id)?;
    let lib = s.library();
    let saved = h.edits.is_empty() && (lib.can_undo_save(id) || lib.can_undo_legacy_save(id));
    let (width, height) = lib.photo(id).map_or((0, 0), |p| (p.width, p.height));
    let mut steps = Vec::with_capacity(h.edits.len().saturating_add(h.redo.len()).saturating_add(1));
    let first = if saved {
        json!({ "index": 0, "label": SAVED, "kind": "save", "redo": false })
    } else {
        json!({ "index": 0, "label": ORIGINAL, "kind": "original", "redo": false })
    };
    steps.push(first);
    // The recipe, then the redo list from its end (the next step to redo first): the recipe a
    // redo of each would build, so a crop's shape follows the entries before it.
    let mut built: Vec<String> = Vec::with_capacity(steps.capacity());
    let redo = h.redo.iter().rev();
    for (i, text) in h.edits.iter().chain(redo).enumerate() {
        built.push(text.clone());
        let op = EditOp::parse(text);
        let mut row = json!({ "index": i + 1, "label": crate::history::label(&op), "op": text, "kind": "edit", "redo": i >= h.edits.len() });
        if let Some(d) = detail(&op, &built, width, height) {
            row["detail"] = json!(d);
        }
        steps.push(row);
    }
    Ok(json!({ "id": id, "current": h.edits.len(), "steps": steps }))
}

/// The short detail of a row: a crop's shape, a straighten's angle, a rotation's turn, a flip's
/// axis, Tuning's values that are not zero. `recipe` is the recipe up to and including `op` (a
/// crop's shape is read after it), on a `width × height` photo (0: unknown).
fn detail(op: &EditOp, recipe: &[String], width: u32, height: u32) -> Option<String> {
    use dalica_edit::Typed;
    match op {
        EditOp::Rotate(turn) => return Some(format!("{}°", turn.degrees())),
        EditOp::FlipH => return Some("Horizontal".into()),
        EditOp::FlipV => return Some("Vertical".into()),
        _ => {}
    }
    match op.typed()?.ok()? {
        Typed::Crop(_) => {
            if width == 0 || height == 0 {
                return None;
            }
            let (w, h) = dalica_edit::shown_size(recipe, width, height, dalica_edit::Cropping::Counted);
            Some(shape(w, h))
        }
        Typed::Straighten(s) if s.degrees.is_finite() => Some(format!("{:.1}°", s.degrees)),
        Typed::FillLight(f) if f.amount.is_finite() => Some(format!("Fill Light {:.2}", f.amount)),
        Typed::Tuning(t) => {
            let mut parts = Vec::new();
            for (name, v) in [("Fill Light", t.fill), ("Highlights", t.highlights), ("Shadows", t.shadows)] {
                if v.is_finite() && v.abs() >= 0.005 {
                    parts.push(format!("{name} {v:.2}"));
                }
            }
            if t.temp.is_finite() && t.temp.abs() >= 0.005 {
                parts.push(format!("Color Temperature {:+.2}", t.temp));
            }
            if t.neutral.is_some() {
                parts.push("Neutral Color".into());
            }
            (!parts.is_empty()).then(|| parts.join(", "))
        }
        _ => None,
    }
}

/// A crop `w × h` pixels as a named ratio ("4:3", "3:4", "1:1") within 1 %, else its size.
fn shape(w: u32, h: u32) -> String {
    let (long, short) = (w.max(h), w.min(h));
    if short > 0 {
        let r = f64::from(long) / f64::from(short);
        for (a, b) in RATIOS {
            let want = f64::from(a) / f64::from(b);
            if (r / want - 1.0).abs() <= 0.01 {
                return if w >= h { format!("{a}:{b}") } else { format!("{b}:{a}") };
            }
        }
    }
    format!("{w} × {h}")
}

fn list(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Photo { id } = params(p)?;
    rows_json(s, id)
}

/// The recipe's and the redo list's lengths of photo `id` now.
fn lengths(s: &Session, id: PhotoId) -> (usize, usize) {
    let info = s.library().info(id);
    (info.edits.len(), info.redo.len())
}

fn goto(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Goto { id, index } = params(p)?;
    history_of(s, id)?;
    let (edits, redo) = lengths(s, id);
    let total = edits.saturating_add(redo);
    if index > total {
        return Err(CmdError::BadParams(format!("index: 0 … {total} for this photo")));
    }
    // Each step moves one entry (a merged or in-place step may move none, a cancelled flip coming
    // back one the other way): bounded, so a history the memory no longer matches still ends.
    let mut moved = 0usize;
    for _ in 0..total.saturating_mul(2).saturating_add(2) {
        let (edits, redo) = lengths(s, id);
        let step = if edits > index {
            s.photo_undo(id)?
        } else if edits < index && redo > 0 {
            s.photo_redo(id)?
        } else {
            None
        };
        if step.is_none() {
            break;
        }
        moved += 1;
    }
    let mut v = rows_json(s, id)?;
    v["moved"] = json!(moved);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn disabled_unknown_and_bad_params() {
        let (_dirs, mut s, ids) = library(1);
        let id = ids[0];
        assert_eq!(s.execute("photo.history.undo", &json!({ "id": id })), Err(CmdError::Disabled("Nothing to undo.".into())));
        assert_eq!(s.execute("photo.history.redo", &json!({ "id": id })), Err(CmdError::Disabled("Nothing to redo.".into())));
        for cmd in ["photo.history.undo", "photo.history.redo", "photo.history.state"] {
            assert!(matches!(s.execute(cmd, &json!({ "id": 999 })), Err(CmdError::NotFound(_))), "{cmd}");
            assert!(matches!(s.execute(cmd, &json!({})), Err(CmdError::BadParams(_))), "{cmd}");
            assert!(matches!(s.execute(cmd, &json!({ "id": "x" })), Err(CmdError::BadParams(_))), "{cmd}");
        }
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": 999, "op": "sepia()" })), Err(CmdError::NotFound(_))));
        for op in ["", "future_op(1)", "legacy:redeye(1)", "rotate(45)", "sepia(", "rotate(090)"] {
            assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id, "op": op })), Err(CmdError::BadParams(_))), "{op}");
        }
        let long = format!("sharpen({})", "1".repeat(dalica_library::MAX_OPERATION));
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id, "op": long })), Err(CmdError::Limit(_))));
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id, "op": 7 })), Err(CmdError::BadParams(_))));
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": id })), Err(CmdError::BadParams(_))));
        assert!(s.library().info(id).is_empty());
    }

    /// A recipe at the cap takes no new entry; a rotation that merges into the last one still
    /// does.
    #[test]
    fn a_full_recipe_is_a_limit_and_changes_nothing() {
        let (_dirs, mut s, ids) = library(1);
        let mut edits = vec!["future_op()".to_string(); dalica_edit::MAX_OPERATIONS - 1];
        edits.push("rotate(90)".into());
        s.apply(dalica_library::Op::SetRecipes { recipes: vec![(ids[0], dalica_library::Recipe { edits, redo: vec![] })] }).unwrap();
        let model = s.library().model().clone();
        assert!(matches!(s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "sepia()" })), Err(CmdError::Limit(_))));
        assert_eq!(s.library().model(), &model);
        s.execute("photo.history.apply", &json!({ "id": ids[0], "op": "rotate(90)" })).unwrap();
        assert_eq!(s.library().info(ids[0]).edits.last().map(String::as_str), Some("rotate(180)"));
    }

    /// A photo the previous organizer saved (its original kept, at another size) with no record
    /// of Dalica's offers Undo Save as a saved one does (M14.13).
    #[test]
    fn a_legacy_original_offers_undo_save() {
        let (_dirs, mut s, ids) = library(2);
        let state = |s: &mut crate::Session, id| s.execute("photo.history.state", &json!({ "id": id })).unwrap();
        assert_eq!(state(&mut s, ids[0])["undoSave"], json!(null));
        let size = s.library().photo(ids[0]).unwrap().size.saturating_add(1);
        let original = dalica_library::LegacyOriginal { size, digest: String::new(), recipe: vec!["sepia()".into()] };
        s.apply_derived(dalica_library::Op::SetLegacyOriginals { originals: vec![(ids[0], Some(original))] }).unwrap();
        let v = state(&mut s, ids[0]);
        assert_eq!((v["undoSave"].clone(), v["canUndo"].clone(), v["undoLabel"].clone()), (json!(true), json!(true), json!(super::UNDO_SAVE)));
        assert_eq!(state(&mut s, ids[1])["undoSave"], json!(null), "a photo without one is not offered it");
        // The History pane's first row is then the Save, marked as one.
        let rows = s.execute("photo.history", &json!({ "id": ids[0] })).unwrap();
        assert_eq!((rows["steps"][0]["label"].clone(), rows["steps"][0]["kind"].clone()), (json!(super::SAVED), json!("save")));
        let rows = s.execute("photo.history", &json!({ "id": ids[1] })).unwrap();
        assert_eq!(rows["steps"][0]["kind"], json!("original"));
    }

    #[test]
    fn a_crop_reads_as_its_named_ratio_or_its_size() {
        assert_eq!(super::shape(400, 300), "4:3");
        assert_eq!(super::shape(300, 400), "3:4");
        assert_eq!(super::shape(1920, 1081), "16:9", "within 1 %");
        assert_eq!(super::shape(50, 50), "1:1");
        assert_eq!(super::shape(96, 48), "96 × 48");
        assert_eq!(super::shape(0, 10), "0 × 10");
    }
}
