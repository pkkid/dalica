//! The library tree pane (`ui-layout.md` §4), left of the light table and as wide as the splitter
//! leaves it (200 … W − 480): the **Folders** group header (30 high: disclosure triangle, SemiBold
//! title with the count, `tree:group:folders`), then one 24-high row per folder, newest first
//! (16 pt folder icon, name, count in `text_dim`). Clicking a row selects its section and scrolls the
//! light table to it; clicking the header folds the rows away. The other groups, year dividers, tree
//! mode, the collapsed light-table sections and the keyboard arrive with M4.

use dalica_ui_kit::fonts;
use dalica_ui_kit::icons::{self, Icon};
use dalica_ui_kit::registry::Role;
use dalica_ui_kit::widgets::{self, Target};
use egui::text::{LayoutJob, TextWrapping};
use egui::{Id, Rect, Sense, Ui, UiBuilder, pos2, vec2};

use super::{Panels, light_table};
use crate::app::App;
use crate::perf::{self, Counter};

/// Group header and row heights (`design-system.md` §4).
pub const GROUP_HEADER: f32 = 30.0;
pub const ROW: f32 = 24.0;
/// A row's icon edge, where it starts, and the gap after it.
const ICON: f32 = 16.0;
const ICON_LEFT: f32 = 24.0;
const ICON_GAP: f32 = 6.0;
/// Space kept free at a row's right end.
const RIGHT: f32 = 8.0;

/// The Folders group's header label: `Folders (174)`.
pub fn group_title(folders: usize) -> String {
    format!("Folders ({folders})")
}

pub fn show(p: &mut Panels, app: &mut App, ui: &mut Ui) {
    let t = *app.tokens();
    let rect = ui.max_rect();
    let Some(session) = app.session() else { return };
    p.sections.update(session.library());
    // Each group is shown only when it has rows (§4).
    if p.sections.list.is_empty() {
        return;
    }
    let open_id = Id::new("tree:group:folders/open");
    let open = ui.ctx().data(|d| d.get_temp::<bool>(open_id)).unwrap_or(true);
    let header_rect = Rect::from_min_size(rect.min, vec2(rect.width(), GROUP_HEADER));
    let mut header_ui = ui.new_child(UiBuilder::new().max_rect(header_rect));
    let header = widgets::section_header(&mut header_ui, &t, "tree:group:folders", &group_title(p.sections.list.len()), open);
    if header.clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(open_id, !open));
    }
    if !open {
        perf::count(ui.ctx(), Counter::TreeRowsVisited, 0);
        return;
    }
    let painter = ui.painter().clone();
    let mut picked = None;
    let mut visited = 0u64;
    for (i, s) in p.sections.list.iter().enumerate() {
        let row = Rect::from_min_size(rect.min + vec2(0.0, GROUP_HEADER + i as f32 * ROW), vec2(rect.width(), ROW));
        if row.min.y > rect.max.y {
            break;
        }
        visited += 1;
        let r = ui.interact(row, Id::new(("tree:folder", s.folder.get())), Sense::click());
        let selected = app.state().folder == Some(s.folder);
        if selected {
            painter.rect_filled(row, 0, t.canvas);
        } else if r.hovered() {
            painter.rect_filled(row, 0, t.hover.gamma_multiply(0.6));
        }
        let icon = Rect::from_center_size(pos2(row.min.x + ICON_LEFT + ICON / 2.0, row.center().y), vec2(ICON, ICON));
        icons::paint(&painter, icon, Icon::Folder, t.icon);
        // The name, cut short to leave room for the count right after it.
        let count = format!("({})", s.photos.len());
        let count_galley = painter.layout_no_wrap(count.clone(), fonts::regular(t.count), t.text_dim);
        let x = icon.max.x + ICON_GAP;
        let room = (row.max.x - RIGHT - x - count_galley.size().x - 4.0).max(1.0);
        let mut job = LayoutJob::simple_singleline(s.title.clone(), fonts::regular(t.body), if selected { t.text } else { t.text_label });
        job.wrap = TextWrapping::truncate_at_width(room);
        let name = painter.layout_job(job);
        let name_w = name.size().x;
        painter.galley(pos2(x, row.center().y - name.size().y / 2.0).round(), name, t.text);
        painter.galley(pos2(x + name_w + 4.0, row.center().y - count_galley.size().y / 2.0).round(), count_galley, t.text_dim);
        let label = format!("{} {count}", s.title);
        widgets::register(ui.ctx(), &format!("tree:folder:{}", s.folder.get()), Role::TreeRow, &label, Target::Widget(&r), selected);
        if r.clicked() {
            picked = Some(s.folder);
        }
    }
    // The header is not a row: report the rows laid out.
    perf::count(ui.ctx(), Counter::TreeRowsVisited, visited);
    if let Some(folder) = picked {
        app.state_mut().folder = Some(folder);
        // The layout recomputes the offset for the light table's real width this frame; this one
        // only asks for the scroll.
        let width = (ui.ctx().content_rect().width() - rect.width()).max(0.0);
        p.scroll_to = light_table::offset_of(&p.sections, folder, width, app.state().thumb_size);
    }
}
