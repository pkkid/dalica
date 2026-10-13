//! The photo's own history (`architecture.md` §4, `editing.md` §5.1): the edit view's undo and redo,
//! over the recipe (`"edits"`) and redo list (`"redo"`) each photo keeps in its folder's sidecar, so
//! the history is linear and survives a restart and a rebuild of the index.
//!
//! - **A step** appends one operation to the recipe by `edit`'s rules ([`Change::Append`]: a
//!   rotation right after a rotation merges into it, the same flip twice cancels) or replaces the
//!   last entry ([`Change::ReplaceLast`], for a Tuning tab session), and clears the redo list.
//! - **Undo** moves the last entry to the end of the redo list; **redo** moves it back. A step that
//!   merged into the entry before it (a second rotation; a Tuning session that updated the last
//!   entry) undoes to that entry's earlier value, and its redo gives the merged entry again: the
//!   session remembers, per photo, how each of its own steps changed the end of the recipe
//!   ([`Record`]). The memory lives as long as the session; a step it no longer matches (the recipe
//!   was changed some other way, by a library rotation, an undo on the library stack, another
//!   application, a restart) undoes as its plain last entry.
//! - A photo step never touches the library undo stack, and a library step never walks a photo's
//!   history (it may change the recipe, which is the same list; the photo's memory of that recipe
//!   is then dropped).
//! - **The labels** ("Undo Crop", "Redo Tuning") come from each operation's history label
//!   ([`label`]).
//! - **The recipe hook** (for the thumbnails, M7.9): [`Session::apply`] notes every photo whose
//!   recipe changed, whichever history or scan changed it; [`Session::take_recipe_changes`] drains
//!   them.

use std::collections::{BTreeSet, HashMap};

use dalica_core::PhotoId;
use dalica_edit::{Op as EditOp, Recipe as EditRecipe};
use dalica_library::{Library, Op, Recipe};

use crate::error::CmdError;
use crate::session::Session;

/// The most steps remembered per photo, each way (the library stack's cap).
pub const MAX_STEPS: usize = crate::undo::CAP;
/// The most photos whose steps are remembered; past it, the least recently stepped is forgotten
/// (its steps then undo as plain entries).
pub const MAX_PHOTOS: usize = 1024;
/// The most changed recipes held for [`Session::take_recipe_changes`]; past it, the changes report
/// an overflow (everything may have changed).
pub const MAX_CHANGES: usize = 1_000_000;

/// The history label of an operation: what "Undo …" and "Redo …" name. The renderer's operation
/// modules carry their own (M7.2); until every operation has one, the names come from here.
pub fn label(op: &EditOp) -> &'static str {
    match op.name() {
        Some("rotate") => "Rotate",
        Some("flip_h" | "flip_v") => "Flip",
        Some("crop") => "Crop",
        Some("straighten") => "Straighten",
        Some("retouch") => "Retouch",
        Some("autofix") => "Auto Fix",
        Some("autocontrast") => "Auto Contrast",
        Some("autocolor") => "Auto Color",
        // A Basic Fixes Fill Light is Tuning's `fill` (`editing.md` §6.5).
        Some("fill_light" | "tuning") => "Tuning",
        Some("trim") => "Trim",
        Some("sharpen") => "Sharpen",
        Some("sepia") => "Sepia",
        Some("bw") => "Black and White",
        Some("warmify") => "Warmify",
        Some("grain") => "Film Grain",
        Some("tint") => "Tint",
        Some("saturation") => "Saturation",
        Some("soft_focus") => "Soft Focus",
        Some("glow") => "Glow",
        Some("filtered_bw") => "Filtered B&W",
        Some("focal_bw") => "Focal B&W",
        Some("graduated_tint") => "Graduated Tint",
        // Legacy, unknown, and names a newer `edit` adds before this table does.
        _ => "Edit",
    }
}

/// The label of the operation `text` stands for.
fn label_of(text: &str) -> &'static str {
    label(&EditOp::parse(text))
}

