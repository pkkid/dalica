//! The slideshow's control bar (`viewing.md` §1.3), on `ui-kit`'s overlay bar:
//!
//! ```text
//! ╭────────────────────────────────────────────────────────────────────────────────────────╮
//! │ [⇦ Exit] [🔍]○━━━━  [↺][↻]  [⇤][❚❚][⇥]  [★]  [▧ Dissolve]▴  [▭☑]     Display time      │
//! │                                                                  [−] 3 seconds [+]    │
//! ╰────────────────────────────────────────────────────────────────────────────────────────╯
//! ```
//!
//! Exit (`slideshow.exit`), the magnifier (fitted ⇄ [`zoom::MAGNIFIER_ZOOM`]) and the zoom slider
//! (1 fitted … 8; zoomed, the show pauses), rotate left and right (`slideshow.rotateLeft`,
//! `.rotateRight`), Previous (greyed on the first photo without Loop), Play / Pause
//! (`slideshow.pause`), Next, Star (lit while the photo is starred), the Transition drop-up of the
//! seven (`slideshow.transition`), the Captions toggle (`slideshow.captions`) and Display time
//! (`slideshow.displayTime`, 1–60 s). The three settings are Options ▸ Slideshow's, kept through
//! `options.set` by their commands.
//!
//! **When it shows** (`viewing.md` §1.2, M11.4's timing): for 2 s when the show starts, for 2.5 s
//! after a mouse move or Space, for good at the end; while the pointer rests on the bar or the
//! drop-up is open it stays. It fades in and out over the overlay bar's 0.2 s on the session's
//! clock (what the show runs on), so a virtual clock drives it in tests.
//!
//! Automation ids (`control-protocol.md` §7): the bar `controls:slideshow`; `button:slideshow.exit`,
//! `button:slideshow.zoom` (the magnifier, active while zoomed), `slider:slideshow.zoom`,
//! `button:slideshow.rotateLeft`, `button:slideshow.rotateRight`, `button:slideshow.previous`,
//! `button:slideshow.play` (labelled Pause while playing), `button:slideshow.next`,
//! `button:slideshow.star`, `button:slideshow.transition` (labelled with the transition) and its
//! drop-up's rows `menuitem:slideshow.transition/<id>` (`cut`, `panAndZoom`, `dissolve`, `wipe`,
//! `circle`, `push`, `rectangle`), `check:slideshow.captions`, and the display time's stepper
//! `stepper:slideshow.displayTime` (its text) with `stepper:slideshow.displayTime/minus` and
//! `/plus`.

use dalica_create::slideshow::{DISPLAY_MAX, DISPLAY_MIN, State, Transition};
use dalica_engine::cmd::slideshow::Frame;
use dalica_ui_kit::icons::{self, EditIcon, Glyph, Icon, ViewingIcon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, OVERLAY_BUTTON, OVERLAY_GLYPH, OVERLAY_PAD, OverlayFade, STEPPER_BUTTON, STEPPER_GAP, Target};
use dalica_ui_kit::{Tokens, fonts};
use egui::{Align, CornerRadius, Id, Layout, Popup, Rect, RectAlign, Response, Sense, Stroke, Ui, Vec2, WidgetType, pos2, vec2};
use serde_json::{Value, json};

use super::zoom;
use crate::app::App;
use crate::commands::slideshow as cmd;
use crate::state::SLIDE_ZOOM_MAX;

/// The bar's automation id.
pub const BAR_ID: &str = "controls:slideshow";
pub const EXIT_ID: &str = "button:slideshow.exit";
pub const ZOOM_ID: &str = "button:slideshow.zoom";
pub const ZOOM_SLIDER_ID: &str = "slider:slideshow.zoom";
pub const ROTATE_LEFT_ID: &str = "button:slideshow.rotateLeft";
pub const ROTATE_RIGHT_ID: &str = "button:slideshow.rotateRight";
pub const PREVIOUS_ID: &str = "button:slideshow.previous";
pub const PLAY_ID: &str = "button:slideshow.play";
pub const NEXT_ID: &str = "button:slideshow.next";
pub const STAR_ID: &str = "button:slideshow.star";
pub const TRANSITION_ID: &str = "button:slideshow.transition";
pub const CAPTIONS_ID: &str = "check:slideshow.captions";
pub const DISPLAY_TIME_ID: &str = "stepper:slideshow.displayTime";
/// The label over the display time's stepper.
pub const DISPLAY_TIME: &str = "Display time";
/// Why Previous is greyed on the first photo without Loop.
pub const FIRST_PHOTO: &str = "This is the first photo (turn on Loop slideshow to go round).";

