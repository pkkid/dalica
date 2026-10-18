//! The edit view's tools (`editing.md` §6–§7): one interface, [`Tool`], that each tool module under
//! `panels/edit/` implements (Crop, Straighten, Retouch, and the Tuning tab's session), and the
//! commands that dispatch to it, so the tools never share a file:
//!
//! - `edit.tool.open {tool, name?}`: opens a tool on the photo shown; its draft starts from
//!   [`Tool::open`] ([`Tool::open_named`] with `name`: the effect a parameter panel opens on,
//!   `{tool: "effect", name: "sharpen"}`, M12). Another tool open first is refused (`disabled`):
//!   it is applied or cancelled first, as the reference does.
//! - `edit.tool.apply`, `edit.tool.cancel`, `edit.tool.reset`: the open tool's Apply (one history
//!   step; the tool closes when it succeeds), Cancel (the draft is dropped, nothing is recorded)
//!   and Reset (the draft back to where it started).
//! - `edit.tool.action {name, params?}`: anything else a tool does with its draft (a drag on the
//!   photo, a patch, a ratio chosen), by name, so scripts drive a tool the way its panel does.
//!
//! Keys reach the open tool first ([`key`]): the tool's own ([`Tool::key`]: Enter applies a crop,
//! the arrows nudge Straighten), then Esc, which cancels any tool. Only then does the edit view's
//! keymap see the key.
//!
//! **The focused side** (`editing.md` §4): every tool works on the recipe of the side with the
//! focus. On A│A's right side that is the photo's variant ([`on_variant`]): a tool builds its draft
//! and its preview candidate from [`recipe`] (the variant's recipe there) and its Apply is one step
//! of the variant's history ([`variant_step`], `photo.history.apply {variant: true}`; Tuning's
//! `photo.history.tuning {variant: true}`), which no sidecar sees until "Keep which version?" →
//! Right makes the variant one step of the photo's history.
//!
//! Every tool is a stub until its task fills it (M7.19 Crop, M7.20 Straighten, M7.21 Tuning, M7.22
//! Retouch, M12.16 the effect panels' session in [`super::effect_session`]): it opens, resets and
//! cancels its draft, and its Apply refuses with "Not available yet.".

use dalica_core::PhotoId;
use dalica_engine::{CmdError, Key};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{UiCommand, effect_session, no_params, parse};
use crate::app::App;
use crate::panels::edit::{crop, retouch, straighten, tuning};
use crate::state::{EditState, ToolKind, View};

/// A tool of the edit view. Each tool module implements it on a unit value ([`tool`] finds it);
/// the commands of this module check the edit view and the open tool before they call it.
pub trait Tool: Sync {
    fn kind(&self) -> ToolKind;

    /// The panel's label ("Crop").
    fn label(&self) -> &'static str;

    /// Whether the rest of the edit panel is drawn disabled while the tool is open (Crop,
    /// Straighten and Retouch; not a Tuning session, `editing.md` §5).
    fn modal(&self) -> bool {
        true
    }