/// How a step changes the recipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Change {
    /// Appends the operation by `edit`'s rules (merging into the last entry where they say so).
    Append(EditOp),
    /// Replaces the last entry with the operation (a Tuning session updating its entry).
    #[allow(dead_code, reason = "the Tuning commands (M7.18, `cmd/tuning.rs`) take these steps; tested here")]
    ReplaceLast(EditOp),
}

/// What the photo's history offers now: its recipe and redo list, and the labels of the step undo
/// and redo would take (`None` when there is none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhotoHistory {
    pub edits: Vec<String>,
    pub redo: Vec<String>,
    pub undo_label: Option<&'static str>,
    pub redo_label: Option<&'static str>,
}

impl PhotoHistory {
    /// "Undo Crop", or "Undo" when there is nothing to undo.
    pub fn undo_text(&self) -> String {
        self.undo_label.map_or_else(|| "Undo".to_string(), |l| format!("Undo {l}"))
    }

    /// "Redo Tuning", or "Redo" when there is nothing to redo.
    pub fn redo_text(&self) -> String {
        self.redo_label.map_or_else(|| "Redo".to_string(), |l| format!("Redo {l}"))
    }
}

/// The recipes that changed since the last [`Session::take_recipe_changes`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecipeChanges {
    /// The photos, by id, each once.
    pub photos: Vec<PhotoId>,
    /// More changed than [`MAX_CHANGES`]: any photo's recipe may have changed.
    pub overflow: bool,
}

/// How one step of the session changed the end of a photo's recipe: `before` (the last entry it
/// consumed, if any) became `after` (the entry it left last, if any), leaving `len` entries. The
/// redo list holds `step` while it is undone.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Record {
    before: Option<String>,
    after: Option<String>,
    step: String,
    len: usize,
    coalesce: Option<String>,
}

impl Record {
    /// Whether `edits` is the recipe this step left.
    fn left(&self, edits: &[String]) -> bool {
        edits.len() == self.len && (self.after.is_none() || edits.last() == self.after.as_ref())
    }

    /// Whether `edits` and `redo` are what undoing this step left.
    fn undone(&self, edits: &[String], redo: &[String]) -> bool {
        let len = self.len - usize::from(self.after.is_some()) + usize::from(self.before.is_some());
        redo.last() == Some(&self.step) && edits.len() == len && (self.before.is_none() || edits.last() == self.before.as_ref())
    }

    /// `edits` with this step undone (`before` back in place of `after`).
    fn undo(&self, edits: &[String]) -> Vec<String> {
        let keep = edits.len().saturating_sub(usize::from(self.after.is_some()));
        edits.iter().take(keep).cloned().chain(self.before.clone()).collect()
    }

    /// `edits` with this step done again (`after` in place of `before`).
    fn redo(&self, edits: &[String]) -> Vec<String> {
        let keep = edits.len().saturating_sub(usize::from(self.before.is_some()));
        edits.iter().take(keep).cloned().chain(self.after.clone()).collect()
    }
}

/// One photo's remembered steps.
#[derive(Clone, Debug, Default)]
struct Memory {
    /// The photo's file name when the steps were taken (ids change in a rebuild).
    name: String,
    undo: Vec<Record>,
    redo: Vec<Record>,
    /// When it was last stepped (a counter), to forget the least recent first.
    used: u64,
}

/// The session's photo histories (`Session::histories`).
#[derive(Debug, Default)]
pub(crate) struct Histories {
    memory: HashMap<PhotoId, Memory>,
    used: u64,
    /// Set while a photo step applies its op: its recipe change keeps the memory.
    in_step: bool,
    changes: BTreeSet<PhotoId>,
    overflow: bool,
}

impl Histories {
    /// The memory of photo `id` named `name`, if it still is that photo's.
    fn get(&self, id: PhotoId, name: &str) -> Option<&Memory> {
        self.memory.get(&id).filter(|m| m.name == name)
    }

    /// The memory of photo `id` named `name`, made (or started afresh) when needed.
    fn entry(&mut self, id: PhotoId, name: &str) -> &mut Memory {
        self.used += 1;
        if !self.memory.contains_key(&id) && self.memory.len() >= MAX_PHOTOS {
            let oldest = self.memory.iter().min_by_key(|(_, m)| m.used).map(|(id, _)| *id);
            if let Some(oldest) = oldest {
                self.memory.remove(&oldest);
            }
        }
        let used = self.used;
        let m = self.memory.entry(id).or_default();
        if m.name != name {
            *m = Memory { name: name.to_string(), ..Memory::default() };
        }
        m.used = used;
        m
    }
}

