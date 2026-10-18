//! The controls of an effect's parameter panel, read from its catalogue row (`editing.md` §8.1,
//! `dalica_edit::catalog`): never a hand list. In the panel's order: the sliders (each number the
//! crosshair does not set, Fade the last of them where the effect has it), the checkboxes, the
//! choices, then the colour swatches. A crosshair effect's centre (and Graduated Tint's angle) is
//! the crosshair's (M12.17), Soften's strokes its brush's (M12.18): neither has a control here.

use dalica_edit::catalog::{Crosshair, Kind, OpInfo, Param};
use dalica_ui_kit::Tokens;
use dalica_ui_kit::widgets::{SWATCH_SIZE, SliderScale, SliderSpec};

/// The gap between two control rows.
pub const ROW_GAP: f32 = 2.0;
/// A swatch row's height: the 22 pt swatch and its label beside it.
pub const SWATCH_ROW_H: f32 = 28.0;
/// The most sliders a panel holds (`editing.md` §8.1).
pub const MAX_SLIDERS: usize = 4;

/// One control of a panel, on the catalogue parameter it edits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// An effect slider (`slider:effect.<param>`), on its scale.
    Slider(&'static Param, f64, f64),
    /// A checkbox (`check:effect.<param>`).
    Check(&'static Param),
    /// A choice among words (`choice:effect.<param>`).
    Choice(&'static Param, &'static [&'static str]),
    /// A colour swatch (`color:effect.<param>`) with its picker popup and eyedropper.
    Swatch(&'static Param),
}

impl Control {
    pub fn param(&self) -> &'static Param {
        match *self {
            Control::Slider(p, ..) | Control::Check(p) | Control::Choice(p, _) | Control::Swatch(p) => p,
        }
    }

    /// The control's automation id.
    pub fn id(&self) -> String {
        let prefix = match self {
            Control::Slider(..) => "slider",
            Control::Check(_) => "check",
            Control::Choice(..) => "choice",
            Control::Swatch(_) => "color",
        };
        format!("{prefix}:effect.{}", self.param().name)
    }

    /// The row's height.
    pub fn height(&self, t: &Tokens) -> f32 {
        match self {
            Control::Slider(..) => t.metrics.slider_row_h,
            Control::Check(_) => t.metrics.text_field_h,
            Control::Choice(..) => t.metrics.text_field_h.max(t.metrics.interact_h),
            Control::Swatch(_) => SWATCH_ROW_H.max(SWATCH_SIZE.y),
        }
    }
}

/// The parameters of `row` its crosshair sets on the photo: a circle's centre, a gradient's start
/// and angle.
pub fn crosshair_params(row: &OpInfo) -> &'static [&'static str] {
    match row.effect.map(|e| e.crosshair) {
        Some(Crosshair::Circle) => &["x", "y"],
        Some(Crosshair::Gradient) => &["x", "y", "angle"],
        _ => &[],
    }
}

/// The controls of effect `row`, in the panel's order.
pub fn controls(row: &'static OpInfo) -> Vec<Control> {
    let crosshair = crosshair_params(row);
    let params = row.params.iter().filter(|p| !crosshair.contains(&p.name));
    let mut sliders = Vec::new();
    let mut checks = Vec::new();
    let mut choices = Vec::new();
    let mut swatches = Vec::new();
    for p in params {
        match p.kind {
            Kind::Number { min, max } if min.is_finite() && max.is_finite() && min < max => sliders.push(Control::Slider(p, min, max)),
            Kind::Flag => checks.push(Control::Check(p)),
            Kind::Choice(words) if !words.is_empty() => choices.push(Control::Choice(p, words)),
            Kind::Colour { .. } => swatches.push(Control::Swatch(p)),
            // A number without a settled range, a time, strokes: no control of the panel's.
            _ => {}
        }
    }
    sliders.truncate(MAX_SLIDERS);
    sliders.into_iter().chain(checks).chain(choices).chain(swatches).collect()
}

/// The height of `list`'s rows with the gaps between them.
pub fn rows_height(t: &Tokens, list: &[Control]) -> f32 {
    let rows: f32 = list.iter().map(|c| c.height(t)).sum();
    rows + ROW_GAP * list.len().saturating_sub(1) as f32
}

/// Whether `v` is a whole number.
fn whole(v: f64) -> bool {
    v.is_finite() && v.fract() == 0.0
}

/// The step and decimals of a slider over `min … max` with `default`: whole numbers over a range of
/// ten or more whose ends and default are whole (Fade, Grain, Pixel Size); tenths over a wider
/// range; hundredths otherwise, and always on a logarithmic one (Glow's Radius from 0.3).
pub fn step_of(min: f64, max: f64, default: f64, log: bool) -> (f64, u8) {
    let span = max - min;
    if log {
        (0.01, 2)
    } else if span >= 10.0 && whole(min) && whole(max) && whole(default) {
        (1.0, 0)
    } else if span > 10.0 {
        (0.1, 1)
    } else {
        (0.01, 2)
    }
}

/// The catalogue default of number parameter `p`, inside `min … max` (`min` when it does not read).
pub fn number_default(p: &Param, min: f64, max: f64) -> f64 {
    p.default.parse::<f64>().ok().filter(|v| v.is_finite()).map_or(min, |v| v.clamp(min, max))
}

/// The slider spec of control `id` over parameter `p`.
pub fn slider_spec<'a>(id: &'a str, p: &'a Param, min: f64, max: f64) -> SliderSpec<'a> {
    let default = number_default(p, min, max);
    let (step, decimals) = step_of(min, max, default, p.log);
    SliderSpec { id, label: p.label, min, max, default, step, decimals, unit: "" }
}

/// The slider's scale: logarithmic where the catalogue says so (Glow's Radius).
pub fn scale_of(p: &Param) -> SliderScale {
    if p.log { SliderScale::Log } else { SliderScale::Linear }
}

/// A choice's word as the menu shows it: `soft_light` → "Soft Light".
pub fn choice_label(word: &str) -> String {
    word.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dalica_edit::catalog;

    fn ids(name: &str) -> Vec<String> {
        controls(catalog::lookup(name).unwrap()).iter().map(Control::id).collect()
    }

    #[test]
    fn each_effect_gets_the_controls_its_row_lists_in_the_panels_order() {
        assert_eq!(ids("grain"), ["slider:effect.grain", "check:effect.lighten"]);
        assert_eq!(ids("tint"), ["slider:effect.fade", "color:effect.color"]);
        assert_eq!(ids("soft_focus"), ["slider:effect.size", "slider:effect.amount"], "the centre is the crosshair's");
        assert_eq!(ids("graduated_tint"), ["slider:effect.feather", "slider:effect.shade", "color:effect.color"]);
        assert_eq!(ids("pixelate"), ["slider:effect.size", "slider:effect.fade", "choice:effect.mode"]);
        assert_eq!(ids("soften"), ["slider:effect.softness", "slider:effect.fade"], "the strokes are the brush's");
        assert_eq!(
            ids("duotone"),
            ["slider:effect.brightness", "slider:effect.contrast", "slider:effect.fade", "color:effect.first", "color:effect.second"]
        );
        for (op, _) in catalog::TABS.iter().flat_map(|t| catalog::effects(*t)) {
            let list = controls(op);
            assert!(list.iter().filter(|c| matches!(c, Control::Slider(..))).count() <= MAX_SLIDERS, "{}", op.name);
        }
    }

    #[test]
    fn sliders_take_steps_that_fit_their_range() {
        assert_eq!(step_of(0.0, 100.0, 0.0, false), (1.0, 0));
        assert_eq!(step_of(-95.0, 95.0, 0.0, false), (1.0, 0));
        assert_eq!(step_of(0.0, 3.0, 0.6, false), (0.01, 2));
        assert_eq!(step_of(1.3, 80.0, 20.0, false), (0.1, 1));
        assert_eq!(step_of(0.3, 30.0, 3.0, true), (0.01, 2));
        let glow = catalog::lookup("glow").unwrap();
        let r = glow.params.iter().find(|p| p.name == "r").unwrap();
        assert_eq!(scale_of(r), SliderScale::Log);
        let spec = slider_spec("slider:effect.r", r, 0.3, 30.0);
        assert_eq!((spec.default, spec.label), (3.0, "Radius"));
        assert_eq!(number_default(&Param { default: "nope", ..*r }, 0.3, 30.0), 0.3);
    }

    #[test]
    fn choice_words_read_as_titles() {
        assert_eq!(choice_label("soft_light"), "Soft Light");
        assert_eq!(choice_label("normal"), "Normal");
        assert_eq!(choice_label("__"), "");
    }
}