    /// Writes the draft the tool starts with into `edit` (the tool is not yet marked open).
    fn open(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError>;

    /// [`Tool::open`] with the `name` `edit.tool.open` was given: what the tool opens on (the
    /// effect of a parameter panel). Only the effect tool takes one; the others refuse it.
    fn open_named(&self, app: &App, edit: &mut EditState, name: Option<&str>) -> Result<(), CmdError> {
        match name {
            None => self.open(app, edit),
            Some(_) => Err(CmdError::BadParams(format!("{} takes no `name`", self.label()))),
        }
    }

    /// Applies the draft as one step of the photo's history. The tool closes when it succeeds.
    fn apply(&self, app: &mut App) -> Result<Value, CmdError>;

    /// The draft back to where [`Tool::open`] started it.
    fn reset(&self, app: &App, edit: &mut EditState) -> Result<(), CmdError> {
        self.open(app, edit)
    }

    /// Runs the tool's action `name` on its draft.
    fn action(&self, _app: &mut App, name: &str, _params: &Value) -> Result<Value, CmdError> {
        Err(CmdError::BadParams(format!("{} has no action `{}`", self.label(), name.chars().take(40).collect::<String>())))
    }

    /// A key pressed while the tool is open: `Some` with what it did when the tool takes the key,
    /// `None` to leave it to Esc and the keymap.
    fn key(&self, _app: &mut App, _key: &Key) -> Option<Result<Value, CmdError>> {
        None
    }
}

/// The module implementing tool `kind`.
pub fn tool(kind: ToolKind) -> &'static dyn Tool {
    match kind {
        ToolKind::Crop => &crop::CROP,
        ToolKind::Straighten => &straighten::STRAIGHTEN,
        ToolKind::Retouch => &retouch::RETOUCH,
        ToolKind::Tuning => &tuning::TUNING,
        ToolKind::Effect => &effect_session::EFFECT,
    }
}

/// The open tool, if any.
pub fn open_tool(app: &App) -> Option<&'static dyn Tool> {
    app.state().edit.tool.map(tool)
}

/// Whether the tools work on A│A's variant of `photo`: the right side has the focus and `photo` is
/// the photo shown.
pub fn on_variant(app: &App, photo: PhotoId) -> bool {
    super::compare::on_variant(app) && app.state().edit.photo == Some(photo)
}

/// The recipe a tool works on for `photo`: its open variant's on A│A's right side, else the
/// photo's own (empty without a session).
pub fn recipe(app: &App, photo: PhotoId) -> Vec<String> {
    let Some(s) = app.session() else { return Vec::new() };
    match s.variant().edits().filter(|_| on_variant(app, photo) && s.variant().photo() == Some(photo)) {
        Some(v) => v.to_vec(),
        None => s.library().info(photo).edits.clone(),
    }
}

/// One step of `photo`'s variant: `op` appended (`photo.history.apply {variant: true}`); `None`
/// (what counts already is what was asked) takes none. Answers the variant's history with
/// `changed`, as `photo.crop.apply` and `photo.straighten.apply` answer the photo's.
pub fn variant_step(app: &mut App, photo: PhotoId, op: Option<String>) -> Result<Value, CmdError> {
    let mut out = match &op {
        Some(op) => app.run("photo.history.apply", &json!({ "id": photo, "op": op, "variant": true }))?,
        None => app.run("photo.history.state", &json!({ "id": photo, "variant": true }))?,
    };
    if let Some(o) = out.as_object_mut() {
        o.insert("changed".into(), json!(op.is_some()));
    }
    Ok(out)
}

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "edit.tool.open",
        label: "Open Tool",
        menu: "",
        keys: &[],
        params: r#"{"tool": "crop" | "straighten" | "retouch" | "tuning" | "effect", "name"?: string}: open a tool on the photo shown in the edit view; "effect" opens the parameter panel of effect `name` ("sharpen")"#,
        run: open,
    },
    UiCommand {
        id: "edit.tool.apply",
        label: "Apply",
        menu: "",
        keys: &[],
        params: "{}: apply the open tool's draft as one step of the photo's history, and close the tool",
        run: apply,
    },
    UiCommand {
        id: "edit.tool.cancel",
        label: "Cancel",
        menu: "",
        keys: &[],
        params: "{}: close the open tool; its draft is dropped and nothing is recorded",
        run: cancel,
    },
    UiCommand {
        id: "edit.tool.reset",
        label: "Reset",
        menu: "",
        keys: &[],
        params: "{}: the open tool's draft back to where it started",
        run: reset,
    },
    UiCommand {
        id: "edit.tool.action",
        label: "Tool Action",
        menu: "",
        keys: &[],
        params: r#"{"name": string, "params"?: object}: run an action of the open tool on its draft (each tool lists its own)"#,
        run: action,
    },
];

