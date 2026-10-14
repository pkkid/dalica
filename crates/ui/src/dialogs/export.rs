//! The Export dialog, "Export to Folder" (`creating.md` §2): opened by `output.export` (File ▸
//! Export Pictures to Folder…, Ctrl+Shift+S, the bottom bar's and a section header's Export) with
//! its draft, the view state's `export` ([`ExportDraft`]), filled from the engine's
//! `export.defaults`. The rows are M8.5's form ([`Form`]), labels right-aligned in one column:
//!
//! - **Export location**: the folder the export folder is made in (`field:dialog.location`) and
//!   Browse… (`button:dialog.browse`: the folder picker, whose folder runs `output.export.location`;
//!   disabled while another file dialog is open or there is no picker);
//! - **Name of exported folder** (`field:dialog.folderName`) and "Add numbers to file names to keep
//!   the order" (`check:dialog.numbers`);
//! - **Image size**: Use original size (`choice:dialog.size/original`) or Resize to
//!   (`choice:dialog.size/resize`) with the size field (`slider:dialog.size/value`, the text as
//!   typed: a size Export takes is digits from 100 to 4000) and the slider with stops at 480, 640,
//!   800, 1024, 1280, 1600, 2048 and 3840 (`slider:dialog.size`); the two stay in step, a drag
//!   snaps at the stops, a typed size never does;
//! - **Image quality**: the presets dropdown (`choice:dialog.quality`, its items
//!   `choice:dialog.quality/<preset>` with their descriptions) and the custom quality's slider
//!   (`slider:dialog.quality`, 1–100), enabled for Custom, which the dropdown then reads
//!   "Custom (75)";
//! - **Export videos as**: First frame or Full video (`choice:dialog.videos/<firstFrame|fullVideo>`),
//!   disabled: no video is exported until M15 decodes them;
//! - **Watermark**: "Add watermark" (`check:dialog.watermark`) enabling its text
//!   (`field:dialog.watermarkText`), and the hint under it.
//!
//! Export (`button:dialog.export`, enabled while [`ExportDraft::ready`]) runs `export.run` with
//! [`run_params`] and closes; Cancel and Esc close it and change nothing.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::{
    ButtonKind, FORM_GAP, Form, SliderSpec, StopsSlider, TextField, checkbox, dropdown, dropdown_item, form_group, hint, radio, radio_row,
    stops_slider, text_button,
};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Ui, pos2, vec2};
use serde_json::{Map, Value, json};

use std::path::PathBuf;

use super::{Button, Dialog, Look};
use crate::app::App;
use crate::picker::FolderRequest;
use crate::state::{CUSTOM_DEFAULT, CUSTOM_MAX, CUSTOM_MIN, ExportDraft, Quality, RESIZE_DEFAULT, RESIZE_MAX, RESIZE_MIN, Videos};

/// The dialog's title.
pub const TITLE: &str = "Export to Folder";
/// Its width (points).
pub const WIDTH: f32 = 560.0;

/// The controls' automation ids (`control-protocol.md` §7).
pub const LOCATION_ID: &str = "field:dialog.location";
pub const BROWSE_ID: &str = "button:dialog.browse";
pub const FOLDER_NAME_ID: &str = "field:dialog.folderName";
pub const NUMBERS_ID: &str = "check:dialog.numbers";
pub const ORIGINAL_ID: &str = "choice:dialog.size/original";
pub const RESIZE_ID: &str = "choice:dialog.size/resize";
pub const SIZE_ID: &str = "slider:dialog.size";
pub const SIZE_FIELD_ID: &str = "slider:dialog.size/value";
pub const QUALITY_ID: &str = "choice:dialog.quality";
pub const CUSTOM_ID: &str = "slider:dialog.quality";
pub const FIRST_FRAME_ID: &str = "choice:dialog.videos/firstFrame";
pub const FULL_VIDEO_ID: &str = "choice:dialog.videos/fullVideo";
pub const WATERMARK_ID: &str = "check:dialog.watermark";
pub const WATERMARK_TEXT_ID: &str = "field:dialog.watermarkText";

