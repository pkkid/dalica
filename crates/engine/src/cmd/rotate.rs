//! Rotation and flip commands: `photo.rotate.cw` / `photo.rotate.ccw`, `edit.flipH` / `edit.flipV`
//! (`organizing.md` §9, `data-model.md` §3.1).
//!
//! Each appends one geometric operation to the recipe of every photo named, through `edit`'s rules
//! ([`dalica_edit::Recipe::push`]): a rotation right after a rotation merges into it and a net whole
//! turn removes it, the same flip twice in a row cancels, and every other operation (an unknown one
//! included) keeps its text and its place. All the photos change in one library undo step
//! (`data-model.md` §3.1: a rotation made in the library is a library step); the edit view's redo
//! list is left as it is. Pixels never change: the recipe is data in the sidecar.

use dalica_core::PhotoId;
use dalica_edit::{Op as EditOp, Recipe as EditRecipe, Turn};
use dalica_library::{Op, Recipe};
use serde_json::{Value, json};

use crate::cmd::photo::{Ids, has_ids, known};
use crate::error::CmdError;
use crate::registry::{CommandSpec, Context, Effect, History, params};
use crate::session::Session;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "photo.rotate.cw",
            label: "Rotate Clockwise",
            menu: "Picture/Rotate Clockwise",
            keys: &[(Context::Library, "Ctrl+R"), (Context::EditView, "Ctrl+R")],
            params: r#"{"ids":[PhotoId]}: turn each photo a quarter turn clockwise (appends rotate(90) to its recipe, merging with a rotation right before it)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: |s, p| append(s, p, EditOp::Rotate(Turn::Quarter)),
        },
        CommandSpec {
            id: "photo.rotate.ccw",
            label: "Rotate Counterclockwise",
            menu: "Picture/Rotate Counterclockwise",
            keys: &[(Context::Library, "Ctrl+Shift+R"), (Context::EditView, "Ctrl+Shift+R")],
            params: r#"{"ids":[PhotoId]}: turn each photo a quarter turn counterclockwise (appends rotate(270) to its recipe, merging with a rotation right before it)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: |s, p| append(s, p, EditOp::Rotate(Turn::ThreeQuarters)),
        },
        CommandSpec {
            id: "edit.flipH",
            label: "Flip Horizontal",
            menu: "Picture/Flip Horizontal",
            keys: &[(Context::Library, "Ctrl+Shift+H"), (Context::EditView, "Ctrl+Shift+H")],
            params: r#"{"ids":[PhotoId]}: mirror each photo left to right (appends flip_h() to its recipe; a flip_h() right before it cancels instead)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: |s, p| append(s, p, EditOp::FlipH),
        },
        CommandSpec {
            id: "edit.flipV",
            label: "Flip Vertical",
            menu: "Picture/Flip Vertical",
            keys: &[(Context::Library, "Ctrl+Shift+V"), (Context::EditView, "Ctrl+Shift+V")],
            params: r#"{"ids":[PhotoId]}: mirror each photo top to bottom (appends flip_v() to its recipe; a flip_v() right before it cancels instead)"#,
            effect: Effect::Library,
            history: History::Library,
            enabled: has_ids,
            run: |s, p| append(s, p, EditOp::FlipV),
        },
    ]
}

/// Appends `op` to the recipe of every photo `params.ids` names, as one op on the model.
fn append(s: &mut Session, p: &Value, op: EditOp) -> Result<Value, CmdError> {
    let Ids { ids } = params(p)?;
    let ids = known(s, ids)?;
    if ids.is_empty() {
        return Err(CmdError::Disabled("Select some photos first.".into()));
    }
    let recipes = ids.iter().map(|id| appended(s, *id, &op)).collect::<Result<Vec<_>, _>>()?;
    s.apply(Op::SetRecipes { recipes })?;
    Ok(json!({ "ids": ids, "operation": op.to_string() }))
}

