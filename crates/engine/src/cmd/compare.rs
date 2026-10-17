//! `edit.variant.open {id}` and `edit.variant.close {keep}` (`editing.md` §4): A│A's variant, held
//! in the session until kept (`variant.rs`). Its steps are `photo.history.*` with `"variant": true`.

use dalica_core::PhotoId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::cmd::history::{history_of, state_json, variant_json};
use crate::error::CmdError;
use crate::history::Side;
use crate::registry::{CommandSpec, Disabled, Effect, History, params};
use crate::session::Session;
use crate::variant::Keep;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "edit.variant.open",
            label: "Compare Two Versions",
            menu: "",
            keys: &[],
            params: r#"{"id":PhotoId}: starts A│A's variant of the photo, a copy of its recipe held in this session only (its steps are photo.history.* with "variant": true; nothing of it reaches a sidecar until kept); the photo's variant already open is kept as it is; refused while another photo's variant is open; answers photo.history.state {variant: true} with "started""#,
            effect: Effect::Library,
            history: History::None,
            enabled: crate::registry::always,
            run: open,
        },
        CommandSpec {
            id: "edit.variant.close",
            label: "Keep Version",
            menu: "",
            keys: &[],
            params: r#"{"keep":"left"|"right"}: closes A│A's variant: left drops it, right makes its recipe one step of the photo's own history (one undo gives the recipe before it back) and drops it; answers the photo's photo.history.state with "kept" and "changed""#,
            effect: Effect::Library,
            history: History::Photo,
            enabled: |s, _| match s.variant().photo() {
                Some(_) => Ok(()),
                None => Err(Disabled("No variant is open.".into())),
            },
            run: close,
        },
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    id: PhotoId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Close {
    keep: Keep,
}

fn open(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Open { id } = params(p)?;
    let started = s.open_variant(id)?;
    let mut v = variant_json(id, &s.history_on(id, Side::Variant)?);
    v["started"] = json!(started);
    Ok(v)
}

fn close(s: &mut Session, p: &Value) -> Result<Value, CmdError> {
    let Close { keep } = params(p)?;
    let (id, label) = s.close_variant(keep)?;
    let mut v = match history_of(s, id) {
        Ok(h) => state_json(id, &h),
        // The left side kept of a photo that has left the library.
        Err(_) => json!({ "id": id }),
    };
    v["kept"] = json!(if keep == Keep::Left { "left" } else { "right" });
    v["changed"] = json!(label.is_some());
    Ok(v)
}
