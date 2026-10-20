//! Add Custom Ratio… (the Crop tool's size dropdown, `editing.md` §6.1): Width × Height
//! (`field:dialog.width`, `field:dialog.height`), Name (`field:dialog.name`), and a live example
//! line showing the row the ratio will add ("Example: Print (3 × 2)") or why it cannot be added
//! yet. The draft is `{width, height, name}`; OK is greyed until both sides are numbers above 0
//! (at most 10 000, within 1:20 … 20:1, as the engine keeps them), the name is not empty and no
//! other custom ratio has it. OK runs `crop.ratios.add` (kept in the settings, not an undo step)
//! and, while the Crop tool is open, chooses the new ratio.

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::{self, ButtonKind, TextField};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Id, RichText, Ui};
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;
use crate::panels::edit::crop;
use crate::state::{MAX_DRAFT, ToolKind};
use crate::tl;

pub const TITLE: &str = "Add Custom Ratio";
/// The dialog's width (points).
pub const WIDTH: f32 = 360.0;
/// The fields.
pub const WIDTH_ID: &str = "field:dialog.width";
pub const HEIGHT_ID: &str = "field:dialog.height";
pub const NAME_ID: &str = "field:dialog.name";
/// The largest side, the most elongated ratio and the longest name (bytes) the engine keeps.
pub const MAX_SIDE: f64 = 10_000.0;
pub const MAX_RATIO: f64 = 20.0;
pub const MAX_NAME: usize = 256;

/// The dialog with an empty draft.
pub fn dialog() -> Dialog {
    let mut d = Dialog::new(DialogKind::CustomRatio);
    if let Value::Object(o) = json!({ "width": "", "height": "", "name": "" }) {
        d.fields = o;
    }
    d
}

/// A side of the ratio in the draft: a number above 0 (typed as text or given as a number).
fn side(d: &Dialog, key: &str) -> Option<f64> {
    let v = match d.fields.get(key)? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (v.is_finite() && v > 0.0).then_some(v)
}

/// The draft's name, trimmed.
fn name(d: &Dialog) -> &str {
    d.fields.get("name").and_then(Value::as_str).unwrap_or("").trim()
}

/// Why the draft cannot be added yet, if it cannot.
pub fn refused(d: &Dialog) -> Option<&'static str> {
    let Some((w, h)) = side(d, "width").zip(side(d, "height")) else { return Some("Type a width and a height above 0.") };
    if w > MAX_SIDE || h > MAX_SIDE {
        Some("Keep each side at most 10000.")
    } else if !(1.0 / MAX_RATIO..=MAX_RATIO).contains(&(w / h)) {
        Some("Keep the ratio between 1:20 and 20:1.")
    } else if name(d).is_empty() {
        Some("Name the ratio.")
    } else if name(d).len() > MAX_NAME {
        Some("Keep the name at most 256 bytes.")
    } else {
        None
    }
}

/// [`refused`], and a name another custom ratio already has.
fn refused_in(app: &App, d: &Dialog) -> Option<&'static str> {
    refused(d).or_else(|| {
        let taken = app.session().is_some_and(|s| s.settings().edit().crop_ratios.iter().any(|r| r.name.trim() == name(d)));
        taken.then_some("A custom ratio already has that name.")
    })
}

/// The live example line: the row the ratio adds to the dropdown, or why it cannot be added.
pub fn example(app: &App, d: &Dialog) -> String {
    match (refused_in(app, d), side(d, "width").zip(side(d, "height"))) {
        (None, Some((w, h))) => tl!("Example: {name} ({width} × {height})", name = name(d), width = w, height = h),
        (Some(why), _) => shown(why),
        (None, None) => String::new(),
    }
}

/// A reason of [`refused`] or [`refused_in`] (English, which a refused OK answers with) in the
/// language in effect.
fn shown(why: &str) -> String {
    match why {
        "Type a width and a height above 0." => tl!("Type a width and a height above 0."),
        "Keep each side at most 10000." => tl!("Keep each side at most 10000."),
        "Keep the ratio between 1:20 and 20:1." => tl!("Keep the ratio between 1:20 and 20:1."),
        "Name the ratio." => tl!("Name the ratio."),
        "Keep the name at most 256 bytes." => tl!("Keep the name at most 256 bytes."),
        "A custom ratio already has that name." => tl!("A custom ratio already has that name."),
        other => crate::i18n::tr(other),
    }
}

pub fn look(app: &App, d: &Dialog) -> Look {
    let ok = Button::new("ok", tl!("OK"), ButtonKind::Default).enabled(refused_in(app, d).is_none());
    Look { title: tl!("Add Custom Ratio"), width: WIDTH, buttons: vec![Button::cancel(), ok] }
}

