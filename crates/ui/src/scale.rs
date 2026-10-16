//! UI scale and font size (`design-system.md` §11.4–§11.5), and the two Interface options read
//! every frame: Show tooltips and Use animations.
//!
//! - **UI scale** is egui's zoom: the factor of `ui-kit`'s
//!   [`UiScale::factor`](dalica_ui_kit::theme::UiScale::factor) (Auto: the operating system's scale,
//!   or 200 % on a monitor of 3840 px or more on its long side when the system does not scale; a
//!   fixed choice as chosen) over the system's own scale, which the window already applies
//!   ([`zoom`]). The monitor is the window's current one, read every frame, so moving the window to
//!   another monitor updates Auto. Widgets never multiply by it themselves. The headless driver has
//!   no monitor: the pixels per point it is given are the system scale, so Auto there is exactly
//!   the scale it was given.
//! - **UI font size** is [`Tokens::with_font_size`], applied to the context's style when it
//!   changes (never compounding).
//! - The Options dialog's preview (`options.previewScale`, `options.previewFontSize`) wins over the
//!   saved settings while the dialog is open, so a choice previews live and reverts when the dialog
//!   closes without an Apply ([`in_effect`]).
//! - **Show tooltips** off makes egui's tooltip delay infinite: no hover tooltip shows, and `ui-kit`'s
//!   [`tooltip`](dalica_ui_kit::widgets::tooltip) adds none.
//! - **Use animations** off makes egui's animations and scroll areas take no time, and
//!   [`animations`] tells the light table's scroll to a section and the edit view's Fit, 1:1 and
//!   pan glide to jump to their end.
//!
//! [`apply`] is called by [`App::frame`](crate::App::frame) at the start of every frame, with or
//! without a library; it writes to the context only when something changed.

use dalica_engine::settings::{FontSize, InterfaceSettings, Settings, UiScaleSetting};
use dalica_ui_kit::Tokens;
use egui::{Context, Id};

use crate::state::{FontChoice, ScaleChoice, UiState};

/// A system scale outside this range (or not a number) counts as 100 %, as in
/// [`UiScale::factor`](dalica_ui_kit::theme::UiScale::factor).
const SYSTEM_RANGE: std::ops::RangeInclusive<f32> = 0.25..=8.0;
/// Zoom factors this close count as equal.
const ZOOM_EPS: f32 = 1e-4;

/// The saved UI scale as the dialog's choice.
pub fn scale_choice(saved: UiScaleSetting) -> ScaleChoice {
    match saved {
        UiScaleSetting::Auto => ScaleChoice::Auto,
        UiScaleSetting::Percent(p) => ScaleChoice::Percent(p),
    }
}

/// The saved UI font size as the dialog's choice.
pub fn font_choice(saved: FontSize) -> FontChoice {
    match saved {
        FontSize::Small => FontChoice::Small,
        FontSize::Medium => FontChoice::Medium,
        FontSize::Large => FontChoice::Large,
    }
}

/// The UI scale and font size in effect: the Options dialog's preview while it is open, else
/// `settings`, else the defaults (Auto, Small).
pub fn in_effect(state: &UiState, settings: Option<&Settings>) -> (ScaleChoice, FontChoice) {
    chosen(state, &settings.map(Settings::interface).unwrap_or_default())
}

fn chosen(state: &UiState, saved: &InterfaceSettings) -> (ScaleChoice, FontChoice) {
    let (scale, font) = (scale_choice(saved.ui_scale), font_choice(saved.font_size));
    let preview = state.options.as_ref();
    (preview.and_then(|o| o.preview_scale).unwrap_or(scale), preview.and_then(|o| o.preview_font_size).unwrap_or(font))
}

/// The system scale `native` as the factor counts it: 1 when unknown or not a number in 0.25 … 8.
fn system(native: Option<f32>) -> f32 {
    native.filter(|n| n.is_finite() && SYSTEM_RANGE.contains(n)).unwrap_or(1.0)
}

/// The UI scale `choice` gives (1.0 = 100 %) on a system scale of `native` (the window's native
/// pixels per point; `None` when unknown) and a monitor `monitor_long_side` physical pixels on its
/// long side (`None` when unknown): `design-system.md` §11.4.
pub fn factor(choice: ScaleChoice, native: Option<f32>, monitor_long_side: Option<u32>) -> f32 {
    choice.ui_scale().factor(system(native), monitor_long_side)
}

/// egui's zoom factor for `choice`: [`factor`] over the system scale the window already applies,
/// so the window's pixels per point (`native × zoom`) is the factor.
pub fn zoom(choice: ScaleChoice, native: Option<f32>, monitor_long_side: Option<u32>) -> f32 {
    factor(choice, native, monitor_long_side) / system(native)
}

/// The long side of the window's current monitor in physical pixels, if the window knows it
/// (egui gives its size in points at this pass's pixels per point).
pub fn monitor_long_side(ctx: &Context) -> Option<u32> {
    let size = ctx.input(|i| i.viewport().monitor_size)?;
    let long = size.x.max(size.y) * ctx.pixels_per_point();
    (long.is_finite() && long >= 1.0).then(|| long.round().min(u32::MAX as f32) as u32)
}

fn animations_off_id() -> Id {
    Id::new("dalica-ui.animations-off")
}

/// Whether Use animations is on (as the last [`apply`] found it; on before the first): off, the
/// light table's scroll to a section, the edit view's Fit and 1:1 and its pan glide jump to their
/// end.
pub fn animations(ctx: &Context) -> bool {
    !ctx.data(|d| d.get_temp::<bool>(animations_off_id())).unwrap_or(false)
}

/// Applies the UI scale and font size in effect ([`in_effect`]) to `ctx` and `tokens`, and the
/// saved Show tooltips and Use animations. Cheap when nothing changed: call it every frame.
pub fn apply(ctx: &Context, state: &UiState, settings: Option<&Settings>, tokens: &mut Tokens) {
    let interface = settings.map(Settings::interface).unwrap_or_default();
    let (scale, font) = chosen(state, &interface);

    let want = zoom(scale, ctx.native_pixels_per_point(), monitor_long_side(ctx));
    if want.is_finite() && want > 0.0 && (ctx.zoom_factor() - want).abs() > ZOOM_EPS {
        ctx.set_zoom_factor(want);
    }

    // The theme again when the font size changed or animations came back on (the theme holds
    // their times); then the options that change its style go on top.
    let sized = tokens.with_font_size(font.font_size());
    let animations_back = interface.animations && !animations(ctx);
    if sized != *tokens || animations_back {
        *tokens = sized;
        tokens.apply(ctx);
    }
    if animations(ctx) != interface.animations {
        ctx.data_mut(|d| d.insert_temp(animations_off_id(), !interface.animations));
    }
    if !interface.animations {
        let none = egui::style::ScrollAnimation::none();
        let style = ctx.global_style();
        if style.animation_time != 0.0 || style.scroll_animation != none {
            ctx.all_styles_mut(|s| {
                s.animation_time = 0.0;
                s.scroll_animation = none;
            });
        }
    }
    let delay = if interface.tooltips { egui::style::Interaction::default().tooltip_delay } else { f32::INFINITY };
    if ctx.global_style().interaction.tooltip_delay != delay {
        ctx.all_styles_mut(|s| s.interaction.tooltip_delay = delay);
    }
}