/// The bar's height (points): the display time's label over its stepper, and the padding.
pub const BAR_H: f32 = 60.0;
/// The least room under the slideshow's caption (points): the bar's place and a gap, so the caption
/// stays clear of the bar.
pub const CAPTION_CLEAR: f32 = widgets::OVERLAY_BOTTOM + BAR_H + 8.0;
/// The zoom slider's track (points).
pub const SLIDER_W: f32 = 72.0;
/// The extra gap between two groups of controls (points, on top of the item spacing).
const GROUP_GAP: f32 = 8.0;
/// The item spacing the overlay bar lays its controls out with (points).
const SPACING: f32 = 6.0;
/// The room the drop-up's chevron takes at the Transition button's right (points).
const CHEVRON_W: f32 = 12.0;
/// The Captions toggle: its glyph and its checkbox (points).
const CAPTIONS_W: f32 = OVERLAY_BUTTON + 18.0;

/// The id of the Transition drop-up.
pub fn transition_popup() -> Id {
    Id::new(TRANSITION_ID).with("popup")
}

/// The automation id of the drop-up's row for `t`.
pub fn transition_item_id(t: Transition) -> String {
    format!("menuitem:slideshow.transition/{}", t.id())
}

/// The stepper's text for `secs`.
pub fn seconds_text(secs: i32) -> String {
    if secs == 1 { "1 second".into() } else { format!("{secs} seconds") }
}

/// What [`show`] drew.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarOut {
    /// The bar's rect while it shows (any opacity above 0).
    pub rect: Option<Rect>,
    /// A fade is running: the caller asks for frames.
    pub fading: bool,
}

/// What the bar shows of the show and its settings.
struct Shown {
    playing: bool,
    can_previous: bool,
    starred: bool,
    zoom: f32,
    transition: Transition,
    captions: bool,
    display: i32,
}