/// The check of every tool command: the edit view is showing a photo.
fn in_edit_view(app: &App) -> Result<(), CmdError> {
    let s = app.state();
    if s.view == View::Edit && s.edit.photo.is_some() { Ok(()) } else { Err(CmdError::Disabled("Open a photo in the edit view first.".into())) }
}

/// The open tool, or `disabled` when none is.
fn the_open_tool(app: &App) -> Result<&'static dyn Tool, CmdError> {
    in_edit_view(app)?;
    open_tool(app).ok_or_else(|| CmdError::Disabled("No tool is open.".into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    tool: ToolKind,
    #[serde(default)]
    name: Option<String>,
}

/// What `edit.tool.open` answers: the tool, and the effect for a parameter panel.
fn opened(edit: &EditState) -> Value {
    match (&edit.tool, &edit.effect) {
        (Some(ToolKind::Effect), Some(e)) => json!({ "tool": ToolKind::Effect, "name": e.name }),
        (tool, _) => json!({ "tool": tool }),
    }
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Open = parse(params)?;
    in_edit_view(app)?;
    if let Some(t) = open_tool(app) {
        let effect = app.state().edit.effect.as_ref().map(|e| e.name.as_str());
        if t.kind() == p.tool && (p.tool != ToolKind::Effect || effect == p.name.as_deref()) {
            return Ok(opened(&app.state().edit));
        }
        return Err(CmdError::Disabled(format!("Apply or cancel {} first.", t.label())));
    }
    let t = tool(p.tool);
    let mut edit = app.state().edit.clone();
    t.open_named(app, &mut edit, p.name.as_deref())?;
    edit.tool = Some(p.tool);
    let reply = opened(&edit);
    app.state_mut().edit = edit;
    Ok(reply)
}

fn apply(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let t = the_open_tool(app)?;
    let result = t.apply(app)?;
    app.state_mut().edit.close_tools();
    Ok(result)
}

fn cancel(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let t = the_open_tool(app)?;
    // The preview goes back to the photo's own recipe.
    if let Some(s) = app.session_mut()
        && s.preview_photo().is_some()
    {
        let _ = s.set_preview_candidate(None);
    }
    app.state_mut().edit.close_tools();
    Ok(json!({ "cancelled": t.kind() }))
}

fn reset(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    let t = the_open_tool(app)?;
    let mut edit = app.state().edit.clone();
    t.reset(app, &mut edit)?;
    app.state_mut().edit = edit;
    Ok(json!({ "reset": t.kind() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Action {
    name: String,
    #[serde(default)]
    params: Option<Value>,
}

fn action(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Action = parse(params)?;
    let t = the_open_tool(app)?;
    let inner = p.params.unwrap_or_else(|| json!({}));
    if !inner.is_object() {
        return Err(CmdError::BadParams("`params` is an object".into()));
    }
    t.action(app, &p.name, &inner)
}

/// Gives `key` to the open tool: the tool's own keys, then Esc, which cancels it. Returns the
/// command that ran, `None` when no tool is open or it does not take the key (the keymap gets it).
pub fn key(app: &mut App, key: &Key) -> Option<String> {
    let t = open_tool(app)?;
    if let Some(result) = t.key(app, key) {
        if let Err(e) = result {
            app.state_mut().status = e.to_string();
        }
        return Some(format!("tool:{}", t.kind().name()));
    }
    if key.to_string() == "Esc" {
        let _ = app.run("edit.tool.cancel", &json!({}));
        return Some("edit.tool.cancel".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::session_with_photos;

    fn in_edit(n: usize) -> (tempfile::TempDir, App) {
        let (dir, s, ids) = session_with_photos(n);
        let mut app = App::new(Some(s));
        app.run("view.edit", &json!({ "id": ids[0] })).unwrap();
        (dir, app)
    }

    #[test]
    fn every_tool_is_found_by_its_kind() {
        for k in ToolKind::ALL {
            assert_eq!(tool(k).kind(), k);
            assert!(!tool(k).label().is_empty());
        }
        assert!(!tool(ToolKind::Tuning).modal() && tool(ToolKind::Crop).modal());
    }

    #[test]
    fn a_tool_opens_once_and_cancel_drops_its_draft() {
        let (_dir, mut app) = in_edit(1);
        assert!(matches!(app.run("edit.tool.apply", &json!({})), Err(CmdError::Disabled(_))), "no tool open");
        assert_eq!(app.run("edit.tool.open", &json!({ "tool": "straighten" })).unwrap(), json!({ "tool": "straighten" }));
        assert_eq!(app.state().edit.straighten.map(|s| s.angle), Some(0.0));
        assert!(matches!(app.run("edit.tool.open", &json!({ "tool": "crop" })), Err(CmdError::Disabled(m)) if m.contains("Straighten")));
        assert_eq!(app.run("edit.tool.open", &json!({ "tool": "straighten" })).unwrap(), json!({ "tool": "straighten" }), "already open");
        app.state_mut().edit.straighten = Some(crate::state::StraightenDraft { angle: 3.5 });
        app.run("edit.tool.reset", &json!({})).unwrap();
        assert_eq!(app.state().edit.straighten.map(|s| s.angle), Some(0.0));
        // Apply with nothing changed succeeds, takes no step and closes the tool.
        let applied = app.run("edit.tool.apply", &json!({})).unwrap();
        assert_eq!((applied["changed"].clone(), applied["edits"].clone()), (json!(false), json!([])));
        assert_eq!((app.state().edit.tool, app.state().edit.straighten), (None, None));
        app.run("edit.tool.open", &json!({ "tool": "straighten" })).unwrap();
        assert!(matches!(app.run("edit.tool.action", &json!({ "name": "nope" })), Err(CmdError::BadParams(_))));
        app.run("edit.tool.cancel", &json!({})).unwrap();
        assert_eq!((app.state().edit.tool, app.state().edit.straighten), (None, None));
    }

    #[test]
    fn tool_commands_refuse_bad_params_and_the_library_view() {
        let (_dir, s, _ids) = session_with_photos(1);
        let mut app = App::new(Some(s));
        assert!(matches!(app.run("edit.tool.open", &json!({ "tool": "crop" })), Err(CmdError::Disabled(_))), "not in the edit view");
        let (_dir, mut app) = in_edit(1);
        for (id, bad) in [
            ("edit.tool.open", json!({})),
            ("edit.tool.open", json!({ "tool": "lasso" })),
            ("edit.tool.open", json!({ "tool": 3 })),
            ("edit.tool.open", json!([])),
            ("edit.tool.apply", json!({ "x": 1 })),
            ("edit.tool.cancel", json!("now")),
            ("edit.tool.reset", json!({ "x": 1 })),
            ("edit.tool.action", json!({})),
            ("edit.tool.action", json!({ "name": 5 })),
            ("edit.tool.action", json!({ "name": "x", "params": [1] })),
        ] {
            let before = app.state().clone();
            assert!(app.run(id, &bad).is_err(), "{id} {bad}");
            assert_eq!(app.state().edit, before.edit, "{id} {bad}: nothing changed");
        }
    }

    #[test]
    fn esc_cancels_the_open_tool_and_otherwise_is_not_taken() {
        let (_dir, mut app) = in_edit(1);
        let esc = Key::parse("Esc").unwrap();
        assert_eq!(key(&mut app, &esc), None, "no tool: the keymap gets Esc");
        app.run("edit.tool.open", &json!({ "tool": "retouch" })).unwrap();
        assert_eq!(key(&mut app, &Key::parse("F5").unwrap()), None);
        assert_eq!(key(&mut app, &esc).as_deref(), Some("edit.tool.cancel"));
        assert_eq!(app.state().edit.tool, None);
        assert_eq!(app.state().view, View::Edit, "Esc cancelled the tool and stayed");
    }
}
