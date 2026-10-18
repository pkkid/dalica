//! The face model dialog's commands (`faces.md` §3.1): `faceModel.open` opens it (Options ▸ People's
//! Download…, turning on Find faces in my photos without a model, the first open of People) on the
//! built-in list, a catalogue entry or a local file; `faceModel.addFile` is its Add a model file…,
//! which picks an `.onnx` file to install after the same licence.
//!
//! M13.4's stubs: `faceModel.open` opens the stub dialog (`crate::dialogs::face_model`) with its
//! draft; `faceModel.addFile` checks its parameters, then refuses with [`NOT_YET`] until M13.27
//! makes the dialog act. Downloads and installs are the engine's `faces.models.*` (M13.16), which
//! need `acknowledged: true`.

use dalica_engine::CmdError;
use serde::Deserialize;
use serde_json::Value;

use super::{Check, UiCommand, no_params, parse_object};
use crate::app::App;
use crate::dialogs::face_model;
use crate::state::FaceModelDraft;

/// Why Add a model file… does not act yet.
pub const NOT_YET: &str = "Arrives with face models.";

pub const COMMANDS: &[UiCommand] = &[
    UiCommand {
        id: "faceModel.open",
        label: "Face Model…",
        menu: "",
        keys: &[],
        params: "{entry?, file?}: open the face model dialog on a model of faces.models.list, or on a local model file",
        run: open,
    },
    UiCommand {
        id: "faceModel.addFile",
        label: "Add a Model File…",
        menu: "",
        keys: &[],
        params: "{}: pick a local .onnx model file for the face model dialog",
        run: add_file,
    },
];

pub const CHECKS: &[(&str, Check)] = &[("faceModel.addFile", not_yet)];

fn not_yet(_: &App, _: &Value) -> Result<(), String> {
    Err(NOT_YET.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    #[serde(default)]
    entry: Option<String>,
    #[serde(default)]
    file: Option<std::path::PathBuf>,
}

fn open(app: &mut App, params: &Value) -> Result<Value, CmdError> {
    let p: Open = parse_object(params)?;
    let draft = FaceModelDraft { entry: p.entry, file: p.file, accepted: false, progress: None };
    draft.check().map_err(CmdError::BadParams)?;
    face_model::open(app, draft)
}

fn add_file(_app: &mut App, params: &Value) -> Result<Value, CmdError> {
    no_params(params)?;
    Err(CmdError::Disabled(NOT_YET.into()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_dialog_opens_with_its_draft_and_download_waits_for_the_box() {
        let mut app = App::new(None);
        assert!(matches!(app.run("faceModel.open", &json!({ "entry": "a", "file": "/m.onnx" })), Err(CmdError::BadParams(_))));
        assert_eq!(app.run("faceModel.open", &json!({ "entry": "sface" })).unwrap(), json!({ "dialog": "faceModel" }));
        assert_eq!(app.state().face_model.as_ref().and_then(|f| f.entry.clone()).as_deref(), Some("sface"));
        assert!(matches!(crate::dialogs::answer(&mut app, None), Err(CmdError::Disabled(_))), "Download is off until the box is ticked");
        if let Some(f) = app.state_mut().face_model.as_mut() {
            f.accepted = true;
        }
        assert_eq!(crate::dialogs::answer(&mut app, None), Err(CmdError::Disabled(face_model::NOT_YET.into())));
        crate::dialogs::cancel(&mut app).unwrap();
        assert!(app.state().face_model.is_none(), "closing drops the draft");
        assert_eq!(app.run("faceModel.addFile", &json!({})), Err(CmdError::Disabled(NOT_YET.into())));
    }
}