/// What a click asks for.
enum Act {
    Run(&'static str, Value),
    Zoom(f32),
}

/// The width of an overlay button with a label (`overlay_button`'s own sum).
fn labelled_w(ui: &Ui, t: &Tokens, label: &str) -> f32 {
    let g = ui.painter().layout_no_wrap(label.to_owned(), fonts::regular(t.body), t.overlay_text);
    (10.0 + OVERLAY_GLYPH + 6.0 + g.size().x + 12.0).ceil()
}

fn text_w(ui: &Ui, t: &Tokens, text: &str) -> f32 {
    ui.painter().layout_no_wrap(text.to_owned(), fonts::regular(t.body), t.overlay_text).size().x
}

/// The widest Transition button (its label the longest name).
fn transition_w(ui: &Ui, t: &Tokens) -> f32 {
    Transition::ALL.iter().map(|tr| labelled_w(ui, t, tr.label())).fold(0.0, f32::max)
}

/// The display time's column: its label over its stepper.
fn display_w(ui: &Ui, t: &Tokens) -> f32 {
    let widest = text_w(ui, t, &seconds_text(DISPLAY_MAX as i32)).max(text_w(ui, t, &seconds_text(DISPLAY_MIN as i32)));
    let stepper = 2.0 * STEPPER_BUTTON + widest.ceil() + 2.0 * STEPPER_GAP + 2.0 * SPACING;
    stepper.max(text_w(ui, t, DISPLAY_TIME)).ceil()
}

/// The bar's width: every control, the spacing between them and the padding.
pub fn bar_width(ui: &Ui, t: &Tokens) -> f32 {
    let items = [
        labelled_w(ui, t, "Exit"),
        OVERLAY_BUTTON,
        SLIDER_W,
        OVERLAY_BUTTON,
        OVERLAY_BUTTON,
        OVERLAY_BUTTON,
        OVERLAY_BUTTON,
        OVERLAY_BUTTON,
        OVERLAY_BUTTON,
        transition_w(ui, t) + CHEVRON_W,
        CAPTIONS_W,
        display_w(ui, t),
    ];
    let groups = 6.0;
    items.iter().sum::<f32>() + SPACING * (items.len() - 1) as f32 + GROUP_GAP * groups + 2.0 * OVERLAY_PAD.x + 2.0
}

/// The fade kept between frames, per egui context.
fn fade_id() -> Id {
    Id::new(BAR_ID).with("fade")
}

/// Draws the bar along the bottom of `area` for `frame` (shown or hidden as the show says, faded on
/// the session's clock) and runs what was clicked.
pub fn show(app: &mut App, ui: &mut Ui, area: Rect, frame: &Frame) -> BarOut {
    let t = *app.tokens();
    let ctx = ui.ctx().clone();
    let Some(session) = app.session() else { return BarOut { rect: None, fading: false } };
    let now = session.clock().now().as_millis() as f64 / 1000.0;
    let settings = session.settings().slideshow();
    let v = frame.view;
    let starred = frame.photo.and_then(|(id, _)| session.library().photo(id)).is_some_and(|p| p.star);
    let zoom_now = cmd::show(app).map_or(1.0, |s| s.zoom);
    let shown = Shown {
        playing: v.state == State::Playing,
        can_previous: v.can_previous,
        starred,
        zoom: zoom_now,
        transition: Transition::from_id(settings.transition.id()).unwrap_or_default(),
        captions: settings.captions,
        display: i32::try_from(settings.display_time).unwrap_or(DISPLAY_MAX as i32),
    };

    // Kept up while the pointer rests on it or the drop-up is open.
    let rect = widgets::overlay_bar_rect(area, vec2(bar_width(ui, &t).min(area.width() - 16.0), BAR_H));
    let over = ctx.pointer_hover_pos().is_some_and(|p| rect.contains(p));
    let menu_open = Popup::is_id_open(&ctx, transition_popup());
    let mut fade = ctx.data(|d| d.get_temp::<OverlayFade>(fade_id())).unwrap_or_else(|| OverlayFade::new(v.bar));
    if (over || menu_open) && fade.visible() {
        cmd::with_show(|s| s.mouse_moved());
    }
    fade.set(v.bar || ((over || menu_open) && fade.visible()), now);
    let opacity = fade.opacity(now);
    let fading = fade.fading(now);
    ctx.data_mut(|d| d.insert_temp(fade_id(), fade));

    let mut act = None;
    let drawn = widgets::overlay_bar(ui, &t, BAR_ID, rect, opacity, |ui| controls(ui, &t, &shown, &mut act));
    match act {
        Some(Act::Run(id, params)) => {
            let _ = app.run(id, &params);
        }
        Some(Act::Zoom(z)) => {
            let _ = cmd::set_zoom(app, z);
        }
        None => {}
    }
    BarOut { rect: drawn.map(|_| rect), fading }
}

/// The bar's controls, left to right.
fn controls(ui: &mut Ui, t: &Tokens, s: &Shown, act: &mut Option<Act>) {
    let run = |act: &mut Option<Act>, id: &'static str, params: Value| *act = Some(Act::Run(id, params));
    if widgets::overlay_button(ui, t, EXIT_ID, ViewingIcon::Exit.into(), Some("Exit"), false, true, "Exit the slideshow (Esc)").clicked() {
        run(act, "slideshow.exit", json!({}));
    }
    ui.add_space(GROUP_GAP);
    let zoomed = s.zoom > 1.0;
    if widgets::overlay_button(ui, t, ZOOM_ID, Icon::Search.into(), None, zoomed, true, "Zoom (pauses the slideshow)").clicked() {
        *act = Some(Act::Zoom(if zoomed { 1.0 } else { zoom::MAGNIFIER_ZOOM }));
    }
    if let Some(z) = zoom_slider(ui, t, s.zoom) {
        *act = Some(Act::Zoom(z));
    }
    ui.add_space(GROUP_GAP);
    if widgets::overlay_button(ui, t, ROTATE_LEFT_ID, Icon::RotateCcw.into(), None, false, true, "Rotate counterclockwise").clicked() {
        run(act, "slideshow.rotateLeft", json!({}));
    }
    if widgets::overlay_button(ui, t, ROTATE_RIGHT_ID, Icon::RotateCw.into(), None, false, true, "Rotate clockwise").clicked() {
        run(act, "slideshow.rotateRight", json!({}));
    }
    ui.add_space(GROUP_GAP);
    let tip = if s.can_previous { "Previous photo" } else { FIRST_PHOTO };
    if widgets::overlay_button(ui, t, PREVIOUS_ID, ViewingIcon::Previous.into(), None, false, s.can_previous, tip).clicked() {
        run(act, "slideshow.previous", json!({}));
    }
    let (glyph, tip): (Glyph, &str) = if s.playing { (ViewingIcon::Pause.into(), "Pause") } else { (EditIcon::Play.into(), "Play") };
    if widgets::overlay_button(ui, t, PLAY_ID, glyph, None, false, true, tip).clicked() {
        run(act, "slideshow.pause", json!({}));
    }
    if widgets::overlay_button(ui, t, NEXT_ID, ViewingIcon::Next.into(), None, false, true, "Next photo").clicked() {
        run(act, "slideshow.next", json!({}));
    }
    ui.add_space(GROUP_GAP);
    let star: Glyph = if s.starred { Icon::Star.into() } else { Icon::StarOutline.into() };
    if widgets::overlay_button(ui, t, STAR_ID, star, None, s.starred, true, "Star").clicked() {
        run(act, "slideshow.star", json!({}));
    }
    ui.add_space(GROUP_GAP);
    if let Some(tr) = transition(ui, t, s.transition) {
        run(act, "slideshow.transition", json!({ "transition": tr.id() }));
    }
    ui.add_space(GROUP_GAP);
    if captions(ui, t, s.captions).clicked() {
        run(act, "slideshow.captions", json!({ "on": !s.captions }));
    }
    ui.add_space(GROUP_GAP);
    if let Some(secs) = display_time(ui, t, s.display) {
        run(act, "slideshow.displayTime", json!({ "seconds": secs }));
    }
}

/// The zoom slider (1 fitted … [`SLIDE_ZOOM_MAX`], light on dark); answers the zoom a click or a
/// drag on it asks for.
fn zoom_slider(ui: &mut Ui, t: &Tokens, zoom: f32) -> Option<f32> {
    let (_, rect) = ui.allocate_space(vec2(SLIDER_W, OVERLAY_BUTTON));
    let r = ui.interact(rect, Id::new(ZOOM_SLIDER_ID), Sense::click_and_drag());
    let track = rect.shrink2(vec2(6.0, 0.0));
    let span = SLIDE_ZOOM_MAX - 1.0;
    let at = |z: f32| track.min.x + track.width() * ((z - 1.0) / span).clamp(0.0, 1.0);
    let p = ui.painter();
    let y = rect.center().y;
    let knob_x = at(if zoom.is_finite() { zoom } else { 1.0 });
    p.line_segment([pos2(track.min.x, y), pos2(track.max.x, y)], Stroke::new(2.0, t.overlay_disabled));
    p.line_segment([pos2(track.min.x, y), pos2(knob_x, y)], Stroke::new(2.0, t.overlay_icon));
    let knob = if r.hovered() || r.dragged() { t.overlay_text } else { t.overlay_icon };
    p.circle_filled(pos2(knob_x, y), 6.0, knob);
    widgets::focus_ring(p, t, &r, rect, t.radius);
    widgets::register(ui.ctx(), ZOOM_SLIDER_ID, Role::Other, "Zoom", Target::Widget(&r), r.dragged());
    r.widget_info(|| egui::WidgetInfo::slider(true, f64::from(zoom), "Zoom"));
    let pointer = r.interact_pointer_pos().filter(|_| r.dragged() || r.clicked())?;
    let f = if track.width() > 0.0 { ((pointer.x - track.min.x) / track.width()).clamp(0.0, 1.0) } else { 0.0 };
    let z = 1.0 + f * span;
    (z - zoom).abs().gt(&1e-3).then_some(z)
}

/// The Transition button (its glyph, the transition's name and the drop-up's chevron, as wide as
/// the widest name, so the bar never moves) and its drop-up of the seven; answers the one picked.
fn transition(ui: &mut Ui, t: &Tokens, current: Transition) -> Option<Transition> {
    let open = Popup::is_id_open(ui.ctx(), transition_popup());
    let (_, rect) = ui.allocate_space(vec2(transition_w(ui, t) + CHEVRON_W, OVERLAY_BUTTON));
    let r = ui.interact(rect, Id::new(TRANSITION_ID), Sense::click());
    let p = ui.painter();
    if open || r.is_pointer_button_down_on() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_pressed);
    } else if r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_hover);
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if open || r.hovered() { t.overlay_text } else { t.overlay_icon };
    let glyph = Rect::from_center_size(pos2(rect.min.x + 10.0 + OVERLAY_GLYPH / 2.0, rect.center().y), Vec2::splat(OVERLAY_GLYPH));
    icons::paint_glyph(p, glyph, ViewingIcon::Transition.into(), color);
    let g = p.layout_no_wrap(current.label().to_owned(), fonts::regular(t.body), t.overlay_text);
    p.galley(pos2(glyph.max.x + 6.0, rect.center().y - g.size().y / 2.0).round(), g, t.overlay_text);
    let mark = Rect::from_center_size(pos2(rect.max.x - 12.0, rect.center().y), Vec2::splat(10.0));
    icons::paint(p, mark, Icon::ChevronUp, color);
    let tip = "Transition";
    widgets::register(ui.ctx(), TRANSITION_ID, Role::Button, tip, Target::Widget(&r), open);
    r.widget_info(|| egui::WidgetInfo::labeled(WidgetType::ComboBox, true, current.label()));
    let r = widgets::tooltip(r, t, tip, None);
    let mut picked = None;
    Popup::menu(&r).id(transition_popup()).align(RectAlign::TOP_START).align_alternatives(&[RectAlign::BOTTOM_START]).show(|ui| {
        for tr in Transition::ALL {
            if widgets::dropdown_item(ui, t, &transition_item_id(tr), tr.label(), "", tr == current).clicked() {
                picked = Some(tr);
            }
        }
    });
    picked.filter(|p| *p != current)
}