/// Pushes `r` on `list`, dropping the oldest past [`MAX_STEPS`].
fn push_capped(list: &mut Vec<Record>, r: Record) {
    list.push(r);
    if list.len() > MAX_STEPS {
        list.remove(0);
    }
}

/// The recipes `op` may change, by id, with their entries now. Walks nested batches without
/// recursion.
pub(crate) fn recipes_before(library: &Library, op: &Op) -> Vec<(PhotoId, Vec<String>)> {
    let mut ids = BTreeSet::new();
    let mut stack = vec![op];
    while let Some(op) = stack.pop() {
        match op {
            Op::SetRecipes { recipes } => ids.extend(recipes.iter().map(|(id, _)| *id)),
            Op::SetPhotoInfo { photos } => ids.extend(photos.iter().map(|(id, _)| *id)),
            Op::Batch { ops } => stack.extend(ops),
            // Nothing else holds a recipe; photos entering or leaving bring or take theirs.
            _ => {}
        }
    }
    ids.into_iter().filter(|id| library.photo(*id).is_some()).map(|id| (id, library.info(id).edits.clone())).collect()
}

impl Session {
    /// The recipe hook, after an op applied: notes the photos of `before` whose recipe changed, and
    /// forgets the remembered steps of those changed outside a photo step.
    pub(crate) fn recipes_applied(&mut self, before: Vec<(PhotoId, Vec<String>)>) {
        for (id, edits) in before {
            if self.library.photo(id).is_some() && self.library.info(id).edits == edits {
                continue;
            }
            if !self.histories.in_step {
                self.histories.memory.remove(&id);
            }
            if self.histories.changes.len() < MAX_CHANGES {
                self.histories.changes.insert(id);
            } else if !self.histories.changes.contains(&id) {
                self.histories.overflow = true;
            }
        }
    }

    /// The photos whose recipe changed since the last call (any history, scans and the watcher
    /// included), each once, by id; drains them. For the thumbnails (M7.9).
    pub fn take_recipe_changes(&mut self) -> RecipeChanges {
        let photos = std::mem::take(&mut self.histories.changes).into_iter().collect();
        RecipeChanges { photos, overflow: std::mem::take(&mut self.histories.overflow) }
    }

    /// Photo `id`'s history now; `None` for an unknown photo.
    pub fn photo_history(&self, id: PhotoId) -> Option<PhotoHistory> {
        let photo = self.library.photo(id)?;
        let info = self.library.info(id);
        let memory = self.histories.get(id, &photo.name);
        // A step that cancelled the last entry (a second flip) can leave the recipe empty.
        let undo_label = match memory.and_then(|m| m.undo.last()).filter(|r| r.left(&info.edits)) {
            Some(r) => Some(label_of(&r.step)),
            None => info.edits.last().map(|last| label_of(last)),
        };
        Some(PhotoHistory { edits: info.edits.clone(), redo: info.redo.clone(), undo_label, redo_label: info.redo.last().map(|t| label_of(t)) })
    }