/// The automation id of the quality dropdown's item for `q`.
pub fn quality_item_id(q: Quality) -> String {
    format!("{QUALITY_ID}/{}", q.name())
}

/// The size slider's stops (`creating.md` §2), in pixels of the longest side.
pub const STOPS: [f64; 8] = [480.0, 640.0, 800.0, 1024.0, 1280.0, 1600.0, 2048.0, 3840.0];

/// The row labels, top to bottom.
pub const LABELS: [&str; 6] = ["Export location:", "Name of exported folder:", "Image size:", "Image quality:", "Export videos as:", "Watermark:"];

/// The title of the folder picker Browse… opens.
pub const BROWSE_TITLE: &str = "Export Location";

/// The watermark row's hint.
pub const WATERMARK_HINT: &str = "Stamp photos with your name, a web address or a copyright notice.";

/// The size slider's spec: 100–4000 pixels, whole pixels.
pub const SIZE: SliderSpec<'static> = SliderSpec {
    id: SIZE_ID,
    label: "Resize to",
    min: RESIZE_MIN as f64,
    max: RESIZE_MAX as f64,
    default: RESIZE_DEFAULT as f64,
    step: 1.0,
    decimals: 0,
    unit: "pixels",
};

/// The custom quality's slider: 1–100.
pub const CUSTOM: SliderSpec<'static> = SliderSpec {
    id: CUSTOM_ID,
    label: "Custom quality",
    min: CUSTOM_MIN as f64,
    max: CUSTOM_MAX as f64,
    default: CUSTOM_DEFAULT as f64,
    step: 1.0,
    decimals: 0,
    unit: "",
};

/// The custom quality slider's width.
const CUSTOM_TRACK_W: f32 = 160.0;
/// The size field's width.
const SIZE_FIELD_W: f32 = 56.0;
/// The space left for Browse… beside the location field.
const BROWSE_W: f32 = 88.0;

/// A preset's title and its description in the quality dropdown (`creating.md` §2).
pub fn preset(q: Quality) -> (&'static str, &'static str) {
    match q {
        Quality::Automatic => ("Automatic", "Keeps the original's quality"),
        Quality::Normal => ("Normal", "Balances quality and size"),
        Quality::Maximum => ("Maximum", "Keeps fine detail; large files"),
        Quality::Minimum => ("Minimum", "Small files with some quality loss"),
        Quality::Custom => ("Custom", "Choose the quality yourself"),
    }
}

/// What the quality dropdown reads: the preset's title, "Custom (75)" for Custom.
pub fn quality_text(draft: &ExportDraft) -> String {
    match draft.quality {
        Quality::Custom => format!("Custom ({})", draft.custom),
        q => preset(q).0.to_string(),
    }
}

/// Where the size slider stands for the field's text: the size it holds, else its digits held to
/// the range (a field reading "99999" shows the slider at 4000), else the default.
pub fn slider_value(draft: &ExportDraft) -> u32 {
    if let Some(px) = draft.pixels() {
        return px;
    }
    let digits: String = draft.resize.chars().filter(char::is_ascii_digit).take(9).collect();
    digits.parse::<u32>().map_or(RESIZE_DEFAULT, |v| v.clamp(RESIZE_MIN, RESIZE_MAX))
}

/// The `export.run` parameters of `draft` (`creating.md` §2): what it exports, `dir`,
/// `folderName`, `numbers`, `size` (pixels of the longest side, or `"original"`), `quality` (the
/// preset's name, or the custom value) and `watermark` (its text, or `false` when off: a
/// `watermark` left out would take the stored setting). Remove location is an option of the
/// settings, so it is left out and the stored one applies.
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
    let watermark = if draft.watermark { json!(draft.watermark_text.trim()) } else { json!(false) };
    p.insert("watermark".into(), watermark);
    Ok(Value::Object(p))
}