/// Photo `id`'s recipe with `op` appended by `edit`'s rules; its redo list unchanged.
fn appended(s: &Session, id: PhotoId, op: &EditOp) -> Result<(PhotoId, Recipe), CmdError> {
    let info = s.library().info(id);
    let limit = |e: dalica_edit::Error| CmdError::Limit(format!("photo {id}: {e}"));
    let mut recipe = EditRecipe::parse(&info.edits).map_err(limit)?;
    recipe.push(op.clone()).map_err(limit)?;
    Ok((id, Recipe { edits: recipe.print(), redo: info.redo.clone() }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::cmd::testing::library;
    use crate::error::CmdError;

    #[test]
    fn each_command_appends_its_operation_and_undo_takes_it_back() {
        let (_dirs, mut s, ids) = library(1);
        for (id, op) in
            [("photo.rotate.cw", "rotate(90)"), ("photo.rotate.ccw", "rotate(270)"), ("edit.flipH", "flip_h()"), ("edit.flipV", "flip_v()")]
        {
            let v = s.execute(id, &json!({ "ids": [ids[0]] })).unwrap();
            assert_eq!(v, json!({ "ids": [ids[0]], "operation": op }));
            assert_eq!(s.library().info(ids[0]).edits, vec![op.to_string()], "{id}");
            s.execute("edit.undo", &json!({})).unwrap();
            assert!(s.library().info(ids[0]).is_empty());
        }
    }

    /// `data-model.md` §10.4: an operation Dalica does not know keeps its text and its place, and
    /// the rotations after it merge among themselves, never across it.
    #[test]
    fn an_unknown_operation_survives_four_rotations_in_the_model_and_the_sidecar() {
        let (dirs, mut s, ids) = library(1);
        let recipe = |e: &[&str]| dalica_library::Recipe { edits: e.iter().map(|t| t.to_string()).collect(), redo: vec!["glow(0.4,3.0)".into()] };
        let before = recipe(&["rotate(90)", "future_op(1;2)", "legacy:redeye(1)"]);
        s.apply(dalica_library::Op::SetRecipes { recipes: vec![(ids[0], before.clone())] }).unwrap();
        s.execute("photo.rotate.cw", &json!({ "ids": [ids[0]] })).unwrap();
        assert_eq!(s.library().info(ids[0]).recipe(), recipe(&["rotate(90)", "future_op(1;2)", "legacy:redeye(1)", "rotate(90)"]));
        for _ in 0..3 {
            s.execute("photo.rotate.cw", &json!({ "ids": [ids[0]] })).unwrap();
        }
        assert_eq!(s.library().info(ids[0]).recipe(), before, "the four rotations after the unknown ones cancel; the redo list is kept");
        s.flush().unwrap();
        let file = dirs.photos.path().join("Lisbon").join(".dalica.json");
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        assert_eq!(v["photos"]["IMG_0001.jpg"]["edits"], json!(["rotate(90)", "future_op(1;2)", "legacy:redeye(1)"]));
        assert_eq!(v["photos"]["IMG_0001.jpg"]["redo"], json!(["glow(0.4,3.0)"]));
        // Undo walks back through every step to the recipe as it stood.
        for _ in 0..4 {
            s.execute("edit.undo", &json!({})).unwrap();
        }
        assert_eq!(s.library().info(ids[0]).recipe(), before);
    }

    /// A recipe already at the cap takes no new entry: the whole command is refused and nothing
    /// changes; a rotation that merges into the last one still works.
    #[test]
    fn a_full_recipe_is_refused_as_a_limit_and_changes_nothing() {
        let (_dirs, mut s, ids) = library(2);
        let mut edits = vec!["future_op()".to_string(); dalica_edit::MAX_OPERATIONS - 1];
        edits.push("rotate(90)".into());
        s.apply(dalica_library::Op::SetRecipes { recipes: vec![(ids[1], dalica_library::Recipe { edits, redo: vec![] })] }).unwrap();
        let model = s.library().model().clone();
        assert!(matches!(s.execute("edit.flipH", &json!({ "ids": [ids[0], ids[1]] })), Err(CmdError::Limit(_))));
        assert_eq!(s.library().model(), &model);
        assert_eq!(s.undo_stack().len(), 0);
        s.execute("photo.rotate.cw", &json!({ "ids": [ids[0], ids[1]] })).unwrap();
        assert_eq!(s.library().info(ids[1]).edits.last().map(String::as_str), Some("rotate(180)"));
        assert_eq!(s.library().info(ids[0]).edits, ["rotate(90)"]);
    }

    #[test]
    fn disabled_and_unknown_photos() {
        let (_dirs, mut s, _) = library(1);
        for id in ["photo.rotate.cw", "photo.rotate.ccw", "edit.flipH", "edit.flipV"] {
            assert_eq!(s.execute(id, &json!({ "ids": [] })), Err(CmdError::Disabled("Select some photos first.".into())));
            assert!(matches!(s.execute(id, &json!({ "ids": [999] })), Err(CmdError::NotFound(_))));
        }
    }
}
