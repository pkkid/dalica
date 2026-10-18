//! Suggestions in the window (`organizing.md` §6.3–§6.4, `faces.md` §5): in a person's section the
//! strip "Suggestions:" with [Confirm All], [Confirm], [Reject], [Find More] and the toggle "Show
//! only suggestions"; ✓ and × on each suggested face, and "Name?" with ✓ / × on Unnamed faces and
//! in the People pane. Confirming makes a face its person's (and a reference for later
//! suggestions), rejecting records the pair so it is never suggested again; each through the
//! engine's `faces.confirm`, `faces.confirmAll`, `faces.reject` and `faces.findMore` (M13.26).
//!
//! M13.4's stubs: the ids and parameters are final, and "Show only suggestions" acts on the view
//! state (`people.onlySuggestions`); the others check their parameters, then refuse with
//! [`NOT_YET`] until M13.30 makes them act. The ids are the UI's own (`suggestions.*`), never the
//! engine's, so `engine.execute faces.confirm` always reaches the engine.

use dalica_core::PersonId;
use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Check, UiCommand, parse, parse_object};
use crate::app::App;
use crate::state::{FaceRef, MAX_PEOPLE};

/// Why the suggestion commands do not act yet.
pub const NOT_YET: &str = "Arrives with name suggestions.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "suggestions.confirm",
        label: "Confirm",
        menu: "",
        keys: &[],
        params: "{faces?}: confirm the suggested names of the faces (default: the faces selected); one undo step",
        run: confirm,
    },
    UiCommand {
        id: "suggestions.reject",
        label: "Reject",
        menu: "",
        keys: &[],
        params: "{faces?}: reject the suggested names of the faces (default: the faces selected), for good",
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
        params: "{person}: list the near misses for the person, most similar first",
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

pub const CHECKS: &[(&str, Check)] =
    &[("suggestions.confirm", not_yet), ("suggestions.reject", not_yet), ("suggestions.confirmAll", not_yet), ("suggestions.findMore", not_yet)];

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Faces {
    #[serde(default)]
    faces: Option<Vec<FaceRef>>,
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

fn confirm(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = super::faces::known_faces(app, super::faces::targets(app, p.faces))?;
    waits("faces.confirm", json!({ "faces": faces }))
}

fn reject(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Faces = parse_object(params)?;
    let faces = super::faces::known_faces(app, super::faces::targets(app, p.faces))?;
    waits("faces.reject", json!({ "faces": faces }))
}

fn confirm_all(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse(params)?;
    waits("faces.confirmAll", json!({ "person": p.person }))
}

fn find_more(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Person = parse(params)?;
    waits("faces.findMore", json!({ "person": p.person }))
}

/// A stub: what it will run once M13.30 lands (the engine's `command` with `params`), refused with
/// [`NOT_YET`] for now.
fn waits(_command: &str, _params: Value) -> Result<Value, CmdError> {
    Err(CmdError::Disabled(NOT_YET.into()))
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

    #[test]
    fn only_suggestions_toggles_and_the_others_wait() {
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
            assert_eq!(app.run(id, &params), Err(CmdError::Disabled(NOT_YET.into())), "{id}");
        }
        assert!(matches!(app.run("suggestions.confirmAll", &json!({})), Err(CmdError::BadParams(_))));
    }
}
