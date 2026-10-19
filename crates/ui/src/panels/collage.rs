//! The Collage room (`creating.md` §3, `ui-layout.md` §8), a stub until M14.18 and M14.19 draw it:
//! under the tab strip while it is in front, the room's content is `region:collage`; on its left
//! the Settings and Clips tabs (`tab:collage/settings`, `tab:collage/clips`; a click runs
//! `collage.tab`) over their page ([`settings`], [`clips`]), on its right the canvas
//! (`canvas:collage`, [`canvas`]). Everything the room shows of the collage comes from the engine's
//! collage session (`collage.inspect`, M14.16), never from the disk.

pub mod canvas;
pub mod clips;
pub mod settings;

use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target, TextTab};
use egui::{Rect, Ui, UiBuilder, pos2};
use serde_json::json;

use crate::app::App;
use crate::state::CollageTab;

/// The room's content, for `ui.screenshot {region: "collage"}` and `ui.widgets {region}`.
pub const REGION_ID: &str = "region:collage";
/// The left panel's tab strip: its tabs are `tab:collage/settings` and `tab:collage/clips`.
pub const TABS_ID: &str = "tab:collage";
/// The left panel's width (points), the room allowing.
pub const PANEL_W: f32 = 300.0;
/// The tabs: their names (as `collage.tab` takes them) and labels.
pub const TABS: [(CollageTab, &str, &str); 2] = [(CollageTab::Settings, "settings", "Settings"), (CollageTab::Clips, "clips", "Clips")];

/// Draws the room into `ui`'s rect (the room's region under the tab strip).
pub fn show(app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    widgets::register(ui.ctx(), REGION_ID, Role::Other, "Collage", Target::Painted { rect, clip: rect }, false);
    let split = (rect.min.x + PANEL_W).min(rect.max.x);
    let left = Rect::from_min_max(rect.min, pos2(split, rect.max.y));
    let right = Rect::from_min_max(pos2(split, rect.min.y), rect.max);
    ui.painter().rect_filled(left, 0, t.chrome);

    let mut panel = ui.new_child(UiBuilder::new().id_salt("collagePanel").max_rect(left));
    let items: Vec<TextTab<'_>> = TABS.iter().map(|(_, name, label)| TextTab { name, label }).collect();
    let current = app.state().collage.tab;
    let active = TABS.iter().position(|(tab, ..)| *tab == current).unwrap_or(0);
    let out = widgets::text_tabs(&mut panel, &t, TABS_ID, &items, active);
    let page = Rect::from_min_max(pos2(left.min.x, out.rect.max.y), left.max);
    widgets::tab_page(ui.painter(), &t, page);
    let mut body = ui.new_child(UiBuilder::new().id_salt("collagePage").max_rect(page.shrink(12.0)));
    match current {
        CollageTab::Settings => settings::show(app, &mut body),
        CollageTab::Clips => clips::show(app, &mut body),
    }
    let mut canvas_ui = ui.new_child(UiBuilder::new().id_salt("collageCanvas").max_rect(right));
    canvas::show(app, &mut canvas_ui);

    if let Some((tab, ..)) = out.clicked.and_then(|i| TABS.get(i)).filter(|(tab, ..)| *tab != current)
        && let Err(e) = app.run("collage.tab", &json!({ "tab": tab }))
    {
        app.toast(&e.to_string(), widgets::ToastKind::Caution);
    }
}
