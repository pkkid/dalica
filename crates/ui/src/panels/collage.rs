//! The Collage room (`creating.md` §3, `ui-layout.md` §8): under the tab strip while it is in
//! front, the room's content is `region:collage`; on its left the Settings and Clips tabs
//! (`tab:collage/settings`, `tab:collage/clips`, the latter labelled "Clips (N)"; a click runs
//! `collage.tab`) over their page ([`settings`], [`clips`]), on its right the canvas
//! (`canvas:collage`, [`canvas`]). Everything the room shows of the collage comes from the engine's
//! collage session (`Session::collage`, `collage.inspect`, M14.16), never from the disk.
//!
//! Each frame the room tells the engine the display's size (for Current Display,
//! `crate::commands::collage::set_display`). While the background's eyedropper is armed
//! ([`settings::eyedropper_armed`]) a click on the canvas's page picks the colour under it
//! (`canvas:collage/eyedropper`, over the canvas so it takes that click; [`settings::pick`]).

pub mod canvas;
pub mod clips;
pub mod settings;

use dalica_ui_kit::registry::{self, Role};
use dalica_ui_kit::widgets::{self, Target, TextTab};
use egui::{CursorIcon, Id, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use serde_json::json;

use crate::app::App;
use crate::state::CollageTab;

/// The room's content, for `ui.screenshot {region: "collage"}` and `ui.widgets {region}`.
pub const REGION_ID: &str = "region:collage";
/// The left panel's tab strip: its tabs are `tab:collage/settings` and `tab:collage/clips`.
pub const TABS_ID: &str = "tab:collage";
/// The eyedropper's catch over the canvas's page while it is armed.
pub const EYEDROPPER_ID: &str = "canvas:collage/eyedropper";
/// The left panel's width (points), the room allowing.
pub const PANEL_W: f32 = 300.0;
/// The space round the left panel's page.
pub const PAGE_PAD: f32 = 12.0;
/// The tabs: their names (as `collage.tab` takes them) and labels.
pub const TABS: [(CollageTab, &str, &str); 2] = [(CollageTab::Settings, "settings", "Settings"), (CollageTab::Clips, "clips", "Clips")];

/// The Clips tab's label: "Clips (N)".
pub fn clips_label(n: usize) -> String {
    format!("Clips ({n})")
}

/// The display's size in pixels: the monitor's when the window knows it, else the window's.
fn display_size(ctx: &egui::Context) -> [u32; 2] {
    ctx.input(|i| {
        let ppp = i.pixels_per_point;
        let points = i.viewport().monitor_size.filter(|s| s.x >= 1.0 && s.y >= 1.0).unwrap_or(i.content_rect().size());
        let px = |v: f32| if (v * ppp).is_finite() { (v * ppp).round().clamp(0.0, 16_384.0) as u32 } else { 0 };
        [px(points.x), px(points.y)]
    })
}

/// Draws the room into `ui`'s rect (the room's region under the tab strip).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    widgets::register(ui.ctx(), REGION_ID, Role::Other, "Collage", Target::Painted { rect, clip: rect }, false);
    crate::commands::collage::set_display(app, display_size(ui.ctx()));
    let split = (rect.min.x + PANEL_W).min(rect.max.x);
    let left = Rect::from_min_max(rect.min, pos2(split, rect.max.y));
    let right = Rect::from_min_max(pos2(split, rect.min.y), rect.max);
    ui.painter().rect_filled(left, 0, t.chrome);

    let mut panel = ui.new_child(UiBuilder::new().id_salt("collagePanel").max_rect(left));
    let clips = clips_label(app.session().and_then(|s| s.collage().document()).map_or(0, |d| d.clips.len()));
    let labels = [TABS[0].2, clips.as_str()];
    let items: Vec<TextTab<'_>> = TABS.iter().zip(labels).map(|((_, name, _), label)| TextTab { name, label }).collect();
    let current = app.state().collage.tab;
    let active = TABS.iter().position(|(tab, ..)| *tab == current).unwrap_or(0);
    let out = widgets::text_tabs(&mut panel, &t, TABS_ID, &items, active);
    let page = Rect::from_min_max(pos2(left.min.x, out.rect.max.y), left.max);
    widgets::tab_page(ui.painter(), &t, page);
    let mut body = ui.new_child(UiBuilder::new().id_salt("collagePage").max_rect(page.shrink(PAGE_PAD)));
    match current {
        CollageTab::Settings => settings::show(app, &mut body),
        CollageTab::Clips => clips::show(app, &mut body),
    }
    let mut canvas_ui = ui.new_child(UiBuilder::new().id_salt("collageCanvas").max_rect(right));
    canvas::show(app, &mut canvas_ui);
    eyedropper(app, ui, right);

    if let Some((tab, ..)) = out.clicked.and_then(|i| TABS.get(i)).filter(|(tab, ..)| *tab != current)
        && let Err(e) = app.run("collage.tab", &json!({ "tab": tab }))
    {
        app.toast(&e.to_string(), widgets::ToastKind::Caution);
    }
}

/// While the eyedropper is armed: a crosshair over the canvas's page, and a click there picks the
/// colour under it as the background's ([`settings::pick`]).
fn eyedropper(app: &mut App, ui: &mut Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    if !settings::eyedropper_armed(&ctx) {
        return;
    }
    let page = registry::find(&ctx, canvas::CANVAS_ID).map(|w| Rect::from_min_size(pos2(w.rect[0], w.rect[1]), vec2(w.rect[2], w.rect[3])));
    let Some(page) = page.map(|p| p.intersect(area)).filter(|p| p.is_positive()) else { return };
    let r = ui.interact(page, Id::new(EYEDROPPER_ID), Sense::click());
    widgets::register(&ctx, EYEDROPPER_ID, Role::Other, "Pick the background colour", Target::Widget(&r), true);
    if r.hovered() {
        ctx.set_cursor_icon(CursorIcon::Crosshair);
    }
    if r.clicked()
        && let Some(pos) = r.interact_pointer_pos()
    {
        let at = [f64::from((pos.x - page.min.x) / page.width()), f64::from((pos.y - page.min.y) / page.height())];
        settings::pick(app, &ctx, at);
    }
}