pub fn look(app: &App, _d: &Dialog) -> Look {
    let ready = app.state().export.as_ref().is_some_and(|e| e.ready().is_ok());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), Button::new("export", "Export", ButtonKind::Default).enabled(ready)] }
}

pub fn body(app: &mut App, ui: &mut Ui, _d: &mut Dialog) {
    let t = *app.tokens();
    let Some(before) = app.state().export.clone() else { return };
    let mut e = before.clone();
    let browse = rows(ui, &t, &mut e, can_browse(app));
    if e != before
        && let Some(slot) = app.state_mut().export.as_mut()
    {
        *slot = e.sanitized();
    }
    if browse {
        browse_clicked(app);
    }
}

/// Whether Browse… can open the folder picker: there is one, and no file dialog is open.
fn can_browse(app: &App) -> bool {
    app.can_pick().is_ok()
}

/// The folder picker Browse… opens: it starts in the location typed, and the folder chosen runs
/// `output.export.location` (over automation it is checked as a place to write).
pub fn browse_request(draft: &ExportDraft) -> FolderRequest {
    let typed = PathBuf::from(draft.location.trim());
    FolderRequest {
        title: BROWSE_TITLE,
        folder: typed.is_absolute().then_some(typed),
        command: "output.export.location",
        params: json!({}),
        output: true,
    }
}

/// Browse…: opens the folder picker for the export location; a refusal lands on the status line.
fn browse_clicked(app: &mut App) {
    let Some(draft) = app.state().export.as_ref() else { return };
    let request = browse_request(draft);
    if let Err(why) = app.pick_folder_for(request) {
        app.state_mut().status = why;
    }
}

/// Draws the form for `e` and edits it in place. Returns whether Browse… was clicked.
fn rows(ui: &mut Ui, t: &Tokens, e: &mut ExportDraft, browse_on: bool) -> bool {
    let form = Form::new(ui, t, &LABELS);
    let controls_w = (ui.available_width() - form.label_w - FORM_GAP).max(120.0);
    let mut browse = false;

    form.row(ui, t, LABELS[0], |ui| {
        ui.horizontal(|ui| {
            TextField::new(LOCATION_ID, "", (controls_w - BROWSE_W).max(60.0)).show(ui, t, &mut e.location);
            browse = form_group(ui, browse_on, |ui| text_button(ui, t, BROWSE_ID, "Browse…", false)).clicked();
        });
    });

    form.row(ui, t, LABELS[1], |ui| {
        TextField::new(FOLDER_NAME_ID, "", controls_w).show(ui, t, &mut e.folder_name);
        checkbox(ui, t, NUMBERS_ID, "Add numbers to file names to keep the order", &mut e.numbers);
    });

    form.row(ui, t, LABELS[2], |ui| {
        if radio(ui, t, ORIGINAL_ID, "Use original size", e.original_size).clicked() {
            e.original_size = true;
        }
        let resize = !e.original_size;
        let (r, ()) = radio_row(ui, t, RESIZE_ID, "Resize to:", resize, |ui| size_controls(ui, t, e));
        if r.clicked() {
            e.original_size = false;
        }
    });

    form.row(ui, t, LABELS[3], |ui| {
        ui.horizontal(|ui| {
            let mut picked = None;
            dropdown(ui, t, QUALITY_ID, &quality_text(e), |ui| {
                for q in Quality::ALL {
                    let (title, description) = preset(q);
                    if dropdown_item(ui, t, &quality_item_id(q), title, description, q == e.quality).clicked() {
                        picked = Some(q);
                    }
                }
            });
            if let Some(q) = picked {
                e.quality = q;
            }
            let s = StopsSlider { spec: CUSTOM, stops: &[], field: false, track_w: CUSTOM_TRACK_W };
            let out = form_group(ui, e.quality == Quality::Custom, |ui| stops_slider(ui, t, &s, f64::from(e.custom)));
            if let Some(v) = out.value {
                e.custom = to_u8(v).clamp(CUSTOM_MIN, CUSTOM_MAX);
            }
        });
    });

    // Videos are skipped until M15 decodes them: the row stays disabled.
    form_group(ui, false, |ui| {
        form.row(ui, t, LABELS[4], |ui| {
            ui.horizontal(|ui| {
                radio(ui, t, FIRST_FRAME_ID, "First frame", e.videos == Videos::FirstFrame);
                radio(ui, t, FULL_VIDEO_ID, "Full video (no resizing)", e.videos == Videos::FullVideo);
            });
        });
    });

    form.row(ui, t, LABELS[5], |ui| {
        ui.horizontal(|ui| {
            checkbox(ui, t, WATERMARK_ID, "Add watermark", &mut e.watermark);
            let w = ui.available_width().max(60.0);
            let on = e.watermark;
            form_group(ui, on, |ui| TextField::new(WATERMARK_TEXT_ID, "", w).show(ui, t, &mut e.watermark_text));
        });
        hint(ui, t, WATERMARK_HINT);
    });
    browse
}