/// The draft's text of `key` (a number given by a script shows as it reads).
fn text(d: &Dialog, key: &str) -> String {
    match d.fields.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// One field of the draft, kept to [`MAX_DRAFT`] characters.
fn field(ui: &mut Ui, t: &Tokens, d: &mut Dialog, key: &str, id: &str, hint: &str, width: f32) {
    let mut value = text(d, key);
    let before = value.clone();
    TextField::new(id, hint, width).select_on_focus(true).show(ui, t, &mut value);
    if let Some((at, _)) = value.char_indices().nth(MAX_DRAFT) {
        value.truncate(at);
    }
    if value != before {
        d.fields.insert(key.into(), Value::String(value));
    }
}

pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    // A dialog drawn anew puts the focus in Width on its first two passes (egui sizes the window
    // on the first, which takes no input).
    let seen = Id::new(WIDTH_ID).with("customRatio.passes");
    let pass = ctx.cumulative_pass_nr();
    let first = match ctx.data(|m| m.get_temp::<(u64, u64)>(seen)) {
        Some((first, last)) if last.checked_add(1) == Some(pass) => first,
        _ => pass,
    };
    ctx.data_mut(|m| m.insert_temp(seen, (first, pass)));
    if pass.saturating_sub(first) < 2 {
        widgets::focus_field(&ctx, WIDTH_ID);
    }
    let label = |ui: &mut Ui, s: &str| {
        ui.label(RichText::new(s).font(fonts::regular(t.body)).color(t.text_label));
    };
    label(ui, &tl!("Width × Height"));
    ui.horizontal(|ui| {
        field(ui, &t, d, "width", WIDTH_ID, &tl!("Width"), 90.0);
        ui.label(RichText::new("×").font(fonts::regular(t.body)).color(t.text_label));
        field(ui, &t, d, "height", HEIGHT_ID, &tl!("Height"), 90.0);
    });
    ui.add_space(6.0);
    label(ui, &tl!("Name"));
    let w = ui.available_width();
    field(ui, &t, d, "name", NAME_ID, &tl!("Print, Banner…"), w);
    ui.add_space(6.0);
    ui.label(RichText::new(example(app, d)).font(fonts::regular(t.small)).color(t.text_dim));
}

/// OK: `crop.ratios.add`; the Crop tool, when open, then uses the new ratio.
pub fn act(app: &mut App, d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    if let Some(why) = refused_in(app, d) {
        return Err(CmdError::Disabled(why.into()));
    }
    let Some((width, height)) = side(d, "width").zip(side(d, "height")) else {
        return Err(CmdError::Disabled("Type a width and a height above 0.".into()));
    };
    let name = name(d).to_string();
    let out = app.run("crop.ratios.add", &json!({ "name": name, "width": width, "height": height }))?;
    crop::ratios_changed();
    if app.state().edit.tool == Some(ToolKind::Crop) {
        // The ratio as the engine writes it ("W:H" of the numbers kept).
        crop::choose(app, &format!("{width}:{height}"), Some(&name))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_waits_for_two_sides_and_a_name() {
        let mut d = dialog();
        assert!(refused(&d).is_some());
        for (w, h, name, ok) in [
            (json!("3"), json!("2"), "Print", true),
            (json!(0), json!(5), "Zero", false),
            (json!("x"), json!("2"), "Bad", false),
            (json!(1.5), json!(1), "  ", false),
            (json!(-1), json!(1), "Negative", false),
            (json!("21"), json!("1"), "Too long", false),
            (json!("20"), json!("1"), "Banner", true),
            (json!("10001"), json!("10000"), "Huge", false),
            (json!(" 8.5 "), json!("11"), "Letter", true),
        ] {
            d.fields.insert("width".into(), w);
            d.fields.insert("height".into(), h);
            d.fields.insert("name".into(), json!(name));
            assert_eq!(refused(&d).is_none(), ok, "{:?}", d.fields);
        }
        d.fields.insert("name".into(), json!("n".repeat(MAX_NAME + 1)));
        assert!(refused(&d).is_some());
    }

    #[test]
    fn a_script_s_numbers_read_as_text() {
        let mut d = dialog();
        d.fields.insert("width".into(), json!(3));
        assert_eq!(text(&d, "width"), "3");
        assert_eq!(text(&d, "height"), "");
        d.fields.insert("name".into(), json!(null));
        assert_eq!(text(&d, "name"), "");
    }
}
