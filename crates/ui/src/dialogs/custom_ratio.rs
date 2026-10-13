//! Add Custom Ratio… (the Crop tool's size dropdown, `editing.md` §6.1): a stub on the dialog host
//! so the Crop tool (M7.19) has its own file to grow. The draft is `{width, height, name}`; OK is
//! greyed until both sides are numbers above 0 and the name is not empty. Keeping the ratio is the
//! engine's (the custom ratios setting, M7.17); until then OK refuses with "Not available yet.".

use dalica_engine::CmdError;
use dalica_ui_kit::widgets::ButtonKind;
use egui::Ui;
use serde_json::{Value, json};

use super::{Button, Dialog, DialogKind, Look};
use crate::app::App;

pub const TITLE: &str = "Add Custom Ratio";
/// The dialog's width (points).
pub const WIDTH: f32 = 360.0;

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

/// Why the draft cannot be added yet, if it cannot.
pub fn refused(d: &Dialog) -> Option<&'static str> {
    let name = d.fields.get("name").and_then(Value::as_str).unwrap_or("").trim();
    if side(d, "width").is_none() || side(d, "height").is_none() {
        Some("Type a width and a height above 0.")
    } else if name.is_empty() {
        Some("Name the ratio.")
    } else {
        None
    }
}

pub fn look(_app: &App, d: &Dialog) -> Look {
    let ok = Button::new("ok", "OK", ButtonKind::Default).enabled(refused(d).is_none());
    Look { title: TITLE.into(), width: WIDTH, buttons: vec![Button::cancel(), ok] }
}

/// The draft as lines until the Crop tool draws its fields.
pub fn body(app: &mut App, ui: &mut Ui, d: &mut Dialog) {
    super::draft_lines(app, ui, d, &["width", "height", "name"]);
}

pub fn act(_app: &mut App, _d: &Dialog, _action: &str) -> Result<Value, CmdError> {
    Err(CmdError::Disabled("Not available yet.".into()))
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
        ] {
            d.fields.insert("width".into(), w);
            d.fields.insert("height".into(), h);
            d.fields.insert("name".into(), json!(name));
            assert_eq!(refused(&d).is_none(), ok, "{:?}", d.fields);
        }
    }
}
