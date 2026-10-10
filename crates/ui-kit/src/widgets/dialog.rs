//! The dialog frame (`design-system.md` §6): a modal window over a dimmed app.

use egui::{Align, Align2, Area, Context, Frame, Id, Key, Layout, Margin, Modifiers, Rect, RichText, Sense, Stroke, Ui, UiBuilder, UiKind, Vec2};

use super::{Target, primary_button, register, text_button};
use crate::Tokens;
use crate::fonts;
use crate::registry::Role;

/// What a dialog button does besides being clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// The default action: a primary button, and what Enter does.
    Default,
    /// What Esc does.
    Cancel,
    /// Any other action.
    Other,
}

/// One button of a [`dialog`]: its automation id (`button:dialog.ok`), label, kind and whether it
/// can be clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DialogButton<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub kind: ButtonKind,
    pub enabled: bool,
}

/// What a [`dialog`] did this pass.
#[derive(Debug, Clone)]
pub struct DialogResponse<R> {
    /// What the body returned.
    pub inner: R,
    /// The index of the button clicked, or chosen by Enter (the enabled default button) or Esc (the
    /// enabled cancel button).
    pub clicked: Option<usize>,
    /// The dialog's window, in points.
    pub rect: Rect,
}

/// A modal dialog (`design-system.md` §6): the app dimmed by a `backdrop` that takes every click
/// meant for it, and centred on it a window `width` wide (`chrome`, 1 pt `button_border`, radius 6,
/// the window shadow, margin 16 × 12) with the SemiBold 15 `title`, the `body`, and the `buttons`
/// right-aligned, laid out right to left in the order given (so the last is rightmost, and the
/// window never grows to fit them). Esc chooses the cancel button and Enter the default one, unless
/// a field keeps the focus (a multi-line text, which takes Enter itself). Registers `id`
/// (`dialog:<kind>`) on the window, `<id>/backdrop` on the backdrop and each button by its id. Show
/// it every pass while it is open.
pub fn dialog<R>(
    ctx: &Context,
    t: &Tokens,
    id: &str,
    title: &str,
    width: f32,
    buttons: &[DialogButton<'_>],
    body: impl FnOnce(&mut Ui) -> R,
) -> DialogResponse<R> {
    let margin = t.metrics.dialog_margin;
    let width = if width.is_finite() { width.max(120.0) } else { 360.0 };
    let area = Area::new(Id::new(id))
        .kind(UiKind::Modal)
        .sense(Sense::hover())
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .order(egui::Order::Foreground)
        .interactable(true);
    let is_top = ctx.memory_mut(|m| {
        m.set_modal_layer(area.layer());
        m.top_modal_layer() == Some(area.layer())
    });
    let mut clicked = None;
    let shown = area.show(ctx, |ui| {
        let screen = ui.ctx().content_rect();
        let mut backdrop = ui.new_child(UiBuilder::new().sense(Sense::CLICK | Sense::DRAG).max_rect(screen));
        backdrop.set_min_size(screen.size());
        ui.painter().rect_filled(screen, 0.0, t.backdrop);
        let backdrop = backdrop.response();
        register(ui.ctx(), &format!("{id}/backdrop"), Role::Other, title, Target::Widget(&backdrop), false);
        let frame = Frame::new()
            .fill(t.chrome)
            .stroke(Stroke::new(1.0, t.button_border))
            .corner_radius(t.window_radius)
            .shadow(t.window_shadow)
            .inner_margin(Margin::symmetric(margin.x as i8, margin.y as i8));
        let window = ui.scope_builder(UiBuilder::new().sense(Sense::CLICK | Sense::DRAG), |ui| {
            frame
                .show(ui, |ui| {
                    let inner_w = width - 2.0 * margin.x;
                    ui.set_width(inner_w);
                    ui.label(RichText::new(title).font(fonts::semibold(t.title)).color(t.text));
                    ui.add_space(8.0);
                    let inner = body(ui);
                    ui.add_space(12.0);
                    ui.allocate_ui_with_layout(Vec2::new(inner_w, t.metrics.button_h), Layout::right_to_left(Align::Center), |ui| {
                        for (i, b) in buttons.iter().enumerate().rev() {
                            let r = ui
                                .add_enabled_ui(b.enabled, |ui| match b.kind {
                                    ButtonKind::Default => primary_button(ui, t, b.id, b.label),
                                    ButtonKind::Cancel | ButtonKind::Other => text_button(ui, t, b.id, b.label, false),
                                })
                                .inner;
                            if r.clicked() {
                                clicked = Some(i);
                            }
                        }
                    });
                    inner
                })
                .inner
        });
        register(ui.ctx(), id, Role::Other, title, Target::Widget(&window.response), true);
        (window.inner, window.response.rect)
    });
    let (inner, rect) = shown.inner;
    if clicked.is_none() && is_top && !egui::Popup::is_any_open(ctx) {
        let pick = |kind: ButtonKind| buttons.iter().position(|b| b.kind == kind && b.enabled);
        let free = ctx.memory(|m| m.focused().is_none());
        if let Some(i) = pick(ButtonKind::Cancel)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
        {
            clicked = Some(i);
        } else if free
            && let Some(i) = pick(ButtonKind::Default)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            clicked = Some(i);
        }
    }
    DialogResponse { inner, clicked, rect }
}