    /// Takes one step of photo `id`'s history: `change` applied to its recipe, its redo list
    /// cleared. With `coalesce`, a step that replaces the entry the photo's last step left, with
    /// the same key, joins that step (one undo for a whole Tuning session). Returns the step's
    /// label, or `None` when the recipe would not change (no step is taken).
    pub(crate) fn photo_step(&mut self, id: PhotoId, change: Change, coalesce: Option<&str>) -> Result<Option<&'static str>, CmdError> {
        let name = self.library.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?.name.clone();
        let edits = self.library.info(id).edits.clone();
        let limit = |e: dalica_edit::Error| CmdError::Limit(format!("photo {id}: {e}"));
        let (after, record) = match change {
            Change::Append(op) => {
                let text = op.to_string();
                let mut recipe = EditRecipe::parse(&edits).map_err(limit)?;
                recipe.push(op).map_err(limit)?;
                let after = recipe.print();
                let appended = after.len() == edits.len() + 1 && after.get(..edits.len()) == Some(&edits[..]);
                let before = if appended { None } else { edits.last().cloned() };
                let record = Record {
                    before,
                    after: if after.len() + 1 > edits.len() { after.last().cloned() } else { None },
                    step: text,
                    len: after.len(),
                    coalesce: None,
                };
                (after, record)
            }
            Change::ReplaceLast(op) => {
                let Some(last) = edits.last().cloned() else {
                    return Err(CmdError::Disabled("The photo has no edit to update.".into()));
                };
                let text = op.to_string();
                let mut after = edits.clone();
                if let Some(slot) = after.last_mut() {
                    slot.clone_from(&text);
                }
                (after, Record { before: Some(last), after: Some(text.clone()), step: text, len: edits.len(), coalesce: None })
            }
        };
        if after == edits {
            return Ok(None);
        }
        let record = Record { coalesce: coalesce.map(str::to_string), ..record };
        let step_label = label_of(&record.step);
        self.set_photo_recipe(id, after, Vec::new())?;
        let m = self.histories.entry(id, &name);
        m.redo.clear();
        let joins = |top: &Record| coalesce.is_some() && top.coalesce == record.coalesce && record.before.is_some() && top.after == record.before;
        match m.undo.last_mut() {
            Some(top) if joins(top) => {
                top.after = record.after;
                top.step = record.step;
                top.len = record.len;
            }
            _ => push_capped(&mut m.undo, record),
        }
        Ok(Some(step_label))
    }

    /// Undoes photo `id`'s last step: its last entry goes to the end of the redo list (a merged
    /// step gives back the entry it merged into). Returns the step's label; `Ok(None)` when there
    /// is nothing to undo.
    pub(crate) fn photo_undo(&mut self, id: PhotoId) -> Result<Option<&'static str>, CmdError> {
        let name = self.library.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?.name.clone();
        let info = self.library.info(id);
        let (edits, mut redo) = (info.edits.clone(), info.redo.clone());
        let record = self.histories.get(id, &name).and_then(|m| m.undo.last()).filter(|r| r.left(&edits)).cloned();
        let (after, step) = match (&record, edits.split_last()) {
            (Some(r), _) => (r.undo(&edits), r.step.clone()),
            (None, Some((last, rest))) => (rest.to_vec(), last.clone()),
            (None, None) => return Ok(None),
        };
        let step_label = label_of(&step);
        redo.push(step);
        self.set_photo_recipe(id, after, redo)?;
        let m = self.histories.entry(id, &name);
        match record {
            Some(r) => {
                m.undo.pop();
                push_capped(&mut m.redo, r);
            }
            // The memory no longer matches the recipe: forget it.
            None => {
                m.undo.clear();
                m.redo.clear();
            }
        }
        Ok(Some(step_label))
    }

    /// Redoes photo `id`'s last undone step: the end of the redo list goes back on the recipe (a
    /// merged step merges again). Returns the step's label; `Ok(None)` when the redo list is empty.
    pub(crate) fn photo_redo(&mut self, id: PhotoId) -> Result<Option<&'static str>, CmdError> {
        let name = self.library.photo(id).ok_or_else(|| CmdError::NotFound(format!("no photo {id}")))?.name.clone();
        let info = self.library.info(id);
        let (edits, mut redo) = (info.edits.clone(), info.redo.clone());
        let Some(step) = redo.pop() else { return Ok(None) };
        let record = self.histories.get(id, &name).and_then(|m| m.redo.last()).filter(|r| r.undone(&edits, &info.redo)).cloned();
        let after = match &record {
            Some(r) => r.redo(&edits),
            None => edits.iter().cloned().chain(std::iter::once(step.clone())).collect(),
        };
        let step_label = label_of(&step);
        self.set_photo_recipe(id, after, redo)?;
        let m = self.histories.entry(id, &name);
        match record {
            Some(r) => {
                m.redo.pop();
                push_capped(&mut m.undo, r);
            }
            None => {
                m.undo.clear();
                m.redo.clear();
            }
        }
        Ok(Some(step_label))
    }

    /// Sets photo `id`'s recipe and redo list as a photo step (the memory is kept).
    fn set_photo_recipe(&mut self, id: PhotoId, edits: Vec<String>, redo: Vec<String>) -> Result<(), CmdError> {
        let op = Op::SetRecipes { recipes: vec![(id, Recipe { edits, redo })] };
        self.histories.in_step = true;
        let result = self.apply(op);
        self.histories.in_step = false;
        result.map(|_| ()).map_err(|e| match e {
            CmdError::BadParams(m) => CmdError::Limit(format!("photo {id}: {m}")),
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use dalica_library::Recipe;
    use serde_json::json;

    use super::*;
    use crate::cmd::testing::library;

    fn text(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| t.to_string()).collect()
    }

    fn set(s: &mut Session, id: PhotoId, edits: &[&str], redo: &[&str]) {
        s.apply(Op::SetRecipes { recipes: vec![(id, Recipe { edits: text(edits), redo: text(redo) })] }).unwrap();
    }

    fn step(s: &mut Session, id: PhotoId, op: &str) -> Option<&'static str> {
        s.photo_step(id, Change::Append(EditOp::parse(op)), None).unwrap()
    }

    #[test]
    fn every_listed_operation_has_a_label_of_its_own() {
        for name in dalica_edit::LISTED {
            assert_ne!(label_of(&format!("{name}()")), "Edit", "{name}");
        }
        assert_eq!(label_of("rotate(90)"), "Rotate");
        assert_eq!(label_of("flip_v()"), "Flip");
        assert_eq!(label_of("legacy:redeye(1)"), "Edit");
        assert_eq!(label_of("future_op(1)"), "Edit");
    }

    /// A flip that cancels the flip before it removes the entry; undo puts it back, and redo
    /// cancels it again.
    #[test]
    fn a_cancelling_flip_undoes_to_the_flip_it_removed() {
        let (_dirs, mut s, ids) = library(1);
        let id = ids[0];
        step(&mut s, id, "crop(0.1,0.1,0.9,0.9)");
        step(&mut s, id, "flip_h()");
        assert_eq!(step(&mut s, id, "flip_h()"), Some("Flip"));
        assert_eq!(s.library().info(id).edits, text(&["crop(0.1,0.1,0.9,0.9)"]));
        assert_eq!(s.photo_history(id).unwrap().undo_text(), "Undo Flip", "the label of the step, not of the entry left last");
        assert_eq!(s.photo_undo(id).unwrap(), Some("Flip"));
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: text(&["crop(0.1,0.1,0.9,0.9)", "flip_h()"]), redo: text(&["flip_h()"]) });
        assert_eq!(s.photo_redo(id).unwrap(), Some("Flip"));
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: text(&["crop(0.1,0.1,0.9,0.9)"]), redo: vec![] });
        assert_eq!(s.photo_undo(id).unwrap(), Some("Flip"));
        assert_eq!(s.photo_undo(id).unwrap(), Some("Flip"));
        assert_eq!(s.photo_undo(id).unwrap(), Some("Crop"));
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: vec![], redo: text(&["flip_h()", "flip_h()", "crop(0.1,0.1,0.9,0.9)"]) });
        assert_eq!(s.photo_undo(id).unwrap(), None);
    }

    /// A Tuning session: its first change appends, the later ones replace the entry, and the whole
    /// session is one step; a session that updates an existing entry undoes to its earlier value.
    #[test]
    fn a_coalesced_session_is_one_step() {
        let (_dirs, mut s, ids) = library(1);
        let id = ids[0];
        s.photo_step(id, Change::Append(EditOp::parse("tuning(0.1,0,0,0,none)")), Some("a")).unwrap();
        for v in ["0.2", "0.3"] {
            s.photo_step(id, Change::ReplaceLast(EditOp::parse(&format!("tuning({v},0,0,0,none)"))), Some("a")).unwrap();
        }
        assert_eq!(s.library().info(id).edits, text(&["tuning(0.3,0,0,0,none)"]));
        s.photo_undo(id).unwrap();
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: vec![], redo: text(&["tuning(0.3,0,0,0,none)"]) });
        s.photo_redo(id).unwrap();
        // A second session on the same entry: one step back to 0.3.
        for v in ["0.5", "0.6"] {
            s.photo_step(id, Change::ReplaceLast(EditOp::parse(&format!("tuning({v},0,0,0,none)"))), Some("b")).unwrap();
        }
        assert_eq!(s.photo_history(id).unwrap().undo_text(), "Undo Tuning");
        s.photo_undo(id).unwrap();
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: text(&["tuning(0.3,0,0,0,none)"]), redo: text(&["tuning(0.6,0,0,0,none)"]) });
        s.photo_redo(id).unwrap();
        assert_eq!(s.library().info(id).edits, text(&["tuning(0.6,0,0,0,none)"]));
        assert!(matches!(s.photo_step(ids[0], Change::ReplaceLast(EditOp::parse("sepia()")), None), Ok(Some("Sepia"))));
        set(&mut s, id, &[], &[]);
        assert!(matches!(s.photo_step(id, Change::ReplaceLast(EditOp::parse("sepia()")), None), Err(CmdError::Disabled(_))));
    }

    /// A recipe changed some other way (a library rotation; another application through a scan)
    /// makes the remembered steps stale: the photo's last entry undoes as itself.
    #[test]
    fn a_recipe_changed_outside_the_photo_history_forgets_its_merges() {
        let (_dirs, mut s, ids) = library(1);
        let id = ids[0];
        step(&mut s, id, "rotate(90)");
        step(&mut s, id, "rotate(90)");
        s.execute("photo.rotate.cw", &json!({ "ids": [id] })).unwrap();
        assert_eq!(s.library().info(id).edits, text(&["rotate(270)"]));
        assert_eq!(s.photo_undo(id).unwrap(), Some("Rotate"));
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: vec![], redo: text(&["rotate(270)"]) });
        // Redo without memory appends the entry as it was.
        s.photo_redo(id).unwrap();
        assert_eq!(s.library().info(id).recipe(), Recipe { edits: text(&["rotate(270)"]), redo: vec![] });
    }

    #[test]
    fn the_recipe_hook_lists_each_changed_photo_once() {
        let (_dirs, mut s, ids) = library(3);
        assert_eq!(s.take_recipe_changes(), RecipeChanges::default());
        step(&mut s, ids[2], "sepia()");
        step(&mut s, ids[2], "bw()");
        s.execute("photo.rotate.cw", &json!({ "ids": [ids[0]] })).unwrap();
        s.execute("photo.star.toggle", &json!({ "ids": [ids[1]] })).unwrap();
        assert_eq!(s.take_recipe_changes(), RecipeChanges { photos: vec![ids[0], ids[2]], overflow: false });
        // The undone star changes no recipe.
        s.execute("edit.undo", &json!({})).unwrap();
        s.photo_undo(ids[2]).unwrap();
        assert_eq!(s.take_recipe_changes().photos, vec![ids[2]]);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.take_recipe_changes().photos, vec![ids[0]], "the library rotation undone");
        assert_eq!(s.take_recipe_changes(), RecipeChanges::default());
    }

    #[test]
    fn the_memory_is_capped_per_photo_and_in_photos() {
        let mut h = Histories::default();
        for i in 0..(MAX_PHOTOS as u64 + 10) {
            let m = h.entry(PhotoId::new(i), "a.jpg");
            for _ in 0..3 {
                push_capped(&mut m.undo, Record { before: None, after: None, step: String::new(), len: 0, coalesce: None });
            }
        }
        assert_eq!(h.memory.len(), MAX_PHOTOS);
        assert!(!h.memory.contains_key(&PhotoId::new(0)), "the least recently used went first");
        let m = h.entry(PhotoId::new(5000), "b.jpg");
        for _ in 0..(MAX_STEPS + 5) {
            push_capped(&mut m.undo, Record { before: None, after: None, step: String::new(), len: 0, coalesce: None });
        }
        assert_eq!(m.undo.len(), MAX_STEPS);
        // Another file under the same id (a rebuild) starts afresh.
        assert!(h.entry(PhotoId::new(5000), "c.jpg").undo.is_empty());
    }
}
