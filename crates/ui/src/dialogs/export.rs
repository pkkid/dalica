//! The Export dialog, "Export to Folder" (`creating.md` §2): opened by `output.export` (File ▸
//! Export Pictures to Folder…, Ctrl+Shift+S, the bottom bar's and a section header's Export), its
//! draft the view state's `export` ([`ExportDraft`]). Export runs `export.run` with the draft and
//! closes; Cancel and Esc change nothing.
//!
//! A stub until M8.20: the draft drawn as text lines, the Export button enabled when the draft is
//! ready ([`ExportDraft::ready`]) and answered with [`run_params`].

use dalica_engine::CmdError;
use dalica_ui_kit::fonts;
use dalica_ui_kit::widgets::ButtonKind;
use egui::{RichText, Ui};
use serde_json::{Map, Value, json};

use super::{Button, Dialog, Look};
use crate::app::App;
use crate::state::{ExportDraft, Quality};

/// The dialog's title.
pub const TITLE: &str = "Export to Folder";
/// Its width (points).
pub const WIDTH: f32 = 520.0;

/// The `export.run` parameters of `draft` (`creating.md` §2): what it exports, `dir`,
/// `folderName`, `numbers`, `size` (pixels of the longest side, or `"original"`), `quality` (the
/// preset's name, or the custom value) and `watermark` (its text, when on).
pub fn run_params(draft: &ExportDraft) -> Result<Value, CmdError> {
    draft.ready().map_err(CmdError::Disabled)?;
    let mut p = Map::new();
    // The photos picked (in a section, from its header), else the whole section.
    match draft.section {
        Some(section) if draft.ids.is_empty() => p.insert("section".into(), json!(section)),
        _ => p.insert("ids".into(), json!(draft.ids)),
    };
    p.insert("dir".into(), json!(draft.location.trim()));
    p.insert("folderName".into(), json!(draft.folder_name.trim()));
    p.insert("numbers".into(), json!(draft.numbers));
    let size = match draft.pixels() {
        Some(px) if !draft.original_size => json!(px),
        _ => json!("original"),
    };
    p.insert("size".into(), size);
    let quality = match draft.quality {
        Quality::Custom => json!(draft.custom),
        q => json!(q.name()),
    };
    p.insert("quality".into(), quality);
    if draft.watermark {
        p.insert("watermark".into(), json!(draft.watermark_text.trim()));
    }
    Ok(Value::Object(p))
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ready = app.state().export.as_ref().is_some_and(|e| e.ready().is_ok());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("export", "Export", ButtonKind::Default).enabled(ready)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    let Some(e) = app.state().export.clone() else { return };
    let size = if e.original_size { "Use original size".to_string() } else { format!("Resize to: {} pixels", e.resize) };
    let quality = match e.quality {
        Quality::Custom => format!("Custom ({})", e.custom),
        q => q.name().to_string(),
    };
    let lines = [
        format!("Export location: {}", e.location),
        format!("Name of exported folder: {}", e.folder_name),
        format!("Add numbers to file names: {}", if e.numbers { "yes" } else { "no" }),
        format!("Image size: {size}"),
        format!("Image quality: {quality}"),
        format!("Watermark: {}", if e.watermark { e.watermark_text.as_str() } else { "none" }),
    ];
    for line in lines {
        let mut line: String = line.chars().take(120).collect();
        line.retain(|c| !c.is_control());
        ui.label(RichText::new(line).font(fonts::regular(t.body)).color(t.text_label));
    }
}

/// Export: runs `export.run` with the draft.
pub fn act(app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    let draft = app.state().export.clone().ok_or_else(|| CmdError::Internal("the Export dialog has no draft".into()))?;
    let params = run_params(&draft)?;
    app.run("export.run", &params)
}

#[cfg(test)]
mod tests {
    use dalica_core::{FolderId, PhotoId};

    use super::*;
    use crate::state::Section;

    #[test]
    fn the_draft_becomes_export_run_parameters_when_it_is_ready() {
        let mut d = ExportDraft { ids: vec![PhotoId::new(2)], location: "/tmp/out".into(), folder_name: " Lisbon ".into(), ..ExportDraft::default() };
        assert_eq!(
            run_params(&d).unwrap(),
            json!({ "ids": [2], "dir": "/tmp/out", "folderName": "Lisbon", "numbers": false, "size": 1600, "quality": "automatic" })
        );
        d.quality = Quality::Custom;
        d.custom = 40;
        d.original_size = true;
        d.watermark = true;
        d.watermark_text = "© Ana".into();
        d.ids.clear();
        d.section = Some(Section::Folder(FolderId::new(5)));
        let p = run_params(&d).unwrap();
        assert_eq!(
            (&p["section"], &p["size"], &p["quality"], &p["watermark"]),
            (&json!("folder:5"), &json!("original"), &json!(40), &json!("© Ana"))
        );
        for bad in ["", "0", "99", "4001", "99999", "abc", "16 00"] {
            let e = ExportDraft { resize: bad.into(), original_size: false, ..d.clone() };
            assert!(matches!(run_params(&e), Err(CmdError::Disabled(_))), "{bad:?}");
        }
        for (bad, why) in [
            (ExportDraft { location: " ".into(), ..d.clone() }, "where"),
            (ExportDraft { folder_name: String::new(), ..d.clone() }, "Name"),
            (ExportDraft { watermark_text: String::new(), ..d.clone() }, "watermark"),
            (ExportDraft { section: None, ..d.clone() }, "nothing"),
        ] {
            assert!(bad.ready().unwrap_err().contains(why), "{why}");
        }
    }
}