/// The Captions toggle: the captions glyph and a checkbox, lit while captions are on.
fn captions(ui: &mut Ui, t: &Tokens, on: bool) -> Response {
    let (_, rect) = ui.allocate_space(vec2(CAPTIONS_W, OVERLAY_BUTTON));
    let r = ui.interact(rect, Id::new(CAPTIONS_ID), Sense::click());
    let p = ui.painter();
    if r.is_pointer_button_down_on() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_pressed);
    } else if r.hovered() {
        p.rect_filled(rect, CornerRadius::same(t.radius), t.overlay_hover);
    }
    widgets::focus_ring(p, t, &r, rect, t.radius);
    let color = if on || r.hovered() { t.overlay_text } else { t.overlay_icon };
    let glyph = Rect::from_center_size(pos2(rect.min.x + OVERLAY_BUTTON / 2.0, rect.center().y), Vec2::splat(OVERLAY_GLYPH));
    icons::paint_glyph(p, glyph, ViewingIcon::Captions.into(), color);
    let boxed = Rect::from_center_size(pos2(rect.max.x - 11.0, rect.center().y), Vec2::splat(12.0));
    p.rect_stroke(boxed, CornerRadius::same(2), Stroke::new(1.2, color), egui::StrokeKind::Inside);
    if on {
        let (a, b, c) =
            (pos2(boxed.min.x + 2.5, boxed.center().y), pos2(boxed.min.x + 5.0, boxed.max.y - 3.0), pos2(boxed.max.x - 2.5, boxed.min.y + 3.0));
        p.line_segment([a, b], Stroke::new(1.6, color));
        p.line_segment([b, c], Stroke::new(1.6, color));
    }
    let tip = "Show captions";
    widgets::register(ui.ctx(), CAPTIONS_ID, Role::Toggle, tip, Target::Widget(&r), on);
    r.widget_info(|| egui::WidgetInfo::selected(WidgetType::Checkbox, true, on, tip));
    widgets::tooltip(r, t, tip, None)
}

/// Display time: its label over `[−] 3 seconds [+]`; answers the new value a click asked for.
fn display_time(ui: &mut Ui, t: &Tokens, secs: i32) -> Option<i32> {
    let w = display_w(ui, t);
    let h = ui.available_height();
    let mut out = None;
    ui.allocate_ui_with_layout(vec2(w, h), Layout::top_down(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let g = ui.painter().layout_no_wrap(DISPLAY_TIME.to_owned(), fonts::regular(t.small), t.overlay_icon);
        let (_, label) = ui.allocate_space(vec2(w, g.size().y));
        ui.painter().galley(pos2(label.center().x - g.size().x / 2.0, label.min.y).round(), g, t.overlay_icon);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SPACING;
            let s = widgets::stepper(ui, t, DISPLAY_TIME_ID, secs, DISPLAY_MIN as i32..=DISPLAY_MAX as i32, seconds_text);
            if s.changed {
                out = Some(s.value);
            }
        });
    });
    out
}
