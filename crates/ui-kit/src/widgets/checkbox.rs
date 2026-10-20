//! Checkbox rows (`design-system.md` §6 and §11.3): the settings dialogs' checkbox on M8.5's form
//! rows, with what an option says about itself: a hint line under it ("Applies at next launch.")
//! and, for an option that cannot be changed yet, a disabled state whose reason is its tooltip
//! ("Arrives with Import"). Dialogs use it in place of egui's own checkbox, so every option has an
//! automation id, the theme's look and the same click target.

use egui::{Rect, Response, Ui, vec2};

use super::form::{CHOICE_LABEL_X, checkbox, hint};
use super::tooltip;
use crate::Tokens;

/// What a [`checkbox_row`] drew.
#[derive(Debug, Clone)]
pub struct CheckboxRowOut {
    /// The checkbox's line: `changed()` when a click flipped it.
    pub response: Response,
    /// Where the hint went, when there is one.
    pub hint: Option<Rect>,
}

/// A [`checkbox`] (`id`, `label`, `on`) with its hint line under it: `hint` in 11 pt `text_dim`,
/// starting under the label ([`CHOICE_LABEL_X`] in) and wrapped to the width left. With
/// `disabled` (the reason it cannot be changed), the row is drawn disabled (the box, the mark, the
/// label and the hint `text_disabled`), ignores clicks and keys, registers as disabled, and shows
/// the reason as its tooltip; `on` is never changed. An enabled row has no tooltip of its own.
pub fn checkbox_row(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    label: &str,
    on: &mut bool,
    hint_text: Option<&str>,
    disabled: Option<&str>,
) -> CheckboxRowOut {
    let reason = disabled.filter(|r| !r.is_empty());
    let enabled = disabled.is_none();
    ui.add_enabled_ui(enabled, |ui| {
        let mut value = *on;
        let response = checkbox(ui, t, id, label, &mut value);
        if enabled && ui.is_enabled() {
            *on = value;
        }
        // The hint (or the reason it cannot be changed) is what a screen reader adds after the name.
        if let Some(more) = reason.or(hint_text.filter(|h| !h.is_empty())) {
            super::access(&response, |node| node.set_description(more));
        }
        let response = match reason {
            Some(reason) => tooltip(response, t, reason, None),
            None => response,
        };
        let hint = hint_text.filter(|h| !h.is_empty()).map(|h| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.allocate_space(vec2(CHOICE_LABEL_X, 0.0));
                hint(ui, t, h)
            })
            .inner
        });
        CheckboxRowOut { response, hint }
    })
    .inner
}