/// The Resize to row's controls, in step: the size field (its text kept as typed), "pixels", and
/// the slider with stops standing at the field's size ([`slider_value`]); a drag writes its value
/// into the field.
fn size_controls(ui: &mut Ui, t: &Tokens, e: &mut ExportDraft) {
    TextField::new(SIZE_FIELD_ID, "", SIZE_FIELD_W).select_on_focus(true).show(ui, t, &mut e.resize);
    let color = if ui.is_enabled() { t.text_label } else { t.text_disabled };
    let galley = ui.painter().layout_no_wrap(SIZE.unit.to_owned(), fonts::regular(t.body), color);
    let (_, rect) = ui.allocate_space(vec2(galley.size().x.ceil(), t.metrics.text_field_h));
    ui.painter().galley(pos2(rect.min.x, rect.center().y - galley.size().y / 2.0).round(), galley, color);
    let s = StopsSlider { spec: SIZE, stops: &STOPS, field: false, track_w: f32::INFINITY };
    if let Some(v) = stops_slider(ui, t, &s, f64::from(slider_value(e))).value {
        e.resize = to_u32(v).clamp(RESIZE_MIN, RESIZE_MAX).to_string();
    }
}

/// A slider's value as a whole number (NaN as 0, held to `u32`).
fn to_u32(v: f64) -> u32 {
    if v.is_finite() { v.round().clamp(0.0, f64::from(u32::MAX)) as u32 } else { 0 }
}

/// A slider's value as a whole number held to `u8`.
fn to_u8(v: f64) -> u8 {
    u8::try_from(to_u32(v)).unwrap_or(u8::MAX)
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
            json!({ "ids": [2], "dir": "/tmp/out", "folderName": "Lisbon", "numbers": false, "size": 1600, "quality": "automatic", "watermark": false })
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

    #[test]
    fn the_slider_stands_at_the_fields_size_or_near_it() {
        let at = |text: &str| slider_value(&ExportDraft { resize: text.into(), ..ExportDraft::default() });
        assert_eq!((at("1700"), at("99999"), at("0"), at("abc"), at(""), at("12a00")), (1700, 4000, 100, 1600, 1600, 1200));
        assert_eq!((to_u32(f64::NAN), to_u32(-3.0), to_u32(2047.6), to_u8(300.0)), (0, 0, 2048, 255));
    }

    #[test]
    fn the_quality_dropdown_reads_the_preset_or_the_custom_value() {
        let d = ExportDraft { quality: Quality::Custom, custom: 75, ..ExportDraft::default() };
        assert_eq!(quality_text(&d), "Custom (75)");
        assert_eq!(quality_text(&ExportDraft { quality: Quality::Maximum, ..d }), "Maximum");
        assert_eq!(quality_item_id(Quality::Minimum), "choice:dialog.quality/minimum");
    }
}
